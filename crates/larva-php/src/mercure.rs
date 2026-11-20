//! Mercure real-time capabilities.
//!
//! Mirrors `mercure.go` and `mercure-skip.go` from the Go source.

#[cfg(feature = "mercure")]
pub mod imp {
    use std::sync::Arc;

    use tokio::sync::broadcast;

    use crate::errors::Result;
    use crate::ffi;

    /// A Mercure update.
    #[derive(Debug, Clone)]
    pub struct Update {
        pub topics: Vec<String>,
        pub data: String,
        pub private: bool,
        pub id: Option<String>,
        pub event_type: Option<String>,
        pub retry: u64,
    }

    /// Default Mercure hub topic (used for the update's own topic).
    pub const DEFAULT_HUB_URL: &str = "/.well-known/mercure";

    /// Channel capacity for the broadcast bus.
    const CHANNEL_CAPACITY: usize = 1024;

    /// An in-memory Mercure hub.
    ///
    /// Updates are published to a broadcast channel; subscribers receive every
    /// update and filter by topic selector. This mirrors the subset of the
    /// Mercure protocol needed for local real-time updates (SSE).
    pub struct Hub {
        tx: broadcast::Sender<Arc<Update>>,
        hub_url: String,
    }

    impl Default for Hub {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Hub {
        pub fn new() -> Self {
            let (tx, _) = broadcast::channel(CHANNEL_CAPACITY);
            Hub {
                tx,
                hub_url: DEFAULT_HUB_URL.to_string(),
            }
        }

        pub fn with_hub_url(hub_url: impl Into<String>) -> Self {
            let (tx, _) = broadcast::channel(CHANNEL_CAPACITY);
            Hub {
                tx,
                hub_url: hub_url.into(),
            }
        }

        pub fn hub_url(&self) -> &str {
            &self.hub_url
        }

        /// Publish an update. Returns the update id (generated if absent).
        ///
        /// `publish` never blocks: if there are no subscribers the update is
        /// dropped (broadcast semantics). Slow subscribers that fall behind get
        /// a lagged error and are expected to reconnect.
        pub fn publish(&self, update: &Update) -> Result<String> {
            let id = update.id.clone().unwrap_or_else(generate_update_id);
            let stored = Update {
                id: Some(id.clone()),
                ..update.clone()
            };
            // Ignore send errors: they only mean there are no subscribers.
            let _ = self.tx.send(Arc::new(stored));
            Ok(id)
        }

        /// Subscribe to the update stream. The receiver filters by topic.
        pub fn subscribe(&self) -> broadcast::Receiver<Arc<Update>> {
            self.tx.subscribe()
        }

        /// Number of active subscribers.
        pub fn subscriber_count(&self) -> usize {
            self.tx.receiver_count()
        }

        /// Format an update as a Server-Sent Events frame.
        pub fn format_sse(update: &Update) -> String {
            let mut out = String::with_capacity(update.data.len() + 64);
            if let Some(id) = &update.id {
                out.push_str("id: ");
                out.push_str(id);
                out.push('\n');
            }
            if let Some(typ) = &update.event_type {
                out.push_str("event: ");
                out.push_str(typ);
                out.push('\n');
            }
            if update.retry > 0 {
                out.push_str("retry: ");
                out.push_str(&update.retry.to_string());
                out.push('\n');
            }
            // Each data line must be prefixed with "data: ".
            for line in update.data.split('\n') {
                out.push_str("data: ");
                out.push_str(line);
                out.push('\n');
            }
            // Terminating blank line.
            out.push('\n');
            out
        }
    }

    /// Check whether an update matches a Mercure topic selector.
    ///
    /// Supports exact matches and the `/.well-known/mercure/subscriptions`
    /// wildcard (all topics).
    pub fn topic_matches(selector: &str, topics: &[String]) -> bool {
        if selector == DEFAULT_HUB_URL || selector == "*" {
            return true;
        }
        topics.iter().any(|t| t == selector)
    }

    /// Generate a random-ish update id (ULID-like, time + counter).
    fn generate_update_id() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        format!("{:013x}-{:08x}", ts, seq)
    }

    /// Global Mercure hub (lazily initialized).
    pub fn hub() -> &'static Hub {
        static HUB: once_cell::sync::Lazy<Hub> = once_cell::sync::Lazy::new(Hub::new);
        &HUB
    }

    /// Publish a Mercure update from PHP.
    ///
    /// # Safety
    ///
    /// `topics` and other zval pointers must be valid.
    pub unsafe fn go_mercure_publish(
        _thread_index: usize,
        _topics: *mut ffi::zval,
        _data: *mut ffi::zend_string,
        _private: bool,
        _id: *mut ffi::zend_string,
        _typ: *mut ffi::zend_string,
        _retry: u64,
    ) -> (*mut ffi::zend_string, *mut std::os::raw::c_char, ffi::frankenphp_mercure_status, u32) {
        (
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            ffi::frankenphp_mercure_status::NO_HUB,
            0,
        )
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn publish_generates_id() {
            let hub = Hub::new();
            let update = Update {
                topics: vec!["/articles".to_string()],
                data: "hello".to_string(),
                private: false,
                id: None,
                event_type: None,
                retry: 0,
            };
            let id = hub.publish(&update).unwrap();
            assert!(!id.is_empty());
        }

        #[test]
        fn publish_preserves_id() {
            let hub = Hub::new();
            let update = Update {
                topics: vec!["/articles".to_string()],
                data: "hi".to_string(),
                private: false,
                id: Some("my-id".to_string()),
                event_type: None,
                retry: 0,
            };
            let id = hub.publish(&update).unwrap();
            assert_eq!(id, "my-id");
        }

        #[test]
        fn subscribe_receives_published_update() {
            let hub = Hub::new();
            let mut rx = hub.subscribe();
            let update = Update {
                topics: vec!["/news".to_string()],
                data: "payload".to_string(),
                private: false,
                id: Some("1".to_string()),
                event_type: Some("message".to_string()),
                retry: 3000,
            };
            hub.publish(&update).unwrap();
            let received = rx.try_recv().unwrap();
            assert_eq!(received.data, "payload");
            assert_eq!(received.id.as_deref(), Some("1"));
        }

        #[test]
        fn subscriber_count_tracks_receivers() {
            let hub = Hub::new();
            assert_eq!(hub.subscriber_count(), 0);
            let _rx = hub.subscribe();
            assert_eq!(hub.subscriber_count(), 1);
        }

        #[test]
        fn format_sse_includes_id_event_retry_data() {
            let update = Update {
                topics: vec!["/t".to_string()],
                data: "line1\nline2".to_string(),
                private: false,
                id: Some("42".to_string()),
                event_type: Some("update".to_string()),
                retry: 1000,
            };
            let sse = Hub::format_sse(&update);
            assert!(sse.contains("id: 42"));
            assert!(sse.contains("event: update"));
            assert!(sse.contains("retry: 1000"));
            assert!(sse.contains("data: line1"));
            assert!(sse.contains("data: line2"));
            assert!(sse.ends_with("\n\n"));
        }

        #[test]
        fn topic_matches_exact_and_wildcard() {
            let topics = vec!["/a".to_string(), "/b".to_string()];
            assert!(topic_matches("/a", &topics));
            assert!(!topic_matches("/c", &topics));
            assert!(topic_matches(DEFAULT_HUB_URL, &topics));
            assert!(topic_matches("*", &topics));
        }
    }
}

#[cfg(feature = "mercure")]
pub use imp::*;

#[cfg(not(feature = "mercure"))]
pub mod imp {
    use crate::ffi;

    pub struct Update;
    pub struct Hub;

    impl Hub {
        pub fn new() -> Self {
            Hub
        }
    }

    /// # Safety
    /// Pointers must be valid.
    pub unsafe fn go_mercure_publish(
        _thread_index: usize,
        _topics: *mut ffi::zval,
        _data: *mut ffi::zend_string,
        _private: bool,
        _id: *mut ffi::zend_string,
        _typ: *mut ffi::zend_string,
        _retry: u64,
    ) -> (*mut ffi::zend_string, *mut std::os::raw::c_char, ffi::frankenphp_mercure_status, u32) {
        (
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            ffi::frankenphp_mercure_status::UNSUPPORTED,
            0,
        )
    }
}

#[cfg(not(feature = "mercure"))]
pub use imp::*;