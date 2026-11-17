//! File watching and hot reload.
//!
//! Mirrors `watcher.go`, `watcher-skip.go`, and `internal/watcher/` from the Go source.

#[cfg(feature = "watcher")]
pub mod imp {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, RwLock};
    use std::time::Duration;

    /// Debounce duration before triggering reload.
    const DEBOUNCE_DURATION: Duration = Duration::from_millis(150);

    /// A pattern group for file watching.
    pub struct PatternGroup {
        pub patterns: Vec<String>,
        pub callback: Box<dyn Fn(&[FileEvent]) + Send + Sync>,
    }

    /// A file system event.
    #[derive(Debug, Clone)]
    pub struct FileEvent {
        pub path: String,
        pub effect_type: FileEffect,
        pub path_type: PathType,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum FileEffect {
        Create = 0,
        Update = 1,
        Remove = 2,
        Rename = 3,
        Destroy = 4,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum PathType {
        File = 0,
        Directory = 1,
        Watcher = 2,
        HardLink = 3,
    }

    /// Global watcher state.
    pub struct WatcherState {
        pub is_active: AtomicBool,
        groups: RwLock<Vec<Arc<PatternGroup>>>,
    }

    impl WatcherState {
        pub fn new() -> Self {
            WatcherState {
                is_active: AtomicBool::new(false),
                groups: RwLock::new(Vec::new()),
            }
        }

        /// Initialize the watcher.
        pub fn init(&self, groups: Vec<Arc<PatternGroup>>) -> Result<(), String> {
            if groups.is_empty() {
                return Ok(());
            }
            if self.is_active.swap(true, Ordering::AcqRel) {
                return Err("watcher is already running".to_string());
            }
            *self.groups.write().unwrap() = groups;
            // TODO: spawn file watching tasks
            Ok(())
        }

        /// Drain/stop the watcher.
        pub fn drain(&self) {
            if !self.is_active.load(Ordering::Acquire) {
                return;
            }
            self.is_active.store(false, Ordering::Release);
        }
    }

    impl Default for WatcherState {
        fn default() -> Self {
            Self::new()
        }
    }

    /// Global watcher instance.
    pub static WATCHER: once_cell::sync::Lazy<RwLock<WatcherState>> =
        once_cell::sync::Lazy::new(|| RwLock::new(WatcherState::new()));

    /// Parse a pattern into directory and glob components.
    pub fn parse_pattern(pattern: &str) -> Result<(String, Vec<String>), String> {
        let abs = crate::cgi::fastabs::fast_abs(pattern)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .into_owned();

        // Simple split: find the first glob character
        let parts: Vec<&str> = abs.split(std::path::MAIN_SEPARATOR).collect();
        let mut dir_parts = Vec::new();
        let mut pattern_parts = Vec::new();
        let mut found_glob = false;

        for part in parts {
            if !found_glob && (part.contains('*') || part.contains('?') || part.contains('{') || part.contains('.')) {
                found_glob = true;
            }
            if found_glob {
                pattern_parts.push(part.to_string());
            } else {
                dir_parts.push(part.to_string());
            }
        }

        let dir = dir_parts.join(&std::path::MAIN_SEPARATOR.to_string());
        let pattern_without_dir = pattern_parts.join(&std::path::MAIN_SEPARATOR.to_string());

        // Split by ** for recursive patterns
        let recursive: Vec<String> = pattern_without_dir
            .split("**")
            .map(|s| s.trim_matches(std::path::MAIN_SEPARATOR).to_string())
            .collect();

        Ok((dir, recursive))
    }

    /// Check if a file name matches a pattern.
    pub fn match_pattern(pattern: &str, file_name: &str) -> bool {
        if pattern.is_empty() {
            return true;
        }

        // Simple glob matching
        let pattern_parts: Vec<&str> = pattern.split(std::path::MAIN_SEPARATOR).collect();
        let file_parts: Vec<&str> = file_name.split(std::path::MAIN_SEPARATOR).collect();

        if pattern_parts.len() != file_parts.len() {
            // Try recursive matching
            return match_recursive(&pattern_parts, &file_parts);
        }

        for (p, f) in pattern_parts.iter().zip(file_parts.iter()) {
            if !match_part(p, f) {
                return false;
            }
        }

        true
    }

    fn match_recursive(pattern: &[&str], file: &[&str]) -> bool {
        if pattern.is_empty() {
            return file.is_empty();
        }

        if pattern[0] == "**" {
            // Match zero or more directories
            for i in 0..=file.len() {
                if match_recursive(&pattern[1..], &file[i..]) {
                    return true;
                }
            }
            false
        } else if !file.is_empty() && match_part(pattern[0], file[0]) {
            match_recursive(&pattern[1..], &file[1..])
        } else {
            false
        }
    }

    fn match_part(pattern: &str, part: &str) -> bool {
        if pattern == "*" {
            return true;
        }
        if pattern == part {
            return true;
        }

        // Simple wildcard matching
        let mut pi = 0;
        let mut si = 0;
        let mut star: Option<usize> = None;
        let mut match_idx = 0;

        let pbytes = pattern.as_bytes();
        let sbytes = part.as_bytes();

        while si < sbytes.len() {
            if pi < pbytes.len() && (pbytes[pi] == sbytes[si] || pbytes[pi] == b'?') {
                pi += 1;
                si += 1;
            } else if pi < pbytes.len() && pbytes[pi] == b'*' {
                star = Some(pi);
                match_idx = si;
                pi += 1;
            } else if let Some(star_pi) = star {
                pi = star_pi + 1;
                match_idx += 1;
                si = match_idx;
            } else {
                return false;
            }
        }

        while pi < pbytes.len() && pbytes[pi] == b'*' {
            pi += 1;
        }

        pi == pbytes.len()
    }
}

#[cfg(feature = "watcher")]
pub use imp::*;

#[cfg(not(feature = "watcher"))]
pub mod imp {
    pub struct PatternGroup {
        pub patterns: Vec<String>,
        pub callback: Box<dyn Fn(&[FileEvent]) + Send + Sync>,
    }

    #[derive(Debug, Clone)]
    pub struct FileEvent;

    pub struct WatcherState;

    impl WatcherState {
        pub fn new() -> Self {
            WatcherState
        }
        pub fn init(&self, _groups: Vec<std::sync::Arc<PatternGroup>>) -> Result<(), String> {
            Err("watcher support is not enabled".to_string())
        }
        pub fn drain(&self) {}
    }

    impl Default for WatcherState {
        fn default() -> Self {
            Self::new()
        }
    }

    pub static WATCHER: once_cell::sync::Lazy<std::sync::RwLock<WatcherState>> =
        once_cell::sync::Lazy::new(|| std::sync::RwLock::new(WatcherState::new()));
}

#[cfg(not(feature = "watcher"))]
pub use imp::*;