//! Worker configuration for Caddy.
//!
//! Mirrors `caddy/workerconfig.go` from the Go source.

use std::sync::{Arc, RwLock};

use crate::{LarvaPHPApp, WorkerConfig};

/// Worker configuration registry.
pub struct WorkerConfigRegistry {
    configs: RwLock<Vec<Arc<WorkerConfig>>>,
}

impl WorkerConfigRegistry {
    pub fn new() -> Self {
        WorkerConfigRegistry {
            configs: RwLock::new(Vec::new()),
        }
    }

    /// Add a worker config.
    pub fn add(&self, config: Arc<WorkerConfig>) {
        self.configs.write().unwrap().push(config);
    }

    /// Get all worker configs.
    pub fn all(&self) -> Vec<Arc<WorkerConfig>> {
        self.configs.read().unwrap().clone()
    }
}

impl Default for WorkerConfigRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub static WORKER_CONFIGS: once_cell::sync::Lazy<RwLock<WorkerConfigRegistry>> =
    once_cell::sync::Lazy::new(|| RwLock::new(WorkerConfigRegistry::new()));

/// Add a worker config to the registry.
pub fn add_worker_config(config: Arc<WorkerConfig>) {
    WORKER_CONFIGS.read().unwrap().add(config);
}

/// Get all worker configs from the registry.
pub fn get_worker_configs() -> Vec<Arc<WorkerConfig>> {
    WORKER_CONFIGS.read().unwrap().all()
}

/// Build worker options from a list of configs.
pub fn build_worker_options(configs: &[Arc<WorkerConfig>]) -> Vec<Box<dyn larva_php::options::WorkerOption>> {
    let mut opts = Vec::new();
    for config in configs {
        opts.extend(config.to_worker_options());
    }
    opts
}

/// Initialize LarvaPHP with the given app config.
pub async fn init_larva_php(app: &Arc<LarvaPHPApp>) -> larva_php::errors::Result<()> {
    app.start().await
}