//! Worker mode (persistent PHP scripts).
//!
//! Mirrors `worker.go` from the Go source.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI32};
use std::sync::{Arc, RwLock};

use crate::context::PhpContext;
use crate::errors::{Error, Result};
use crate::thread::PhpThread;

/// A worker script and its associated threads.
pub struct Worker {
    name: String,
    file_name: String,
    declared_file_name: String,
    match_request: Option<Box<dyn Fn(&hyper::Request<hyper::body::Incoming>) -> bool + Send + Sync>>,
    num: i32,
    max_threads: i32,
    request_options: RwLock<Vec<Box<dyn crate::options::RequestOption>>>,
    threads: RwLock<Vec<Arc<PhpThread>>>,
    max_consecutive_failures: i32,
    on_thread_ready: Option<Box<dyn Fn(i32) + Send + Sync>>,
    on_thread_shutdown: Option<Box<dyn Fn(i32) + Send + Sync>>,
    queued_requests: AtomicI32,
    server: Option<Arc<crate::server::Server>>,
    stopped: AtomicBool,
}

impl Worker {
    /// Create a new worker.
    pub fn new(
        name: String,
        file_name: String,
        declared_file_name: String,
        num: i32,
        max_threads: i32,
        max_consecutive_failures: i32,
        server: Option<Arc<crate::server::Server>>,
    ) -> Self {
        Worker {
            name,
            file_name,
            declared_file_name,
            match_request: None,
            num,
            max_threads,
            request_options: RwLock::new(Vec::new()),
            threads: RwLock::new(Vec::new()),
            max_consecutive_failures,
            on_thread_ready: None,
            on_thread_shutdown: None,
            queued_requests: AtomicI32::new(0),
            server,
            stopped: AtomicBool::new(false),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    pub fn declared_file_name(&self) -> &str {
        &self.declared_file_name
    }

    pub fn match_request(&self) -> Option<&(dyn Fn(&hyper::Request<hyper::body::Incoming>) -> bool + Send + Sync)> {
        self.match_request.as_deref()
    }

    pub fn server(&self) -> Option<&Arc<crate::server::Server>> {
        self.server.as_ref()
    }

    pub fn set_match_request(
        &mut self,
        f: Box<dyn Fn(&hyper::Request<hyper::body::Incoming>) -> bool + Send + Sync>,
    ) {
        self.match_request = Some(f);
    }

    pub fn add_request_options(&self, opts: Vec<Box<dyn crate::options::RequestOption>>) {
        self.request_options.write().unwrap().extend(opts);
    }

    pub fn attach_thread(&self, thread: Arc<PhpThread>) {
        self.threads.write().unwrap().push(thread);
    }

    pub fn detach_thread(&self, thread: &Arc<PhpThread>) {
        let mut threads = self.threads.write().unwrap();
        threads.retain(|t| !Arc::ptr_eq(t, thread));
    }

    pub fn threads_snapshot(&self) -> Vec<Arc<PhpThread>> {
        self.threads.read().unwrap().clone()
    }

    pub fn count_threads(&self) -> i32 {
        self.threads.read().unwrap().len() as i32
    }

    pub fn is_at_thread_limit(&self) -> bool {
        if self.max_threads <= 0 {
            return false;
        }
        self.threads.read().unwrap().len() >= self.max_threads as usize
    }

    /// Handle a request by dispatching to a worker thread.
    pub async fn handle_request(&self, mut fc: PhpContext) -> Result<()> {
        // TODO: implement worker request dispatch
        let _ = &mut fc;
        Ok(())
    }
}

/// Global registry of workers.
pub struct WorkerRegistry {
    pub workers: RwLock<Vec<Arc<Worker>>>,
    pub workers_by_name: RwLock<HashMap<String, Arc<Worker>>>,
    pub global_workers_by_path: RwLock<HashMap<String, Arc<Worker>>>,
    pub watcher_is_enabled: AtomicBool,
}

impl WorkerRegistry {
    pub fn new() -> Self {
        WorkerRegistry {
            workers: RwLock::new(Vec::new()),
            workers_by_name: RwLock::new(HashMap::new()),
            global_workers_by_path: RwLock::new(HashMap::new()),
            watcher_is_enabled: AtomicBool::new(false),
        }
    }
}

impl Default for WorkerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Global worker registry instance.
pub static WORKER_REGISTRY: once_cell::sync::Lazy<RwLock<WorkerRegistry>> =
    once_cell::sync::Lazy::new(|| RwLock::new(WorkerRegistry::new()));

/// Resolve a worker file path to an absolute path.
pub fn resolve_worker_file(file_name: &str) -> Result<(String, String)> {
    let declared = crate::cgi::fastabs::fast_abs(file_name)
        .map_err(|e| Error::InvalidWorkerFilename(file_name.to_string(), e.to_string()))?
        .to_string_lossy()
        .into_owned();

    let abs = crate::cgi::fastabs::fast_abs(file_name)
        .map_err(|e| Error::InvalidWorkerFilename(file_name.to_string(), e.to_string()))?
        .to_string_lossy()
        .into_owned();

    if !std::path::Path::new(&abs).exists() {
        return Err(Error::WorkerFileNotFound(abs, "file not found".to_string()));
    }

    Ok((declared, abs))
}

/// Validate worker declarations (mirror `checkWorkerDeclaration`).
pub fn check_worker_declaration(
    file_name: &str,
    name: &str,
    server: Option<&Arc<crate::server::Server>>,
    match_request: bool,
    name_taken: impl Fn(&str) -> bool,
    path_taken: impl Fn(Option<&Arc<crate::server::Server>>, &str) -> bool,
) -> Result<()> {
    if server.is_none() {
        if path_taken(None, file_name) {
            return Err(Error::DuplicateWorkerFilename(file_name.to_string()));
        }
        if match_request {
            return Err(Error::WorkerMatcherWithoutServer(name.to_string()));
        }
    } else if !match_request && path_taken(server, file_name) {
        return Err(Error::DuplicateServerWorkerFilename(file_name.to_string()));
    }

    if name_taken(name) {
        return Err(Error::DuplicateWorkerName(name.to_string()));
    }

    Ok(())
}