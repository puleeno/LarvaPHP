//! Configuration options.
//!
//! Mirrors `options.go` and `requestoptions.go` from the Go source.

use std::collections::HashMap;
use std::time::Duration;

use crate::context::PhpContext;
use crate::env::PreparedEnv;
use crate::errors::Result;

/// Trait for options that configure the global LarvaPHP instance.
pub trait Option: Send + Sync {
    fn apply(&self, opt: &mut Opt) -> Result<()>;
}

/// Trait for options that configure a worker.
pub trait WorkerOption: Send + Sync {
    fn apply(&self, worker: &mut WorkerOpt) -> Result<()>;
}

/// Trait for options that configure a server.
pub trait ServerOption: Send + Sync {
    fn apply(&self, server: &mut crate::server::Server) -> Result<()>;
}

pub use crate::context::RequestOption;

/// Internal options struct (mirrors the `opt` struct in options.go).
pub struct Opt {
    pub num_threads: i32,
    pub max_threads: i32,
    pub workers: Vec<WorkerOpt>,
    pub php_ini: HashMap<String, String>,
    pub max_wait_time: Duration,
    pub max_idle_time: Duration,
    pub max_requests: i32,
    pub servers: Vec<std::sync::Arc<crate::server::Server>>,
}

impl Default for Opt {
    fn default() -> Self {
        Self {
            num_threads: 0,
            max_threads: 0,
            workers: Vec::new(),
            php_ini: HashMap::new(),
            max_wait_time: Duration::from_secs(0),
            max_idle_time: Duration::from_secs(5),
            max_requests: 0,
            servers: Vec::new(),
        }
    }
}

/// Internal worker options struct (mirrors `workerOpt`).
pub struct WorkerOpt {
    pub name: String,
    pub file_name: String,
    pub declared_file_name: String,
    pub num: i32,
    pub max_threads: i32,
    pub server: ::std::option::Option<::std::sync::Arc<crate::server::Server>>,
    pub env: PreparedEnv,
    pub request_options: Vec<Box<dyn RequestOption>>,
    pub watch: Vec<String>,
    pub max_consecutive_failures: i32,
}

impl Default for WorkerOpt {
    fn default() -> Self {
        Self {
            name: String::new(),
            file_name: String::new(),
            declared_file_name: String::new(),
            num: 0,
            max_threads: 0,
            env: PreparedEnv::new(),
            server: None,
            request_options: Vec::new(),
            watch: Vec::new(),
            max_consecutive_failures: crate::DEFAULT_MAX_CONSECUTIVE_FAILURES,
        }
    }
}

// ============================================================================
// Global Option builders
// ============================================================================

pub struct WithNumThreads(pub i32);
impl Option for WithNumThreads {
    fn apply(&self, opt: &mut Opt) -> Result<()> {
        opt.num_threads = self.0;
        Ok(())
    }
}

pub struct WithMaxThreads(pub i32);
impl Option for WithMaxThreads {
    fn apply(&self, opt: &mut Opt) -> Result<()> {
        opt.max_threads = self.0;
        Ok(())
    }
}

pub struct WithPhpIni(pub HashMap<String, String>);
impl Option for WithPhpIni {
    fn apply(&self, opt: &mut Opt) -> Result<()> {
        opt.php_ini = self.0.clone();
        Ok(())
    }
}

pub struct WithMaxWaitTime(pub Duration);
impl Option for WithMaxWaitTime {
    fn apply(&self, opt: &mut Opt) -> Result<()> {
        opt.max_wait_time = self.0;
        Ok(())
    }
}

pub struct WithMaxIdleTime(pub Duration);
impl Option for WithMaxIdleTime {
    fn apply(&self, opt: &mut Opt) -> Result<()> {
        opt.max_idle_time = self.0;
        Ok(())
    }
}

pub struct WithMaxRequests(pub i32);
impl Option for WithMaxRequests {
    fn apply(&self, opt: &mut Opt) -> Result<()> {
        opt.max_requests = self.0;
        Ok(())
    }
}

pub struct WithServer(pub std::sync::Arc<crate::server::Server>);
impl Option for WithServer {
    fn apply(&self, opt: &mut Opt) -> Result<()> {
        opt.servers.push(self.0.clone());
        Ok(())
    }
}

pub struct WithWorkers {
    pub name: String,
    pub file_name: String,
    pub num: i32,
    pub options: Vec<Box<dyn WorkerOption>>,
}
impl Option for WithWorkers {
    fn apply(&self, opt: &mut Opt) -> Result<()> {
        let mut worker = WorkerOpt {
            name: self.name.clone(),
            file_name: self.file_name.clone(),
            num: self.num,
            env: crate::env::prepare_env(HashMap::new()),
            watch: Vec::new(),
            max_consecutive_failures: crate::DEFAULT_MAX_CONSECUTIVE_FAILURES,
            ..Default::default()
        };
        for option in &self.options {
            option.apply(&mut worker)?;
        }
        opt.workers.push(worker);
        Ok(())
    }
}

// ============================================================================
// Worker Option builders
// ============================================================================

pub struct WithWorkerEnv(pub HashMap<String, String>);
impl WorkerOption for WithWorkerEnv {
    fn apply(&self, worker: &mut WorkerOpt) -> Result<()> {
        worker.env = crate::env::prepare_env(self.0.clone());
        Ok(())
    }
}

pub struct WithWorkerMaxThreads(pub i32);
impl WorkerOption for WithWorkerMaxThreads {
    fn apply(&self, worker: &mut WorkerOpt) -> Result<()> {
        worker.max_threads = self.0;
        Ok(())
    }
}

pub struct WithWorkerWatchMode(pub Vec<String>);
impl WorkerOption for WithWorkerWatchMode {
    fn apply(&self, worker: &mut WorkerOpt) -> Result<()> {
        worker.watch = self.0.clone();
        Ok(())
    }
}

pub struct WithWorkerMaxFailures(pub i32);
impl WorkerOption for WithWorkerMaxFailures {
    fn apply(&self, worker: &mut WorkerOpt) -> Result<()> {
        if self.0 < -1 {
            return Err(crate::errors::Error::Ffi(
                format!("max consecutive failures must be >= -1, got {}", self.0)
            ));
        }
        worker.max_consecutive_failures = self.0;
        Ok(())
    }
}

pub struct WithWorkerServerScope(pub ::std::sync::Arc<crate::server::Server>);
impl WorkerOption for WithWorkerServerScope {
    fn apply(&self, worker: &mut WorkerOpt) -> Result<()> {
        worker.server = Some(self.0.clone());
        Ok(())
    }
}

// ============================================================================
// crate::server::Server Option builders
// ============================================================================

pub struct WithServerName(pub String);
impl ServerOption for WithServerName {
    fn apply(&self, server: &mut crate::server::Server) -> Result<()> {
        server.set_name(self.0.clone());
        Ok(())
    }
}

pub struct WithServerSplitPath(pub Vec<String>);
impl ServerOption for WithServerSplitPath {
    fn apply(&self, server: &mut crate::server::Server) -> Result<()> {
        let mut split = self.0.clone();
        crate::cgi::normalize_split_path(&mut split)?;
        server.set_split_path(split);
        Ok(())
    }
}

pub struct WithServerEnv(pub HashMap<String, String>);
impl ServerOption for WithServerEnv {
    fn apply(&self, server: &mut crate::server::Server) -> Result<()> {
        server.set_env(self.0.clone());
        Ok(())
    }
}

// ============================================================================
// Request Option builders
// ============================================================================

pub struct WithRequestDocumentRoot {
    pub document_root: String,
    pub resolve_symlink: bool,
}
impl RequestOption for WithRequestDocumentRoot {
    fn apply(&self, ctx: &mut PhpContext) -> Result<()> {
        let root = crate::cgi::fastabs::fast_abs(&self.document_root)
            .map_err(|e| crate::errors::Error::Ffi(e.to_string()))?;
        let root_str = if self.resolve_symlink {
            std::fs::canonicalize(&root)
                .unwrap_or(root.clone())
                .to_string_lossy()
                .into_owned()
        } else {
            root.to_string_lossy().into_owned()
        };
        ctx.document_root = root_str;
        Ok(())
    }
}

pub struct WithRequestSplitPath(pub Vec<String>);
impl RequestOption for WithRequestSplitPath {
    fn apply(&self, ctx: &mut PhpContext) -> Result<()> {
        let mut split = self.0.clone();
        crate::cgi::normalize_split_path(&mut split)?;
        ctx.split_path = split;
        Ok(())
    }
}

pub struct WithRequestEnv(pub HashMap<String, String>);
impl RequestOption for WithRequestEnv {
    fn apply(&self, ctx: &mut PhpContext) -> Result<()> {
        ctx.env = crate::env::prepare_env(self.0.clone());
        Ok(())
    }
}

pub struct WithRequestPreparedEnv(pub PreparedEnv);
impl RequestOption for WithRequestPreparedEnv {
    fn apply(&self, ctx: &mut PhpContext) -> Result<()> {
        ctx.env = crate::env::ensure_prepared_env(self.0.clone());
        Ok(())
    }
}

pub struct WithRequestBodyTimeout(pub Duration);
impl RequestOption for WithRequestBodyTimeout {
    fn apply(&self, ctx: &mut PhpContext) -> Result<()> {
        ctx.request_body_timeout = Some(self.0);
        Ok(())
    }
}

pub struct WithWorkerName(pub String);
impl RequestOption for WithWorkerName {
    fn apply(&self, ctx: &mut PhpContext) -> Result<()> {
        if self.0.is_empty() {
            return Ok(());
        }
        if let Some(worker) = crate::thread::get_worker_by_name(&self.0) {
            ctx.worker = Some(worker);
        } else {
            return Err(crate::errors::Error::UnknownWorker(self.0.clone()));
        }
        Ok(())
    }
}

