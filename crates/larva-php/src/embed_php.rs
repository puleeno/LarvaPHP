//! Embedded PHP runtime via FFI (in-process, no CGI subprocess).
//!
//! Loads `php8.dll` (NTS) or `php8ts.dll` (ZTS) dynamically and runs PHP
//! inside the worker process, like `mod_php`. This eliminates the per-request
//! `php-cgi.exe` spawn.
//!
//! The struct layouts below are taken verbatim from **PHP 8.3** (`main/SAPI.h`,
//! `Zend/zend_stream.h`) so the ABI matches. If you build against a different
//! PHP version, regenerate these (bindgen) or adjust them.
//!
//! The NTS build is not thread-safe; callers serialize PHP access (the worker
//! process runs one request at a time).

#[cfg(feature = "embedded")]
pub mod imp {
    use std::ffi::{c_char, c_int, c_uint, c_void, CString};
    use std::cell::RefCell;

    use crate::errors::{Error, Result};

    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryA(name: *const u8) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    }

    // =========================================================================
    // PHP 8.3 ABI (verbatim from main/SAPI.h)
    // =========================================================================

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
        get_stat: Option<unsafe extern "C" fn() -> *mut c_void>,
        getenv: Option<unsafe extern "C" fn(*const c_char, usize) -> *mut c_char>,
        sapi_error: Option<unsafe extern "C" fn(c_int, *const c_char, ...)>,
        header_handler: Option<unsafe extern "C" fn(*mut c_void, c_int, *mut c_void) -> c_int>,
        send_headers: Option<unsafe extern "C" fn(*mut c_void) -> c_int>,
        send_header: Option<unsafe extern "C" fn(*mut c_void, *mut c_void)>,
        read_post: Option<unsafe extern "C" fn(*mut c_char, usize) -> usize>,
        read_cookies: Option<unsafe extern "C" fn() -> *mut c_char>,
        register_server_variables: Option<unsafe extern "C" fn(*mut c_void)>,
        log_message: Option<unsafe extern "C" fn(*const c_char, c_int)>,
        get_request_time: Option<unsafe extern "C" fn(*mut f64) -> c_int>,
        terminate_process: Option<unsafe extern "C" fn()>,
        php_ini_path_override: *const c_char,
        default_post_reader: Option<unsafe extern "C" fn()>,
        treat_data: Option<unsafe extern "C" fn(c_int, *mut c_char, *mut c_void)>,
        executable_location: *const c_char,
        php_ini_ignore: c_int,
        php_ini_ignore_cwd: c_int,
        get_fd: Option<unsafe extern "C" fn(*mut c_int) -> c_int>,
        force_http_10: Option<unsafe extern "C" fn() -> c_int>,
        get_target_uid: Option<unsafe extern "C" fn(*mut u32) -> c_int>,
        get_target_gid: Option<unsafe extern "C" fn(*mut u32) -> c_int>,
        input_filter: Option<
            unsafe extern "C" fn(c_int, *const c_char, *mut *mut c_char, usize, *mut usize) -> c_uint,
        >,
        ini_defaults: Option<unsafe extern "C" fn(*mut c_void)>,
        phpinfo_as_text: c_int,
        ini_entries: *const c_char,
        additional_functions: *const c_void,
        input_filter_init: Option<unsafe extern "C" fn() -> c_uint>,
    }

    impl SapiModule {
        fn new() -> Self {
            unsafe { std::mem::zeroed() }
        }
    }

    /// An 8-byte-aligned, generously sized buffer for `zend_file_handle`.
    /// `zend_stream_init_filename` fills it; 256 B covers the real ~48 B struct.
    #[repr(C, align(8))]
    struct FileHandleBuf([u8; 256]);

    // Function pointer types.
    type FnSapiStartup = unsafe extern "C" fn(*mut c_void);
    type FnPhpModuleStartup = unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int;
    type FnPhpRequestStartup = unsafe extern "C" fn() -> c_int;
    type FnPhpRequestShutdown = unsafe extern "C" fn(*mut c_void);
    type FnPhpExecuteScript = unsafe extern "C" fn(*mut c_void);
    type FnZendStreamInitFilename = unsafe extern "C" fn(*mut c_void, *const c_char);
    type FnZendParseIniString = unsafe extern "C" fn(
        *const c_char,
        bool,
        c_int,
        *mut c_void,
        *mut c_void,
    ) -> c_int;

    struct PhpApi {
        sapi_startup: FnSapiStartup,
        php_module_startup: FnPhpModuleStartup,
        php_request_startup: FnPhpRequestStartup,
        php_request_shutdown: FnPhpRequestShutdown,
        php_execute_script: FnPhpExecuteScript,
        zend_stream_init_filename: FnZendStreamInitFilename,
        zend_parse_ini_string: Option<FnZendParseIniString>,
    }

    unsafe impl Send for PhpApi {}
    unsafe impl Sync for PhpApi {}

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

    // Minimal SAPI callbacks (must not be NULL — PHP calls them).
    unsafe extern "C" fn flush_noop(_: *mut c_void) {}
    unsafe extern "C" fn send_headers_ok(_: *mut c_void) -> c_int {
        1 // SAPI_HEADER_SENT_SUCCESSFULLY
    }
    unsafe extern "C" fn send_header_noop(_: *mut c_void, _: *mut c_void) {}
    unsafe extern "C" fn log_message_noop(msg: *const c_char, _syslog_type_int: c_int) {
        if !msg.is_null() {
            let s = std::ffi::CStr::from_ptr(msg).to_string_lossy();
            eprintln!("[php] {}", s);
        }
    }
    unsafe extern "C" fn read_post_zero(_: *mut c_char, _: usize) -> usize {
        0
    }
    unsafe extern "C" fn read_cookies_null() -> *mut c_char {
        std::ptr::null_mut()
    }

    fn load_symbol(module: *mut c_void, name: &str) -> Option<*mut c_void> {
        let cname = CString::new(name).ok()?;
        let p = unsafe { GetProcAddress(module, cname.as_ptr() as *const u8) };
        if p.is_null() { None } else { Some(p) }
    }

    /// Loaded PHP API.
    struct PhpApiLoaded {
        api: PhpApi,
        /// Kept alive for the PHP runtime's lifetime (PHP holds a pointer to it).
        _sapi: Box<SapiModule>,
    }

    unsafe impl Send for PhpApiLoaded {}
    unsafe impl Sync for PhpApiLoaded {}

    impl PhpApiLoaded {
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
                ($name:literal, $ty:ty) => {{
                    let p = load_symbol(module, $name).ok_or_else(|| {
                        Error::Ffi(format!("missing symbol: {}", $name))
                    })?;
                    unsafe { std::mem::transmute::<*mut c_void, $ty>(p) }
                }};
            }

            let api = PhpApi {
                sapi_startup: sym!("sapi_startup", FnSapiStartup),
                php_module_startup: sym!("php_module_startup", FnPhpModuleStartup),
                php_request_startup: sym!("php_request_startup", FnPhpRequestStartup),
                php_request_shutdown: sym!("php_request_shutdown", FnPhpRequestShutdown),
                php_execute_script: sym!("php_execute_script", FnPhpExecuteScript),
                zend_stream_init_filename: sym!("zend_stream_init_filename", FnZendStreamInitFilename),
                // zend_parse_ini_string is optional; load if present.
                zend_parse_ini_string: load_symbol(module, "zend_parse_ini_string")
                    .map(|p| unsafe { std::mem::transmute::<*mut c_void, FnZendParseIniString>(p) }),
            };

            let mut sapi = Box::new(SapiModule::new());
            sapi.name = b"larva-embedded\0".as_ptr() as *const c_char;
            sapi.pretty_name = b"LarvaPHP Embedded\0".as_ptr() as *const c_char;
            sapi.ub_write = Some(ub_write);
            sapi.flush = Some(flush_noop);
            sapi.send_headers = Some(send_headers_ok);
            sapi.send_header = Some(send_header_noop);
            sapi.log_message = Some(log_message_noop);
            sapi.read_post = Some(read_post_zero);
            sapi.read_cookies = Some(read_cookies_null);

            // Resolve PHP INI configuration (Strategy + Chain of Responsibility).
            // The embedded runtime does NOT scan conf.d, so we set:
            //   1. php_ini_path_override -> the php.ini file
            //   2. PHPRC -> the php.ini directory (for extension_dir resolution)
            //   3. ini_entries -> force-load extensions (mysqli, pdo_mysql, ...)
            let ini_cfg = crate::runtime::php_ini::resolve();

            if let Some(ref dir) = ini_cfg.dir {
                std::env::set_var("PHPRC", dir);
            }

            if let Some(ref path) = ini_cfg.path {
                if let Ok(cstr) = CString::new(path.to_string_lossy().to_string()) {
                    // Leak the CString: PHP holds the pointer for the process lifetime.
                    let ptr = Box::into_raw(Box::new(cstr)) as *const c_char;
                    sapi.php_ini_path_override = ptr;
                }
                tracing::info!("embedded PHP using php.ini: {}", path.display());
            }

            if let Some(ref entries) = ini_cfg.entries {
                if let Ok(cstr) = CString::new(entries.clone()) {
                    // Leak: PHP holds the ini_entries pointer for the process lifetime.
                    let ptr = Box::into_raw(Box::new(cstr)) as *const c_char;
                    sapi.ini_entries = ptr;
                    tracing::info!("embedded PHP ini_entries: {} bytes", entries.len());
                }
            }

            unsafe {
                // sapi_startup copies the struct into the global sapi_module.
                (api.sapi_startup)(&mut *sapi as *mut SapiModule as *mut c_void);
                // php_module_startup initializes the Zend Engine + standard modules.
                // php_init_config() (called inside) reads php_ini_path_override
                // and ini_entries that we set above.
                let rc = (api.php_module_startup)(
                    &mut *sapi as *mut SapiModule as *mut c_void,
                    std::ptr::null_mut(),
                );
                if rc != 0 {
                    return Err(Error::Ffi(format!("php_module_startup failed: {}", rc)));
                }
            }

            tracing::info!("embedded PHP runtime loaded (php8.dll)");

            Ok(Self { api, _sapi: sapi })
        }

        fn execute(&self, script_path: &str) -> Result<Vec<u8>> {
            OUTPUT.with(|o| o.borrow_mut().clear());

            let cpath = CString::new(script_path)
                .map_err(|_| Error::Ffi("invalid script path".to_string()))?;

            unsafe {
                let rc = (self.api.php_request_startup)();
                if rc != 0 {
                    return Err(Error::Ffi(format!("php_request_startup failed: {}", rc)));
                }

                let mut fh = FileHandleBuf([0u8; 256]);
                let fh_ptr = &mut fh as *mut FileHandleBuf as *mut c_void;
                (self.api.zend_stream_init_filename)(fh_ptr, cpath.as_ptr());
                (self.api.php_execute_script)(fh_ptr);

                (self.api.php_request_shutdown)(std::ptr::null_mut());
            }

            Ok(OUTPUT.with(|o| o.borrow().clone()))
        }
    }

    fn runtime() -> Option<&'static PhpApiLoaded> {
        static RT: std::sync::OnceLock<Option<PhpApiLoaded>> = std::sync::OnceLock::new();
        RT.get_or_init(|| PhpApiLoaded::load().ok()).as_ref()
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