//! Server configuration module.
//!
//! Supports multiple declaration formats (CLI --root, Apache <DocumentRoot>,
//! Nginx root, Caddy file) through the Strategy pattern, and a Chain of
//! Responsibility middleware pipeline for rewrite / .htaccess-style rules.
//!
//! Design patterns used:
//! - Strategy: ConfigParser implementations for each format
//! - Builder: ServerConfigBuilder for step-by-step construction
//! - Chain of Responsibility: MiddlewareChain for request preprocessing
//! - Adapter: each parser adapts its native format to the internal ServerConfig
//! - Flyweight: Arc-shared config data across servers/workers
//!
//! Performance: regexes are compiled once at config load, middleware chain is
//! built once at startup, shared state uses Arc<DashMap>.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use hyper::{Request, Response, StatusCode};
use http_body_util::Full;
use bytes::Bytes;
use regex::Regex;
use once_cell::sync::Lazy;

use crate::errors::{Error, Result};
use crate::server::RateLimitConfig;

// ============================================================================
// Internal server configuration (Flyweight + Adapter target)
// ============================================================================

/// Canonical server configuration shared across all request handlers.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Document root (absolute path)
    pub root: PathBuf,
    /// Server name (virtual host)
    pub server_name: String,
    /// Listen port
    pub port: u16,
    /// PHP split path extensions
    pub split_path: Vec<String>,
    /// Environment variables
    pub env: HashMap<String, String>,
    /// Rate limit configuration
    pub rate_limit: RateLimitConfig,
    /// Max request body size in bytes
    pub max_body_size: u64,
    /// Request body timeout
    pub request_body_timeout: Option<Duration>,
    /// Pre-compiled middleware chain (built once at startup)
    pub middleware: Arc<MiddlewareChain>,
    /// Additional server aliases
    pub server_aliases: Vec<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            root: PathBuf::from("."),
            server_name: "localhost".to_string(),
            port: 8000,
            split_path: vec![".php".to_string()],
            env: HashMap::new(),
            rate_limit: RateLimitConfig::default(),
            max_body_size: 10 * 1024 * 1024,
            request_body_timeout: None,
            middleware: Arc::new(MiddlewareChain::new()),
            server_aliases: Vec::new(),
        }
    }
}

// ============================================================================
// Strategy: ConfigParser
// ============================================================================

/// Strategy interface for parsing a configuration source into ServerConfig.
pub trait ConfigParser: Send + Sync {
    /// Format identifier (e.g. "cli", "apache", "nginx", "caddy").
    fn format(&self) -> &str;
    /// Parse the raw configuration string into a ServerConfig.
    fn parse(&self, source: &str) -> Result<ServerConfig>;
}

/// CLI `--root <dir>` parser.
pub struct CliParser;

impl ConfigParser for CliParser {
    fn format(&self) -> &str { "cli" }

    fn parse(&self, source: &str) -> Result<ServerConfig> {
        let root = fast_abs(source)
            .map_err(|e| Error::Ffi(format!("invalid root path: {}", e)))?
            .to_path_buf();
        Ok(ServerConfig {
            root,
            ..Default::default()
        })
    }
}

/// Apache `<DocumentRoot>` / `.htaccess` parser.
pub struct ApacheParser;

impl ConfigParser for ApacheParser {
    fn format(&self) -> &str { "apache" }

    fn parse(&self, source: &str) -> Result<ServerConfig> {
        let mut config = ServerConfig::default();
        let mut _current_block: Option<String> = None;

        for line in source.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') { continue; }

            // <Directory ...>, <VirtualHost ...>, etc.
            if let Some(rest) = line.strip_prefix('<') {
                if let Some((tag, _)) = rest.split_once('>') {
                    _current_block = Some(tag.trim().to_string());
                }
                continue;
            }
            if line.starts_with("</") {
                _current_block = None;
                continue;
            }

            // Directive: key value...
            if let Some((key, rest)) = line.split_once(char::is_whitespace) {
                let value = rest.trim();
                match key.to_lowercase().as_str() {
                    "documentroot" => {
                        config.root = PathBuf::from(value.trim_matches('"'));
                    }
                    "servername" => {
                        config.server_name = value.trim_matches('"').to_string();
                    }
                    "serveralias" => {
                        config.server_aliases.push(value.trim_matches('"').to_string());
                    }
                    "listen" => {
                        if let Ok(port) = value.parse::<u16>() {
                            config.port = port;
                        }
                    }
                    _ => {}
                }
            }
        }

        Ok(config)
    }
}

/// Nginx `root` / `server` block parser.
pub struct NginxParser;

impl ConfigParser for NginxParser {
    fn format(&self) -> &str { "nginx" }

    fn parse(&self, source: &str) -> Result<ServerConfig> {
        let mut config = ServerConfig::default();
        let mut in_server = false;
        let _depth: &mut usize = &mut 0;

        for line in source.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') { continue; }

            if trimmed.starts_with("server {") || trimmed == "server {" {
                in_server = true;
                continue;
            }
            if trimmed == "}" {
                if in_server { in_server = false; }
                continue;
            }
            if !in_server { continue; }

            if let Some((key, rest)) = trimmed.split_once(char::is_whitespace) {
                let value = rest.trim().trim_matches(';').trim_matches('"');
                match key {
                    "root" => {
                        config.root = PathBuf::from(value);
                    }
                    "server_name" => {
                        config.server_name = value.to_string();
                    }
                    "listen" => {
                        if let Some(port_str) = value.split(':').last() {
                            if let Ok(port) = port_str.trim_matches(';').parse::<u16>() {
                                config.port = port;
                            }
                        }
                    }
                    "index" => {
                        // Nginx index directive - could set default file
                        let _ = value;
                    }
                    _ => {}
                }
            }
        }

        Ok(config)
    }
}

/// Caddyfile parser (Caddy v2 style).
pub struct CaddyParser;

impl ConfigParser for CaddyParser {
    fn format(&self) -> &str { "caddy" }

    fn parse(&self, source: &str) -> Result<ServerConfig> {
        let mut config = ServerConfig::default();
        let mut in_site_block = false;

        for line in source.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') { continue; }

            // Site address block: example.com { ... }
            if trimmed.ends_with('{') {
                let addr = trimmed.trim_end_matches('{').trim();
                if !addr.is_empty() && !addr.starts_with('$') {
                    config.server_name = addr.to_string();
                    in_site_block = true;
                }
                continue;
            }
            if trimmed == "}" {
                in_site_block = false;
                continue;
            }
            if !in_site_block { continue; }

            if let Some((key, rest)) = trimmed.split_once(char::is_whitespace) {
                let value = rest.trim().trim_matches(';');
                let value = value.trim_start_matches('*').trim();
                match key {
                    "root" => {
                        config.root = PathBuf::from(value);
                    }
                    "listen" => {
                        if let Ok(port) = value.parse::<u16>() {
                            config.port = port;
                        }
                    }
                    "encode" | "tls" | "log" | "file_server" | "php" | "rewrite" => {
                        // Handled by middleware directives; skip here
                    }
                    _ => {}
                }
            }
        }

        Ok(config)
    }
}

// ============================================================================
// ConfigLoader - Strategy dispatcher
// ============================================================================

/// Loads configuration using the appropriate parser strategy.
pub struct ConfigLoader {
    parsers: HashMap<String, Box<dyn ConfigParser>>,
}

impl ConfigLoader {
    pub fn new() -> Self {
        let mut parsers: HashMap<String, Box<dyn ConfigParser>> = HashMap::new();
        parsers.insert("cli".to_string(), Box::new(CliParser));
        parsers.insert("apache".to_string(), Box::new(ApacheParser));
        parsers.insert("nginx".to_string(), Box::new(NginxParser));
        parsers.insert("caddy".to_string(), Box::new(CaddyParser));
        Self { parsers }
    }

    pub fn register(&mut self, name: &str, parser: Box<dyn ConfigParser>) {
        self.parsers.insert(name.to_string(), parser);
    }

    pub fn parse(&self, format: &str, source: &str) -> Result<ServerConfig> {
        self.parsers
            .get(format)
            .ok_or_else(|| Error::Ffi(format!("unknown config format: {}", format)))
            .and_then(|p| p.parse(source))
    }
}

impl Default for ConfigLoader {
    fn default() -> Self { Self::new() }
}

// ============================================================================
// Builder pattern for ServerConfig
// ============================================================================

/// Builder for constructing ServerConfig step by step.
pub struct ServerConfigBuilder {
    config: ServerConfig,
}

impl ServerConfigBuilder {
    pub fn new() -> Self {
        Self {
            config: ServerConfig::default(),
        }
    }

    pub fn root<P: Into<PathBuf>>(mut self, root: P) -> Self {
        self.config.root = root.into();
        self
    }

    pub fn server_name(mut self, name: impl Into<String>) -> Self {
        self.config.server_name = name.into();
        self
    }

    pub fn port(mut self, port: u16) -> Self {
        self.config.port = port;
        self
    }

    pub fn split_path(mut self, paths: Vec<String>) -> Self {
        self.config.split_path = paths;
        self
    }

    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.config.env.insert(key.into(), value.into());
        self
    }

    pub fn rate_limit(mut self, max_requests: u64, window_secs: u64) -> Self {
        self.config.rate_limit = RateLimitConfig { max_requests, window_secs };
        self
    }

    pub fn max_body_size(mut self, bytes: u64) -> Self {
        self.config.max_body_size = bytes;
        self
    }

    pub fn request_body_timeout(mut self, secs: u64) -> Self {
        self.config.request_body_timeout = Some(Duration::from_secs(secs));
        self
    }

    pub fn middleware(mut self, middleware: Arc<MiddlewareChain>) -> Self {
        self.config.middleware = middleware;
        self
    }

    pub fn server_alias(mut self, alias: impl Into<String>) -> Self {
        self.config.server_aliases.push(alias.into());
        self
    }

    pub fn build(self) -> ServerConfig {
        self.config
    }
}

impl Default for ServerConfigBuilder {
    fn default() -> Self { Self::new() }
}

// ============================================================================
// Chain of Responsibility: Middleware
// ============================================================================

/// Middleware trait - each middleware inspects/modifies the request and
/// optionally short-circuits with a response.
///
/// The body type is `()` because middleware only reads/modifies the URI,
/// never the body. Use [`strip_body`] / [`restore_body`] to adapt a
/// `Request<IncomingBody>` for the chain.
pub trait Middleware: Send + Sync {
    /// Name for debugging / logging.
    fn name(&self) -> &str;
    /// Process the request. Returns Ok(Some(response)) to short-circuit,
    /// Ok(None) to continue to the next middleware, or Err to abort.
    fn handle(
        &self,
        req: &mut Request<()>,
        config: &ServerConfig,
    ) -> Result<Option<Response<Full<Bytes>>>>;
}

/// Strip the body from a request so it can be processed by middleware.
/// Returns the bodyless request and the original body to be restored later.
pub fn strip_body<B>(req: Request<B>) -> (Request<()>, B) {
    let (parts, body) = req.into_parts();
    (Request::from_parts(parts, ()), body)
}

/// Restore the original body into a request after middleware processing.
pub fn restore_body<B>(req: Request<()>, body: B) -> Request<B> {
    let (parts, _) = req.into_parts();
    Request::from_parts(parts, body)
}

/// Pre-compiled rewrite rule (Flyweight - regex compiled once).
#[derive(Debug, Clone)]
pub struct RewriteRule {
    pub pattern: Regex,
    pub replacement: String,
    pub flag: RewriteFlag,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RewriteFlag {
    /// Stop processing further rules after match.
    Last,
    /// Continue processing further rules.
    Next,
    /// Return immediately (redirect).
    Redirect,
}

impl RewriteRule {
    pub fn new(pattern: &str, replacement: &str, flag: RewriteFlag) -> Result<Self> {
        let re = Regex::new(pattern)
            .map_err(|e| Error::Ffi(format!("invalid rewrite regex '{}': {}", pattern, e)))?;
        Ok(Self {
            pattern: re,
            replacement: replacement.to_string(),
            flag,
        })
    }

    pub fn apply(&self, path: &str) -> Option<String> {
        if self.pattern.is_match(path) {
            Some(self.pattern.replace_all(path, &self.replacement).to_string())
        } else {
            None
        }
    }
}

/// Rewrite middleware implementing .htaccess-style mod_rewrite.
pub struct RewriteMiddleware {
    rules: Arc<[RewriteRule]>,
}

impl RewriteMiddleware {
    pub fn new(rules: Vec<RewriteRule>) -> Self {
        Self { rules: rules.into_iter().collect() }
    }

    pub fn from_htaccess(content: &str) -> Result<Self> {
        let mut rules = Vec::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') { continue; }
            if let Some(rest) = line.strip_prefix("RewriteRule ") {
                let parts: Vec<&str> = rest.splitn(3, char::is_whitespace).collect();
                if parts.len() >= 2 {
                    let pattern = parts[0];
                    let replacement = parts[1];
                    let flag = if parts.len() > 2 && parts[2].contains("[L]") {
                        RewriteFlag::Last
                    } else {
                        RewriteFlag::Next
                    };
                    rules.push(RewriteRule::new(pattern, replacement, flag)?);
                }
            }
        }
        Ok(Self::new(rules))
    }
}

impl Middleware for RewriteMiddleware {
    fn name(&self) -> &str { "rewrite" }

    fn handle(
        &self,
        req: &mut Request<()>,
        _config: &ServerConfig,
    ) -> Result<Option<Response<Full<Bytes>>>> {
        let uri = req.uri().to_string();
        for rule in self.rules.iter() {
            if let Some(new_path) = rule.apply(&uri) {
                // Rebuild URI with rewritten path
                let new_uri = if let Some(query) = uri.split_once('?') {
                    format!("{}?{}", new_path, query.1)
                } else {
                    new_path
                };
                if let Ok(parsed) = new_uri.parse::<hyper::Uri>() {
                    *req.uri_mut() = parsed;
                }
                match rule.flag {
                    RewriteFlag::Last => return Ok(None), // stop rules, continue chain
                    RewriteFlag::Redirect => {
                        return Ok(Some(
                            Response::builder()
                                .status(StatusCode::MOVED_PERMANENTLY)
                                .header("location", &new_uri)
                                .body(Full::new(Bytes::new()))
                                .unwrap(),
                        ));
                    }
                    RewriteFlag::Next => continue,
                }
            }
        }
        Ok(None)
    }
}

/// Middleware chain - Chain of Responsibility.
/// Compiled once at startup; per-request iteration is O(n) with no allocation.
#[derive(Clone, Default)]
pub struct MiddlewareChain {
    middlewares: Vec<Arc<dyn Middleware>>,
}

impl std::fmt::Debug for MiddlewareChain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MiddlewareChain")
            .field("middlewares", &self.middlewares.iter().map(|m| m.name()).collect::<Vec<_>>())
            .finish()
    }
}

impl MiddlewareChain {
    pub fn new() -> Self {
        Self { middlewares: Vec::new() }
    }

    pub fn add(mut self, middleware: Arc<dyn Middleware>) -> Self {
        self.middlewares.push(middleware);
        self
    }

    pub fn len(&self) -> usize { self.middlewares.len() }

    pub fn is_empty(&self) -> bool { self.middlewares.is_empty() }

    /// Execute the chain. Returns Some(response) if short-circuited.
    /// Takes `Request<()>` since middleware only touches the URI.
    pub fn execute(
        &self,
        req: &mut Request<()>,
        config: &ServerConfig,
    ) -> Result<Option<Response<Full<Bytes>>>> {
        for mw in self.middlewares.iter() {
            match mw.handle(req, config)? {
                Some(resp) => return Ok(Some(resp)),
                None => continue,
            }
        }
        Ok(None)
    }
}

// ============================================================================
// Global config registry (Singleton-like)
// ============================================================================

static GLOBAL_CONFIG: Lazy<DashMap<String, Arc<ServerConfig>>> =
    Lazy::new(|| DashMap::new());

/// Register a named server config globally.
pub fn register_config(name: impl Into<String>, config: Arc<ServerConfig>) {
    GLOBAL_CONFIG.insert(name.into(), config);
}

/// Lookup a named server config.
pub fn get_config(name: &str) -> Option<Arc<ServerConfig>> {
    GLOBAL_CONFIG.get(name).map(|v| v.clone())
}

// Re-export fast_abs for parser use
fn fast_abs(path: &str) -> std::io::Result<PathBuf> {
    let p = Path::new(path);
    if p.is_absolute() {
        Ok(p.to_path_buf())
    } else {
        std::env::current_dir().map(|cwd| cwd.join(p))
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- ConfigParser strategies ---

    #[test]
    fn cli_parser_basic() {
        let parser = CliParser;
        assert_eq!(parser.format(), "cli");
        let config = parser.parse("C:\\www\\wordpress").unwrap();
        assert_eq!(config.root, PathBuf::from("C:\\www\\wordpress"));
        assert_eq!(config.server_name, "localhost");
        assert_eq!(config.port, 8000);
    }

    #[test]
    fn cli_parser_relative_root() {
        let parser = CliParser;
        let config = parser.parse("www").unwrap();
        assert!(config.root.is_relative() || config.root.ends_with("www"));
    }

    #[test]
    fn apache_parser_documentroot() {
        let parser = ApacheParser;
        let source = r#"
<VirtualHost *:80>
    DocumentRoot "C:/www/site"
    ServerName example.com
    ServerAlias www.example.com
    Listen 8080
</VirtualHost>
"#;
        let config = parser.parse(source).unwrap();
        assert_eq!(config.root, PathBuf::from("C:/www/site"));
        assert_eq!(config.server_name, "example.com");
        assert!(config.server_aliases.contains(&"www.example.com".to_string()));
        assert_eq!(config.port, 8080);
    }

    #[test]
    fn nginx_parser_basic() {
        let parser = NginxParser;
        let source = r#"
server {
    listen 80;
    server_name example.com;
    root /var/www/html;
    index index.php;
}
"#;
        let config = parser.parse(source).unwrap();
        assert_eq!(config.root, PathBuf::from("/var/www/html"));
        assert_eq!(config.server_name, "example.com");
        assert_eq!(config.port, 80);
    }

    #[test]
    fn caddy_parser_basic() {
        let parser = CaddyParser;
        let source = r#"
example.com {
    root * /var/www/html
    listen 8080
}
"#;
        let config = parser.parse(source).unwrap();
        // Caddy `root * /path` - strip the `*` placeholder
        let root_str = config.root.to_string_lossy().to_string();
        assert!(root_str.ends_with("/var/www/html") || root_str.ends_with("\\var\\www\\html"));
        assert_eq!(config.server_name, "example.com");
        assert_eq!(config.port, 8080);
    }

    #[test]
    fn config_loader_unknown_format() {
        let loader = ConfigLoader::default();
        let result = loader.parse("unknown", "whatever");
        assert!(result.is_err());
    }

    #[test]
    fn config_loader_dispatches() {
        let loader = ConfigLoader::default();
        let cli = loader.parse("cli", "/srv/www").unwrap();
        // fast_abs makes the path absolute, so check the suffix
        assert!(cli.root.to_string_lossy().ends_with("/srv/www") || cli.root.to_string_lossy().ends_with("\\srv\\www"));

        let apache = loader.parse(
            "apache",
            "DocumentRoot /srv/www\nServerName test.com\n",
        ).unwrap();
        assert_eq!(apache.root, PathBuf::from("/srv/www"));
        assert_eq!(apache.server_name, "test.com");
    }

    // --- Builder ---

    #[test]
    fn builder_chain() {
        let config = ServerConfigBuilder::new()
            .root("/custom/root")
            .server_name("myapp.local")
            .port(9000)
            .rate_limit(30, 60)
            .max_body_size(5 * 1024 * 1024)
            .server_alias("alias.local")
            .build();

        assert_eq!(config.root, PathBuf::from("/custom/root"));
        assert_eq!(config.server_name, "myapp.local");
        assert_eq!(config.port, 9000);
        assert_eq!(config.rate_limit.max_requests, 30);
        assert_eq!(config.max_body_size, 5 * 1024 * 1024);
        assert!(config.server_aliases.contains(&"alias.local".to_string()));
    }

    // --- RewriteRule ---

    #[test]
    fn rewrite_rule_match() {
        let rule = RewriteRule::new(r"^/old/(.+)$", "/new/$1", RewriteFlag::Last).unwrap();
        assert_eq!(rule.apply("/old/foo/bar"), Some("/new/foo/bar".to_string()));
        assert_eq!(rule.apply("/other/path"), None);
    }

    #[test]
    fn rewrite_rule_invalid_regex() {
        let result = RewriteRule::new("[invalid", "/x", RewriteFlag::Last);
        assert!(result.is_err());
    }

    // --- MiddlewareChain ---

    #[test]
    fn middleware_chain_passthrough() {
        let chain = MiddlewareChain::new();
        let mut req = Request::builder()
            .uri("/test.php")
            .body(())
            .unwrap();
        let config = ServerConfig::default();
        let result = chain.execute(&mut req, &config).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn middleware_chain_short_circuit() {
        struct BlockMiddleware;
        impl Middleware for BlockMiddleware {
            fn name(&self) -> &str { "block" }
            fn handle(
                &self,
                _req: &mut Request<()>,
                _config: &ServerConfig,
            ) -> Result<Option<Response<Full<Bytes>>>> {
                Ok(Some(
                    Response::builder()
                        .status(StatusCode::FORBIDDEN)
                        .body(Full::new(Bytes::from("blocked")))
                        .unwrap(),
                ))
            }
        }

        let chain = MiddlewareChain::new().add(Arc::new(BlockMiddleware));
        let mut req = Request::builder()
            .uri("/test.php")
            .body(())
            .unwrap();
        let config = ServerConfig::default();
        let result = chain.execute(&mut req, &config).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn rewrite_middleware_from_htaccess() {
        let htaccess = r#"
RewriteRule ^/old/(.+)$ /new/$1 [L]
RewriteRule ^/legacy/(.+)$ /current/$1
"#;
        let mw = RewriteMiddleware::from_htaccess(htaccess).unwrap();
        assert_eq!(mw.rules.len(), 2);

        let config = ServerConfig::default();
        let mut req = Request::builder()
            .uri("/old/page")
            .body(())
            .unwrap();
        let result = mw.handle(&mut req, &config).unwrap();
        assert!(result.is_none()); // Last flag -> continue chain
        assert_eq!(req.uri().path(), "/new/page");

        let mut req2 = Request::builder()
            .uri("/legacy/item")
            .body(())
            .unwrap();
        let result = mw.handle(&mut req2, &config).unwrap();
        assert!(result.is_none());
        assert_eq!(req2.uri().path(), "/current/item");
    }

    #[test]
    fn rewrite_middleware_redirect() {
        let rule = RewriteRule::new(r"^/go$", "https://example.com", RewriteFlag::Redirect).unwrap();
        let mw = RewriteMiddleware::new(vec![rule]);
        let config = ServerConfig::default();
        let mut req = Request::builder()
            .uri("/go")
            .body(())
            .unwrap();
        let result = mw.handle(&mut req, &config).unwrap();
        assert!(result.is_some());
        let resp = result.unwrap();
        assert_eq!(resp.status(), StatusCode::MOVED_PERMANENTLY);
        assert_eq!(resp.headers().get("location").unwrap(), "https://example.com");
    }

    // --- Global config registry ---

    #[test]
    fn global_config_register_and_get() {
        let config = Arc::new(ServerConfigBuilder::new()
            .root("/registry/root")
            .server_name("reg.test")
            .build());
        register_config("mysite", config.clone());
        let got = get_config("mysite").unwrap();
        assert_eq!(got.server_name, "reg.test");
        assert_eq!(got.root, PathBuf::from("/registry/root"));
        assert!(Arc::ptr_eq(&got, &config));
    }

    #[test]
    fn global_config_missing() {
        assert!(get_config("nonexistent").is_none());
    }

    // --- ServerConfig defaults ---

    #[test]
    fn server_config_defaults() {
        let config = ServerConfig::default();
        assert_eq!(config.root, PathBuf::from("."));
        assert_eq!(config.server_name, "localhost");
        assert_eq!(config.port, 8000);
        assert_eq!(config.split_path, vec![".php".to_string()]);
        assert_eq!(config.rate_limit.max_requests, 60);
        assert_eq!(config.max_body_size, 10 * 1024 * 1024);
        assert!(config.middleware.is_empty());
    }
}
