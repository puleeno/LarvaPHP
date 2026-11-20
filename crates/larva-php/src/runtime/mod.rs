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

use std::sync::OnceLock;

/// Global worker pool (initialized once via [`init_pool`]).
static POOL: OnceLock<WorkerPool> = OnceLock::new();

/// Initialize the global worker pool with `n` workers.
pub fn init_pool(n: usize) -> Result<(), String> {
    let pool = WorkerPool::spawn(n)?;
    POOL.set(pool)
        .map_err(|_| "worker pool already initialized".to_string())
}

/// Get the global worker pool, if initialized.
pub fn pool() -> Option<&'static WorkerPool> {
    POOL.get()
}

/// Dispatch a request to the pool and convert the result to a CGI output
/// string (headers + body), or `None` if the pool is not initialized.
pub fn try_dispatch(req: &Request) -> Option<Result<String, String>> {
    let pool = POOL.get()?;
    Some(pool.dispatch(req).map(|resp| {
        let mut out = format!("Status: {}\r\n", resp.status);
        for (k, v) in &resp.headers {
            if k.eq_ignore_ascii_case("status")
                || k.eq_ignore_ascii_case("content-length")
                || k.eq_ignore_ascii_case("connection")
            {
                continue;
            }
            out.push_str(k);
            out.push_str(": ");
            out.push_str(v);
            out.push_str("\r\n");
        }
        out.push_str("\r\n");
        let mut bytes = out.into_bytes();
        bytes.extend_from_slice(&resp.body);
        String::from_utf8_lossy(&bytes).into_owned()
    }))
}