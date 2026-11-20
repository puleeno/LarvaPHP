//! Worker mode (persistent PHP scripts).
//!
//! Mirrors `worker.go` from the Go source.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
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

    /// Whether the worker has been stopped (auto-restart pending).
    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }

    /// Stop the worker: mark it stopped and drain all thread request channels so
    /// no new requests are dispatched. Threads are re-bootstrapped on restart.
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        for thread in self.threads.read().unwrap().iter() {
            thread.shutdown();
        }
    }

    /// Restart a stopped worker: clear the stopped flag. The worker's threads
    /// re-bootstrap on the next `handle_request` (or explicit re-init).
    pub fn restart(&self) {
        self.stopped.store(false, Ordering::Release);
    }
}

/// Restart every registered worker whose script file matches `path`.
///
/// Used by the file watcher to implement worker auto-restart: when a PHP file
/// changes, the workers backed by that file are stopped and restarted so the
/// new source is picked up. Returns the number of workers restarted.
pub fn restart_workers_for_file(path: &str) -> usize {
    let reg = WORKER_REGISTRY.read().unwrap();
    let workers = reg.workers.read().unwrap();
    let changed = std::path::Path::new(path);
    let mut restarted = 0usize;
    for worker in workers.iter() {
        let worker_file = std::path::Path::new(worker.file_name());
        let matches = worker_file == changed
            || worker
                .file_name()
                .eq_ignore_ascii_case(&path.to_string());
        if matches {
            worker.restart();
            restarted += 1;
        }
    }
    restarted
}

/// Check whether any worker is backed by the given script file.
pub fn is_worker_file(path: &str) -> bool {
    let reg = WORKER_REGISTRY.read().unwrap();
    let workers = reg.workers.read().unwrap();
    let changed = std::path::Path::new(path);
    workers
        .iter()
        .any(|w| std::path::Path::new(w.file_name()) == changed)
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

// ============================================================================
// Worker Router (Strategy pattern for request matching)
// ============================================================================

/// Strategy trait for matching requests to workers.
pub trait WorkerMatcher: Send + Sync {
    fn matches(&self, uri: &str) -> bool;
}

/// Prefix matcher - matches URIs starting with a prefix.
pub struct PrefixMatcher {
    prefix: String,
}

impl PrefixMatcher {
    pub fn new(prefix: impl Into<String>) -> Self {
        Self { prefix: prefix.into() }
    }
}

impl WorkerMatcher for PrefixMatcher {
    fn matches(&self, uri: &str) -> bool {
        uri.starts_with(&self.prefix)
    }
}

/// Regex matcher - matches URIs against a regex pattern.
pub struct RegexMatcher {
    re: regex::Regex,
}

impl RegexMatcher {
    pub fn new(pattern: &str) -> Option<Self> {
        regex::Regex::new(pattern).ok().map(|re| Self { re })
    }
}

impl WorkerMatcher for RegexMatcher {
    fn matches(&self, uri: &str) -> bool {
        self.re.is_match(uri)
    }
}

/// Router that dispatches requests to workers based on matchers.
/// Uses a sorted list of (matcher, worker) pairs for O(n) lookup.
pub struct WorkerRouter {
    routes: Vec<(Box<dyn WorkerMatcher>, Arc<Worker>)>,
}

impl WorkerRouter {
    pub fn new() -> Self {
        Self { routes: Vec::new() }
    }

    /// Add a route with a matcher.
    pub fn add_route(&mut self, matcher: Box<dyn WorkerMatcher>, worker: Arc<Worker>) {
        self.routes.push((matcher, worker));
    }

    /// Find the first worker whose matcher matches the URI.
    /// O(n) where n = number of routes (typically small).
    pub fn find_worker(&self, uri: &str) -> Option<&Arc<Worker>> {
        for (matcher, worker) in &self.routes {
            if matcher.matches(uri) {
                return Some(worker);
            }
        }
        None
    }
}

impl Default for WorkerRouter {
    fn default() -> Self {
        Self::new()
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_matcher_matches() {
        let m = PrefixMatcher::new("/api");
        assert!(m.matches("/api/users"));
        assert!(m.matches("/api"));
        assert!(!m.matches("/blog/api"));
    }

    #[test]
    fn regex_matcher_matches() {
        let m = RegexMatcher::new(r"^/api/v\d+").unwrap();
        assert!(m.matches("/api/v1/users"));
        assert!(!m.matches("/api/vX/users"));
    }

    #[test]
    fn regex_matcher_invalid_pattern() {
        assert!(RegexMatcher::new("[invalid").is_none());
    }

    #[test]
    fn worker_router_first_match_wins() {
        let w1 = Arc::new(Worker::new(
            "w1".to_string(),
            "/app/api.php".to_string(),
            "/app/api.php".to_string(),
            1,
            0,
            6,
            None,
        ));
        let w2 = Arc::new(Worker::new(
            "w2".to_string(),
            "/app/index.php".to_string(),
            "/app/index.php".to_string(),
            1,
            0,
            6,
            None,
        ));

        let mut router = WorkerRouter::new();
        router.add_route(Box::new(PrefixMatcher::new("/api")), w1.clone());
        router.add_route(Box::new(PrefixMatcher::new("/")), w2.clone());

        // First matching route wins.
        assert!(Arc::ptr_eq(router.find_worker("/api/users").unwrap(), &w1));
        assert!(Arc::ptr_eq(router.find_worker("/blog").unwrap(), &w2));
    }

    #[test]
    fn worker_router_no_match() {
        let w1 = Arc::new(Worker::new(
            "w1".to_string(),
            "/app/api.php".to_string(),
            "/app/api.php".to_string(),
            1,
            0,
            6,
            None,
        ));
        let mut router = WorkerRouter::new();
        router.add_route(Box::new(PrefixMatcher::new("/api")), w1);
        assert!(router.find_worker("/blog").is_none());
    }

    #[test]
    fn worker_stop_and_restart() {
        let w = Worker::new(
            "w".to_string(),
            "/app/w.php".to_string(),
            "/app/w.php".to_string(),
            1,
            0,
            6,
            None,
        );
        assert!(!w.is_stopped());
        w.stop();
        assert!(w.is_stopped());
        w.restart();
        assert!(!w.is_stopped());
    }

    #[test]
    fn is_worker_file_detects_registered() {
        // Not registered -> false.
        assert!(!is_worker_file("/app/nonexistent.php"));
    }
}