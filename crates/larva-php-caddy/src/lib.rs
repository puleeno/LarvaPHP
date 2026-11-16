//! Caddy web server module for LarvaPHP.
//!
//! Mirrors the `caddy/` directory from the Go source.
//!
//! This provides:
//! - `FrankenPHPApp` (now `LarvaPHPApp`): Global configuration
//! - `FrankenPHPModule` (now `LarvaPHPModule`): The `php`/`php_server` HTTP handler
//! - `php-server` command: Production-ready PHP server
//! - `php-cli` command: Run PHP scripts

pub mod app;
pub mod module;
pub mod servername;
pub mod workerconfig;

pub use app::*;
pub use module::*;
pub use servername::*;
pub use workerconfig::*;