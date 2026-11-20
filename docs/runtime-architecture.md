# Runtime Architecture (LiteSpeed-style 2-layer)

A fault-isolated, high-performance PHP runtime with two layers:

```
┌──────────────────────────────────────────────────────────────────────┐
│  LAYER 1: Master HTTP Server (Rust + Tokio + Hyper)                   │
│  • HTTP/1.1, HTTP/2, HTTP/3 (QUIC), TLS                              │
│  • Static file serving                                               │
│  • Load balancing across workers                                     │
│  • NO PHP/C code runs here — pure Rust, crash-proof                  │
└──────────────────────────────────────────────────────────────────────┘
                    │                         ▲
          request   │  (shared memory)        │ response
          (zero-copy)▼                         │
┌──────────────────────────────────────────────────────────────────────┐
│  LAYER 2: Worker Process Pool (N child processes)                    │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐                          │
│  │ Worker 1 │  │ Worker 2 │  │ Worker N │  ← each embeds libphp    │
│  │ (NTS PHP)│  │ (NTS PHP)│  │ (NTS PHP)│    via Rust FFI          │
│  └──────────┘  └──────────┘  └──────────┘                          │
│  • Segfault in one worker → master kills & respawns it               │
│  • NTS single-thread per process → no ZTS lock overhead              │
└──────────────────────────────────────────────────────────────────────┘
```

## Why two layers?

| Concern | Layer 1 (Master) | Layer 2 (Worker) |
|---------|------------------|-------------------|
| Runs PHP/C | ❌ Never | ✅ Yes (FFI) |
| Crash risk | None (safe Rust) | Isolated per process |
| Responsibility | HTTP, TLS, routing, static | PHP execution only |
| Scaling | Async (Tokio) | N processes |

**Fault isolation**: a PHP segfault, stack overflow, or `exit()` kills only the
worker process. The master survives, respawns the worker, and keeps serving.

**Performance**: each worker uses **PHP NTS (non-thread-safe)** single-threaded,
so there is no ZTS mutex overhead. The master runs N workers in parallel,
matching the FrakenPHP model but without CGO.

## IPC: zero-copy shared memory ring buffer

Each worker has a **shared memory ring buffer** mapped into both the master and
the worker:

```
Shared memory layout (per worker):
┌─────────────────────────────────────────────────┐
│ Header:                                           │
│   write_pos: AtomicU32  (master → worker)        │
│   read_pos:  AtomicU32  (worker → master)        │
│   state:     AtomicU32  (IDLE/REQ_READY/RESP...) │
├─────────────────────────────────────────────────┤
│ Data region (ring):                               │
│   [ request bytes ][ response bytes ]...          │
└─────────────────────────────────────────────────┘
```

**Zero-copy path**:
1. Master writes the HTTP request (method, URI, headers, body) into the ring.
2. Master advances `write_pos` and signals the worker (eventfd / socket).
3. Worker reads the request **in place** (no copy) and passes a raw pointer to
   the Zend Engine via FFI — the zval string points directly into the ring.
4. Worker writes the response into the ring and signals the master.
5. Master reads the response in place and streams it to the HTTP client.

On Linux, wakeups use **eventfd** (cheapest). On Windows, a **socketpair**
(TCP loopback) or named pipe. The data path never copies request/response bytes.

## Worker lifecycle

```
Master spawns: larva-php --worker --id N --shm <path> --ctrl <addr>
                    │
                    ▼
Worker boots:   attach shared memory, link libphp, php_module_startup
                    │
                    ▼
Worker loop:    wait for request → execute PHP → write response → signal
                    │
       crash/exit → master detects (waitpid) → respawn worker
```

## Configuration

```rust
let pool = WorkerPool::builder()
    .workers(4)                       // N worker processes
    .shm_size(1 << 20)                // 1 MB ring per worker
    .executor(PhpExecutor::ffi())     // or PhpExecutor::mock() for tests
    .build()?;

pool.serve_http(listener).await?;
```

## Fault isolation guarantees

| Failure | Effect | Recovery |
|---------|--------|----------|
| PHP segfault | Worker dies | Master respawns (backoff) |
| PHP `exit()` | Worker exits | Master respawns |
| OOM kill | Worker dies | Master respawns, marks unhealthy |
| Slow request | Worker busy | Master routes to another worker |
| Worker crash loop | N failures | Master backs off (exponential) |

The master is a pure-Rust Tokio process — it cannot segfault from PHP code.

## Comparison to alternatives

| Model | Perf | Isolation | Complexity |
|-------|------|-----------|------------|
| CGI (php-cgi spawn) | Low (spawn/request) | High | Low |
| FastCGI (php-fpm) | High | High | Medium |
| mod_php (Apache) | High | **None** (crash kills Apache) | Low |
| FrankenPHP (Go+CGO) | High | **None** (thread crash) | High |
| **LarvaPHP 2-layer** | High | **High** (process isolation) | Medium |

## Implementation status

- **Master HTTP server**: already built on Hyper (HTTP/1.1 + HTTP/2 + HTTP/3).
- **Shared-memory ring buffer**: see `larva_php::runtime::ipc::RingBuffer`.
- **Worker pool + process spawn**: see `larva_php::runtime::master` / `worker`.
- **PHP FFI executor**: requires PHP development headers (see `embed_php.rs`).
  A `MockExecutor` is provided so the full pipeline is testable without PHP.

## See also

- [`performance.md`](performance.md) — why per-request CGI spawn is slow
- [`architecture.md`](architecture.md) — current module breakdown
- [`feature-comparison.md`](feature-comparison.md) — vs FrankenPHP