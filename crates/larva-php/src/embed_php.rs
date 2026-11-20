//! Embedded PHP runtime via FFI (in-process, no CGI subprocess).
//!
//! **EXPERIMENTAL — requires PHP development headers.**
//!
//! This loads `php8.dll` (or `php8ts.dll`) dynamically and runs PHP inside the
//! LarvaPHP process, like `mod_php`, eliminating the per-request `php-cgi.exe`
//! spawn. However, calling into the Zend Engine requires the **exact C ABI**:
//! the `sapi_module_struct` and `zend_file_handle` layouts must match the PHP
//! build byte-for-byte. Without PHP development headers (bindgen), the struct
//! layout is wrong and PHP crashes.
//!
//! To make this work you must:
//! 1. Install the PHP development package (provides headers + import library).
//! 2. Generate exact bindings with bindgen (see `larva-php-sys`), OR
//! 3. Vendor the PHP source tree and build against it.
//!
//! Until then this module is **not** wired into `execute_php_cgi`; it exists as
//! a reference implementation. Enable with `--features embedded` (off by default).
//! The NTS (`php8.dll`) build serializes PHP behind a global mutex; the ZTS
//! (`php8ts.dll`) build would allow true concurrency.

#[cfg(feature = "embedded")]
pub mod imp {
    use std::ffi::{c_char, c_int, c_void, CString};
    use std::cell::RefCell;
    use std::sync::{Mutex, OnceLock};

    use crate::errors::{Error, Result};

    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryA(name: *const u8) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    }

    type FnSapiStartup = unsafe extern "C" fn(*mut c_void) -> c_int;
    type FnPhpModuleStartup = unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int;
    type FnPhpRequestStartup = unsafe extern "C" fn() -> c_int;
    type FnPhpRequestShutdown = unsafe extern "C" fn(*mut c_void);
    type FnPhpExecuteScript = unsafe extern "C" fn(*mut c_void);
    type FnZendStreamInitFilename = unsafe extern "C" fn(*mut c_void, *const c_char);

    /// Loaded PHP API function pointers.
    struct PhpApi {
        sapi_startup: FnSapiStartup,
        php_module_startup: FnPhpModuleStartup,
        php_request_startup: FnPhpRequestStartup,
        php_request_shutdown: FnPhpRequestShutdown,
        php_execute_script: FnPhpExecuteScript,
        zend_stream_init_filename: FnZendStreamInitFilename,
    }

    unsafe impl Send for PhpApi {}
    unsafe impl Sync for PhpApi {}

    /// The minimal `sapi_module_struct` prefix. The first 8 fields match
    /// PHP 8.x's `sapi_module_struct`; the remainder is zeroed padding so
    // `sapi_startup` can read whatever trailing fields it expects.
    #[repr(C)]
    struct SapiModule {
        name: *const c_char,
        pretty_name: *const c_char,
        startup: Option<unsafe extern "C" fn(*mut SapiModule) -> c_int>,
        shutdown: Option<unsafe extern "C" fn(*mut SapiModule) -> c_int>,
        activate: Option<unsafe extern "C" fn() -> c_int>,
        deactivate: Option<unsafe extern "C" fn() -> c_int>,
        ub_write: Option<unsafe extern "C" fn(*const c_char, usize) -> usize>,
        flush: Option<unsafe extern "C" fn(*mut c_void)>,
        _rest: [u8; 960],
    }

    impl SapiModule {
        fn new() -> Self {
            let mut m: SapiModule = unsafe { std::mem::zeroed() };
            m.name = b"larva-embedded\0".as_ptr() as *const c_char;
            m.pretty_name = b"LarvaPHP Embedded\0".as_ptr() as *const c_char;
            m.ub_write = Some(ub_write);
            m
        }
    }

    /// `zend_file_handle` prefix matching PHP 8.x.
    #[repr(C)]
    struct ZendFileHandle {
        type_: c_int,
        filename: *const c_char,
        opened_path: *mut c_void,
        handle: *mut c_void,
        primary_script: u8,
        _pad: u8,
        _reserved: *mut c_void,
    }

    impl ZendFileHandle {
        fn new() -> Self {
            unsafe { std::mem::zeroed() }
        }
    }

    thread_local! {
        static OUTPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    }

    /// SAPI `ub_write` callback: append to the thread-local output buffer.
    unsafe extern "C" fn ub_write(ptr: *const c_char, len: usize) -> usize {
        if !ptr.is_null() && len > 0 {
            let bytes = std::slice::from_raw_parts(ptr as *const u8, len);
            OUTPUT.with(|o| o.borrow_mut().extend_from_slice(bytes));
        }
        len
    }

    fn load_symbol(module: *mut c_void, name: &str) -> Option<*mut c_void> {
        let cname = CString::new(name).ok()?;
        let p = unsafe { GetProcAddress(module, cname.as_ptr() as *const u8) };
        if p.is_null() { None } else { Some(p) }
    }

    /// The embedded PHP runtime, initialized once.
    struct Embedded {
        api: PhpApi,
        /// Held alive for the PHP runtime's lifetime (PHP keeps a pointer to it).
        #[allow(dead_code)]
        sapi: Box<SapiModule>,
        lock: Mutex<()>,
    }

    unsafe impl Send for Embedded {}
    unsafe impl Sync for Embedded {}

    impl Embedded {
        /// Load php8.dll (or php8ts.dll) and run module startup.
        fn load() -> Result<Self> {
            let candidates = ["php8ts.dll", "php8.dll"];
            let mut module = std::ptr::null_mut();
            for name in &candidates {
                let cname = CString::new(*name).unwrap();
                let m = unsafe { LoadLibraryA(cname.as_ptr() as *const u8) };
                if !m.is_null() {
                    module = m;
                    break;
                }
            }
            if module.is_null() {
                return Err(Error::Ffi("php8.dll / php8ts.dll not found".to_string()));
            }

            macro_rules! sym {
                ($field:ident, $name:literal, $ty:ty) => {{
                    let p = load_symbol(module, $name).ok_or_else(|| {
                        Error::Ffi(format!("missing symbol: {}", $name))
                    })?;
                    unsafe { std::mem::transmute::<*mut c_void, $ty>(p) }
                }};
            }

            let api = PhpApi {
                sapi_startup: sym!(sapi_startup, "sapi_startup", FnSapiStartup),
                php_module_startup: sym!(php_module_startup, "php_module_startup", FnPhpModuleStartup),
                php_request_startup: sym!(php_request_startup, "php_request_startup", FnPhpRequestStartup),
                php_request_shutdown: sym!(php_request_shutdown, "php_request_shutdown", FnPhpRequestShutdown),
                php_execute_script: sym!(php_execute_script, "php_execute_script", FnPhpExecuteScript),
                zend_stream_init_filename: sym!(zend_stream_init_filename, "zend_stream_init_filename", FnZendStreamInitFilename),
            };

            let mut sapi = Box::new(SapiModule::new());

            unsafe {
                // sapi_startup copies the module struct.
                (api.sapi_startup)(&mut *sapi as *mut SapiModule as *mut c_void);
                // php_module_startup initializes the Zend Engine + standard modules.
                let rc = (api.php_module_startup)(&mut *sapi as *mut SapiModule as *mut c_void, std::ptr::null_mut());
                if rc != 0 {
                    return Err(Error::Ffi(format!("php_module_startup failed: {}", rc)));
                }
            }

            tracing::info!("Embedded PHP runtime loaded (php8.dll)");

            Ok(Self {
                api,
                sapi,
                lock: Mutex::new(()),
            })
        }

        /// Execute a PHP script in-process and return its output.
        fn execute(&self, script_path: &str) -> Result<Vec<u8>> {
            let _guard = self.lock.lock().unwrap();

            OUTPUT.with(|o| o.borrow_mut().clear());

            let cpath = CString::new(script_path)
                .map_err(|_| Error::Ffi("invalid script path".to_string()))?;

            unsafe {
                let rc = (self.api.php_request_startup)();
                if rc != 0 {
                    return Err(Error::Ffi(format!("php_request_startup failed: {}", rc)));
                }

                let mut fh = ZendFileHandle::new();
                (self.api.zend_stream_init_filename)(
                    &mut fh as *mut ZendFileHandle as *mut c_void,
                    cpath.as_ptr(),
                );
                (self.api.php_execute_script)(&mut fh as *mut ZendFileHandle as *mut c_void);

                (self.api.php_request_shutdown)(std::ptr::null_mut());
            }

            let out = OUTPUT.with(|o| o.borrow().clone());
            Ok(out)
        }
    }

    /// Lazily-initialized global embedded PHP runtime.
    fn runtime() -> Option<&'static Embedded> {
        static RT: OnceLock<Option<Embedded>> = OnceLock::new();
        RT.get_or_init(|| Embedded::load().ok()).as_ref()
    }

    /// Execute a PHP script using the embedded runtime. Returns the script's
    /// stdout as bytes.
    pub fn execute_script(script_path: &str) -> Result<Vec<u8>> {
        match runtime() {
            Some(rt) => rt.execute(script_path),
            None => Err(Error::Ffi(
                "embedded PHP runtime not available (php8.dll not found)".to_string(),
            )),
        }
    }

    /// Whether the embedded runtime loaded successfully.
    pub fn is_available() -> bool {
        runtime().is_some()
    }
}

#[cfg(feature = "embedded")]
pub use imp::*;

#[cfg(not(feature = "embedded"))]
pub mod imp {
    use crate::errors::{Error, Result};

    pub fn execute_script(_script_path: &str) -> Result<Vec<u8>> {
        Err(Error::Ffi(
            "embedded PHP support is not enabled (build with --features embedded)".to_string(),
        ))
    }

    pub fn is_available() -> bool {
        false
    }
}

#[cfg(not(feature = "embedded"))]
pub use imp::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(feature = "embedded"))]
    #[test]
    fn embedded_stub_returns_error() {
        // Default build (no `embedded` feature): execute_script is a stub.
        assert!(imp::execute_script("/tmp/x.php").is_err());
        assert!(!imp::is_available());
    }
}