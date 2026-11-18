# Examples

Complete, runnable examples for common LarvaPHP scenarios.

## Example 1: Basic PHP Application Server

```rust
use larva_php::server::Server;
use larva_php::thread;
use tokio;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    let root = "C:/www/wordpress";
    let addr = "127.0.0.1:8000".parse::<std::net::SocketAddr>()?;

    // Create server
    let server = Server::new(root)?;
    server.activate();

    // Initialize PHP runtime
    thread::init(Vec::new())?;

    // Start HTTP server
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        server.start_server(addr).await?;
        Ok(())
    })
}
```

Run:
```powershell
cargo run -- --root C:/www/wordpress --addr 127.0.0.1:8000
```

## Example 2: CLI Mode (Run PHP Script)

```rust
use larva_php::execute_script_cli;

fn main() {
    let script = "script.php";
    let args = vec!["arg1".to_string(), "arg2".to_string()];
    let status = execute_script_cli(script, &args, Vec::new());
    std::process::exit(status);
}
```

Run:
```powershell
cargo run -- script.php arg1 arg2
```

## Example 3: Rate Limiting

```rust
use larva_php::server::{RateLimitConfig, RateLimiter};

let limiter = RateLimiter::new(RateLimitConfig {
    max_requests: 30,
    window_secs: 60,
});

let ip = "192.168.1.1";

if limiter.check(ip) {
    println!("Request allowed");
} else {
    println!("HTTP 429 Too Many Requests - rate limited");
}

// Test burst
for i in 0..35 {
    let allowed = limiter.check("192.168.1.1");
    println!("Request {}: {}", i + 1, if allowed { "allowed" } else { "rate limited" });
}
// First 30 allowed, next 5 rate limited
```

## Example 4: .htaccess-Style URL Rewrite

```rust
use larva_php::config::{MiddlewareChain, RewriteMiddleware, RewriteRule, RewriteFlag};
use std::sync::Arc;

let rules = vec![
    # Redirect old blog URLs
    RewriteRule::new(r"^/blog/2020/(.+)$", "/blog/2024/$1", RewriteFlag::Last)?,
    # Rewrite API calls to index.php
    RewriteRule::new(r"^/api/(.+)$", "/api/index.php/$1", RewriteFlag::Last)?,
    # Legacy path forwarding (continue processing)
    RewriteRule::new(r"^/legacy/(.+)$", "/current/$1", RewriteFlag::Next)?,
    # External redirect
    RewriteRule::new(r"^/go$", "https://example.com", RewriteFlag::Redirect)?,
];

let mw = RewriteMiddleware::new(rules);
let chain = MiddlewareChain::new().add(Arc::new(mw));

// Per request:
let result = chain.execute(&mut req, &config)?;
if let Some(response) = result {
    // Return response immediately (redirect or error)
}
```

## Example 5: .htaccess File Parsing

```rust
use larva_php::config::RewriteMiddleware;

let htaccess_content = std::fs::read_to_string(".htaccess")?;
let mw = RewriteMiddleware::from_htaccess(&htaccess_content)?;

// Supported directives in .htaccess:
// RewriteRule ^/old/(.+)$ /new/$1 [L]
// RewriteRule ^/legacy/(.+)$ /current/$1
```

## Example 6: Hot Reload

```rust
use larva_php::config::{ConfigLoader, ServerConfigBuilder};
use larva_php::config::hot_reload::HotReloadManager;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let loader = ConfigLoader::default();
    let mut mgr = HotReloadManager::new("cli", "C:/www/app", loader.clone());
    
    mgr.watch_dir(PathBuf::from("C:/www/app"));
    mgr.start()?;
    
    // Server runs here...
    // When files in C:/www/app change, config is auto-reloaded
    
    mgr.shutdown();
    Ok(())
}
```

## Example 7: Programmatic Config with Builder

```rust
use larva_php::config::{ServerConfigBuilder, RateLimitConfig};

let config = ServerConfigBuilder::new()
    .root("C:/www/myapp")
    .server_name("myapp.local")
    .port(9000)
    .split_path(vec![".php".to_string(), ".phtml".to_string()])
    .env("APP_ENV", "production")
    .env("APP_DEBUG", "false")
    .rate_limit(100, 60)      // 100 req/min
    .max_body_size(20 * 1024 * 1024)  // 20 MB
    .request_body_timeout(60)   // 60 seconds
    .server_alias("www.myapp.local")
    .build();

assert_eq!(config.root, PathBuf::from("C:/www/myapp"));
assert_eq!(config.port, 9000);
assert_eq!(config.rate_limit.max_requests, 100);
```

## Example 8: Multiple Config Formats

```rust
use larva_php::config::ConfigLoader;

let loader = ConfigLoader::default();

// CLI format
let c1 = loader.parse("cli", "/var/www/html")?;
assert_eq!(c1.root, PathBuf::from("/var/www/html"));

// Apache format
let c2 = loader.parse("apache", r#"
<VirtualHost *:80>
    DocumentRoot "/var/www/site"
    ServerName example.com
    ServerAlias www.example.com
    Listen 8080
</VirtualHost>
"#)?;
assert_eq!(c2.server_name, "example.com");
assert_eq!(c2.port, 8080);

// Nginx format
let c3 = loader.parse("nginx", r#"
server {
    listen 80;
    server_name example.com;
    root /var/www/html;
}
"#)?;
assert_eq!(c3.root, PathBuf::from("/var/www/html"));

// Caddy format
let c4 = loader.parse("caddy", r#"
example.com {
    root * /var/www/html
    listen 8080
}
"#)?;
assert_eq!(c4.server_name, "example.com");
```

## Example 9: Sensitive File Protection (Automatic)

LarvaPHP automatically blocks access to sensitive files. No configuration needed:

| Request | Response |
|---------|----------|
| `GET /wp-config.php` | 403 Forbidden |
| `GET /.env` | 403 Forbidden |
| `GET /.git/config` | 403 Forbidden |
| `GET /db.sql` | 403 Forbidden |
| `GET /composer.lock` | 403 Forbidden |
| `GET /index.php` | 200 OK (normal) |

## Example 10: Custom Rate Limit Per Server

```rust
use larva_php::server::{Server, RateLimitConfig};
use larva_php::config::ServerConfigBuilder;

let config = ServerConfigBuilder::new()
    .root("C:/www/api")
    .rate_limit(10, 60)  // Strict: 10 req/min for API
    .build();

let server = Server::new_from_config(config)?;
// API clients get 10 requests per minute, then 429
```

## Example 11: MIME Type Detection

```rust
use larva_php::server::Server;

let server = Server::new("C:/www")?;

assert_eq!(server.mime_type("style.css"), "text/css; charset=UTF-8");
assert_eq!(server.mime_type("app.js"), "application/javascript; charset=UTF-8");
assert_eq!(server.mime_type("image.png"), "image/png");
assert_eq!(server.mime_type("document.pdf"), "application/octet-stream");
assert_eq!(server.mime_type("data.json"), "application/json");
```

## Example 12: Running with WordPress

```powershell
# Build
cargo build --release

# Run against WordPress
.\target\release\larva-php.exe --root C:\Users\Administrator\Projects\wordpress.org --addr 127.0.0.1:9000

# Test
curl http://127.0.0.1:9000/
# Expected: HTTP 302 (WordPress redirects to setup page)
```

## Example 13: Running Tests

```powershell
# Run all unit tests
cargo test --lib

# Run specific module tests
cargo test --lib config::
cargo test --lib server::
cargo test --lib cgi::

# Run with watcher feature (default)
cargo test --features watcher --lib

# Build release
cargo build --release
```

## Complete Example: main.rs

```rust
use std::process::ExitCode;
use larva_php::{self, VERSION, server::Server, thread};
use tokio;

fn main() -> ExitCode {
    tracing_subscriber::fmt::init();

    let root = std::env::args().nth(1).unwrap_or_else(|| "C:/www".to_string());
    let addr: std::net::SocketAddr = "127.0.0.1:8000".parse().expect("Invalid address");

    let server = match Server::new(&root) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to create server: {}", e);
            return ExitCode::from(1);
        }
    };
    server.activate();

    if let Err(e) = thread::init(Vec::new()) {
        eprintln!("Failed to init PHP: {}", e);
        return ExitCode::from(1);
    }

    let rt = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Failed to create Tokio runtime: {}", e);
            return ExitCode::from(1);
        }
    };

    match rt.block_on(server.start_server(addr)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Server error: {}", e);
            ExitCode::from(1)
        }
    }
}
```
