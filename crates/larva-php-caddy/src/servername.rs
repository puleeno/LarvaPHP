//! Server name handling for Caddy.
//!
//! Mirrors `caddy/servername.go` from the Go source.

use std::sync::{Arc, RwLock};

/// A mapping from server name to LarvaPHP module.
#[derive(Clone, Debug)]
pub struct ServerName {
    pub name: String,
    pub module: Arc<crate::LarvaPHPModule>,
}

/// Registry of server names.
pub struct ServerNameRegistry {
    names: RwLock<Vec<ServerName>>,
}

impl ServerNameRegistry {
    pub fn new() -> Self {
        ServerNameRegistry {
            names: RwLock::new(Vec::new()),
        }
    }

    /// Register a server name.
    pub fn register(&self, name: &str, module: Arc<crate::LarvaPHPModule>) {
        let mut names = self.names.write().unwrap();
        names.push(ServerName {
            name: name.to_string(),
            module,
        });
    }

    /// Find a module by server name.
    pub fn find(&self, name: &str) -> Option<Arc<crate::LarvaPHPModule>> {
        let names = self.names.read().unwrap();
        names
            .iter()
            .find(|n| n.name == name)
            .map(|n| n.module.clone())
    }
}

impl Default for ServerNameRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub static SERVER_NAMES: once_cell::sync::Lazy<RwLock<ServerNameRegistry>> =
    once_cell::sync::Lazy::new(|| RwLock::new(ServerNameRegistry::new()));

/// Register a server name.
pub fn register_server_name(name: &str, module: Arc<crate::LarvaPHPModule>) {
    SERVER_NAMES.read().unwrap().register(name, module);
}

/// Find a module by server name.
pub fn find_server_name(name: &str) -> Option<Arc<crate::LarvaPHPModule>> {
    SERVER_NAMES.read().unwrap().find(name)
}