//! Metrics collection (Prometheus-compatible).
//!
//! Mirrors `metrics.go` from the Go source.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::RwLock;
use std::time::Duration;

/// Stop reasons for worker termination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    Crash = 0,
    Restart = 1,
    BootFailure = 2,
}

/// Trait for metrics collectors.
pub trait Metrics: Send + Sync {
    fn start_worker(&self, _name: &str) {}
    fn ready_worker(&self, _name: &str) {}
    fn stop_worker(&self, _name: &str, _reason: StopReason) {}
    fn total_workers(&self, _name: &str, _num: i32) {}
    fn total_threads(&self, _num: i32) {}
    fn start_request(&self) {}
    fn stop_request(&self) {}
    fn stop_worker_request(&self, _name: &str, _duration: Duration) {}
    fn start_worker_request(&self, _name: &str) {}
    fn queued_worker_request(&self, _name: &str) {}
    fn dequeued_worker_request(&self, _name: &str) {}
    fn queued_request(&self) {}
    fn dequeued_request(&self) {}
    fn shutdown(&self) {}
}

/// Optional opcache metrics.
pub trait OpcacheMetrics: Metrics {
    fn opcache_restart(&self, _reason: &str) {}
}

/// Null metrics implementation (no-op).
pub struct NullMetrics;

impl Metrics for NullMetrics {}

/// Simple atomic metrics implementation.
#[allow(dead_code)]
pub struct AtomicMetrics {
    total_threads: AtomicI64,
    busy_threads: AtomicI64,
    queue_depth: AtomicI64,
    total_workers: RwLock<Vec<(String, AtomicI64)>>,
    ready_workers: RwLock<Vec<(String, AtomicI64)>>,
    busy_workers: RwLock<Vec<(String, AtomicI64)>>,
    worker_crashes: RwLock<Vec<(String, AtomicI64)>>,
    worker_restarts: RwLock<Vec<(String, AtomicI64)>>,
}

impl AtomicMetrics {
    pub fn new() -> Self {
        AtomicMetrics {
            total_threads: AtomicI64::new(0),
            busy_threads: AtomicI64::new(0),
            queue_depth: AtomicI64::new(0),
            total_workers: RwLock::new(Vec::new()),
            ready_workers: RwLock::new(Vec::new()),
            busy_workers: RwLock::new(Vec::new()),
            worker_crashes: RwLock::new(Vec::new()),
            worker_restarts: RwLock::new(Vec::new()),
        }
    }
}

impl Default for AtomicMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl Metrics for AtomicMetrics {
    fn start_worker(&self, _name: &str) {
        self.busy_threads.fetch_add(1, Ordering::Relaxed);
    }

    fn ready_worker(&self, _name: &str) {}

    fn stop_worker(&self, _name: &str, _reason: StopReason) {
        self.busy_threads.fetch_sub(1, Ordering::Relaxed);
    }

    fn total_threads(&self, num: i32) {
        self.total_threads.store(num as i64, Ordering::Relaxed);
    }

    fn start_request(&self) {
        self.busy_threads.fetch_add(1, Ordering::Relaxed);
    }

    fn stop_request(&self) {
        self.busy_threads.fetch_sub(1, Ordering::Relaxed);
    }

    fn queued_request(&self) {
        self.queue_depth.fetch_add(1, Ordering::Relaxed);
    }

    fn dequeued_request(&self) {
        self.queue_depth.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Global metrics instance.
pub static METRICS: once_cell::sync::Lazy<RwLock<Box<dyn Metrics + Send + Sync>>> =
    once_cell::sync::Lazy::new(|| RwLock::new(Box::new(NullMetrics)));

/// Set the global metrics collector.
pub fn set_metrics(metrics: Box<dyn Metrics + Send + Sync>) {
    *METRICS.write().unwrap() = metrics;
}

/// Get metrics for internal use.
#[allow(dead_code)]
pub(crate) fn metrics() -> impl std::ops::Deref<Target = Box<dyn Metrics + Send + Sync>> {
    METRICS.read().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn null_metrics_is_noop() {
        let metrics = NullMetrics;
        metrics.start_worker("test");
        metrics.ready_worker("test");
        metrics.stop_worker("test", StopReason::Crash);
        metrics.total_workers("test", 5);
        metrics.total_threads(10);
        metrics.start_request();
        metrics.stop_request();
    }

    #[test]
    fn atomic_metrics_thread_count() {
        let metrics = AtomicMetrics::new();
        metrics.total_threads(4);
        metrics.start_request();
        metrics.start_request();
        assert_eq!(metrics.busy_threads.load(Ordering::Relaxed), 2);
        metrics.stop_request();
        assert_eq!(metrics.busy_threads.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn atomic_metrics_queue_depth() {
        let metrics = AtomicMetrics::new();
        metrics.queued_request();
        metrics.queued_request();
        metrics.dequeued_request();
        assert_eq!(metrics.queue_depth.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn stop_reason_variants() {
        assert_eq!(StopReason::Crash as u8, 0);
        assert_eq!(StopReason::Restart as u8, 1);
        assert_eq!(StopReason::BootFailure as u8, 2);
    }

    #[test]
    fn atomic_metrics_default() {
        let metrics = AtomicMetrics::default();
        assert_eq!(metrics.total_threads.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.busy_threads.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.queue_depth.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn duration_zero() {
        let d = Duration::from_secs(0);
        assert_eq!(d.as_secs(), 0);
    }

    #[test]
    fn duration_from_secs() {
        let d = Duration::from_secs(5);
        assert_eq!(d.as_secs(), 5);
    }
}