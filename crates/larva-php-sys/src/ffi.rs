//! Raw FFI declarations to the Zend Engine.
//!
//! These mirror the C bridge in `frankenphp.c` and the `#cgo` directives
//! scattered across the Go source files. The functions here are called from
//! safe Rust wrappers in the `larva-php` crate.
//!
//! # Safety
//!
//! All functions in this module are `unsafe` to call. The caller must ensure
//! that:
//! - The Zend Engine has been initialized (via `php_module_startup` etc.)
//! - The calling thread has a valid TSRM context (in ZTS builds)
//! - Pointer arguments are valid and properly aligned
//! - Reference counting is handled correctly

#![allow(clippy::missing_safety_doc)]

use core::ffi::{c_char, c_int, c_uint, c_void};

// ============================================================================
// Opaque Zend types
// ============================================================================

/// Opaque `zval` (Zend value container). 16 bytes on 64-bit.
#[repr(C)]
pub struct zval {
    pub value: [u8; 8],
    pub u1: u32,
    pub u2: u32,
}

/// Opaque `zend_string`.
#[repr(C)]
pub struct zend_string {
    pub gc: zend_refcounted_h,
    pub h: u64,
    pub len: usize,
    pub val: [c_char; 1],
}

/// `zend_refcounted_h` header.
#[repr(C)]
pub struct zend_refcounted_h {
    pub refcount: u32,
    pub u: u32,
}

/// Opaque `zend_array` (HashTable).
#[repr(C)]
pub struct zend_array {
    pub gc: zend_refcounted_h,
    pub u: zend_array_u,
    pub nTableSize: u32,
    pub nNumUsed: u32,
    pub nNumOfElements: u32,
    pub nTableMask: u32,
    pub nInternalPointer: u32,
    pub nNextFreeElement: u64,
    pub pInternalPointer: *mut Bucket,
}

#[repr(C)]
pub struct zend_array_u {
    pub flags: u32,
    pub _unused: u32,
}

/// A single hash table bucket.
#[repr(C)]
pub struct Bucket {
    pub val: zval,
    pub h: u64,
    pub key: *mut zend_string,
}

/// Opaque `zend_class_entry`.
#[repr(C)]
pub struct zend_class_entry {
    pub _opaque: [u8; 0],
}

/// Opaque `zend_object`.
#[repr(C)]
pub struct zend_object {
    pub _opaque: [u8; 0],
}

/// Opaque `zend_module_entry`.
#[repr(C)]
pub struct zend_module_entry {
    pub size: c_uint,
    pub zend_api: c_uint,
    pub zend_debug: c_uchar,
    pub zts: c_uchar,
    pub ini_entry: *const c_char,
    pub deps: *const zend_module_dep,
    pub name: *const c_char,
    pub functions: *const zend_function_entry,
    pub module_startup_func: Option<unsafe extern "C" fn() -> c_int>,
    pub module_shutdown_func: Option<unsafe extern "C" fn() -> c_int>,
    pub request_startup_func: Option<unsafe extern "C" fn() -> c_int>,
    pub request_shutdown_func: Option<unsafe extern "C" fn() -> c_int>,
    pub info_func: Option<unsafe extern "C" fn()>,
    pub version: *const c_char,
    pub globals_size: usize,
    pub globals_id_ptr: *mut u32,
    pub globals_ptr: *mut c_void,
    pub functions_ptr: *mut c_void,
    pub module_obj: *mut c_void,
    pub type_: c_uchar,
    pub module_number: c_int,
    pub _reserved: c_int,
}

// Helper types
pub type c_uchar = u8;
pub type zend_long = i64;
pub type zend_ulong = u64;
pub type dtor_func_t = Option<unsafe extern "C" fn(*mut c_void)>;
pub type zend_module_dep = c_void;
pub type zend_function_entry = c_void;

/// Opaque `sapi_module_struct`.
#[repr(C)]
pub struct sapi_module_struct {
    pub name: *const c_char,
    pub pretty_name: *const c_char,
    pub startup: Option<unsafe extern "C" fn(*mut sapi_module_struct) -> c_int>,
    pub shutdown: Option<unsafe extern "C" fn(*mut sapi_module_struct) -> c_int>,
    pub activate: Option<unsafe extern "C" fn() -> c_int>,
    pub deactivate: Option<unsafe extern "C" fn() -> c_int>,
    pub ub_write: Option<
        unsafe extern "C" fn(*const c_char, usize) -> usize,
    >,
    pub flush: Option<unsafe extern "C" fn(*mut c_void)>,
    pub get_uid: Option<unsafe extern "C" fn() -> u64>,
    pub getenv: Option<unsafe extern "C" fn(*const c_char, usize) -> *mut c_char>,
    pub error_handler: Option<unsafe extern "C" fn(c_int, *const c_char, ...)>,
    pub header_handler: Option<
        unsafe extern "C" fn(*mut sapi_header_struct, *mut sapi_header_struct, c_uchar) -> c_int,
    >,
    pub send_headers: Option<unsafe extern "C" fn(*mut sapi_headers_struct) -> c_int>,
    pub send_header: Option<unsafe extern "C" fn(*mut sapi_header_struct, *mut c_void)>,
    pub read_post: Option<unsafe extern "C" fn(*mut c_char, usize) -> usize>,
    pub read_cookies: Option<unsafe extern "C" fn() -> *mut c_char>,
    pub register_server_variables: Option<unsafe extern "C" fn(*mut zval)>,
    pub log_message: Option<unsafe extern "C" fn(*const c_char, c_int)>,
    pub get_request_time: Option<unsafe extern "C" fn(*mut c_void)>,
    pub terminate_process: Option<unsafe extern "C" fn()>,
    pub php_ini_path_override: *mut c_char,
    pub default_charset: *const c_char,
    pub sapi_headers: *mut sapi_headers_struct,
    pub read_post_bytes: usize,
    pub post_reader: Option<unsafe extern "C" fn()>,
    pub arginfo: *mut c_void,
    pub ini_entries: *const c_char,
    pub phpinfo_as_text: c_int,
    pub ini_defaults: Option<unsafe extern "C" fn(*mut zend_array)>,
    pub php_ini_ignore: c_int,
    pub php_ini_ignore_cwd: c_int,
    pub http_response_code: c_int,
    pub _reserved: [c_void; 32],
}

/// Opaque `sapi_headers_struct`.
#[repr(C)]
pub struct sapi_headers_struct {
    pub headers: zend_llist,
    pub http_response_code: c_int,
    pub send_default_content_type: c_uchar,
    pub mimetype: *const c_char,
    pub http_status_line: *mut c_char,
    pub _reserved: *mut c_void,
}

/// `zend_llist` (linked list).
#[repr(C)]
pub struct zend_llist {
    pub head: *mut zend_llist_element,
    pub tail: *mut zend_llist_element,
    pub count: usize,
    pub size: usize,
    pub dtor: Option<unsafe extern "C" fn(*mut c_void)>,
    pub persistent: c_uchar,
}

#[repr(C)]
pub struct zend_llist_element {
    pub prev: *mut zend_llist_element,
    pub next: *mut zend_llist_element,
    pub data: [u8; 0],
}

/// `sapi_header_struct`.
#[repr(C)]
pub struct sapi_header_struct {
    pub header: *mut c_char,
    pub header_len: usize,
    pub resp: *mut c_char,
    pub resp_len: usize,
    pub _reserved: *mut c_void,
}

/// `sapi_request_info`.
#[repr(C)]
pub struct sapi_request_info {
    pub request_method: *const c_char,
    pub query_string: *const c_char,
    pub cookie_data: *mut c_char,
    pub content_length: zend_long,
    pub path_translated: *const c_char,
    pub request_uri: *const c_char,
    pub request_body: *mut c_void,
    pub content_type: *const c_char,
    pub headers_only: c_uchar,
    pub headers_read: c_uchar,
    pub no_headers: c_uchar,
    pub headers_sent: c_uchar,
    pub response_code: c_int,
    pub proto_num: c_int,
    pub content_type_dup: *mut c_char,
    pub auth_user: *const c_char,
    pub auth_password: *const c_char,
    pub auth_digest: *const c_char,
    pub current_user: *mut c_char,
    pub current_user_length: c_int,
    pub _reserved: *mut c_void,
}

/// `zend_file_handle`.
#[repr(C)]
pub struct zend_file_handle {
    pub type_: zend_file_handle_type,
    pub filename: *const c_char,
    pub opened_path: *mut zend_string,
    pub handle: zend_stream_handle,
    pub primary_script: c_uchar,
    pub _reserved: *mut c_void,
}

pub type zend_file_handle_type = c_int;
pub type zend_stream_handle = *mut c_void;

/// `zend_refcounted_h` constants.
pub const GC_NOT_COLLECTABLE: u32 = 1 << 11;
pub const GC_PROTECTED: u32 = 1 << 12;
pub const GC_IMMUTABLE: u32 = 1 << 13;
pub const GC_PERSISTENT: u32 = 1 << 14;
pub const GC_RECURSIVE: u32 = 1 << 15;

// ============================================================================
// Zend Engine API (from Zend/zend.h, zend_API.h, zend_hash.h, etc.)
// ============================================================================

extern "C" {
    // ========================================================================
    // zval construction (mirror types.c helpers)
    // ========================================================================

    pub fn __zval_null__(zv: *mut zval);
    pub fn __zval_bool__(zv: *mut zval, val: bool);
    pub fn __zval_long__(zv: *mut zval, val: zend_long);
    pub fn __zval_double__(zv: *mut zval, val: f64);
    pub fn __zval_string__(zv: *mut zval, str: *mut zend_string);
    pub fn __zval_empty_string__(zv: *mut zval);
    pub fn __zval_arr__(zv: *mut zval, arr: *mut zend_array);
    pub fn __zend_new_array__(size: u32) -> *mut zend_array;

    // ========================================================================
    // Hash table operations
    // ========================================================================

    pub fn zend_hash_str_update(
        ht: *mut zend_array,
        key: *const c_char,
        len: usize,
        val: *mut zval,
    ) -> *mut zval;
    pub fn zend_hash_next_index_insert(ht: *mut zend_array, val: *mut zval) -> *mut zval;
    pub fn zend_hash_init(
        ht: *mut zend_array,
        nSize: u32,
        pDestructor: dtor_func_t,
        persistent: bool,
    );
    pub fn zend_hash_destroy(ht: *mut zend_array);
    pub fn zend_hash_release(ht: *mut zend_array);
    pub fn zend_array_dup(ht: *const zend_array) -> *mut zend_array;
    pub fn zend_hash_num_elements(ht: *const zend_array) -> u32;
    pub fn zend_hash_extend(ht: *mut zend_array, nSize: u32, persistent: bool);
    pub fn zend_hash_copy(
        target: *mut zend_array,
        source: *mut zend_array,
        pCopyConstructor: Option<unsafe extern "C" fn(*mut zval)>,
    );
    pub fn __hash_update_string__(ht: *mut zend_array, k: *mut zend_string, v: *mut zend_string);
    pub fn __zend_hash_init__(
        ht: *mut zend_array,
        nSize: u32,
        pDestructor: dtor_func_t,
        persistent: bool,
    );

    // ========================================================================
    // String operations
    // ========================================================================

    pub fn zend_string_init(
        str: *const c_char,
        len: usize,
        persistent: bool,
    ) -> *mut zend_string;
    pub fn zend_string_release(zs: *mut zend_string);
    pub fn zend_string_init_interned(
        str: *const c_char,
        len: usize,
        persistent: bool,
    ) -> *mut zend_string;
    pub fn zend_string_addref(zs: *mut zend_string) -> *mut zend_string;
    pub fn zend_string_dup(zs: *const zend_string, persistent: bool) -> *mut zend_string;
    pub fn zend_string_free(zs: *mut zend_string);
    pub fn zend_string_hash_val(zs: *mut zend_string);

    // ========================================================================
    // Memory management
    // ========================================================================

    pub fn __emalloc__(size: usize) -> *mut c_void;
    pub fn __efree__(ptr: *mut c_void);
    pub fn emalloc(size: usize) -> *mut c_void;
    pub fn efree(ptr: *mut c_void);
    pub fn pemalloc(size: usize, persistent: bool) -> *mut c_void;
    pub fn pefree(ptr: *mut c_void, persistent: bool);

    // ========================================================================
    // zval type inspection
    // ========================================================================

    pub fn zval_get_type(zv: *const zval) -> u8;
    pub fn zval_ptr_dtor(zv: *mut zval);
    pub fn zval_ptr_dtor_nogc(zv: *mut zval);
    pub fn zval_add_ref(zv: *mut zval);
    pub fn ZVAL_DEREF(zv: *mut zval) -> *mut zval;
    pub fn ZVAL_COPY_VALUE(dst: *mut zval, src: *const zval);

    // ========================================================================
    // Array recursion protection (mirror types.c)
    // ========================================================================

    pub fn __zend_array_is_recursive__(arr: *mut zend_array) -> bool;
    pub fn __zend_array_protect_recursion__(arr: *mut zend_array);
    pub fn __zend_array_unprotect_recursion__(arr: *mut zend_array);

    // ========================================================================
    // Hash table data access (mirror types.c)
    // ========================================================================

    pub fn get_ht_packed_data(ht: *mut zend_array, index: u32) -> *mut zval;
    pub fn get_ht_bucket_data(ht: *mut zend_array, index: u32) -> *mut Bucket;

    // ========================================================================
    // Callable invocation (mirror types.c)
    // ========================================================================

    pub fn __zend_is_callable__(cb: *mut zval) -> c_int;
    pub fn __call_user_function__(
        function_name: *mut zval,
        retval: *mut zval,
        param_count: u32,
        params: *mut zval,
    ) -> c_int;

    // ========================================================================
    // PHP lifecycle (from php_main.h, php_module.h)
    // ========================================================================

    pub fn php_module_startup(
        sf: *mut sapi_module_struct,
        additional_modules: *mut zend_module_entry,
    ) -> c_int;
    pub fn php_module_shutdown_wrapper(sapi: *mut sapi_module_struct) -> c_int;
    pub fn php_request_startup() -> c_int;
    pub fn php_request_shutdown(arg: *mut c_void);
    pub fn php_execute_script(file_handle: *mut zend_file_handle);
    pub fn php_output_end_all();
    pub fn php_output_deactivate();
    pub fn php_output_activate();
    pub fn php_output_start_user(
        output_handler: *mut zval,
        chunk_size: usize,
        flags: c_uint,
    ) -> c_int;
    pub fn php_output_set_implicit_flush(flag: c_int);
    pub fn php_startup_auto_globals();
    pub fn php_hash_environment();
    pub fn php_import_environment_variables(track_vars_array: *mut zval);

    // ========================================================================
    // SAPI lifecycle
    // ========================================================================

    pub fn sapi_startup(sf: *mut sapi_module_struct);
    pub fn sapi_shutdown();
    pub fn sapi_activate();
    pub fn sapi_deactivate();
    pub fn sapi_send_headers() -> c_int;
    pub fn sapi_add_header(header: *const c_char, header_len: usize, rep: c_uchar);
    pub fn sapi_handle_aborted_connection();
    pub fn php_handle_aborted_connection();
    pub fn php_handle_auth_data(auth: *const c_char);

    // ========================================================================
    // TSRM (Zend Thread Safety)
    // ========================================================================

    pub fn ts_resource(id: c_int) -> *mut c_void;
    pub fn ts_free_thread();
    pub fn tsrm_shutdown();
    pub fn tsrm_startup();
    pub fn php_tsrm_startup();
    pub fn php_tsrm_startup_ex(num_threads: isize);

    // ========================================================================
    // PHP INI
    // ========================================================================

    pub fn cfg_get_string(name: *const c_char, value: *mut *mut c_char) -> c_int;
    pub fn zend_set_timeout(seconds: c_int, reset_signals: c_int);
    pub fn zend_unset_timeout();
    pub fn zend_memory_usage(real_usage: bool) -> usize;
    pub fn zend_set_memory_limit(limit: usize) -> c_int;

    // ========================================================================
    // Output
    // ========================================================================

    pub fn php_printf(format: *const c_char, ...) -> usize;
    pub fn php_info_print_table_row(num: c_int, ...);
    pub fn php_info_print_table_start();
    pub fn php_info_print_table_end();
    pub fn php_info_print_table_header(num: c_int, ...);

    // ========================================================================
    // Header handling
    // ========================================================================

    pub fn sapi_header_op(op: c_int, header: *mut sapi_header_struct) -> c_int;
    pub fn sapi_get_default_content_type(
        mimetype: *const c_char,
        len: usize,
        charset: *mut c_char,
        charset_len: usize,
    ) -> *mut c_char;

    // ========================================================================
    // Module registration
    // ========================================================================

    pub fn zend_register_internal_module(module: *mut zend_module_entry) -> *mut zend_module_entry;
    pub fn zend_register_functions(
        functions: *const zend_function_entry,
        module: *mut zend_module_entry,
    ) -> c_int;
    pub fn zend_hash_str_find_ptr(
        ht: *mut zend_array,
        str: *const c_char,
        len: usize,
    ) -> *mut c_void;

    // ========================================================================
    // Error handling
    // ========================================================================

    pub fn zend_throw_exception(
        exception: *mut c_void,
        message: *const c_char,
        code: zend_long,
    );
    pub fn zend_throw_exception_ex(
        exception: *mut c_void,
        code: zend_long,
        format: *const c_char,
        ...
    );
    pub fn zend_bailout();
    pub fn zend_is_unwind_exit(exception: *mut c_void) -> bool;
    pub fn zend_is_graceful_exit(exception: *mut c_void) -> bool;
    pub fn zend_exception_error(exception: *mut c_void, severity: c_int);

    // ========================================================================
    // Misc
    // ========================================================================

    pub fn zend_stream_open(handle: *mut zend_file_handle) -> c_int;
    pub fn zend_stream_init_filename(handle: *mut zend_file_handle, filename: *const c_char);
    pub fn zend_destroy_file_handle(handle: *mut zend_file_handle);
    pub fn zend_lookup_class(name: *mut zend_string) -> *mut zend_class_entry;
    pub fn zend_enum_get_case_cstr(
        ce: *mut zend_class_entry,
        case_name: *const c_char,
    ) -> *mut zend_object;
    pub fn zend_call_function(fci: *mut c_void, fcc: *mut c_void) -> c_int;
}

// ============================================================================
// PHP standard library functions
// ============================================================================

extern "C" {
    pub fn php_register_variable_safe(
        key: *const c_char,
        val: *const c_char,
        val_len: usize,
        track_vars_array: *mut zval,
    );
    pub fn php_register_variable(
        key: *const c_char,
        val: *const c_char,
        val_len: usize,
        track_vars_array: *mut zval,
    );
    pub fn php_session_flush(write: bool);
    pub fn php_session_start() -> c_int;
}

// ============================================================================
// LarvaPHP C bridge functions (from frankenphp.c)
// ============================================================================

extern "C" {
    // ========================================================================
    // Version and config (mirror frankenphp_get_version, frankenphp_get_config)
    // ========================================================================

    pub fn frankenphp_get_version() -> frankenphp_version;
    pub fn frankenphp_get_config() -> frankenphp_config;

    // ========================================================================
    // Thread management (mirror frankenphp_new_main_thread, frankenphp_new_php_thread)
    // ========================================================================

    pub fn frankenphp_new_main_thread(num_threads: c_int) -> c_int;
    pub fn frankenphp_new_php_thread(thread_index: usize) -> bool;
    pub fn frankenphp_init_thread_metrics(max_threads: c_int);
    pub fn frankenphp_destroy_thread_metrics();
    pub fn frankenphp_get_thread_memory_usage(thread_index: usize) -> usize;
    pub fn frankenphp_thread_index() -> usize;

    // ========================================================================
    // Context management
    // ========================================================================

    pub fn frankenphp_update_local_thread_context(is_worker: bool);
    pub fn frankenphp_shutdown_dummy_request() -> bool;
    pub fn frankenphp_execute_script_cli(
        script: *mut c_char,
        argc: c_int,
        argv: *mut *mut c_char,
        eval: bool,
    ) -> c_int;

    // ========================================================================
    // Environment variables (mirror env.go)
    // ========================================================================

    pub fn frankenphp_register_known_variable(
        z_key: *mut zend_string,
        value: *mut c_char,
        val_len: usize,
        track_vars_array: *mut zval,
    );
    pub fn frankenphp_register_variable_safe(
        key: *mut c_char,
        val: *mut c_char,
        val_len: usize,
        track_vars_array: *mut zval,
    );
    pub fn frankenphp_register_server_vars(
        track_vars_array: *mut zval,
        vars: frankenphp_server_vars,
    );
    pub fn frankenphp_add_to_prepared_env(
        name: *mut c_char,
        name_len: usize,
        val: *mut c_char,
        val_len: usize,
        size: usize,
    );
    pub fn frankenphp_init_persistent_string(
        string: *const c_char,
        len: usize,
    ) -> *mut zend_string;
    pub fn frankenphp_get_current_memory_limit() -> c_int;

    // ========================================================================
    // Force kill (mirror frankenphp_force_kill_thread)
    // ========================================================================

    pub fn frankenphp_force_kill_thread(slot: force_kill_slot);
    pub fn frankenphp_release_thread_for_kill(slot: force_kill_slot);

    // ========================================================================
    // Extension registration (mirror ext.go)
    // ========================================================================

    pub fn register_extensions(
        m: *mut *mut zend_module_entry,
        len: c_int,
    );

    // ========================================================================
    // Interned strings (mirror frankenphp_strings)
    // ========================================================================

    pub static mut frankenphp_strings: frankenphp_interned_strings_t;
}

// ============================================================================
// LarvaPHP FFI return types and structs (mirror frankenphp.h)
// ============================================================================

/// Mirrors `frankenphp_version` from frankenphp.h.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct frankenphp_version {
    pub major_version: c_uchar,
    pub minor_version: c_uchar,
    pub release_version: c_uchar,
    pub extra_version: *const c_char,
    pub version: *const c_char,
    pub version_id: c_ulong,
}

pub type c_ulong = u64;

/// Mirrors `frankenphp_config` from frankenphp.h.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct frankenphp_config {
    pub zts: bool,
    pub zend_signals: bool,
    pub zend_max_execution_timers: bool,
}

/// Mirrors `frankenphp_server_vars` from frankenphp.h.
#[repr(C)]
pub struct frankenphp_server_vars {
    pub total_num_vars: usize,
    pub remote_addr: *mut c_char,
    pub remote_addr_len: usize,
    pub remote_host: *mut c_char,
    pub remote_host_len: usize,
    pub remote_port: *mut c_char,
    pub remote_port_len: usize,
    pub document_root: *mut c_char,
    pub document_root_len: usize,
    pub path_info: *mut c_char,
    pub path_info_len: usize,
    pub path_translated: *mut c_char,
    pub path_translated_len: usize,
    pub php_self: *mut c_char,
    pub php_self_len: usize,
    pub document_uri: *mut c_char,
    pub document_uri_len: usize,
    pub script_filename: *mut c_char,
    pub script_filename_len: usize,
    pub script_name: *mut c_char,
    pub script_name_len: usize,
    pub server_name: *mut c_char,
    pub server_name_len: usize,
    pub server_port: *mut c_char,
    pub server_port_len: usize,
    pub server_addr: *mut c_char,
    pub server_addr_len: usize,
    pub content_length: *mut c_char,
    pub content_length_len: usize,
    pub server_protocol: *mut c_char,
    pub server_protocol_len: usize,
    pub http_host: *mut c_char,
    pub http_host_len: usize,
    pub request_uri: *mut c_char,
    pub request_uri_len: usize,
    pub ssl_cipher: *mut c_char,
    pub ssl_cipher_len: usize,
    pub request_scheme: *mut zend_string,
    pub ssl_protocol: *mut zend_string,
    pub https: *mut zend_string,
}

/// Mirrors `force_kill_slot` from frankenphp.h.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct force_kill_slot {
    pub vm_interrupt: *mut bool,
    pub timed_out: *mut bool,
    #[cfg(not(windows))]
    pub tid: libc::pthread_t,
    #[cfg(windows)]
    pub thread_handle: *mut c_void,
}

/// Mirrors `frankenphp_interned_strings_t` from frankenphp.h.
#[repr(C)]
pub struct frankenphp_interned_strings_t {
    pub remote_addr: *mut zend_string,
    pub remote_host: *mut zend_string,
    pub remote_port: *mut zend_string,
    pub document_root: *mut zend_string,
    pub path_info: *mut zend_string,
    pub php_self: *mut zend_string,
    pub document_uri: *mut zend_string,
    pub script_filename: *mut zend_string,
    pub script_name: *mut zend_string,
    pub https: *mut zend_string,
    pub httpsLowercase: *mut zend_string,
    pub httpLowercase: *mut zend_string,
    pub ssl_protocol: *mut zend_string,
    pub request_scheme: *mut zend_string,
    pub server_name: *mut zend_string,
    pub server_port: *mut zend_string,
    pub server_addr: *mut zend_string,
    pub content_length: *mut zend_string,
    pub server_protocol: *mut zend_string,
    pub http_host: *mut zend_string,
    pub request_uri: *mut zend_string,
    pub ssl_cipher: *mut zend_string,
    pub server_software: *mut zend_string,
    pub frankenphp: *mut zend_string,
    pub gateway_interface: *mut zend_string,
    pub cgi11: *mut zend_string,
    pub auth_type: *mut zend_string,
    pub remote_ident: *mut zend_string,
    pub content_type: *mut zend_string,
    pub path_translated: *mut zend_string,
    pub query_string: *mut zend_string,
    pub remote_user: *mut zend_string,
    pub request_method: *mut zend_string,
    pub tls1: *mut zend_string,
    pub tls11: *mut zend_string,
    pub tls12: *mut zend_string,
    pub tls13: *mut zend_string,
    pub on: *mut zend_string,
    pub empty: *mut zend_string,
}

/// Mirrors `frankenphp_thread_metrics` from frankenphp.h.
#[repr(C)]
pub struct frankenphp_thread_metrics {
    pub last_memory_usage: usize,
}

/// Mirrors `frankenphp_mercure_status` from frankenphp.h.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum frankenphp_mercure_status {
    OK = 0,
    NO_HUB = 1,
    INVALID_UPDATE = 2,
    PUBLISH_FAILED = 3,
    UNSUPPORTED = 4,
}

// ============================================================================
// Go callback function types (called from C via the bridge)
// ============================================================================

/// Mirrors the `go_string` struct from frankenphp.h.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct go_string {
    pub len: usize,
    pub data: *mut c_char,
}

// ============================================================================
// Interned strings accessor
// ============================================================================

impl frankenphp_interned_strings_t {
    /// Get the interned string by field name.
    ///
    /// # Safety
    /// The caller must ensure the Zend Engine is initialized.
    pub unsafe fn get(&self, name: &str) -> *mut zend_string {
        match name {
            "remote_addr" => self.remote_addr,
            "remote_host" => self.remote_host,
            "remote_port" => self.remote_port,
            "document_root" => self.document_root,
            "path_info" => self.path_info,
            "php_self" => self.php_self,
            "document_uri" => self.document_uri,
            "script_filename" => self.script_filename,
            "script_name" => self.script_name,
            "https" => self.https,
            "httpsLowercase" => self.httpsLowercase,
            "httpLowercase" => self.httpLowercase,
            "ssl_protocol" => self.ssl_protocol,
            "request_scheme" => self.request_scheme,
            "server_name" => self.server_name,
            "server_port" => self.server_port,
            "server_addr" => self.server_addr,
            "content_length" => self.content_length,
            "server_protocol" => self.server_protocol,
            "http_host" => self.http_host,
            "request_uri" => self.request_uri,
            "ssl_cipher" => self.ssl_cipher,
            "server_software" => self.server_software,
            "frankenphp" => self.frankenphp,
            "gateway_interface" => self.gateway_interface,
            "cgi11" => self.cgi11,
            "auth_type" => self.auth_type,
            "remote_ident" => self.remote_ident,
            "content_type" => self.content_type,
            "path_translated" => self.path_translated,
            "query_string" => self.query_string,
            "remote_user" => self.remote_user,
            "request_method" => self.request_method,
            "tls1" => self.tls1,
            "tls11" => self.tls11,
            "tls12" => self.tls12,
            "tls13" => self.tls13,
            "on" => self.on,
            "empty" => self.empty,
            _ => std::ptr::null_mut(),
        }
    }

}