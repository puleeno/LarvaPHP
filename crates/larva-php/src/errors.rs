//! Error types for LarvaPHP.
//!
//! Mirrors the error definitions in `frankenphp.go`.

use std::fmt;

/// Result type alias for LarvaPHP.
pub type Result<T> = std::result::Result<T, Error>;

/// Main error type for LarvaPHP operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not a LarvaPHP request")]
    InvalidRequest,

    #[error("LarvaPHP is already started")]
    AlreadyStarted,

    #[error("LarvaPHP is only compatible with PHP 8.2+")]
    InvalidPHPVersion,

    #[error("LarvaPHP is not compatible with Zend Signals, recompile PHP with --disable-zend-signals")]
    ZendSignals,

    #[error("error creating the main thread")]
    MainThreadCreation,

    #[error("error creating a PHP thread")]
    ThreadCreation,

    #[error("error during PHP script execution")]
    ScriptExecution,

    #[error("server is not registered, you must first call init() with WithServer()")]
    NotRunning,

    #[error("maximum request handling time exceeded")]
    MaxWaitTimeExceeded,

    #[error("request body too large")]
    RequestBodyTooLarge,

    #[error("max amount of overall threads reached")]
    MaxThreadsReached,

    #[error("split path contains non-ASCII characters")]
    InvalidSplitPath,

    #[error("unknown worker: {0}")]
    UnknownWorker(String),

    #[error("worker filename is invalid {0:?}: {1}")]
    InvalidWorkerFilename(String, String),

    #[error("worker file not found {0:?}: {1}")]
    WorkerFileNotFound(String, String),

    #[error("two global workers cannot have the same filename: {0:?}")]
    DuplicateWorkerFilename(String),

    #[error("worker {0:?} has a request matcher but no server scope")]
    WorkerMatcherWithoutServer(String),

    #[error("two workers in a server cannot have the same filename: {0:?}")]
    DuplicateServerWorkerFilename(String),

    #[error("two workers cannot have the same name: {0:?}")]
    DuplicateWorkerName(String),

    #[error("invalid response: {0}")]
    Rejected(RejectedError),

    #[error("failed to initialize workers: {0}")]
    WorkerInit(String),

    #[error("invalid Content-Length header: {0:?}")]
    InvalidContentLengthHeader(String),

    #[error("invalid request path")]
    InvalidRequestPath,

    #[error("invalid script path")]
    InvalidScriptPath,

    #[error("watcher support is not enabled")]
    WatcherNotEnabled,

    #[error("embedded app error: {0}")]
    EmbeddedApp(String),

    #[error("PHP FFI error: {0}")]
    Ffi(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("channel error: {0}")]
    Channel(String),

    #[error("thread error: {0}")]
    Thread(String),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

/// Mirrors the `ErrRejected` type from `frankenphp.go`.
#[derive(Debug, Clone)]
pub struct RejectedError {
    pub message: String,
    pub status: u16,
}

impl fmt::Display for RejectedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl RejectedError {
    pub fn new(message: impl Into<String>, status: u16) -> Self {
        Self {
            message: message.into(),
            status,
        }
    }
}

/// Convenience constructors for common rejected errors.
pub fn err_invalid_request_path() -> Error {
    Error::Rejected(RejectedError::new("invalid request path", 400))
}

pub fn err_invalid_script_path() -> Error {
    Error::Rejected(RejectedError::new("invalid script path", 404))
}

pub fn err_invalid_content_length_header(msg: &str) -> Error {
    Error::Rejected(RejectedError::new(
        format!("invalid Content-Length header: {:?}", msg),
        400,
    ))
}

pub fn err_max_wait_time_exceeded() -> Error {
    Error::Rejected(RejectedError::new(
        "maximum request handling time exceeded",
        503,
    ))
}

pub fn err_request_body_too_large() -> Error {
    Error::Rejected(RejectedError::new("request body too large", 413))
}

/// Check if an error is a "rejected" error (has an HTTP status).
pub fn is_rejected(err: &Error) -> bool {
    matches!(err, Error::Rejected(_))
}

/// Get the HTTP status code for a rejected error.
pub fn rejected_status(err: &Error) -> Option<u16> {
    match err {
        Error::Rejected(re) => Some(re.status),
        _ => None,
    }
}