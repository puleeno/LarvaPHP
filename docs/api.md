# API Reference

This document describes the public API of the `larva-php` crate.

## Top-Level Functions

### `larva_php::init()`

```rust
pub fn init(options: Vec<Box<dyn Option>>) -> Result<()>
```

Initialize the PHP runtime and start workers. Mirrors `frankenphp.Init()`.

**Parameters:**
- `options` - Configuration options implementing the `Option` trait

**Returns:** `Ok(())` on success, `Err` on failure

### `larva_php::shutdown()`

```rust
pub fn shutdown()
```

Shutdown the PHP runtime. Mirrors `frankenphp.Shutdown()`.

### `larva_php::validate()`

```rust
pub fn validate(options: &[Box<dyn Option>]) -> Result<()>
```

Validate options without starting the runtime. Mirrors `frankenphp.Validate()`.

### `larva_php::version()`

```rust
pub fn version() -> ffi::frankenphp_version
```

Get PHP version information.

### `larva_php::config()`

```rust
pub fn config() -> ffi::frankenphp_config
```

Get PHP configuration.

### `larva_php::execute_script_cli()`

```rust
pub fn execute_script_cli(
    script: &str,
    args: &[String],
    options: Vec<Box<dyn Option>>,
) -> i32
```

Execute a PHP script via the CLI SAPI. Mirrors `frankenphp.ExecuteScriptCLI()`.

## Modules

### `larva_php::server`

#### `Server`

```rust
pub struct Server {
    pub idx: i32,
    pub configured_name: String,
    pub name: String,
    pub root: String,
    pub split_path: Vec<String>,
    pub env: HashMap<String, String>,
    pub rate_limiter: RateLimiter,
    pub max_body_size: u64,
    // ... internal fields
}
```

**Methods:**

| Method | Description |
|--------|-------------|
| `Server::new(root)` | Create a new server with document root |
| `Server::fallback()` | Create an unregistered fallback server |
| `server.root()` | Get document root |
| `server.name()` | Get server name |
| `server.split_path()` | Get PHP split path extensions |
| `server.is_registered()` | Check if server is registered |
| `server.mime_type(filename)` | Get MIME type for file |
| `server.set_name(name)` | Set server name |
| `server.set_split_path(paths)` | Set split path extensions |

#### `RateLimitConfig`

```rust
pub struct RateLimitConfig {
    pub max_requests: u64,  // default: 60
    pub window_secs: u64,   // default: 60
}
```

#### `RateLimiter`

```rust
pub struct RateLimiter {
    // internal state shared via Arc<DashMap>
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self
    pub fn check(&self, ip: &str) -> bool  // Returns true if request is allowed
}
```

#### `CachedFile`

```rust
pub struct CachedFile {
    pub size: u64,
    pub mtime: u64,
    pub etag: String,
}
```

### `larva_php::context`

#### `PhpContext`

Per-request context holding CGI environment variables.

```rust
pub struct PhpContext {
    pub document_root: String,
    pub split_path: Vec<String>,
    pub env: PreparedEnv,
    pub server: Arc<Server>,
    pub request_uri: String,
    pub query_string: String,
    pub request_method: String,
    pub content_type: String,
    pub content_length: String,
    pub server_protocol: String,
    pub server_name: String,
    pub server_port: String,
    pub https: String,
    pub is_done: AtomicBool,
    // ... internal fields
}
```

**Methods:**

| Method | Description |
|--------|-------------|
| `PhpContext::from_request(req, server, opts)` | Create context from HTTP request |
| `ctx.validate()` | Validate context (rejects null bytes, etc.) |
| `ctx.is_done()` | Check if request is complete |
| `ctx.close_context()` | Mark request as done |

#### `RequestOption`

Trait for per-request configuration:

```rust
pub trait RequestOption: Send + Sync {
    fn apply(&self, ctx: &mut PhpContext) -> Result<()>;
}
```

### `larva_php::cgi`

#### CGI Header Mapping

```rust
pub fn common_request_headers() -> HashMap<&'static str, &'static str>
```

Maps HTTP headers to PHP `$_SERVER` variables (e.g., `Content-Type` → `HTTP_CONTENT_TYPE`).

#### Path Utilities

```rust
pub fn fast_abs(path: &str) -> io::Result<PathBuf>
pub fn split_pos(path: &str, split_path: &[String]) -> i32
pub fn clean_request_path(path: &str, is_windows: bool) -> String
pub fn path_clean(path: &str) -> String
pub fn sanitized_path_join(root: &str, path: &str) -> String
pub fn split_remote_addr(addr: &str) -> (String, String)
pub fn tls_protocol(version: u16) -> &str
pub fn c_string_http_method(method: &str) -> Option<&'static str>
```

### `larva_php::worker`

#### `Worker`

Persistent PHP script worker:

```rust
pub struct Worker {
    pub name: String,
    pub file_name: String,
    pub num: i32,
    pub max_threads: i32,
    pub server: Option<Arc<Server>>,
    // ... internal fields
}
```

**Methods:**

| Method | Description |
|--------|-------------|
| `Worker::new(name, file, declared, num, max_threads, max_failures, server)` | Create worker |
| `worker.match_request(matcher)` | Set request path matcher |
| `worker.num()` | Get worker number |
| `worker.max_threads()` | Get max threads |

### `larva_php::thread`

#### `ThreadState`

```rust
pub enum ThreadState {
    Reserved, Booting, BootRequested, ShuttingDown, Done,
    Inactive, Ready, TransitionRequested, TransitionInProgress,
    TransitionComplete, Rebooting, ForceRebooting, RebootReady,
    YieldingForReboot,
}
```

### `larva_php::errors`

#### `Error`

```rust
pub enum Error {
    InvalidRequest,
    AlreadyStarted,
    InvalidPHPVersion,
    ZendSignals,
    MainThreadCreation,
    ThreadCreation,
    ScriptExecution,
    NotRunning,
    MaxWaitTimeExceeded,
    RequestBodyTooLarge,
    MaxThreadsReached,
    InvalidSplitPath,
    UnknownWorker(String),
    InvalidWorkerFilename(String, String),
    WorkerFileNotFound(String, String),
    DuplicateWorkerFilename(String),
    WorkerMatcherWithoutServer(String),
    DuplicateServerWorkerFilename(String),
    DuplicateWorkerName(String),
    Rejected(RejectedError),
    WorkerInit(String),
    InvalidContentLengthHeader(String),
    InvalidRequestPath,
    InvalidScriptPath,
    WatcherNotEnabled,
    EmbeddedApp(String),
    Ffi(String),
    Io(io::Error),
    Channel(String),
    Thread(String),
    Serde(serde_json::Error),
}
```

#### `RejectedError`

Errors with HTTP status codes:

```rust
pub struct RejectedError {
    pub message: String,
    pub status: u16,
}
```

**Convenience constructors:**

| Function | Status | Message |
|----------|--------|---------|
| `err_invalid_request_path()` | 400 | invalid request path |
| `err_invalid_script_path()` | 404 | invalid script path |
| `err_invalid_content_length_header(msg)` | 400 | invalid Content-Length header |
| `err_max_wait_time_exceeded()` | 503 | maximum request handling time exceeded |
| `err_request_body_too_large()` | 413 | request body too large |

**Helpers:**

```rust
pub fn is_rejected(err: &Error) -> bool
pub fn rejected_status(err: &Error) -> Option<u16>
```

### `larva_php::options`

#### Traits

| Trait | Configures | Method |
|-------|-----------|--------|
| `Option` | Global instance | `apply(&self, opt: &mut Opt)` |
| `WorkerOption` | Worker | `apply(&self, worker: &mut WorkerOpt)` |
| `ServerOption` | Server block | `apply(&self, server: &mut Server)` |
| `RequestOption` | Per-request | `apply(&self, ctx: &mut PhpContext)` |

#### `Opt`

Global configuration struct:

```rust
pub struct Opt {
    pub num_threads: i32,
    pub max_threads: i32,
    pub workers: Vec<WorkerOpt>,
    pub php_ini: HashMap<String, String>,
    pub max_wait_time: Duration,
    pub max_idle_time: Duration,
    pub max_requests: i32,
    pub servers: Vec<Arc<Server>>,
}
```

### `larva_php::env`

Environment variable management for PHP:

```rust
pub fn prepare_env(env: HashMap<String, String>) -> PreparedEnv
pub fn ensure_prepared_env(env: PreparedEnv) -> PreparedEnv
pub fn length_of_env() -> usize
pub fn to_cstring(s: &str) -> Option<CString>
pub unsafe fn cstr_to_string(ptr: *const c_char) -> String
pub unsafe fn cstr_to_string_n(ptr: *const c_char, len: usize) -> String
```

### `larva_php::types`

PHP value wrappers:

```rust
pub enum PhpValue {
    Null,
    Bool(bool),
    Long(i64),
    Double(f64),
    String(String),
    Array(AssociativeArray),
}

pub struct AssociativeArray {
    pub order: Vec<String>,
    pub values: HashMap<String, PhpValue>,
}
```

**Methods:**

| Method | Description |
|--------|-------------|
| `AssociativeArray::new()` | Create empty array |
| `aa.insert(key, value)` | Insert key-value pair |
| `aa.get(key)` | Get value by key |
| `aa.len()` | Get number of entries |
| `aa.is_empty()` | Check if empty |

```rust
pub unsafe fn call_php_callable(cb: *const zval, params: &[PhpValue]) -> PhpValue
pub unsafe fn php_value(val: &PhpValue) -> *mut zval
pub unsafe fn go_value(zval: *const zval) -> Result<PhpValue>
```

### `larva_php::watcher` (feature: `watcher`)

File watching and hot reload:

```rust
pub struct FileEvent {
    pub path: String,
    pub effect_type: FileEffect,
    pub path_type: PathType,
}

pub enum FileEffect { Create, Update, Remove, Rename, Destroy }
pub enum PathType { File, Directory, Watcher, HardLink }

pub struct PatternGroup {
    pub patterns: Vec<String>,
    pub callback: Box<dyn Fn(&[FileEvent]) + Send + Sync>,
}

pub struct WatcherState {
    pub is_active: AtomicBool,
    // ... internal state
}

impl WatcherState {
    pub fn new() -> Self
    pub fn init(&self, groups: Vec<Arc<PatternGroup>>) -> Result<(), String>
    pub fn register_pattern(&self, dir: PathBuf, group: Arc<PatternGroup>)
    pub fn drain(&self)
    pub fn set_config(&self, config: Option<Arc<ServerConfig>>)
    pub fn get_config(&self) -> Arc<Option<Arc<ServerConfig>>>
}

pub static WATCHER: Lazy<RwLock<WatcherState>>

pub fn start_watcher_task() -> (mpsc::UnboundedSender<notify::Event>, Arc<AtomicBool>)
pub fn watch_directory(tx: &mpsc::UnboundedSender<notify::Event>, dir: &Path) -> Result<(), String>
pub fn parse_pattern(pattern: &str) -> Result<(String, Vec<String>), String>
pub fn match_pattern(pattern: &str, file_name: &str) -> bool
```

### `larva_php::config`

Configuration module with Strategy/Builder/Chain of Responsibility patterns:

```rust
pub trait ConfigParser: Send + Sync {
    fn format(&self) -> &str;
    fn parse(&self, source: &str) -> Result<ServerConfig>;
}

// Built-in parsers:
pub struct CliParser;      // --root format
pub struct ApacheParser;   // <DocumentRoot> format
pub struct NginxParser;    // root format
pub struct CaddyParser;    // Caddyfile root format

pub struct ConfigLoader {
    parsers: HashMap<String, Box<dyn ConfigParser>>,
}

impl ConfigLoader {
    pub fn new() -> Self
    pub fn register(&mut self, name: &str, parser: Box<dyn ConfigParser>)
    pub fn parse(&self, format: &str, source: &str) -> Result<ServerConfig>
}

pub struct ServerConfig {
    pub root: PathBuf,
    pub server_name: String,
    pub port: u16,
    pub split_path: Vec<String>,
    pub env: HashMap<String, String>,
    pub rate_limit: RateLimitConfig,
    pub max_body_size: u64,
    pub middleware: Arc<MiddlewareChain>,
    pub server_aliases: Vec<String>,
}

pub struct ServerConfigBuilder { ... }

pub trait Middleware: Send + Sync {
    fn name(&self) -> &str;
    fn handle(&self, req: &mut Request<()>, config: &ServerConfig)
        -> Result<Option<Response<Full<Bytes>>>>;
}

pub struct RewriteMiddleware { ... }
pub struct RewriteRule { ... }
pub enum RewriteFlag { Last, Next, Redirect }

pub struct MiddlewareChain { ... }

impl MiddlewareChain {
    pub fn new() -> Self
    pub fn add(mut self, middleware: Arc<dyn Middleware>) -> Self
    pub fn execute<B>(&self, req: &mut Request<B>, config: &ServerConfig)
        -> Result<Option<Response<Full<Bytes>>>>  // Note: B must be ()
}
```

#### `HotReloadManager` (feature: `watcher`)

```rust
pub struct HotReloadManager { ... }

impl HotReloadManager {
    pub fn new(format: &str, source: &str, loader: ConfigLoader) -> Self
    pub fn start(&mut self) -> Result<(), String>
    pub fn update_source(&self, source: &str) -> Result<(), String>
    pub fn current_config(&self) -> Option<Arc<ServerConfig>>
    pub fn watch_dir(&mut self, dir: PathBuf)
    pub fn shutdown(&mut self)
}
```

### `larva_php::metrics`

```rust
pub trait Metrics: Send + Sync {
    fn start_worker(&self, name: &str);
    fn ready_worker(&self, name: &str);
    fn stop_worker(&self, name: &str, reason: StopReason);
    fn total_workers(&self, name: &str, count: i32);
    fn total_threads(&self, count: i32);
    fn start_request(&self);
    fn stop_request(&self);
}

pub struct AtomicMetrics { ... }  // Thread-safe implementation
pub struct NullMetrics;           // No-op implementation

pub enum StopReason { Crash, Restart, BootFailure }
```

## Re-exports

| Item | Module |
|------|--------|
| `Error` | `larva_php::errors` |
| `Result<T>` | `larva_php::errors` |
| `Option` | `larva_php::options` |
| `WorkerOption` | `larva_php::options` |
| `ServerOption` | `larva_php::options` |
| `RequestOption` | `larva_php::context` |
| `Server` | `larva_php::server` |
| `RateLimitConfig` | `larva_php::server` |
| `RateLimiter` | `larva_php::server` |
| `ConfigParser` | `larva_php::config` |
| `ConfigLoader` | `larva_php::config` |
| `ServerConfig` | `larva_php::config` |
| `ServerConfigBuilder` | `larva_php::config` |
| `Middleware` | `larva_php::config` |
| `MiddlewareChain` | `larva_php::config` |
| `RewriteMiddleware` | `larva_php::config` |
| `RewriteRule` | `larva_php::config` |
| `HotReloadManager` | `larva_php::config::hot_reload` (feature: watcher) |
| `ffi` | `larva_php::larva_php_sys` |

## Version

```rust
pub const VERSION: &str = env!("CARGO_PKG_VERSION");  // "1.13.5"
pub const SERVER_HEADER: &str = "LarvaPHP";
pub const DEFAULT_DOCUMENT_ROOT: &str = "public";
pub const DEFAULT_WATCH_PATTERN: &str = "./**/*.{env,php,twig,yaml,yml}";
pub const DEFAULT_HOT_RELOAD_PATTERN: &str = "./**/*.{css,env,gif,htm,html,jpg,jpeg,js,mjs,php,png,svg,twig,webp,xml,yaml,yml}";
```
