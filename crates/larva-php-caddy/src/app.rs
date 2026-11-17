//! Caddy app module (global configuration).
//!
//! Mirrors `caddy/app.go` from the Go source.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use larva_php::errors::Result;
use larva_php::options::WorkerOption;
use larva_php::server::Server;

/// Global LarvaPHP configuration (the "frankenphp" directive in Caddyfile).
pub struct LarvaPHPApp {
    /// Number of PHP threads for non-worker requests.
    pub num_threads: i32,
    /// Maximum threads (including workers). Negative = auto.
    pub max_threads: i32,
    /// Worker configurations.
    pub workers: Vec<WorkerConfig>,
    /// PHP ini overrides.
    pub php_ini: std::collections::HashMap<String, String>,
    /// Max wait time for a thread.
    pub max_wait_time: std::time::Duration,
    /// Max idle time for auto-scaled threads.
    pub max_idle_time: std::time::Duration,
    /// Max requests per thread (0 = unlimited).
    pub max_requests: i32,

    // Internal state
    pub(crate) opts: RwLock<Vec<Box<dyn larva_php::options::Option>>>,
    pub(crate) provision_opts: RwLock<Vec<Box<dyn larva_php::options::Option>>>,
    pub(crate) modules: RwLock<Vec<Arc<LarvaPHPModule>>>,
    pub(crate) has_started: AtomicBool,
    pub(crate) started: tokio::sync::watch::Sender<Option<()>>,
}

impl LarvaPHPApp {
    pub fn new() -> Self {
        let (tx, _rx) = tokio::sync::watch::channel(None);
        LarvaPHPApp {
            num_threads: 0,
            max_threads: 0,
            workers: Vec::new(),
            php_ini: std::collections::HashMap::new(),
            max_wait_time: std::time::Duration::from_secs(0),
            max_idle_time: std::time::Duration::from_secs(5),
            max_requests: 0,
            opts: RwLock::new(Vec::new()),
            provision_opts: RwLock::new(Vec::new()),
            modules: RwLock::new(Vec::new()),
            has_started: AtomicBool::new(false),
            started: tx,
        }
    }

    /// Collect all options for initialization.
    pub fn collect_options(&self) -> Result<Vec<Box<dyn larva_php::options::Option>>> {
        let mut opts: Vec<Box<dyn larva_php::options::Option>> = Vec::new();

        // Add provisioned options
        opts.extend(self.provision_opts.read().unwrap().iter().map(|_| {
            // Can't clone Box<dyn Option>, so we need a different approach
            // For now, return empty
            Box::new(WithNumThreads(0)) as Box<dyn larva_php::options::Option>
        }));

        Ok(opts)
    }

    /// Start the LarvaPHP app.
    pub async fn start(&self) -> Result<()> {
        let opts = self.collect_options()?;

        larva_php::shutdown();
        larva_php::init(opts)?;

        self.has_started.store(true, Ordering::Release);
        let _ = self.started.send(Some(()));

        Ok(())
    }

    /// Stop the LarvaPHP app.
    pub async fn stop(&self) {
        larva_php::shutdown();
    }

    /// Add a module.
    pub fn add_module(&self, module: Arc<LarvaPHPModule>) {
        self.modules.write().unwrap().push(module);
    }
}

impl Default for LarvaPHPApp {
    fn default() -> Self {
        Self::new()
    }
}

struct WithNumThreads(i32);
impl larva_php::options::Option for WithNumThreads {
    fn apply(&self, _opt: &mut larva_php::options::Opt) -> Result<()> {
        Ok(())
    }
}

/// Worker configuration (mirrors `workerConfig`).
#[derive(Debug, Clone, Default)]
pub struct WorkerConfig {
    pub name: String,
    pub file_name: String,
    pub num: i32,
    pub max_threads: i32,
    pub env: std::collections::HashMap<String, String>,
    pub watch: Vec<String>,
    pub match_path: Vec<String>,
    pub max_consecutive_failures: Option<i32>,
}

impl WorkerConfig {
    pub fn to_worker_options(&self) -> Vec<Box<dyn WorkerOption>> {
        let mut opts: Vec<Box<dyn WorkerOption>> = Vec::new();
        opts.push(Box::new(larva_php::options::WithWorkerEnv(self.env.clone())));
        opts.push(Box::new(larva_php::options::WithWorkerWatchMode(self.watch.clone())));
        opts.push(Box::new(larva_php::options::WithWorkerMaxThreads(self.max_threads)));
        if let Some(max_failures) = self.max_consecutive_failures {
            opts.push(Box::new(larva_php::options::WithWorkerMaxFailures(max_failures)));
        }
        opts
    }
}

/// The LarvaPHP HTTP handler module (php/php_server directive).
pub struct LarvaPHPModule {
    pub root: String,
    pub split_path: Vec<String>,
    pub resolve_root_symlink: bool,
    pub env: std::collections::HashMap<String, String>,
    pub workers: Vec<WorkerConfig>,
    pub request_body_timeout: Option<std::time::Duration>,
    pub name: String,

    pub(crate) resolved_document_root: String,
    pub(crate) resolved_env: std::collections::HashMap<String, String>,
    pub(crate) server: RwLock<Option<Arc<Server>>>,
    pub(crate) app: RwLock<Option<Arc<LarvaPHPApp>>>,
}

impl LarvaPHPModule {
    pub fn new() -> Self {
        LarvaPHPModule {
            root: String::new(),
            split_path: vec![".php".to_string()],
            resolve_root_symlink: true,
            env: std::collections::HashMap::new(),
            workers: Vec::new(),
            request_body_timeout: None,
            name: String::new(),
            resolved_document_root: String::new(),
            resolved_env: std::collections::HashMap::new(),
            server: RwLock::new(None),
            app: RwLock::new(None),
        }
    }
}

impl Default for LarvaPHPModule {
    fn default() -> Self {
        Self::new()
    }
}