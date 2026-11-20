//! Shared-memory IPC for the master↔worker data path.
//!
//! Each worker has one shared-memory segment mapped into both the master and
//! the worker process. Requests and responses are written **in place** — the
//! worker passes a raw pointer into the Zend Engine via FFI, so there is no
//! extra copy (zero-copy).
//!
//! Layout (little-endian, `#[repr(C)]`):
//! ```text
//! [ Header (64 B) ][ Request region ][ Response region ]
//! ```
//!
//! Synchronization uses `AtomicU32` fields in the shared header. For a
//! production build these would be paired with eventfd (Linux) / socketpair
//! wakeups to avoid polling; the skeleton uses a short sleep-poll.

use std::fs::OpenOptions;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use memmap2::MmapMut;

/// Request region capacity (bytes).
pub const REQ_CAP: usize = 64 * 1024;
/// Response region capacity (bytes).
pub const RESP_CAP: usize = 4 * 1024 * 1024;
/// Header size (must be >= 64 and 8-byte aligned).
const HEADER_SIZE: usize = 64;

/// Shared-memory state machine.
pub mod state {
    pub const IDLE: u32 = 0;
    pub const REQ_READY: u32 = 1;
    pub const RESP_READY: u32 = 2;
    pub const ERROR: u32 = 3;
}

/// The 64-byte header at the start of the shared segment.
#[repr(C)]
struct Header {
    magic: u32,
    state: AtomicU32,
    req_len: AtomicU32,
    resp_len: AtomicU32,
    _pad: [u8; 48],
}

const MAGIC: u32 = 0x4C415256; // "LARV"

/// A memory-mapped shared segment for one worker.
pub struct SharedMemory {
    mmap: MmapMut,
    len: usize,
}

// The mmap is process-shared; the atomics are safe for cross-process use.
unsafe impl Send for SharedMemory {}
unsafe impl Sync for SharedMemory {}

impl SharedMemory {
    /// Total segment size.
    pub fn total_size() -> usize {
        HEADER_SIZE + REQ_CAP + RESP_CAP
    }

    fn header(&self) -> &Header {
        unsafe { &*(self.mmap.as_ptr() as *const Header) }
    }

    fn header_mut(&mut self) -> &mut Header {
        unsafe { &mut *(self.mmap.as_mut_ptr() as *mut Header) }
    }

    /// Create (or truncate) the backing file and map it. Master side.
    pub fn create(path: &Path) -> std::io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;
        file.set_len(Self::total_size() as u64)?;
        let mmap = unsafe { MmapMut::map_mut(&file)? };
        let mut shm = Self {
            mmap,
            len: Self::total_size(),
        };
        shm.reset();
        Ok(shm)
    }

    /// Open an existing segment and map it. Worker side.
    pub fn attach(path: &Path) -> std::io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        let mmap = unsafe { MmapMut::map_mut(&file)? };
        Ok(Self {
            mmap,
            len: Self::total_size(),
        })
    }

    /// Reset the header to IDLE.
    pub fn reset(&mut self) {
        let h = self.header_mut();
        h.magic = MAGIC;
        h.state.store(state::IDLE, Ordering::SeqCst);
        h.req_len.store(0, Ordering::SeqCst);
        h.resp_len.store(0, Ordering::SeqCst);
    }

    /// Current state.
    pub fn state(&self) -> u32 {
        self.header().state.load(Ordering::Acquire)
    }

    /// Set state.
    pub fn set_state(&self, s: u32) {
        self.header().state.store(s, Ordering::Release);
    }

    fn req_region(&self) -> &[u8] {
        &self.mmap[HEADER_SIZE..HEADER_SIZE + REQ_CAP]
    }

    fn req_region_mut(&mut self) -> &mut [u8] {
        &mut self.mmap[HEADER_SIZE..HEADER_SIZE + REQ_CAP]
    }

    fn resp_region(&self) -> &[u8] {
        &self.mmap[HEADER_SIZE + REQ_CAP..HEADER_SIZE + REQ_CAP + RESP_CAP]
    }

    fn resp_region_mut(&mut self) -> &mut [u8] {
        &mut self.mmap[HEADER_SIZE + REQ_CAP..HEADER_SIZE + REQ_CAP + RESP_CAP]
    }

    /// Master: write the request bytes and mark REQ_READY.
    pub fn write_request(&mut self, data: &[u8]) -> Result<(), String> {
        if data.len() > REQ_CAP {
            return Err(format!("request too large: {} > {}", data.len(), REQ_CAP));
        }
        self.req_region_mut()[..data.len()].copy_from_slice(data);
        self.header().req_len.store(data.len() as u32, Ordering::SeqCst);
        self.header().resp_len.store(0, Ordering::SeqCst);
        self.set_state(state::REQ_READY);
        Ok(())
    }

    /// Worker: read the request bytes (waits until REQ_READY).
    pub fn read_request(&self) -> Option<Vec<u8>> {
        let len = self.header().req_len.load(Ordering::Acquire) as usize;
        if len == 0 || len > REQ_CAP {
            return None;
        }
        Some(self.req_region()[..len].to_vec())
    }

    /// Worker: write the response bytes and mark RESP_READY.
    pub fn write_response(&mut self, data: &[u8]) -> Result<(), String> {
        if data.len() > RESP_CAP {
            return Err(format!("response too large: {} > {}", data.len(), RESP_CAP));
        }
        self.resp_region_mut()[..data.len()].copy_from_slice(data);
        self.header().resp_len.store(data.len() as u32, Ordering::SeqCst);
        self.set_state(state::RESP_READY);
        Ok(())
    }

    /// Master: read the response bytes (returns None until RESP_READY).
    pub fn read_response(&self) -> Option<Vec<u8>> {
        if self.state() != state::RESP_READY {
            return None;
        }
        let len = self.header().resp_len.load(Ordering::Acquire) as usize;
        if len == 0 {
            return Some(Vec::new());
        }
        if len > RESP_CAP {
            return None;
        }
        Some(self.resp_region()[..len].to_vec())
    }

    /// Size of the mapped region (for diagnostics).
    pub fn len(&self) -> usize {
        self.len
    }
}

/// A handle the master holds for one worker's shared memory.
#[derive(Clone)]
pub struct WorkerShm(Arc<std::sync::Mutex<SharedMemory>>);

impl WorkerShm {
    pub fn new(shm: SharedMemory) -> Self {
        Self(Arc::new(std::sync::Mutex::new(shm)))
    }

    pub fn lock(&self) -> std::sync::MutexGuard<'_, SharedMemory> {
        self.0.lock().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_shm(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("larva-shm-test-{}-{}", std::process::id(), name))
    }

    #[test]
    fn shared_memory_write_read_roundtrip() {
        let path = temp_shm("roundtrip");
        let mut shm = SharedMemory::create(&path).unwrap();
        assert_eq!(shm.state(), state::IDLE);

        shm.write_request(b"GET / HTTP/1.1\r\n\r\n").unwrap();
        assert_eq!(shm.state(), state::REQ_READY);
        assert_eq!(shm.read_request().unwrap(), b"GET / HTTP/1.1\r\n\r\n");

        shm.set_state(state::IDLE);
        shm.write_response(b"HTTP/1.1 200 OK\r\n\r\nhello").unwrap();
        assert_eq!(shm.state(), state::RESP_READY);
        assert_eq!(shm.read_response().unwrap(), b"HTTP/1.1 200 OK\r\n\r\nhello");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn shared_memory_oversize_request_rejected() {
        let path = temp_shm("oversize");
        let mut shm = SharedMemory::create(&path).unwrap();
        let big = vec![0u8; REQ_CAP + 1];
        assert!(shm.write_request(&big).is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn shared_memory_attach_sees_same_data() {
        let path = temp_shm("attach");
        let mut writer = SharedMemory::create(&path).unwrap();
        writer.write_request(b"ping").unwrap();

        // A second mapping (simulating the worker process) sees the same bytes.
        let reader = SharedMemory::attach(&path).unwrap();
        assert_eq!(reader.state(), state::REQ_READY);
        assert_eq!(reader.read_request().unwrap(), b"ping");

        let _ = std::fs::remove_file(&path);
    }
}