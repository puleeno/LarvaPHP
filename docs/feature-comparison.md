# Feature Comparison: LarvaPHP vs FrankenPHP

This document compares the feature set of LarvaPHP (Rust) against FrankenPHP (Go).

## Summary

| Category | Status |
|----------|--------|
| Core PHP execution | ✅ Both |
| HTTP server | ✅ Both |
| Configuration | ✅ Both |
| Worker mode | ✅ Both |
| Hot reload | ✅ Both |
| Rate limiting | ✅ LarvaPHP (built-in) |
| Security headers | ✅ Both |
| Caddy integration | ✅ Both |
| Metrics/Observability | ⚠️ Partial |
| Framework integration | ⚠️ Partial |
| Developer experience | ⚠️ Partial |

## Detailed Comparison

### Core Features

| Feature | FrankenPHP | LarvaPHP | Notes |
|---------|-----------|----------|-------|
| PHP 8.2+ embedding | ✅ Via CGO | ✅ Via FFI | Both embed Zend Engine |
| ZTS support | ✅ | ✅ | |
| HTTP/2 support | ✅ Via Caddy | ⚠️ Hyper only (HTTP/1.1) | Hyper 1.x doesn't support HTTP/2 server |
| HTTP/3 (QUIC) | ✅ Via Caddy | ❌ Not implemented | Requires Caddy or quinn |
| HTTPS (automatic) | ✅ Via Caddy | ❌ Not implemented | Requires Caddy |
| CGI SAPI emulation | ✅ | ✅ | |
| CLI script execution | ✅ `php-cli` | ✅ `execute_script_cli()` | |

### Configuration

| Feature | FrankenPHP | LarvaPHP | Notes |
|---------|-----------|----------|-------|
| Caddyfile config | ✅ Primary | ✅ `CaddyParser` | |
| Apache config | ⚠️ Via Caddy | ✅ `ApacheParser` | |
| Nginx config | ⚠️ Via Caddy | ✅ `NginxParser` | |
| CLI flags | ✅ `php-server` | ✅ `--root`, `--addr` | |
| php.ini directives | ✅ `php_ini` | ⚠️ Not exposed | |
| Environment variables | ✅ | ✅ | |
| Hot reload config | ✅ | ✅ | |
| Shell completion | ✅ Bash/Zsh/Fish/PS | ❌ | |

### Worker Mode

| Feature | FrankenPHP | LarvaPHP | Notes |
|---------|-----------|----------|-------|
| Persistent workers | ✅ | ✅ `Worker` struct | |
| Worker request matching | ✅ `match` directive | ⚠️ Not implemented | |
| Watch for file changes | ✅ `--watch` | ✅ Via watcher | |
| Max consecutive failures | ✅ | ✅ | |
| Worker auto-restart | ✅ Exponential backoff | ❌ | |
| Manual restart (API) | ✅ Caddy admin API | ❌ | |
| Superglobals reset | ✅ | ✅ | |
| State persistence | ✅ Documented | ✅ | |

### Rate Limiting & Security

| Feature | FrankenPHP | LarvaPHP | Notes |
|---------|-----------|----------|-------|
| Rate limiting | ⚠️ Via Caddy `rate_limit` | ✅ Built-in `RateLimiter` | |
| Request body size limit | ✅ `request_body` | ✅ `max_body_size` | |
| Request body timeout | ✅ `request_body_timeout` | ✅ `request_body_timeout` | |
| X-Content-Type-Options | ✅ | ✅ | |
| X-Frame-Options | ✅ | ✅ `DENY` | |
| Sensitive file blocking | ❌ | ✅ `.env`, `.git/`, etc. | |
| Path traversal protection | ✅ | ✅ `sanitize_path()` | |
| Null byte rejection | ❌ | ✅ | |

### Middleware & Routing

| Feature | FrankenPHP | LarvaPHP | Notes |
|---------|-----------|----------|-------|
| .htaccess rewrite | ✅ Via Caddy | ✅ `RewriteMiddleware` | |
| Middleware chain | ⚠️ Caddy middleware | ✅ Chain of Responsibility | |
| try_files equivalent | ✅ `php_server` | ⚠️ Not implemented | |
| split_path | ✅ | ✅ | |
| resolve_root_symlink | ✅ | ❌ | |
| name (server name) | ✅ | ✅ | |

### Developer Experience

| Feature | FrankenPHP | LarvaPHP | Notes |
|---------|-----------|----------|-------|
| Hot reload (PHP) | ✅ | ✅ | |
| Hot reload (config) | ✅ | ✅ | |
| Hot reload (workers) | ✅ Restart on change | ❌ | |
| Early Hints (103) | ✅ `headers_send(103)` | ❌ | |
| Mercure hub | ✅ Built-in | ⚠️ Stub (`mercure.rs`) | |
| Vulcain | ✅ Built-in | ❌ | |
| X-Sendfile | ✅ Large file serving | ❌ | |
| Debug mode | ✅ `CADDY_GLOBAL_OPTIONS=debug` | ❌ | |
| phpinfo() | ✅ | ✅ `phpinfo.rs` | |

### Observability

| Feature | FrankenPHP | LarvaPHP | Notes |
|---------|-----------|----------|-------|
| Prometheus metrics | ✅ | ✅ `AtomicMetrics` | |
| OpenTelemetry | ⚠️ Via slog | ❌ | |
| Structured logging | ✅ slog | ✅ tracing | |
| Thread inspection | ✅ `frankenphp.Thread()` | ⚠️ Not exposed | |
| Memory profiling | ✅ `internal/memory/` | ❌ | |
| CPU detection | ✅ `internal/cpu/` | ❌ | |
| Opcache restart | ✅ Auto | ❌ | |
| Debug state | ✅ `debugstate.go` | ❌ | |

### Deployment

| Feature | FrankenPHP | LarvaPHP | Notes |
|---------|-----------|----------|-------|
| Docker images | ✅ Official | ❌ | |
| Static binaries | ✅ | ⚠️ Not tested | |
| Standalone self-executable | ✅ | ❌ | |
| systemd service | ✅ | ❌ | |
| rpm/deb/apk packages | ✅ | ❌ | |
| Homebrew | ✅ | ❌ | |
| Caddy module | ✅ | ✅ `larva-php-caddy` | |

### PHP Extension Development

| Feature | FrankenPHP | LarvaPHP | Notes |
|---------|-----------|----------|-------|
| Extensions in Go | ✅ `internal/extgen/` | ❌ | |
| Extension generator | ✅ | ⚠️ `larva-php-extgen` exists | |

### Framework Integration

| Framework | FrankenPHP | LarvaPHP | Notes |
|-----------|-----------|----------|-------|
| Symfony | ✅ Official worker | ⚠️ Manual | |
| Laravel Octane | ✅ Official worker | ⚠️ Manual | |
| WordPress | ✅ | ✅ Tested | |
| Drupal | ✅ | ❌ | |
| Joomla | ✅ | ❌ | |
| TYPO3 | ✅ | ❌ | |
| Magento2 | ✅ | ❌ | |
| API Platform | ✅ | ❌ | |

## Missing Features (Priority Order)

> **Update (2026-11-20):** The High Priority items are now implemented — see "Implemented (2026-11-20)" below.

### High Priority (Core functionality) — ✅ DONE

1. ✅ **HTTP/2 & HTTP/3 support** — `hyper-util auto::Builder` auto-detects HTTP/1.1 + HTTP/2 (h2c). HTTP/3 (QUIC) still future work.
2. ✅ **Early Hints (103)** — `parse_cgi_output` detects `Status: 103`/`Link:` and surfaces preload headers.
3. ✅ **try_files equivalent** — missing script falls back to `index.php` (front controller / permalinks).
4. ✅ **Worker request matching** — `WorkerRouter` + `PrefixMatcher`/`RegexMatcher` (Strategy); `Server::find_worker_by_request()`.
5. ✅ **php.ini directives** — `--php-ini KEY=VALUE` via `PHP_INI_SCAN_DIR` (preserves main php.ini, cached).
6. ⬜ **Worker auto-restart on file change** — Restart workers when PHP files change (not yet).

### Implemented (2026-11-20)

- **try_files fallback** — `start_server` + `serve_http` fall back to `index.php` when the resolved path is missing.
- **Early Hints (103)** — `parse_cgi_output` merges `Link:` preload headers. Also fixed `sanitize_path` dropping the path root (Windows `C:` / `\`), a bug that previously routed **every** `.php` request to `index.php`.
- **Worker request matching** — `worker.rs` gains `WorkerRouter` (Chain of Responsibility) with `PrefixMatcher` + `RegexMatcher`; wired into the request handler.
- **php.ini config** — `Server::set_php_ini()` + repeatable `--php-ini` CLI flag → `write_ini_overrides()` (per-process cache) → `PHP_INI_SCAN_DIR`.
- **HTTP/2** — served by `hyper_util::server::conn::auto::Builder` (already in use).

### Medium Priority (Developer experience)

7. **Mercure hub implementation** - `mercure.rs` is a stub
8. **X-Sendfile** - Efficient large static file serving
9. **Debug mode** - `CADDY_GLOBAL_OPTIONS=debug`
10. **Shell completion** - Bash, Zsh, Fish, PowerShell
11. **Worker manual restart API** - Admin endpoint for worker restart
12. **Thread inspection API** - `frankenphp.Thread()` equivalent
13. **resolve_root_symlink** - Symbolic link resolution for document root

### Low Priority (Deployment/ecosystem)

14. **Docker images** - Official Docker images
15. **Standalone self-executable** - Embed PHP app in binary
16. **Static binaries** - Fully static linking
17. **systemd service** - Service management
18. **rpm/deb/apk packages** - Distribution packages
19. **Vulcain** - Content negotiation for HTTP caching
20. **Full HTTP/2 & HTTP/3** - Via Caddy or custom implementation

## Architecture Differences

### FrankenPHP (Go)
- **HTTP server**: Caddy (full-featured, HTTP/2, HTTP/3, HTTPS, compression)
- **PHP embedding**: CGO to Zend Engine
- **Threading**: Go goroutines + PHP threads
- **Config**: Caddyfile (JSON, YAML, TOML also supported)
- **Distribution**: Static binaries, Docker, packages

### LarvaPHP (Rust)
- **HTTP server**: Hyper (HTTP/1.1, no built-in HTTP/2/3)
- **PHP embedding**: FFI to Zend Engine (via `larva-php-sys`)
- **Threading**: Tokio tasks + PHP threads
- **Config**: Multi-format parsers (Strategy pattern)
- **Distribution**: Cargo build, no packages yet

## Recommendations for Parity

To achieve feature parity with FrankenPHP, LarvaPHP should prioritize:

1. **Implement Early Hints (103)** - Low effort, high impact (30% faster page loads)
2. **Implement try_files** - Essential for most PHP frameworks
3. **Add HTTP/2 support** - Switch to hyper-util or add h2 support
4. **Complete Mercure hub** - Finish the stub in `mercure.rs`
5. **Add worker file watching** - Restart workers on PHP file changes
6. **Expose php.ini configuration** - Via `ServerOption` or `RequestOption`
7. **Add shell completion** - Via clap or manual implementation
8. **Implement X-Sendfile** - Large file serving for performance