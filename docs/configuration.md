# Configuration

LarvaPHP supports multiple configuration declaration formats through the **Strategy pattern**. All formats are adapted to a single internal `ServerConfig` structure.

## Configuration Formats

### 1. CLI Format (`--root`)

The simplest format, passed via command line:

```powershell
.\larva-php.exe --root C:\path\to\php-app --addr 127.0.0.1:8000
```

| Flag | Short | Default | Description |
|------|-------|---------|-------------|
| `--root` | `-r` | (required) | Document root directory |
| `--addr` | `-a` | `127.0.0.1:8000` | Bind address (host:port) |
| `--version` | `-v` | - | Show version |
| `--help` | `-h` | - | Show help |

### 2. Apache Format (`<DocumentRoot>`)

Apache-style configuration with `<VirtualHost>` blocks:

```apache
<VirtualHost *:80>
    DocumentRoot "C:/www/site"
    ServerName example.com
    ServerAlias www.example.com
    Listen 8080
</VirtualHost>
```

Supported directives:
- `DocumentRoot` - Document root directory
- `ServerName` - Virtual host name
- `ServerAlias` - Alternative host names
- `Listen` - Port to listen on

### 3. Nginx Format (`root`)

Nginx-style server blocks:

```nginx
server {
    listen 80;
    server_name example.com;
    root /var/www/html;
    index index.php;
}
```

Supported directives:
- `root` - Document root directory
- `server_name` - Virtual host name
- `listen` - Port to listen on
- `index` - Default index file (stored for reference)

### 4. Caddy Format (`root`)

Caddyfile-style site blocks:

```caddy
example.com {
    root * /var/www/html
    listen 8080
}
```

Supported directives:
- `root` - Document root directory (supports Caddy `*` placeholder)
- `listen` - Port to listen on

## ConfigLoader

The `ConfigLoader` dispatches to the correct parser based on format name:

```rust
use larva_php::config::ConfigLoader;

let loader = ConfigLoader::default();

// CLI format
let config = loader.parse("cli", "C:\\www\\app")?;

// Apache format
let config = loader.parse("apache", r#"
<VirtualHost *:80>
    DocumentRoot "C:/www/app"
    ServerName example.com
</VirtualHost>
"#)?;

// Nginx format
let config = loader.parse("nginx", r#"
server {
    listen 80;
    root /var/www/html;
}
"#)?;

// Caddy format
let config = loader.parse("caddy", r#"
example.com {
    root * /var/www/html
}
"#)?;
```

Register custom parsers:

```rust
let mut loader = ConfigLoader::default();
loader.register("custom", Box::new(MyCustomParser));
let config = loader.parse("custom", source)?;
```

## ServerConfig

The canonical internal configuration structure:

```rust
pub struct ServerConfig {
    pub root: PathBuf,                    // Document root
    pub server_name: String,              // Virtual host name (default: "localhost")
    pub port: u16,                        // Listen port (default: 8000)
    pub split_path: Vec<String>,          // PHP extensions (default: [".php"])
    pub env: HashMap<String, String>,     // Environment variables
    pub rate_limit: RateLimitConfig,      // Rate limiter config
    pub max_body_size: u64,               // Max request body (default: 10 MB)
    pub request_body_timeout: Option<Duration>,
    pub middleware: Arc<MiddlewareChain>, // Middleware pipeline
    pub server_aliases: Vec<String>,      // Alternative host names
}
```

### ServerConfigBuilder

Builder pattern for programmatic construction:

```rust
use larva_php::config::ServerConfigBuilder;

let config = ServerConfigBuilder::new()
    .root("C:/www/app")
    .server_name("example.com")
    .port(8080)
    .split_path(vec![".php".to_string(), ".phtml".to_string()])
    .env("APP_ENV", "production")
    .rate_limit(60, 60)           // 60 requests per 60 seconds
    .max_body_size(5 * 1024 * 1024)  // 5 MB
    .request_body_timeout(30)     // 30 seconds
    .server_alias("alias.example.com")
    .build();
```

## RateLimitConfig

Fixed-window per-IP rate limiter:

```rust
use larva_php::server::{RateLimitConfig, RateLimiter};

let config = RateLimitConfig {
    max_requests: 60,   // max requests per window
    window_secs: 60,    // window duration in seconds
};

let limiter = RateLimiter::new(config);

if limiter.check("192.168.1.1") {
    // Request allowed
} else {
    // Return HTTP 429 Too Many Requests
}
```

## Sensitive Files

The following file patterns are automatically blocked (HTTP 403):

| Pattern | Example |
|---------|---------|
| `wp-config.php` | WordPress config |
| `.env` | Environment files |
| `.htaccess` | Apache config |
| `.git/` | Git directories |
| `*.sql` | Database dumps |
| `composer.lock` | Composer lock |

## MIME Types

| Extension | MIME Type |
|-----------|-----------|
| `.css` | `text/css; charset=UTF-8` |
| `.js` | `application/javascript; charset=UTF-8` |
| `.html` | `text/html; charset=UTF-8` |
| `.png` | `image/png` |
| `.jpg`, `.jpeg` | `image/jpeg` |
| `.svg` | `image/svg+xml` |
| `.json` | `application/json` |
| `.xml` | `application/xml` |
| `.php` | `application/x-httpd-php` |
| (unknown) | `application/octet-stream` |

## Security Headers

All responses include:

| Header | Value |
|--------|-------|
| `X-Content-Type-Options` | `nosniff` |
| `X-Frame-Options` | `DENY` |
| `Server` | `LarvaPHP` |
