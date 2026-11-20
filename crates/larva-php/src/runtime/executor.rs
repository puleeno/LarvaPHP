//! PHP executor abstraction for the worker process.
//!
//! A worker embeds a PHP runtime (FFI) and exposes it through the
//! [`PhpExecutor`] trait. The trait is transport-agnostic so the worker can be
//! tested with a [`MockExecutor`] without linking PHP.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// A single PHP request: raw CGI-style fields (already UTF-8/bytes).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub method: String,
    pub uri: String,
    pub headers: Vec<(String, String)>,
    #[serde(with = "serde_bytes")]
    pub body: Vec<u8>,
    pub script_path: String,
}

/// The result of executing a PHP script.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    #[serde(with = "serde_bytes")]
    pub body: Vec<u8>,
}

/// Minimal serde_bytes-like module (avoids an extra dependency).
mod serde_bytes {
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(bytes)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        <Vec<u8> as serde::Deserialize>::deserialize(d)
    }
}

/// Trait for a PHP executor (embedded FFI, FastCGI client, or mock).
pub trait PhpExecutor: Send + Sync {
    /// Human-readable executor name.
    fn name(&self) -> &str;
    /// Execute a request and return the response.
    fn execute(&self, req: &Request) -> Result<Response, String>;
}

/// A mock executor that echoes the request — used to test the full master↔worker
/// pipeline without a real PHP runtime.
pub struct MockExecutor;

impl PhpExecutor for MockExecutor {
    fn name(&self) -> &str {
        "mock"
    }

    fn execute(&self, req: &Request) -> Result<Response, String> {
        let body = format!(
            "MOCK {method} {uri}\nScript: {script}\nHeaders: {n}\nBody: {b} bytes",
            method = req.method,
            uri = req.uri,
            script = req.script_path,
            n = req.headers.len(),
            b = req.body.len(),
        );
        Ok(Response {
            status: 200,
            headers: vec![("content-type".to_string(), "text/plain".to_string())],
            body: body.into_bytes(),
        })
    }
}

/// An embedded PHP executor (requires the `embedded` feature + PHP headers).
///
/// This is a stub: it wires to `crate::embed_php::execute_script`, which needs
/// the exact PHP 8.x ABI (sapi_module_struct / zend_file_handle). See
/// `embed_php.rs` for the ABI requirement.
pub struct FfiExecutor;

impl PhpExecutor for FfiExecutor {
    fn name(&self) -> &str {
        "ffi"
    }

    fn execute(&self, _req: &Request) -> Result<Response, String> {
        #[cfg(feature = "embedded")]
        {
            let out = crate::embed_php::execute_script(&_req.script_path)
                .map_err(|e| e.to_string())?;
            // Minimal CGI-output parsing: split headers/body at blank line.
            let text = String::from_utf8_lossy(&out).to_string();
            let (head, body) = match text.find("\r\n\r\n") {
                Some(p) => (&text[..p], &text[p + 4..]),
                None => match text.find("\n\n") {
                    Some(p) => (&text[..p], &text[p + 2..]),
                    None => ("", text.as_str()),
                },
            };
            let mut status = 200u16;
            let mut headers = Vec::new();
            for line in head.lines() {
                if let Some((k, v)) = line.split_once(':') {
                    if k.eq_ignore_ascii_case("status") {
                        status = v.split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(200);
                        continue;
                    }
                    headers.push((k.trim().to_string(), v.trim().to_string()));
                }
            }
            Ok(Response { status, headers, body: body.as_bytes().to_vec() })
        }
        #[cfg(not(feature = "embedded"))]
        {
            Err("embedded PHP not enabled (build with --features embedded)".to_string())
        }
    }
}

/// Construct the default executor for a worker.
pub fn default_executor() -> Arc<dyn PhpExecutor> {
    // Use FFI when the feature is compiled in and the runtime is available;
    // otherwise fall back to the mock so the pipeline stays testable.
    #[cfg(feature = "embedded")]
    {
        if crate::embed_php::is_available() {
            return Arc::new(FfiExecutor);
        }
    }
    Arc::new(MockExecutor)
}