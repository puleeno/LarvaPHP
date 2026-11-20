//! File watching and hot reload.
//!
//! Mirrors `watcher.go`, `watcher-skip.go`, and `internal/watcher/` from the Go source.
//!
//! Implements cross-platform file system watching using the `notify` crate,
//! with debounce coalescing and atomic config swaps via `ArcSwap`.

#[cfg(feature = "watcher")]
pub mod imp {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, RwLock};
    use std::time::Duration;

    use arc_swap::ArcSwap;
    use dashmap::DashMap;
    use notify::{Config as NotifyConfig, Event, RecommendedWatcher, RecursiveMode, Watcher};
    use tokio::sync::mpsc;

    /// Debounce duration before triggering reload.
    const DEBOUNCE_DURATION: Duration = Duration::from_millis(150);

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

    /// A pattern group for file watching.
    pub struct PatternGroup {
        pub patterns: Vec<String>,
        pub callback: Box<dyn Fn(&[FileEvent]) + Send + Sync>,
    }

    impl PatternGroup {
        pub fn new(patterns: Vec<String>, callback: Box<dyn Fn(&[FileEvent]) + Send + Sync>) -> Self {
            Self { patterns, callback }
        }
    }

    /// Internal mapping from watched directory -> set of pattern groups.
    #[derive(Default)]
    struct WatcherGroups {
        groups: DashMap<PathBuf, Vec<Arc<PatternGroup>>>,
    }

    impl WatcherGroups {
        fn add(&self, dir: PathBuf, group: Arc<PatternGroup>) {
            self.groups.entry(dir).or_default().push(group);
        }

        fn get(&self, dir: &Path) -> Option<Vec<Arc<PatternGroup>>> {
            self.groups.get(dir).map(|v| v.clone())
        }
    }

    /// Global watcher state with hot-reload support.
    pub struct WatcherState {
        pub is_active: AtomicBool,
        pub shutdown: AtomicBool,
        /// Config hot-swap slot (ArcSwap for lock-free reads).
        pub config: ArcSwap<Option<Arc<crate::config::ServerConfig>>>,
        groups: RwLock<WatcherGroups>,
    }

    impl WatcherState {
        pub fn new() -> Self {
            WatcherState {
                is_active: AtomicBool::new(false),
                shutdown: AtomicBool::new(false),
                config: ArcSwap::new(Arc::new(None)),
                groups: RwLock::new(WatcherGroups::default()),
            }
        }

        /// Set the current config (hot-swap safe).
        pub fn set_config(&self, config: Option<Arc<crate::config::ServerConfig>>) {
            self.config.store(Arc::new(config));
        }

        /// Get the current config (lock-free read).
        pub fn get_config(&self) -> Arc<Option<Arc<crate::config::ServerConfig>>> {
            self.config.load_full()
        }

        /// Initialize the watcher with pattern groups.
        pub fn init(&self, groups: Vec<Arc<PatternGroup>>) -> Result<(), String> {
            if groups.is_empty() {
                return Ok(());
            }
            if self.is_active.swap(true, Ordering::AcqRel) {
                return Err("watcher is already running".to_string());
            }
            self.shutdown.store(false, Ordering::Release);
            *self.groups.write().unwrap() = WatcherGroups::default();
            Ok(())
        }

        /// Register a pattern group to watch a directory.
        pub fn register_pattern(&self, dir: PathBuf, group: Arc<PatternGroup>) {
            self.groups.write().unwrap().add(dir, group);
        }

        /// Signal shutdown.
        pub fn shutdown(&self) {
            self.shutdown.store(true, Ordering::Release);
        }

        /// Drain/stop the watcher.
        pub fn drain(&self) {
            self.shutdown();
            self.is_active.store(false, Ordering::Release);
        }

        /// Check if shutdown was requested.
        pub fn is_shutdown(&self) -> bool {
            self.shutdown.load(Ordering::Acquire)
        }

        /// Get all registered groups for a given path (longest prefix match).
        pub fn matching_groups(&self, path: &Path) -> Vec<Arc<PatternGroup>> {
            let groups = self.groups.read().unwrap();
            let mut best: Option<(usize, Vec<Arc<PatternGroup>>)> = None;
            for entry in groups.groups.iter() {
                let dir = entry.key();
                if path.starts_with(dir) {
                    let len = dir.as_os_str().len();
                    let candidate = entry.value().clone();
                    match best {
                        Some((cur_len, _)) if len > cur_len => best = Some((len, candidate)),
                        None => best = Some((len, candidate)),
                        _ => {}
                    }
                }
            }
            best.map(|(_, g)| g).unwrap_or_default()
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

    /// Spawn the file watching task.
    ///
    /// Receives raw notify events, debounces them, and dispatches to matching
    /// pattern group callbacks. Runs until `shutdown` flag is set or channel
    /// is closed.
    pub async fn run_watcher(
        mut rx: mpsc::UnboundedReceiver<notify::Event>,
        shutdown: Arc<AtomicBool>,
    ) {
        let mut pending: HashMap<PathBuf, Vec<FileEvent>> = HashMap::new();
        let mut debounce_timer: Option<tokio::time::Instant> = None;

        loop {
            tokio::select! {
                biased;
                Some(event) = rx.recv() => {
                    if shutdown.load(Ordering::Acquire) {
                        break;
                    }
                    let path = match event.paths.iter().next() {
                        Some(p) => p.clone(),
                        None => continue,
                    };
                    let effect = match event.kind {
                        notify::event::EventKind::Create(_) => FileEffect::Create,
                        notify::event::EventKind::Modify(_) => FileEffect::Update,
                        notify::event::EventKind::Remove(_) => FileEffect::Remove,
                        _ => FileEffect::Destroy,
                    };
                    let path_type = if path.is_dir() { PathType::Directory } else { PathType::File };
                    let fe = FileEvent {
                        path: path.to_string_lossy().to_string(),
                        effect_type: effect,
                        path_type,
                    };
                    pending.entry(path.parent().unwrap_or(&path).to_path_buf())
                        .or_default()
                        .push(fe);
                    debounce_timer = Some(tokio::time::Instant::now() + DEBOUNCE_DURATION);
                }
                _ = tokio::time::sleep_until(debounce_timer.unwrap_or_else(|| tokio::time::Instant::now() + DEBOUNCE_DURATION)), if debounce_timer.is_some() => {
                    if shutdown.load(Ordering::Acquire) {
                        break;
                    }
                    if pending.is_empty() {
                        debounce_timer = None;
                        continue;
                    }
                    let events = std::mem::take(&mut pending);
                    let watcher = WATCHER.read().unwrap();
                    for (dir, evts) in &events {
                        for group in watcher.matching_groups(dir) {
                            (group.callback)(evts);
                        }
                        // Worker auto-restart: restart workers whose backing
                        // script file was created or modified.
                        for evt in evts {
                            if matches!(
                                evt.effect_type,
                                FileEffect::Update | FileEffect::Create
                            ) {
                                crate::worker::restart_workers_for_file(&evt.path);
                            }
                        }
                    }
                    debounce_timer = None;
                }
                else => break,
            }
        }
    }

    /// Create a new watcher channel and spawn the background task.
    ///
    /// Returns a `(sender, shutdown_handle)` where `sender` is used to forward
    /// notify events from the OS watcher, and `shutdown_handle` (`Arc<AtomicBool>`)
    /// can be set to true to stop the watcher task.
    pub fn start_watcher_task() -> (mpsc::UnboundedSender<notify::Event>, Arc<AtomicBool>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_child = Arc::clone(&shutdown);
        tokio::spawn(async move {
            run_watcher(rx, shutdown_child).await;
        });
        (tx, shutdown)
    }

    /// Watch a directory recursively and forward events to the watcher channel.
    pub fn watch_directory(
        tx: &mpsc::UnboundedSender<notify::Event>,
        dir: &Path,
    ) -> Result<(), String> {
        let tx_clone = tx.clone();
        let mut watcher: RecommendedWatcher = Watcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    let _ = tx_clone.send(event);
                }
            },
            NotifyConfig::default(),
        )
        .map_err(|e| format!("failed to create watcher: {}", e))?;

        watcher.watch(dir, RecursiveMode::Recursive)
            .map_err(|e| format!("failed to watch directory {}: {}", dir.display(), e))?;

        // Leak the watcher intentionally - it must live for process lifetime
        // to keep receiving OS events.
        let _ = Box::into_raw(Box::new(watcher)) as *mut RecommendedWatcher;
        Ok(())
    }

    /// Parse a pattern into directory and glob components.
    pub fn parse_pattern(pattern: &str) -> Result<(String, Vec<String>), String> {
        let abs = crate::cgi::fastabs::fast_abs(pattern)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .into_owned();

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

        let pattern_parts: Vec<&str> = pattern.split(std::path::MAIN_SEPARATOR).collect();
        let file_parts: Vec<&str> = file_name.split(std::path::MAIN_SEPARATOR).collect();

        if pattern_parts.len() != file_parts.len() {
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
        pub fn set_config(&self, _config: Option<std::sync::Arc<crate::config::ServerConfig>>) {}
        pub fn get_config(&self) -> std::sync::Arc<Option<std::sync::Arc<crate::config::ServerConfig>>> {
            std::sync::Arc::new(None)
        }
        pub fn register_pattern(&self, _dir: std::path::PathBuf, _group: std::sync::Arc<PatternGroup>) {}
        pub fn matching_groups(&self, _path: &std::path::Path) -> Vec<std::sync::Arc<PatternGroup>> {
            Vec::new()
        }
        pub fn shutdown(&self) {}
        pub fn is_shutdown(&self) -> bool {
            false
        }
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
