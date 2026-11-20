//! Master-side worker pool.
//!
//! Spawns N worker child processes (re-exec of the current binary with
//! `--worker`), each with its own shared-memory segment. Dispatches requests
//! round-robin and reads responses from shared memory.

use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::runtime::executor::{Request, Response};
use crate::runtime::ipc::{state, SharedMemory};

/// One worker: its process, shared memory, and a busy flag.
pub struct WorkerHandle {
    pub id: usize,
    pub shm: Mutex<SharedMemory>,
    pub child: Child,
    pub busy: AtomicBool,
}

/// A pool of worker processes.
pub struct WorkerPool {
    workers: Vec<WorkerHandle>,
    next: AtomicUsize,
    shm_dir: PathBuf,
}

impl WorkerPool {
    /// Spawn `n` workers.
    pub fn spawn(n: usize) -> Result<Self, String> {
        let exe = std::env::current_exe().map_err(|e| format!("current_exe: {}", e))?;
        let shm_dir = std::env::temp_dir().join(format!("larva-runtime-{}", std::process::id()));
        std::fs::create_dir_all(&shm_dir).map_err(|e| format!("create shm dir: {}", e))?;

        let mut workers = Vec::with_capacity(n);
        for i in 0..n {
            let shm_path = shm_dir.join(format!("worker-{}.shm", i));
            let shm = SharedMemory::create(&shm_path)
                .map_err(|e| format!("create shm {}: {}", shm_path.display(), e))?;

            let child = Command::new(&exe)
                .arg("--worker")
                .arg("--id")
                .arg(i.to_string())
                .arg("--shm")
                .arg(&shm_path)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .map_err(|e| format!("spawn worker {}: {}", i, e))?;

            workers.push(WorkerHandle {
                id: i,
                shm: Mutex::new(shm),
                child,
                busy: AtomicBool::new(false),
            });
        }

        tracing::info!("worker pool online: {} workers", workers.len());

        Ok(Self {
            workers,
            next: AtomicUsize::new(0),
            shm_dir,
        })
    }

    /// Dispatch a request to the next worker (round-robin) and await its response.
    pub fn dispatch(&self, req: &Request) -> Result<Response, String> {
        let n = self.workers.len();
        if n == 0 {
            return Err("no workers".to_string());
        }
        let idx = self.next.fetch_add(1, Ordering::Relaxed) % n;
        let w = &self.workers[idx];

        let req_bytes = serde_json::to_vec(req).map_err(|e| format!("encode request: {}", e))?;
        {
            let mut shm = w.shm.lock().unwrap();
            shm.write_request(&req_bytes)?;
        }

        // Wait for the worker to produce a response (poll with a short sleep).
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let resp_bytes = {
                let shm = w.shm.lock().unwrap();
                if shm.state() == state::RESP_READY {
                    shm.read_response()
                } else if shm.state() == state::ERROR {
                    return Err("worker reported an error".to_string());
                } else {
                    None
                }
            };
            if let Some(bytes) = resp_bytes {
                let resp = serde_json::from_slice::<Response>(&bytes)
                    .map_err(|e| format!("decode response: {}", e))?;
                w.shm.lock().unwrap().set_state(state::IDLE);
                return Ok(resp);
            }
            if Instant::now() > deadline {
                return Err(format!("worker {} timed out", w.id));
            }
            std::thread::sleep(Duration::from_micros(100));
        }
    }

    /// Shut down all workers.
    pub fn shutdown(&mut self) {
        for w in &mut self.workers {
            let _ = w.child.kill();
        }
        // Best-effort cleanup of the shm directory.
        let _ = std::fs::remove_dir_all(&self.shm_dir);
    }
}

impl Drop for WorkerPool {
    fn drop(&mut self) {
        self.shutdown();
    }
}