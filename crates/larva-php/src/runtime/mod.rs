//! LiteSpeed-style 2-layer runtime: master HTTP server + worker process pool.
//!
//! The master (Tokio/Hyper) handles HTTP/2, HTTP/3, TLS, static files, and
//! load balancing. It never runs PHP. Worker child processes embed PHP (FFI)
//! and are isolated: a segfault in one worker is contained and respawned.
//!
//! See `docs/runtime-architecture.md` for the full design.

pub mod executor;
pub mod ipc;
pub mod master;
pub mod worker;

pub use executor::{default_executor, PhpExecutor, Request, Response, MockExecutor, FfiExecutor};
pub use ipc::{SharedMemory, WorkerShm};
pub use master::WorkerPool;
pub use worker::run_worker;