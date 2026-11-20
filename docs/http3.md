# HTTP/3 (QUIC) Support

LarvaPHP supports **HTTP/3** via [cloudflare/quiche](https://github.com/cloudflare/quiche),
a QUIC + HTTP/3 implementation. HTTP/3 runs over UDP; browsers that don't
support it transparently fall back to HTTP/2 or HTTP/1.1 over TCP.

## Requirements

Building with HTTP/3 needs:

- **NASM** assembler (for BoringSSL): `winget install nasm.nasm`
- **LLVM / libclang** (for bindgen): `winget install LLVM.LLVM`, then set
  `LIBCLANG_PATH=C:\Program Files\LLVM\bin`
- A C/C++ toolchain (MSVC on Windows, gcc/clang on Linux)

## Building

```powershell
# One-time: install build deps
winget install nasm.nasm
winget install LLVM.LLVM

# Build with HTTP/3
$env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"
cargo build --release -p larva-php --features http3
```

The default build (`cargo build`) does **not** include HTTP/3 — quiche is an
optional dependency behind the `http3` feature, so normal builds stay fast and
dependency-free.

## Running

```powershell
# HTTP/3 on the same port as HTTP (UDP). A self-signed cert is auto-generated.
larva-php --root C:\www\app --addr 127.0.0.1:8000 --http3

# Or provide your own certificate
larva-php --root C:\www\app --addr 127.0.0.1:8000 --http3 --cert cert.pem --key key.pem
```

## How the fallback works

Both transports share the **same request core** (`Server::handle_request_parts`):

```
                    ┌─────────────────────────────┐
   HTTP/1.1, h2 ───▶│  TCP listener (hyper)        │
   (TCP)            │  auto::Builder               │
                    └──────────────┬──────────────┘
                                   │ handle_request_parts()
                    ┌──────────────▼──────────────┐
   HTTP/3 (h3) ────▶│  UDP listener (quiche/QUIC)  │
   (UDP)            │  http3.rs                    │
                    └─────────────────────────────┘
```

- **Browser with HTTP/3** → connects over QUIC/UDP (ALPN `h3`).
- **Browser without HTTP/3** → uses TCP HTTP/2 (ALPN `h2`) or HTTP/1.1.

The TCP responses carry an `Alt-Svc: h3=":PORT"; ma=86400` header (set via
`server::set_http3_port`) so capable browsers discover and upgrade to HTTP/3
automatically, while others keep using TCP — no user action needed.

## Design

- **Strategy / Adapter**: `Http3Server` (quiche) and the hyper TCP server are
  two transports that both delegate to `Server::handle_request_parts`.
- **No protocol conflict**: each transport owns its own socket (TCP vs UDP) and
  ALPN token; they never interfere.
- **Feature-gated**: the `http3` feature pulls in quiche + rcgen only when
  enabled; the default build compiles a no-op stub.

## Testing

```powershell
# Verify HTTP/3 is listening (UDP)
.\target\release\larva-php.exe --root . --addr 127.0.0.1:8000 --http3
# Log: "HTTP/3 (QUIC) listening on 127.0.0.1:8000"

# With a client that supports h3 (e.g. curl with HTTP/3 support):
curl --http3 -k https://127.0.0.1:8000/index.php
```