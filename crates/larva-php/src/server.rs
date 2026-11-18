//! Server blocks (scoped request handling).
//!
//! Mirrors `server.go` from the Go source.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;

use bytes::Bytes;
use http_body::Body as _;
use http_body_util::Full;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper::body::Incoming as IncomingBody;
use hyper_util::server::conn::auto;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;

use crate::context::{PhpContext, RequestOption};
use crate::cgi::fastabs::fast_abs;
use crate::errors::{Error, Result};
use crate::worker::Worker;
use tracing::{error, info, Level};

/// A preconfigured server block.
///
/// Requests and workers can be scoped to a Server.
pub struct Server {
    pub idx: i32,
    pub configured_name: String,
    pub name: String,
    pub root: String,
    pub split_path: Vec<String>,
    pub env: HashMap<String, String>,
    pub workers: RwLock<Vec<Arc<Worker>>>,
    pub workers_by_path: RwLock<HashMap<String, Arc<Worker>>>,
    pub workers_with_request_matcher: RwLock<Vec<Arc<Worker>>>,
    pub is_registered: AtomicBool,
    pub logger_level: Level,
}

impl Server {
    /// Create a new server with the given root directory.
    pub fn new(root: &str) -> Result<Self> {
        let abs_root = fast_abs(root)
            .map_err(|e| Error::Ffi(e.to_string()))?
            .to_string_lossy()
            .into_owned();

        Ok(Server {
            idx: -1,
            configured_name: String::new(),
            name: String::new(),
            root: abs_root,
            split_path: vec![".php".to_string()],
            env: HashMap::new(),
            workers: RwLock::new(Vec::new()),
            workers_by_path: RwLock::new(HashMap::new()),
            workers_with_request_matcher: RwLock::new(Vec::new()),
            is_registered: AtomicBool::new(false),
            logger_level: Level::INFO,
        })
    }

    /// Create a fallback server (used when no server is registered).
    pub fn fallback() -> Self {
        Server {
            idx: -1,
            configured_name: String::new(),
            name: String::new(),
            root: String::new(),
            split_path: vec![".php".to_string()],
            env: HashMap::new(),
            workers: RwLock::new(Vec::new()),
            workers_by_path: RwLock::new(HashMap::new()),
            workers_with_request_matcher: RwLock::new(Vec::new()),
            is_registered: AtomicBool::new(true),
            logger_level: Level::INFO,
        }
    }

    /// Get the server name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the root directory.
    pub fn root(&self) -> &str {
        &self.root
    }

    /// Get the split path.
    pub fn split_path(&self) -> &[String] {
        &self.split_path
    }

    /// Get the logger level.
    pub fn logger_level(&self) -> Level {
        self.logger_level
    }

    /// Set the server name.
    pub fn set_name(&mut self, name: String) {
        self.name = name.clone();
        self.configured_name = name;
    }

    /// Set the split path.
    pub fn set_split_path(&mut self, split_path: Vec<String>) {
        self.split_path = split_path;
    }

    /// Set the environment.
    pub fn set_env(&mut self, env: HashMap<String, String>) {
        self.env = env;
    }

    /// Add a worker to this server.
    pub fn add_worker(&self, worker: Arc<Worker>) -> Result<()> {
        let mut workers = self.workers.write().unwrap();
        workers.push(worker.clone());

        if worker.match_request().is_some() {
            let mut matchers = self.workers_with_request_matcher.write().unwrap();
            matchers.push(worker);
            return Ok(());
        }

        let mut by_path = self.workers_by_path.write().unwrap();
        let file_name = worker.file_name().to_string();
        if by_path.contains_key(&file_name) {
            return Err(Error::DuplicateServerWorkerFilename(file_name));
        }
        by_path.insert(file_name.clone(), worker.clone());
        by_path.insert(worker.declared_file_name().to_string(), worker);
        Ok(())
    }

    /// Reset workers (for reload).
    pub fn reset_workers(&self) {
        self.workers.write().unwrap().clear();
        self.workers_by_path.write().unwrap().clear();
        self.workers_with_request_matcher.write().unwrap().clear();
    }

    /// Check if this server is registered (accepting requests).
    pub fn is_registered(&self) -> bool {
        self.is_registered.load(Ordering::Acquire)
    }

    /// Activate this server (start accepting requests).
    pub fn activate(&self) {
        self.is_registered.store(true, Ordering::Release);
    }

    /// Deactivate this server.
    pub fn deactivate(&self) {
        self.is_registered.store(false, Ordering::Release);
    }

    /// Get the worker for a script filename.
    pub fn get_worker_by_path(&self, path: &str) -> Option<Arc<Worker>> {
        self.workers_by_path.read().unwrap().get(path).cloned()
    }

    /// Serve an HTTP request.
    ///
    /// This mirrors `Server.ServeHTTP()`.
    pub async fn serve_http(
        &self,
        req: Request<IncomingBody>,
        opts: Vec<Box<dyn RequestOption>>,
    ) -> Result<Response<Full<Bytes>>> {
        if !self.is_registered() {
            return Ok(Response::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .body(Full::new(Bytes::from("Service Unavailable")))
                .unwrap());
        }

        let ctx = PhpContext::from_request(&req, Arc::new(Server::fallback()), opts)?;
        ctx.validate()?;

        // Determine the script filename
        let script_path = self.resolve_script_path(ctx.request_uri.as_str())?;

        // If the resolved path doesn't exist, try index.php (WordPress permalinks)
        let script_path = if !std::path::Path::new(&script_path).exists() {
            let alt = self.resolve_index_php(ctx.request_uri.as_str());
            match alt {
                Some(ref p) if std::path::Path::new(p).exists() => alt.unwrap(),
                _ => {
                    return Ok(Response::builder()
                        .status(StatusCode::NOT_FOUND)
                        .body(Full::new(Bytes::from("Not Found")))
                        .unwrap());
                }
            }
        } else {
            script_path
        };

        // Read request body (drain the stream so hyper doesn't hang)
        let mut body_bytes = Vec::new();
        let mut body = req.into_body();
        while let Some(frame) = body.frame().await {
            match frame {
                Ok(frame) => {
                    if let Some(chunk) = frame.data_ref() {
                        body_bytes.extend_from_slice(chunk);
                    }
                }
                Err(_) => break,
            }
        }
        let body = Bytes::from(body_bytes);

        // Execute the PHP script via CGI and capture output
        let output = execute_php_cgi(&script_path, &ctx, &body)?;

        // Parse CGI output: headers + optional blank line + body
        let (headers, body_bytes) = parse_cgi_output(&output);

        let mut builder = Response::builder()
            .status(StatusCode::OK)
            .header("server", "LarvaPHP");

        for (name, value) in &headers {
            builder = builder.header(name, value);
        }

        Ok(builder.body(Full::new(Bytes::from(body_bytes))).unwrap())
    }

    /// Resolve the PHP script path from the request URI.
    fn resolve_script_path(&self, uri: &str) -> Result<String> {
        // Remove query string
        let path = uri.split('?').next().unwrap_or(uri);

        // URL-decode the path
        let decoded = url_decode(path);

        // Build the full path: root + path
        let mut full_path = PathBuf::from(&self.root);
        if decoded.starts_with('/') {
            full_path.push(&decoded[1..]);
        } else {
            full_path.push(&decoded);
        }

        // If it's a directory, look for index.php
        if full_path.is_dir() {
            full_path.push("index.php");
        }

        // If no .php extension, add one
        if full_path.extension().is_none() {
            full_path.set_extension("php");
        }

        Ok(full_path.to_string_lossy().to_string())
    }

    /// Try resolving index.php for the given URI (WordPress permalink fallback).
    fn resolve_index_php(&self, uri: &str) -> Option<String> {
        let path = uri.split('?').next().unwrap_or(uri);

        // Try root index.php first (WordPress front controller)
        let mut index_path = PathBuf::from(&self.root);
        index_path.push("index.php");
        if index_path.exists() {
            return Some(index_path.to_string_lossy().to_string());
        }

        None
    }

    /// Start the HTTP server listening on the given address.
    pub async fn start_server(&self, addr: SocketAddr) -> Result<()> {
        self.activate();
        info!("LarvaPHP server listening on http://{}", addr);

        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| Error::Io(e))?;
        let server_root = self.root.clone();
        let server_split_path = self.split_path.clone();

        loop {
            let (stream, _) = listener.accept().await.map_err(|e| Error::Io(e))?;

            let root = server_root.clone();
            let split_path = server_split_path.clone();
            let registered = self.is_registered.load(Ordering::Acquire);

            tokio::spawn(async move {
                let io = TokioIo::new(stream);
                if let Err(err) = auto::Builder::new(hyper_util::rt::TokioExecutor::new())
                    .serve_connection(
                        io,
                        service_fn(move |req: Request<IncomingBody>| {
                            let root = root.clone();
                            let split_path = split_path.clone();
                            let registered = registered.clone();
                            async move {
                                if !registered {
                                    return Ok::<Response<Full<Bytes>>, hyper::Error>(
                                        Response::builder()
                                            .status(StatusCode::SERVICE_UNAVAILABLE)
                                            .body(Full::new(Bytes::from("Service Unavailable")))
                                            .unwrap(),
                                    );
                                }

                                let uri = req.uri().to_string();
                                let script_path =
                                    resolve_script_path_static(&uri, &root, &split_path);

                                // If the resolved path doesn't exist, try index.php
                                let script_path =
                                    if let Some(ref p) = script_path {
                                        if std::path::Path::new(p).exists() {
                                            script_path
                                        } else {
                                            resolve_index_php_static(&uri, &root)
                                        }
                                    } else {
                                        resolve_index_php_static(&uri, &root)
                                    };

                                let Some(path) = script_path else {
                                    return Ok::<Response<Full<Bytes>>, hyper::Error>(
                                        Response::builder()
                                            .status(StatusCode::NOT_FOUND)
                                            .body(Full::new(Bytes::from("Not Found")))
                                            .unwrap(),
                                    );
                                };

                                // Build minimal CGI context for environment variables
                                let rel = path.trim_start_matches(&root);
                                let rel = rel.trim_start_matches('\\').trim_start_matches('/');
                                let script_name = if rel.is_empty() {
                                    "/".to_string()
                                } else {
                                    format!("/{}", rel.replace('\\', "/"))
                                };

                                let ctx = PhpContext {
                                    ctx: tokio::sync::watch::channel(None).1,
                                    document_root: root.clone(),
                                    split_path: split_path.clone(),
                                    env: Default::default(),
                                    logger: Level::INFO,
                                    worker: None,
                                    server: Arc::new(Server::fallback()),
                                    request_body_timeout: None,
                                    doc_uri: String::new(),
                                    path_info: String::new(),
                                    script_name,
                                    script_filename: path.clone(),
                                    request_uri: uri.clone(),
                                    query_string: uri
                                        .split_once('?')
                                        .map(|(_, q)| q.to_string())
                                        .unwrap_or_default(),
                                    request_method: req.method().to_string(),
                                    content_type: req
                                        .headers()
                                        .get("content-type")
                                        .and_then(|v| v.to_str().ok())
                                        .unwrap_or_default()
                                        .to_string(),
                                    content_length: req
                                        .headers()
                                        .get("content-length")
                                        .and_then(|v| v.to_str().ok())
                                        .unwrap_or_default()
                                        .to_string(),
                                    server_protocol: "HTTP/1.1".to_string(),
                                    server_name: req
                                        .headers()
                                        .get("host")
                                        .and_then(|v| v.to_str().ok())
                                        .and_then(|v| v.split_once(':').map(|(h, _)| h.to_string()))
                                        .unwrap_or_else(|| "localhost".to_string()),
                                    server_port: req
                                        .headers()
                                        .get("host")
                                        .and_then(|v| v.to_str().ok())
                                        .and_then(|v| v.split_once(':').map(|(_, p)| p.to_string()))
                                        .unwrap_or_else(|| "80".to_string()),
                                    https: "off".to_string(),
                                    headers_written: false,
                                    is_done: AtomicBool::new(false),
                                    client_had_closed: AtomicBool::new(false),
                                    started_at: Instant::now(),
                                    handler_return: None,
                                };

                                // Read request body (drain the stream so hyper doesn't hang)
                                let mut body_bytes = Vec::new();
                                let mut body = req.into_body();
                                while let Some(frame) = body.frame().await {
                                    match frame {
                                        Ok(frame) => {
                                            if let Some(chunk) = frame.data_ref() {
                                                body_bytes.extend_from_slice(chunk);
                                            }
                                        }
                                        Err(_) => break,
                                    }
                                }
                                let body = Bytes::from(body_bytes);

                                match execute_php_cgi(&path, &ctx, &body) {
                                    Ok(output) => {
                                        let (headers, body_bytes) =
                                            parse_cgi_output(&output);

                                        let mut builder =
                                            Response::builder()
                                                .status(StatusCode::OK)
                                                .header("server", "LarvaPHP");

                                        for (name, value) in &headers {
                                            builder = builder.header(name, value);
                                        }

                                        Ok::<Response<Full<Bytes>>, hyper::Error>(
                                            builder
                                                .body(Full::new(Bytes::from(body_bytes)))
                                                .unwrap(),
                                        )
                                    }
                                    Err(e) => {
                                        error!("PHP execution error: {}", e);
                                        Ok::<Response<Full<Bytes>>, hyper::Error>(
                                            Response::builder()
                                                .status(StatusCode::INTERNAL_SERVER_ERROR)
                                                .body(Full::new(Bytes::from(
                                                    "Internal Server Error",
                                                )))
                                                .unwrap(),
                                        )
                                    }
                                }
                            }
                        }))
                        .await
                    {
                        error!("Connection error: {}", err);
                    }
                });
        }
    }
}

/// Find the php-cgi.exe executable.
fn find_php_cgi() -> Option<String> {
    let candidates = [
        r"C:\Program Files\PHP\current\php-cgi.exe",
        r"C:\Program Files (x86)\PHP\current\php-cgi.exe",
        r"C:\php\php-cgi.exe",
    ];
    for c in &candidates {
        if std::path::Path::new(c).exists() {
            return Some(c.to_string());
        }
    }
    if let Ok(path) = std::env::var("PATH") {
        for dir in path.split(';') {
            let p = std::path::Path::new(dir).join("php-cgi.exe");
            if p.exists() {
                return Some(p.to_string_lossy().to_string());
            }
        }
    }
    None
}

/// Execute a PHP script via php-cgi.exe CGI subprocess and capture output.
fn execute_php_cgi(script_path: &str, ctx: &PhpContext, body: &[u8]) -> Result<String> {
    let php_exe = find_php_cgi()
        .ok_or_else(|| Error::Ffi("php-cgi.exe not found".to_string()))?;

    let mut cmd = std::process::Command::new(php_exe);
    cmd.current_dir(
        std::path::Path::new(script_path)
            .parent()
            .unwrap_or(std::path::Path::new(".")),
    );

    // Set CGI environment variables
    cmd.env("REQUEST_METHOD", &ctx.request_method);
    cmd.env("REQUEST_URI", &ctx.request_uri);
    cmd.env("QUERY_STRING", &ctx.query_string);
    cmd.env("SCRIPT_NAME", &ctx.script_name);
    cmd.env("SCRIPT_FILENAME", script_path);
    cmd.env("DOCUMENT_ROOT", &ctx.document_root);
    cmd.env("SERVER_PROTOCOL", &ctx.server_protocol);
    cmd.env("SERVER_NAME", &ctx.server_name);
    cmd.env("SERVER_PORT", &ctx.server_port);
    cmd.env("HTTPS", &ctx.https);
    cmd.env("CONTENT_TYPE", &ctx.content_type);
    cmd.env("CONTENT_LENGTH", &ctx.content_length);
    cmd.env("PATH_INFO", &ctx.path_info);
    cmd.env("PHP_SELF", &ctx.script_name);
    cmd.env("REDIRECT_STATUS", "200");
    cmd.env("GATEWAY_INTERFACE", "CGI/1.1");
    cmd.env("SERVER_SOFTWARE", "LarvaPHP/1.0");

    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| Error::Ffi(format!("Failed to spawn php-cgi: {}", e)))?;

    // Write request body to stdin
    if !body.is_empty() {
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(body);
        }
    }

    let output = child
        .wait_with_output()
        .map_err(|e| Error::Ffi(format!("Failed to wait for php-cgi: {}", e)))?;

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Parse CGI output: split headers from body at the first blank line.
fn parse_cgi_output(output: &str) -> (Vec<(String, String)>, Vec<u8>) {
    let mut headers = Vec::new();
    let mut body_start = 0;

    // Find the blank line separating headers from body
    if let Some(pos) = output.find("\r\n\r\n") {
        let header_section = &output[..pos];
        body_start = pos + 4;

        for line in header_section.split("\r\n") {
            if let Some((name, value)) = line.split_once(':') {
                headers.push((name.trim().to_string(), value.trim().to_string()));
            }
        }
    } else if let Some(pos) = output.find("\n\n") {
        let header_section = &output[..pos];
        body_start = pos + 2;

        for line in header_section.split('\n') {
            if let Some((name, value)) = line.split_once(':') {
                headers.push((name.trim().to_string(), value.trim().to_string()));
            }
        }
    }

    let body_bytes = output[body_start..].as_bytes().to_vec();

    // Ensure content-type header exists
    let has_content_type =
        headers.iter().any(|(n, _)| n.eq_ignore_ascii_case("content-type"));
    if !has_content_type {
        headers.push((
            "content-type".to_string(),
            "text/html; charset=UTF-8".to_string(),
        ));
    }

    (headers, body_bytes)
}

/// Resolve the PHP script path from the request URI (static helper).
fn resolve_script_path_static(uri: &str, root: &str, split_path: &[String]) -> Option<String> {
    let path = uri.split('?').next().unwrap_or(uri);
    let decoded = url_decode(path);

    let mut full_path = PathBuf::from(root);
    if decoded.starts_with('/') {
        full_path.push(&decoded[1..]);
    } else {
        full_path.push(&decoded);
    }

    if full_path.is_dir() {
        full_path.push("index.php");
    }

    if full_path.extension().is_none() {
        for ext in split_path {
            let mut p = full_path.clone();
            p.set_extension(ext.trim_start_matches('.'));
            if p.exists() {
                return Some(p.to_string_lossy().to_string());
            }
        }
        full_path.set_extension("php");
    }

    Some(full_path.to_string_lossy().to_string())
}

/// Try resolving index.php for the given URI (WordPress permalink fallback).
fn resolve_index_php_static(uri: &str, root: &str) -> Option<String> {
    let _path = uri.split('?').next().unwrap_or(uri);

    // Try root index.php first (WordPress front controller)
    let mut index_path = PathBuf::from(root);
    index_path.push("index.php");
    if index_path.exists() {
        return Some(index_path.to_string_lossy().to_string());
    }

    None
}

/// URL-decode a string.
fn url_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = &s[i + 1..i + 3];
            if let Ok(val) = u8::from_str_radix(hex, 16) {
                result.push(val as char);
                i += 3;
            } else {
                result.push(bytes[i] as char);
                i += 1;
            }
        } else if bytes[i] == b'+' {
            result.push(' ');
            i += 1;
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }
    result
}
