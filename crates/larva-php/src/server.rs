//! Server blocks (scoped request handling).
//!
//! Mirrors `server.go` from the Go source.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper::body::Incoming as IncomingBody;
use http_body_util::Full;
use bytes::Bytes;
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
        if !std::path::Path::new(&script_path).exists() {
            return Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Full::new(Bytes::from("Not Found")))
                .unwrap());
        }

        // Execute the PHP script and capture output
        let output = execute_php_script(&script_path)?;

        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/html; charset=UTF-8")
            .header("server", "LarvaPHP")
            .body(Full::new(Bytes::from(output)))
            .unwrap())
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
                                return Ok::<Response<Full<Bytes>>, hyper::Error>(Response::builder()
                                    .status(StatusCode::SERVICE_UNAVAILABLE)
                                    .body(Full::new(Bytes::from("Service Unavailable")))
                                    .unwrap());
                            }

                            let uri = req.uri().to_string();
                            let script_path = resolve_script_path_static(&uri, &root, &split_path);

                            if let Some(path) = script_path {
                                if std::path::Path::new(&path).exists() {
                                    let body = execute_php_script(&path).unwrap_or_default();

                                    return Ok::<Response<Full<Bytes>>, hyper::Error>(Response::builder()
                                        .status(StatusCode::OK)
                                        .header("content-type", "text/html; charset=UTF-8")
                                        .header("server", "LarvaPHP")
                                        .body(Full::new(Bytes::from(body)))
                                        .unwrap());
                                }
                            }

                            Ok::<Response<Full<Bytes>>, hyper::Error>(Response::builder()
                                .status(StatusCode::NOT_FOUND)
                                .body(Full::new(Bytes::from("Not Found")))
                                .unwrap())
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

/// Find the php.exe executable.
fn find_php_exe() -> Option<String> {
    let candidates = [
        r"C:\Program Files\PHP\current\php.exe",
        r"C:\Program Files (x86)\PHP\current\php.exe",
        r"C:\php\php.exe",
    ];
    for c in &candidates {
        if std::path::Path::new(c).exists() {
            return Some(c.to_string());
        }
    }
    if let Ok(path) = std::env::var("PATH") {
        for dir in path.split(';') {
            let p = std::path::Path::new(dir).join("php.exe");
            if p.exists() {
                return Some(p.to_string_lossy().to_string());
            }
        }
    }
    None
}

/// Execute a PHP script via php.exe subprocess and capture stdout.
fn execute_php_script(script_path: &str) -> Result<String> {
    let php_exe = find_php_exe()
        .ok_or_else(|| Error::Ffi("php.exe not found".to_string()))?;

    let output = std::process::Command::new(php_exe)
        .arg(script_path)
        .output()
        .map_err(|e| Error::Ffi(format!("Failed to execute php: {}", e)))?;

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

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
