//! Environment variable management.
//!
//! Mirrors `env.go` from the Go source. Initializes PHP's environment
//! variables from the OS environment and manages prepared environments
//! (per-server and per-request env vars).

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::ffi;

/// Thread-local prepared environment: key\0 -> value.
pub type PreparedEnv = HashMap<Vec<u8>, Vec<u8>>;

/// Length of the OS environment snapshot, used for CGI var pre-allocation.
pub(crate) static LENGTH_OF_ENV: AtomicUsize = AtomicUsize::new(0);

/// Initialize PHP's environment from the OS environment.
///
/// This is called from the C side (via `go_init_os_env` callback in Go,
/// now called directly from Rust).
///
/// # Safety
///
/// `main_thread_env` must be a valid pointer to a PHP HashTable (zval*).
pub unsafe fn init_os_env(main_thread_env: *mut ffi::zval) {
    let env_vars: Vec<(String, String)> = std::env::vars().collect();
    LENGTH_OF_ENV.store(env_vars.len(), Ordering::Release);

    for (key, val) in &env_vars {
        let zkey = new_persistent_zend_string(key);
        let zstr = new_persistent_zend_string(val);
        ffi::__hash_update_string__(
            main_thread_env as *mut ffi::zend_array,
            zkey,
            zstr,
        );
    }
}

/// Create a persistent (interned) Zend string.
///
/// The string is allocated with `pemalloc(..., 1)` and marked as interned,
/// so the GC ignores it. It lives for the entire process lifetime.
///
/// # Safety
///
/// The Zend Engine must be initialized.
pub unsafe fn new_persistent_zend_string(s: &str) -> *mut ffi::zend_string {
    if s.is_empty() {
        return std::ptr::null_mut();
    }
    let cstr = CString::new(s).unwrap_or_default();
    ffi::frankenphp_init_persistent_string(cstr.as_ptr(), s.len())
}

/// Prepare an environment map for use as a prepared env.
///
/// Keys are NUL-terminated (matching the Go source's `PrepareEnv`).
/// Empty keys are dropped.
pub fn prepare_env(env: HashMap<String, String>) -> PreparedEnv {
    let mut prepared = PreparedEnv::with_capacity(env.len());
    for (k, v) in env {
        if k.is_empty() {
            continue;
        }
        let mut key = k.into_bytes();
        if key.last() != Some(&0) {
            key.push(0);
        }
        prepared.insert(key, v.into_bytes());
    }
    prepared
}

/// Ensure all keys in a prepared env are NUL-terminated.
pub fn ensure_prepared_env(env: PreparedEnv) -> PreparedEnv {
    if env.keys().all(|k| k.last() == Some(&0)) {
        return env;
    }
    env.into_iter()
        .filter(|(k, _)| !k.is_empty())
        .map(|(mut k, v)| {
            if k.last() != Some(&0) {
                k.push(0);
            }
            (k, v)
        })
        .collect()
}

/// Register prepared environment variables for `getenv()`.
///
/// # Safety
///
/// `fc_env` and `server_env` must be valid prepared env maps.
/// The Zend Engine must be initialized on the current thread.
pub unsafe fn register_prepared_env(
    fc_env: &PreparedEnv,
    server_env: &PreparedEnv,
    prepared_env_len: usize,
) {
    for (k, v) in server_env {
        let name_len = k.len().saturating_sub(1); // strip NUL
        ffi::frankenphp_add_to_prepared_env(
            k.as_ptr() as *mut c_char,
            name_len,
            v.as_ptr() as *mut c_char,
            v.len(),
            prepared_env_len,
        );
    }
    for (k, v) in fc_env {
        let name_len = k.len().saturating_sub(1);
        ffi::frankenphp_add_to_prepared_env(
            k.as_ptr() as *mut c_char,
            name_len,
            v.as_ptr() as *mut c_char,
            v.len(),
            prepared_env_len,
        );
    }
}

/// Get a CString from a string, returning null on failure.
pub(crate) fn to_cstring(s: &str) -> Option<CString> {
    CString::new(s).ok()
}

/// Get the length of the OS environment snapshot.
pub fn length_of_env() -> usize {
    LENGTH_OF_ENV.load(Ordering::Acquire)
}

/// Read a C string into a Rust String.
///
/// # Safety
///
/// `ptr` must be a valid, NUL-terminated C string.
pub unsafe fn cstr_to_string(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    CStr::from_ptr(ptr).to_string_lossy().into_owned()
}

/// Read a C string slice into a Rust String.
///
/// # Safety
///
/// `ptr` must be valid for `len` bytes.
pub unsafe fn cstr_to_string_n(ptr: *const c_char, len: usize) -> String {
    if ptr.is_null() || len == 0 {
        return String::new();
    }
    let bytes = std::slice::from_raw_parts(ptr as *const u8, len);
    String::from_utf8_lossy(bytes).into_owned()
}