//! Worker process main loop.
//!
//! Each worker is a separate OS process (spawned by the master via re-exec).
//! It attaches its shared-memory segment, waits for requests, executes PHP via
//! the [`PhpExecutor`], and writes responses back — all in place in shared
//! memory (zero-copy).

use std::path::Path;
use std::time::Duration;

use crate::runtime::executor::{default_executor, Request};
use crate::runtime::ipc::{state, SharedMemory};

/// Run the worker loop. Blocks forever (until the process is killed).
pub fn run_worker(shm_path: &str) -> Result<(), String> {
    let mut shm = SharedMemory::attach(Path::new(shm_path))
        .map_err(|e| format!("attach shm {}: {}", shm_path, e))?;
    let executor = default_executor();

    tracing::info!(
        "worker online: shm={} executor={} pid={}",
        shm_path,
        executor.name(),
        std::process::id()
    );

    // Poll interval: short enough for low latency, long enough to avoid a
    // busy spin. Production would block on eventfd / a socket instead.
    let poll = Duration::from_micros(100);

    loop {
        match shm.state() {
            state::REQ_READY => {
                let result = shm
                    .read_request()
                    .ok_or_else(|| "failed to read request".to_string())
                    .and_then(|bytes| {
                        serde_json::from_slice::<Request>(&bytes)
                            .map_err(|e| format!("decode request: {}", e))
                    })
                    .and_then(|req| executor.execute(&req))
                    .and_then(|resp| {
                        serde_json::to_vec(&resp)
                            .map_err(|e| format!("encode response: {}", e))
                    })
                    .and_then(|bytes| shm.write_response(&bytes));

                match result {
                    Ok(()) => {}
                    Err(e) => {
                        tracing::error!("worker error: {}", e);
                        shm.set_state(state::ERROR);
                    }
                }
            }
            state::IDLE => {
                std::thread::sleep(poll);
            }
            _ => {
                // RESP_READY (master reading) or ERROR: wait for master.
                std::thread::sleep(poll);
            }
        }
    }
}