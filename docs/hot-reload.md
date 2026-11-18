# Hot Reload

LarvaPHP supports hot reloading of configuration and PHP source files without restarting the server. When watched files change, the configuration is re-parsed and atomically swapped into place using `ArcSwap` for lock-free reads.

## Enabling Hot Reload

Hot reload is enabled by default via the `watcher` feature:

```toml
[dependencies]
larva-php = { version = "1.13.5", features = ["watcher"] }
```

To disable:

```toml
larva-php = { version = "1.13.5", default-features = false, features = ["mercure"] }
```

## How It Works

```
┌──────────────┐     ┌──────────────┐     ┌──────────────────┐
│  File System │────▶│  notify      │────▶│  Debounce (150ms)│
│  (PHP/config)│     │  watcher     │     │  (coalescing)      │
└──────────────┘     └──────────────┘     └──────────────────┘
                                                      │
                                                      ▼
                                              ┌──────────────┐
                                              │  Re-parse    │
                                              │  config      │
                                              └──────────────┘
                                                      │
                                                      ▼
                                              ┌──────────────┐
                                              │  ArcSwap     │
                                              │  atomic swap │
                                              └──────────────┘
                                                      │
                                                      ▼
                                              ┌──────────────┐
                                              │  New config  │
                                              │  active      │
                                              └──────────────┘
```

### Key Design Decisions

1. **Debounce (150ms)**: Rapid file changes (e.g., IDE saving multiple files) are coalesced into a single reload to avoid thrashing.

2. **ArcSwap**: Config reads are lock-free. Writers atomically swap the `Arc<ServerConfig>` pointer. Readers always see a consistent config snapshot.

3. **Pattern matching**: Only files matching watched extensions (`.php`, `.htaccess`, `.conf`, `.yaml`, `.yml`, `.json`, `.toml`) trigger reloads.

4. **Longest prefix match**: When multiple directories are watched, the most specific match wins for event routing.

## Usage

### Basic Hot Reload Manager

```rust
use larva_php::config::{ConfigLoader, ServerConfigBuilder};
use larva_php::config::hot_reload::HotReloadManager;
use std::path::PathBuf;

let loader = ConfigLoader::default();
let mut mgr = HotReloadManager::new("cli", "C:/www/app", loader.clone());

// Watch the application directory
mgr.watch_dir(PathBuf::from("C:/www/app"));

// Start the background watcher task
mgr.start()?;

// Config is now hot-reloaded. When files change, the config is
// automatically re-parsed and swapped.

// Later, shutdown
mgr.shutdown();
```

### Programmatic Config Update

```rust
// Update config source without waiting for file watcher
mgr.update_source("C:/www/new-app")?;

// Get current hot-swapped config
if let Some(config) = mgr.current_config() {
    println!("Current root: {:?}", config.root);
}
```

### Integrating with Server

```rust
use larva_php::server::Server;
use larva_php::config::hot_reload::HotReloadManager;

let loader = ConfigLoader::default();
let mut hot_reload = HotReloadManager::new("cli", &root, loader.clone());
hot_reload.watch_dir(PathBuf::from(&root));
hot_reload.start()?;

let server = Server::new(&root)?;
server.activate();

// In your request handler, read current config:
let current = hot_reload.current_config();
// Use config.root, config.rate_limit, etc.
```

## Watchable File Extensions

The following extensions trigger reload when modified:

| Extension | Purpose |
|-----------|---------|
| `.php` | PHP source files |
| `.htaccess` | Apache rewrite rules |
| `.conf` | Configuration files |
| `.yaml`, `.yml` | YAML config |
| `.json` | JSON config |
| `.toml` | TOML config |

## API Reference

### `HotReloadManager::new()`

```rust
pub fn new(format: &str, source: &str, loader: ConfigLoader) -> Self
```

Create a new hot reload manager.

- `format` - Config format name (`"cli"`, `"apache"`, `"nginx"`, `"caddy"`)
- `source` - Initial configuration source string
- `loader` - `ConfigLoader` instance for re-parsing

### `HotReloadManager::start()`

```rust
pub fn start(&mut self) -> Result<(), String>
```

Start the background file watcher task. Spawns a Tokio task that watches for file changes and triggers config re-parsing.

### `HotReloadManager::update_source()`

```rust
pub fn update_source(&self, source: &str) -> Result<(), String>
```

Programmatically update the config source and trigger an immediate re-parse and atomic swap.

### `HotReloadManager::current_config()`

```rust
pub fn current_config(&self) -> Option<Arc<ServerConfig>>
```

Get the current hot-swapped configuration. Lock-free read via `ArcSwap`.

### `HotReloadManager::watch_dir()`

```rust
pub fn watch_dir(&mut self, dir: PathBuf)
```

Add a directory to watch for file changes.

### `HotReloadManager::shutdown()`

```rust
pub fn shutdown()
```

Stop the background watcher task.

## Performance

| Aspect | Implementation | Cost |
|--------|---------------|------|
| Config reads | `ArcSwap::load_full()` | Zero lock contention |
| File watching | Event-driven (`notify`) | No polling |
| Reload triggering | Debounce (150ms) | Coalesces rapid changes |
| Config swap | Atomic `Arc` pointer swap | O(1) |

## Limitations

- Hot reload re-parses the entire config source; incremental config changes are not supported
- PHP worker threads are not restarted on config reload (requires server restart for worker-level changes)
- File watching relies on OS filesystem events; network drives may not generate reliable events

## Example

See [`docs/examples.md`](docs/examples.md) for a complete hot reload example.
