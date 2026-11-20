//! phpinfo() integration.
//!
//! Mirrors `phpinfo.go` from the Go source.

use std::sync::{Arc, RwLock};

/// A phpinfo entry (key, value).
#[derive(Debug, Clone)]
pub struct PhpInfoEntry {
    pub key: String,
    pub value: String,
}

/// A pinned generation of phpinfo tables.
pub struct PhpInfoTable {
    pub entries: Vec<PhpInfoEntry>,
    pub modules: Vec<PhpInfoEntry>,
    pub readers: usize,
    pub retired: bool,
}

/// Global phpinfo state.
#[allow(dead_code)]
pub struct PhpInfoState {
    entries: RwLock<Vec<PhpInfoEntry>>,
    dirty: RwLock<bool>,
    current: RwLock<Option<Arc<PhpInfoTable>>>,
    retired: RwLock<Vec<Arc<PhpInfoTable>>>,
}

impl PhpInfoState {
    pub fn new() -> Self {
        PhpInfoState {
            entries: RwLock::new(Vec::new()),
            dirty: RwLock::new(true),
            current: RwLock::new(None),
            retired: RwLock::new(Vec::new()),
        }
    }

    /// Add an entry to the frankenphp section of phpinfo().
    pub fn add_entry(&self, key: &str, value: &str) {
        let mut entries = self.entries.write().unwrap();
        entries.push(PhpInfoEntry {
            key: key.to_string(),
            value: value.to_string(),
        });
        *self.dirty.write().unwrap() = true;
    }

    /// Collect phpinfo entries.
    pub fn collect(&self) -> (Vec<PhpInfoEntry>, Vec<PhpInfoEntry>) {
        let entries = self.entries.read().unwrap().clone();
        let modules = Vec::new(); // TODO: collect module info
        (entries, modules)
    }
}

impl Default for PhpInfoState {
    fn default() -> Self {
        Self::new()
    }
}

/// Global phpinfo state.
pub static PHPINFO: once_cell::sync::Lazy<RwLock<PhpInfoState>> =
    once_cell::sync::Lazy::new(|| RwLock::new(PhpInfoState::new()));

/// Add an entry to phpinfo().
pub fn add_phpinfo_entry(key: &str, value: &str) {
    PHPINFO.read().unwrap().add_entry(key, value);
}