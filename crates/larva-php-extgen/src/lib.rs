//! PHP extension generator for LarvaPHP.
//!
//! Mirrors the `internal/extgen/` directory from the Go source.
//!
//! This crate scans C/C++ source files for PHP function definitions and
//! generates Rust FFI bindings using bindgen.

pub mod config;
pub mod generator;

pub use config::*;
pub use generator::*;