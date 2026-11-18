# Middleware & Rewrite Rules

LarvaPHP includes a **Chain of Responsibility** middleware pipeline for request preprocessing. Middleware runs after the HTTP request is received but before PHP execution, enabling URL rewriting, access control, and request transformation.

## Middleware Architecture

```
HTTP Request
    │
    ▼
┌─────────────────────────────────────────────┐
│       MiddlewareChain (Chain of CoR)        │
│  Middleware 1  →  Middleware 2  →  ...      │
│  (each can short-circuit or pass through)   │
└─────────────────────────────────────────────┘
    │
    ├─ Short-circuit → Return response immediately
    │
    ▼
┌─────────────────────────────────────────────┐
│      PhpContext → PHP Execution → Response  │
└─────────────────────────────────────────────┘
```

## Middleware Trait

```rust
use larva_php::config::{Middleware, ServerConfig};
use hyper::{Request, Response};
use http_body_util::Full;
use bytes::Bytes;
use larva_php::errors::Result;

pub trait Middleware: Send + Sync {
    fn name(&self) -> &str;
    fn handle(
        &self,
        req: &mut Request<()>,   // Body is stripped for middleware
        config: &ServerConfig,
    ) -> Result<Option<Response<Full<Bytes>>>>;
}
```

Return values:
- `Ok(None)` - Continue to next middleware
- `Ok(Some(response))` - Short-circuit and return this response
- `Err(...)` - Abort request processing

## Built-in Middleware: RewriteMiddleware

Implements Apache `.htaccess`-style `mod_rewrite` rules.

### RewriteRule

```rust
pub struct RewriteRule {
    pub pattern: Regex,      // Pre-compiled regex
    pub replacement: String, // Replacement pattern
    pub flag: RewriteFlag,   // Last, Next, or Redirect
}

pub enum RewriteFlag {
    Last,      // Stop processing further rules after match
    Next,      // Continue processing further rules
    Redirect,  // Return HTTP 301 immediately
}
```

### Creating Rewrite Rules

```rust
use larva_php::config::{RewriteMiddleware, RewriteRule, RewriteFlag};

// Rule with [L] flag (stop after match)
let rule1 = RewriteRule::new(r"^/old/(.+)$", "/new/$1", RewriteFlag::Last)?;

// Rule continuing to next rule
let rule2 = RewriteRule::new(r"^/legacy/(.+)$", "/current/$1", RewriteFlag::Next)?;

// Redirect rule
let rule3 = RewriteRule::new(r"^/go$", "https://example.com", RewriteFlag::Redirect)?;

let mw = RewriteMiddleware::new(vec![rule1, rule2, rule3]);
let chain = larva_php::config::MiddlewareChain::new().add(Arc::new(mw));
```

### Parsing .htaccess Files

```rust
use larva_php::config::RewriteMiddleware;

let htaccess = r#"
RewriteEngine On
RewriteRule ^/old/(.+)$ /new/$1 [L]
RewriteRule ^/legacy/(.+)$ /current/$1
RewriteRule ^/go$ https://example.com [R]
"#;

let mw = RewriteMiddleware::from_htaccess(htaccess)?;
```

Supported `.htaccess` directives:
- `RewriteRule <pattern> <replacement> [flags]`
- Flags: `[L]` (last), `[R]` (redirect)

## MiddlewareChain

```rust
use larva_php::config::MiddlewareChain;
use std::sync::Arc;

let chain = MiddlewareChain::new()
    .add(Arc::new(rewrite_middleware))
    .add(Arc::new(auth_middleware))
    .add(Arc::new(logging_middleware));

// Execute per request
let result = chain.execute(&mut req, &config)?;
// result is Some(response) if short-circuited, None if passed through
```

## Custom Middleware Example

```rust
use larva_php::config::{Middleware, ServerConfig};
use hyper::{Request, Response, StatusCode};
use http_body_util::Full;
use bytes::Bytes;
use larva_php::errors::Result;

struct AuthMiddleware;

impl Middleware for AuthMiddleware {
    fn name(&self) -> &str { "auth" }

    fn handle(
        &self,
        req: &mut Request<()>,
        config: &ServerConfig,
    ) -> Result<Option<Response<Full<Bytes>>>> {
        // Check authorization header
        let auth = req.headers().get("authorization");
        if auth.is_none() {
            return Ok(Some(
                Response::builder()
                    .status(StatusCode::UNAUTHORIZED)
                    .header("www-authenticate", "Bearer")
                    .body(Full::new(Bytes::from("Unauthorized")))
                    .unwrap(),
            ));
        }
        Ok(None) // Continue
    }
}

let chain = MiddlewareChain::new().add(Arc::new(AuthMiddleware));
```

## Custom Middleware Example: IP Allowlist

```rust
use std::net::IpAddr;

struct AllowlistMiddleware {
    allowed_ips: std::collections::HashSet<IpAddr>,
}

impl Middleware for AllowlistMiddleware {
    fn name(&self) -> &str { "allowlist" }

    fn handle(
        &self,
        req: &mut Request<()>,
        config: &ServerConfig,
    ) -> Result<Option<Response<Full<Bytes>>>> {
        let peer_addr = req.extensions().get::<std::net::SocketAddr>();
        if let Some(addr) = peer_addr {
            if !self.allowed_ips.contains(&addr.ip()) {
                return Ok(Some(
                    Response::builder()
                        .status(StatusCode::FORBIDDEN)
                        .body(Full::new(Bytes::from("Forbidden")))
                        .unwrap(),
                ));
            }
        }
        Ok(None)
    }
}
```

## Using Middleware with Server

```rust
use larva_php::server::Server;
use larva_php::config::{MiddlewareChain, RewriteMiddleware, RewriteRule, RewriteFlag};
use std::sync::Arc;

// Build middleware chain
let rules = vec![
    RewriteRule::new(r"^/api/(.+)$", "/api/index.php/$1", RewriteFlag::Last)?,
    RewriteRule::new(r"^/assets/(.+)$", "/public/assets/$1", RewriteFlag::Next)?,
];
let rewrite_mw = RewriteMiddleware::new(rules);
let chain = MiddlewareChain::new().add(Arc::new(rewrite_mw));

// Create server with middleware
let config = larva_php::config::ServerConfigBuilder::new()
    .root("C:/www/app")
    .middleware(Arc::new(chain))
    .build();

// Note: Server::new() takes a root path directly.
// To use config middleware, integrate via HotReloadManager or
// pass config through ServerOption.
```

## Request URI Modification

Middleware can modify `req.uri()` in-place. The modified URI is used for subsequent PHP routing:

```rust
// Before middleware: /old/page
// After RewriteRule ^/old/(.+)$ → /new/$1: /new/page
// PHP receives: REQUEST_URI=/new/page, SCRIPT_NAME=/new/page
```

Query strings are preserved during rewrite:

```
/old/page?foo=bar  →  /new/page?foo=bar
```

## Performance

- Middleware chain is built once at server startup
- Per-request iteration is O(n) with no heap allocation
- Regex patterns are pre-compiled at config load time (Flyweight pattern)
- Request body is stripped before middleware processing (zero-copy URI manipulation)

## Examples

See [`docs/examples.md`](docs/examples.md) for complete middleware examples.
