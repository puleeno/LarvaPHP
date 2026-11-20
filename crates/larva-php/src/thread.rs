//! PHP thread management and state machine.
//!
//! Mirrors `phpthread.go`, `phpmainthread.go`, `threadregular.go`,
//! `threadworker.go`, `threadinactive.go`, and `internal/state/state.go`.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, Instant};

use crate::errors::{Error, Result};
use crate::ffi;

/// Thread state (mirrors internal/state/state.go).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ThreadState {
    Reserved = 0,
    Booting = 1,
    BootRequested = 2,
    ShuttingDown = 3,
    Done = 4,
    Inactive = 5,
    Ready = 6,
    TransitionRequested = 7,
    TransitionInProgress = 8,
    TransitionComplete = 9,
    Rebooting = 10,
    ForceRebooting = 11,
    RebootReady = 12,
    YieldingForReboot = 13,
}

impl ThreadState {
    pub fn name(&self) -> &'static str {
        match self {
            ThreadState::Reserved => "reserved",
            ThreadState::Booting => "booted",
            ThreadState::BootRequested => "boot requested",
            ThreadState::ShuttingDown => "shutting down",
            ThreadState::Done => "done",
            ThreadState::Inactive => "inactive",
            ThreadState::Ready => "ready",
            ThreadState::TransitionRequested => "transition requested",
            ThreadState::TransitionInProgress => "transition in progress",
            ThreadState::TransitionComplete => "transition complete",
            ThreadState::Rebooting => "rebooting",
            ThreadState::ForceRebooting => "rebooting (force)",
            ThreadState::RebootReady => "reboot ready",
            ThreadState::YieldingForReboot => "yielding for reboot",
        }
    }

    pub fn is_stable(&self) -> bool {
        matches!(self, ThreadState::Inactive | ThreadState::Ready)
    }
}

/// Thread-safe state container (mirrors ThreadState in state.go).
pub struct ThreadStateContainer {
    inner: Mutex<ThreadStateInner>,
    condvar: Condvar,
}

struct ThreadStateInner {
    current: ThreadState,
    subscribers: Vec<Subscriber>,
    waiting_since: AtomicI64,
}

struct Subscriber {
    states: Vec<ThreadState>,
    notified: bool,
}

impl ThreadStateContainer {
    pub fn new() -> Self {
        ThreadStateContainer {
            inner: Mutex::new(ThreadStateInner {
                current: ThreadState::Reserved,
                subscribers: Vec::new(),
                waiting_since: AtomicI64::new(0),
            }),
            condvar: Condvar::new(),
        }
    }

    pub fn get(&self) -> ThreadState {
        self.inner.lock().unwrap().current
    }

    pub fn is(&self, state: ThreadState) -> bool {
        self.get() == state
    }

    pub fn set(&self, new_state: ThreadState) {
        let mut inner = self.inner.lock().unwrap();
        inner.current = new_state;
        inner.notify_subscribers(new_state);
        self.condvar.notify_all();
    }

    pub fn compare_and_swap(&self, cmp: ThreadState, new: ThreadState) -> bool {
        let mut inner = self.inner.lock().unwrap();
        if inner.current == cmp {
            inner.current = new;
            inner.notify_subscribers(new);
            self.condvar.notify_all();
            true
        } else {
            false
        }
    }

    pub fn wait_for(&self, states: &[ThreadState]) {
        let mut inner = self.inner.lock().unwrap();
        while !states.contains(&inner.current) {
            inner.subscribers.push(Subscriber {
                states: states.to_vec(),
                notified: false,
            });
            inner = self.condvar.wait(inner).unwrap();
            // Clean up notified subscribers
            inner.subscribers.retain(|s| !s.notified);
        }
    }

    pub fn wait_for_timeout(&self, timeout: Duration, states: &[ThreadState]) -> bool {
        let deadline = Instant::now() + timeout;
        let mut inner = self.inner.lock().unwrap();
        while !states.contains(&inner.current) {
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            inner.subscribers.push(Subscriber {
                states: states.to_vec(),
                notified: false,
            });
            let (guard, result) = self.condvar.wait_timeout(inner, deadline - now).unwrap();
            inner = guard;
            inner.subscribers.retain(|s| !s.notified);
            if result.timed_out() && !states.contains(&inner.current) {
                return false;
            }
        }
        true
    }

    pub fn request_safe_state_change(&self, next: ThreadState) -> bool {
        let mut inner = self.inner.lock().unwrap();
        match inner.current {
            ThreadState::Reserved | ThreadState::ShuttingDown | ThreadState::Done => false,
            ThreadState::Ready | ThreadState::Inactive => {
                inner.current = next;
                inner.notify_subscribers(next);
                self.condvar.notify_all();
                true
            }
            _ => {
                // Wait for a stable state
                let states = [ThreadState::Ready, ThreadState::Inactive, ThreadState::Reserved, ThreadState::Done];
                while !states.contains(&inner.current) {
                    inner = self.condvar.wait(inner).unwrap();
                }
                drop(inner);
                self.request_safe_state_change(next)
            }
        }
    }

    pub fn mark_as_waiting(&self, is_waiting: bool) {
        let inner = self.inner.lock().unwrap();
        if is_waiting {
            inner.waiting_since.store(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as i64,
                Ordering::Release,
            );
        } else {
            inner.waiting_since.store(0, Ordering::Release);
        }
    }

    pub fn is_in_waiting_state(&self) -> bool {
        self.inner.lock().unwrap().waiting_since.load(Ordering::Acquire) != 0
    }

    pub fn wait_time(&self) -> i64 {
        let inner = self.inner.lock().unwrap();
        let since = inner.waiting_since.load(Ordering::Acquire);
        if since == 0 {
            return 0;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        now - since
    }
}

impl ThreadStateInner {
    fn notify_subscribers(&mut self, state: ThreadState) {
        for sub in &mut self.subscribers {
            if sub.states.contains(&state) {
                sub.notified = true;
            }
        }
    }
}

/// A PHP thread.
///
/// This represents the Rust-side management of a PHP thread.
/// The actual PHP thread runs in C (via `frankenphp_new_php_thread`).
pub struct PhpThread {
    pub thread_index: usize,
    pub state: Arc<ThreadStateContainer>,
    pub request_count: AtomicI64,
    pub request_chan: crossbeam::channel::Sender<Arc<crate::context::PhpContext>>,
    pub drain_chan: RwLock<crossbeam::channel::Sender<()>>,
    handler: RwLock<Box<dyn ThreadHandler>>,
}

/// Trait for thread handlers (mirrors the `threadHandler` interface).
pub trait ThreadHandler: Send + Sync {
    fn name(&self) -> &str;
    fn before_script_execution(&self) -> String;
    fn after_script_execution(&self, exit_status: i32);
    fn franken_php_context(&self) -> Option<Arc<crate::context::PhpContext>>;
    fn drain(&self) {}
}

impl PhpThread {
    pub fn new(thread_index: usize) -> Self {
        PhpThread {
            thread_index,
            state: Arc::new(ThreadStateContainer::new()),
            request_count: AtomicI64::new(0),
            request_chan: crossbeam::channel::unbounded().0,
            drain_chan: RwLock::new(crossbeam::channel::unbounded().0),
            handler: RwLock::new(Box::new(InactiveThread)),
        }
    }

    /// Boot the underlying C thread.
    pub fn boot(&self) -> Result<()> {
        if !self.state.compare_and_swap(ThreadState::Reserved, ThreadState::Booting)
            && !self.state.compare_and_swap(ThreadState::BootRequested, ThreadState::Booting)
        {
            return Err(Error::Thread("thread is not in reserved state".into()));
        }

        // Set initial handler as inactive
        *self.handler.write().unwrap() = Box::new(InactiveThread);

        // Start the actual C thread
        let idx = self.thread_index;
        let success = unsafe { ffi::frankenphp_new_php_thread(idx) };
        if !success {
            self.state.set(ThreadState::Reserved);
            return Err(Error::ThreadCreation);
        }

        self.state.wait_for(&[ThreadState::Inactive]);
        Ok(())
    }

    /// Shutdown the thread.
    pub fn shutdown(&self) {
        if !self.state.request_safe_state_change(ThreadState::ShuttingDown) {
            self.state.wait_for(&[ThreadState::Done, ThreadState::Reserved]);
            let _ = self.state.compare_and_swap(ThreadState::Done, ThreadState::Reserved);
            return;
        }

        // Close drain channel
        let _ = self.drain_chan.read().unwrap().send(());

        self.state.wait_for(&[ThreadState::Done]);
        self.state.set(ThreadState::Reserved);
    }

    /// Reboot the thread.
    pub fn reboot(&self) -> bool {
        if !self.state.compare_and_swap(ThreadState::Ready, ThreadState::Rebooting) {
            return false;
        }
        // TODO: start rebooted thread
        true
    }

    /// Force reboot the thread.
    pub fn force_reboot(&self) -> bool {
        if !self.state.request_safe_state_change(ThreadState::ForceRebooting) {
            return false;
        }
        true
    }

    /// Set the thread handler.
    pub fn set_handler(&self, handler: Box<dyn ThreadHandler>) {
        if !self.state.request_safe_state_change(ThreadState::TransitionRequested) {
            return;
        }
        *self.handler.write().unwrap() = handler;
        self.state.set(ThreadState::TransitionComplete);
    }

    /// Get the current handler name.
    pub fn name(&self) -> String {
        self.handler.read().unwrap().name().to_string()
    }
}

/// Inactive thread handler (no work assigned).
struct InactiveThread;

impl ThreadHandler for InactiveThread {
    fn name(&self) -> &str {
        "Inactive PHP Thread"
    }
    fn before_script_execution(&self) -> String {
        String::new()
    }
    fn after_script_execution(&self, _exit_status: i32) {}
    fn franken_php_context(&self) -> Option<Arc<crate::context::PhpContext>> {
        None
    }
}

/// Regular thread handler (executes PHP scripts in web context).
pub struct RegularThread {
    pub fc: RwLock<Option<Arc<crate::context::PhpContext>>>,
    pub request_count: i32,
}

impl ThreadHandler for RegularThread {
    fn name(&self) -> &str {
        "Regular PHP Thread"
    }
    fn before_script_execution(&self) -> String {
        // TODO: implement wait_for_request logic
        String::new()
    }
    fn after_script_execution(&self, _exit_status: i32) {
        // TODO: implement after_request logic
    }
    fn franken_php_context(&self) -> Option<Arc<crate::context::PhpContext>> {
        self.fc.read().unwrap().clone()
    }
}

/// Worker thread handler (executes a worker script in a loop).
pub struct WorkerThread {
    pub worker: Arc<crate::worker::Worker>,
    pub dummy_fc: RwLock<Option<Arc<crate::context::PhpContext>>>,
    pub worker_fc: RwLock<Option<Arc<crate::context::PhpContext>>>,
    pub is_booting_script: AtomicBool,
    pub failure_count: AtomicI32,
    pub request_count: AtomicI32,
}

impl ThreadHandler for WorkerThread {
    fn name(&self) -> &str {
        "Worker PHP Thread"
    }
    fn before_script_execution(&self) -> String {
        // TODO: implement worker setup logic
        self.worker.file_name().to_string()
    }
    fn after_script_execution(&self, _exit_status: i32) {
        // TODO: implement teardown logic
    }
    fn franken_php_context(&self) -> Option<Arc<crate::context::PhpContext>> {
        self.worker_fc
            .read()
            .unwrap()
            .clone()
            .or_else(|| self.dummy_fc.read().unwrap().clone())
    }
}

/// Main PHP thread manager.
pub struct PhpMainThread {
    pub state: Arc<ThreadStateContainer>,
    pub num_threads: i32,
    pub max_threads: i32,
    pub php_ini: RwLock<std::collections::HashMap<String, String>>,
    pub is_rebooting: AtomicBool,
    done: crossbeam::channel::Sender<()>,
}

impl PhpMainThread {
    pub fn new(num_threads: i32, max_threads: i32) -> Self {
        PhpMainThread {
            state: Arc::new(ThreadStateContainer::new()),
            num_threads,
            max_threads,
            php_ini: RwLock::new(std::collections::HashMap::new()),
            is_rebooting: AtomicBool::new(false),
            done: crossbeam::channel::unbounded().0,
        }
    }

    /// Start the main PHP thread.
    pub fn start(&self) -> Result<()> {
        let num = self.num_threads;
        let result = unsafe { ffi::frankenphp_new_main_thread(num as i32) };
        if result != 0 {
            return Err(Error::MainThreadCreation);
        }
        self.state.wait_for(&[ThreadState::Ready]);
        Ok(())
    }

    /// Reboot all PHP threads.
    pub fn reboot_all_threads(&self) -> bool {
        if self.is_rebooting.swap(true, Ordering::AcqRel) {
            return false;
        }
        // TODO: implement reboot logic
        self.is_rebooting.store(false, Ordering::Release);
        true
    }
}

/// Global PHP thread registry.
pub struct PhpThreadRegistry {
    pub threads: RwLock<Vec<Option<Arc<PhpThread>>>>,
    pub main_thread: RwLock<Option<Arc<PhpMainThread>>>,
    pub is_running: AtomicBool,
    pub max_requests_per_thread: AtomicI32,
    pub max_wait_time: AtomicI64,
    pub hot_reload_enabled: AtomicBool,
}

impl PhpThreadRegistry {
    pub fn new() -> Self {
        PhpThreadRegistry {
            threads: RwLock::new(Vec::new()),
            main_thread: RwLock::new(None),
            is_running: AtomicBool::new(false),
            max_requests_per_thread: AtomicI32::new(0),
            max_wait_time: AtomicI64::new(0),
            hot_reload_enabled: AtomicBool::new(false),
        }
    }
}

impl Default for PhpThreadRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub static REGISTRY: once_cell::sync::Lazy<RwLock<PhpThreadRegistry>> =
    once_cell::sync::Lazy::new(|| RwLock::new(PhpThreadRegistry::new()));

/// Initialize PHP threads.
pub fn init(options: Vec<Box<dyn crate::options::Option>>) -> Result<()> {
    let registry = REGISTRY.write().unwrap();
    if registry.is_running.swap(true, Ordering::AcqRel) {
        return Err(Error::AlreadyStarted);
    }
    drop(registry);

    // Apply options
    let mut opt = crate::options::Opt::default();
    for option in &options {
        option.apply(&mut opt)?;
    }

    // Start the main PHP thread
    let main_thread = Arc::new(PhpMainThread::new(opt.num_threads, opt.max_threads));
    main_thread.state.set(ThreadState::Booting);
    let result = unsafe { ffi::frankenphp_new_main_thread(main_thread.num_threads) };
    if result != 0 {
        main_thread.state.set(ThreadState::Reserved);
        return Err(Error::MainThreadCreation);
    }
    main_thread.state.set(ThreadState::Ready);
    *REGISTRY.write().unwrap().main_thread.write().unwrap() = Some(main_thread.clone());

    let registry = REGISTRY.write().unwrap();
    registry.is_running.store(true, Ordering::Release);
    Ok(())
}

/// Shutdown PHP threads.
pub fn shutdown() {
    let registry = REGISTRY.write().unwrap();
    if !registry.is_running.load(Ordering::Acquire) {
        return;
    }

    // Shutdown all worker threads
    for thread_opt in registry.threads.read().unwrap().iter() {
        if let Some(thread) = thread_opt {
            thread.shutdown();
        }
    }

    // Shutdown main thread
    if let Some(main_thread) = registry.main_thread.read().unwrap().clone() {
        main_thread.state.set(ThreadState::ShuttingDown);
        unsafe { ffi::frankenphp_shutdown_dummy_request() };
        main_thread.state.set(ThreadState::Done);
    }

    registry.is_running.store(false, Ordering::Release);
}

/// Validate options without starting.
pub fn validate(options: &[Box<dyn crate::options::Option>]) -> Result<()> {
    let mut opt = crate::options::Opt::default();
    for option in options {
        option.apply(&mut opt)?;
    }
    Ok(())
}

/// Get a worker by name.
pub fn get_worker_by_name(name: &str) -> Option<Arc<crate::worker::Worker>> {
    let _reg = REGISTRY.read().unwrap();
    let worker_reg = crate::worker::WORKER_REGISTRY.read().unwrap();
    let workers = worker_reg.workers_by_name.read().unwrap();
    workers.get(name).cloned()
}

// ============================================================================
// Thread inspection API
// ============================================================================

/// A point-in-time snapshot of a PHP worker thread's state.
#[derive(Debug, Clone)]
pub struct ThreadInfo {
    /// Thread slot index.
    pub index: usize,
    /// Current state.
    pub state: ThreadState,
    /// Human-readable state name.
    pub state_name: &'static str,
    /// Whether the thread is in a stable (idle/ready) state.
    pub is_stable: bool,
    /// Number of requests handled by this thread.
    pub request_count: i64,
    /// Milliseconds spent in the current wait (0 if not waiting).
    pub wait_time_ms: i64,
}

/// Overall PHP runtime status (for monitoring / health checks).
#[derive(Debug, Clone)]
pub struct ThreadStatus {
    pub is_running: bool,
    pub main_state: Option<ThreadState>,
    pub num_threads: usize,
    pub ready_threads: usize,
    pub busy_threads: usize,
}

/// Snapshot all registered PHP worker threads.
///
/// Returns an empty vector when the runtime is not running. O(n) over the
/// thread table; intended for monitoring endpoints, not the hot path.
pub fn threads() -> Vec<ThreadInfo> {
    let reg = REGISTRY.read().unwrap();
    if !reg.is_running.load(Ordering::Acquire) {
        return Vec::new();
    }
    let threads = reg.threads.read().unwrap();
    threads
        .iter()
        .enumerate()
        .filter_map(|(i, slot)| {
            slot.as_ref().map(|t| {
                let state = t.state.get();
                ThreadInfo {
                    index: i,
                    state,
                    state_name: state.name(),
                    is_stable: state.is_stable(),
                    request_count: t.request_count.load(Ordering::Relaxed),
                    wait_time_ms: t.state.wait_time(),
                }
            })
        })
        .collect()
}

/// Get the main PHP thread's current state, if the runtime is running.
pub fn main_thread_state() -> Option<ThreadState> {
    let reg = REGISTRY.read().unwrap();
    if !reg.is_running.load(Ordering::Acquire) {
        return None;
    }
    let mt = reg.main_thread.read().unwrap();
    mt.as_ref().map(|m| m.state.get())
}

/// Check whether the PHP runtime is currently running.
pub fn is_running() -> bool {
    REGISTRY.read().unwrap().is_running.load(Ordering::Acquire)
}

/// Aggregate runtime status: running flag, main state, and thread counts.
pub fn status() -> ThreadStatus {
    let reg = REGISTRY.read().unwrap();
    let running = reg.is_running.load(Ordering::Acquire);
    let main_state = if running {
        reg.main_thread.read().unwrap().as_ref().map(|mt| mt.state.get())
    } else {
        None
    };
    let (num_threads, ready_threads, busy_threads) = if running {
        let threads = reg.threads.read().unwrap();
        let total = threads.len();
        let mut ready = 0usize;
        for slot in threads.iter().flatten() {
            if slot.state.is(ThreadState::Ready) {
                ready += 1;
            }
        }
        (total, ready, total.saturating_sub(ready))
    } else {
        (0, 0, 0)
    };
    ThreadStatus {
        is_running: running,
        main_state,
        num_threads,
        ready_threads,
        busy_threads,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_state_names() {
        assert_eq!(ThreadState::Reserved.name(), "reserved");
        assert_eq!(ThreadState::Ready.name(), "ready");
        assert_eq!(ThreadState::Inactive.name(), "inactive");
        assert_eq!(ThreadState::ShuttingDown.name(), "shutting down");
        assert_eq!(ThreadState::Booting.name(), "booted");
    }

    #[test]
    fn thread_state_stability() {
        assert!(ThreadState::Ready.is_stable());
        assert!(ThreadState::Inactive.is_stable());
        assert!(!ThreadState::Reserved.is_stable());
        assert!(!ThreadState::Booting.is_stable());
    }

    #[test]
    fn status_when_not_running() {
        // The global registry is not running during unit tests.
        let status = status();
        assert!(!status.is_running);
        assert_eq!(status.num_threads, 0);
        assert_eq!(status.ready_threads, 0);
        assert!(threads().is_empty());
    }
}