//! Per-request context.
//!
//! Mirrors `context.go` from the Go source. Holds the request context that
//! is passed to PHP threads for execution.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crate::errors::{Error, Result};
use crate::env::PreparedEnv;

/// Trait for options that configure a request.
pub trait RequestOption: Send + Sync {
    fn apply(&self, ctx: &mut PhpContext) -> Result<()>;
}

/// The context for a single PHP request.
///
/// This is the Rust equivalent of `frankenPHPContext` in Go.
pub struct PhpContext {
    /// The request context (cancellation, deadlines, etc.)
    pub ctx: tokio::sync::watch::Receiver<Option<()>>,
    /// Document root directory
    pub document_root: String,
    /// Split path patterns (e.g. [".php"])
    pub split_path: Vec<String>,
    /// Prepared environment variables
    pub env: PreparedEnv,
    /// Logger for this request
    pub logger: tracing::Level,
    /// The assigned worker (if any)
    pub worker: Option<Arc<crate::worker::Worker>>,
    /// The server block
    pub server: Arc<crate::server::Server>,
    /// Request body idle timeout
    pub request_body_timeout: Option<std::time::Duration>,
    /// Document URI (path minus PATH_INFO)
    pub doc_uri: String,
    /// PATH_INFO
    pub path_info: String,
    /// SCRIPT_NAME
    pub script_name: String,
    /// SCRIPT_FILENAME (absolute path)
    pub script_filename: String,
    /// REQUEST_URI
    pub request_uri: String,
    /// Whether headers have been written
    pub headers_written: bool,
    /// Whether the request is done
    pub is_done: AtomicBool,
    /// Client connection state at the time is_done was set
    pub client_had_closed: std::sync::atomic::AtomicBool,
    /// When the request started
    pub started_at: Instant,
    /// Handler return value
    pub handler_return: Option<crate::types::PhpValue>,
}

/// Builder for creating a PhpContext.
pub struct PhpContextBuilder {
    ctx: PhpContext,
}

impl PhpContext {
    /// Create a new context from an HTTP request.
    pub fn from_request(
        request: &hyper::Request<hyper::body::Incoming>,
        server: Arc<crate::server::Server>,
        opts: Vec<Box<dyn RequestOption>>,
    ) -> Result<Self> {
        let req_uri = request.uri().to_string();

        let mut ctx = PhpContext {
            ctx: tokio::sync::watch::channel(None).1,
            document_root: server.root().to_string(),
            split_path: server.split_path().to_vec(),
            env: PreparedEnv::new(),
            logger: server.logger_level(),
            worker: None,
            server,
            request_body_timeout: None,
            doc_uri: String::new(),
            path_info: String::new(),
            script_name: String::new(),
            script_filename: String::new(),
            request_uri: req_uri,
            headers_written: false,
            is_done: AtomicBool::new(false),
            client_had_closed: std::sync::atomic::AtomicBool::new(false),
            started_at: Instant::now(),
            handler_return: None,
        };

        for opt in &opts {
            opt.apply(&mut ctx)?;
        }

        Ok(ctx)
    }

    /// Create a dummy context for worker startup.
    pub fn worker_dummy(worker: &Arc<crate::worker::Worker>) -> Self {
        let server = worker.server().cloned().unwrap_or_else(|| {
            Arc::new(crate::server::Server::fallback())
        });

        PhpContext {
            ctx: tokio::sync::watch::channel(None).1,
            document_root: worker.file_name().to_string(),
            split_path: server.split_path().to_vec(),
            env: PreparedEnv::new(),
            logger: server.logger_level(),
            worker: Some(worker.clone()),
            server,
            request_body_timeout: None,
            doc_uri: String::new(),
            path_info: String::new(),
            script_name: String::new(),
            script_filename: worker.file_name().to_string(),
            request_uri: String::new(),
            headers_written: false,
            is_done: AtomicBool::new(false),
            client_had_closed: std::sync::atomic::AtomicBool::new(false),
            started_at: Instant::now(),
            handler_return: None,
        }
    }

    /// Create a context from a message (for external workers).
pub fn from_message(
        message: crate::types::PhpValue,
        server: Arc<crate::server::Server>,
        worker: Arc<crate::worker::Worker>,
    ) -> Self {
        PhpContext {
            ctx: tokio::sync::watch::channel(None).1,
            document_root: server.root().to_string(),
            split_path: server.split_path().to_vec(),
            env: PreparedEnv::new(),
            logger: server.logger_level(),
            worker: Some(worker.clone()),
            server,
            request_body_timeout: None,
            doc_uri: String::new(),
            path_info: String::new(),
            script_name: String::new(),
            script_filename: worker.file_name().to_string(),
            request_uri: String::new(),
            headers_written: false,
            is_done: AtomicBool::new(false),
            client_had_closed: std::sync::atomic::AtomicBool::new(false),
            started_at: Instant::now(),
            handler_return: Some(message),
        }
    }

    /// Close the context, signaling that the request is done.
    pub fn close_context(&self) {
        if self.is_done.load(Ordering::Acquire) {
            return;
        }
        self.client_had_closed.store(self.client_has_closed(), Ordering::Release);
        self.is_done.store(true, Ordering::Release);
    }

    /// Check if the client has closed the connection.
    pub fn client_has_closed(&self) -> bool {
        self.ctx.borrow().is_some()
    }

    /// Reject the request with an error.
    pub fn reject(&self, err: Error) {
        if self.is_done.load(Ordering::Acquire) {
            return;
        }
        if let Error::Rejected(_re) = err {
            self.close_context();
        }
    }

    /// Validate the request.
    pub fn validate(&self) -> Result<()> {
        if self.request_uri.contains('\0') {
            return Err(Error::InvalidRequestPath);
        }
        Ok(())
    }

    /// Check if the context is done.
    pub fn is_done(&self) -> bool {
        self.is_done.load(Ordering::Acquire)
    }
}