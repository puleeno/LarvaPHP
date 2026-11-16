//! Server blocks (scoped request handling).
//!
//! Mirrors `server.go` from the Go source.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use crate::context::PhpContext;
use crate::errors::{Error, Result};
use crate::options::RequestOption;
use crate::worker::Worker;

/// A preconfigured server block.
///
/// Requests and workers can be scoped to a Server.
pub struct Server {
    idx: i32,
    configured_name: String,
    name: String,
    root: String,
    split_path: Vec<String>,
    env: HashMap<String, String>,
    workers: RwLock<Vec<Arc<Worker>>>,
    workers_by_path: RwLock<HashMap<String, Arc<Worker>>>,
    workers_with_request_matcher: RwLock<Vec<Arc<Worker>>>,
    is_registered: AtomicBool,
    logger_level: tracing::Level,
}

impl Server {
    /// Create a new server with the given root directory.
    pub fn new(root: &str) -> Result<Self> {
        let abs_root = crate::cgi::fastabs::fast_abs(root)
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
            logger_level: tracing::Level::INFO,
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
            logger_level: tracing::Level::INFO,
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
    pub fn logger_level(&self) -> tracing::Level {
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
        _req: hyper::Request<hyper::body::Incoming>,
        opts: Vec<Box<dyn RequestOption>>,
    ) -> Result<()> {
        if !self.is_registered() {
            return Err(Error::NotRunning);
        }

        let ctx = PhpContext::from_request(&_req, Arc::new(Server::fallback()), opts)?;
        ctx.validate()?;

        // TODO: handle request with worker or regular threads
        Ok(())
    }
}