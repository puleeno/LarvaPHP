use std::env;
use std::path::PathBuf;

fn main() {
    let php_includes = env::var("PHP_INCLUDES").unwrap_or_default();

    if php_includes.is_empty() {
        // No PHP headers available - skip bindgen generation.
        // The ffi.rs module contains manual declarations instead.
        println!("cargo:warning=PHP_INCLUDES not set, skipping bindgen. Using manual FFI declarations in ffi.rs.");
        return;
    }

    let mut builder = bindgen::Builder::default()
        .header("src/zend_wrapper.h")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .derive_default(true)
        .derive_debug(true)
        .allowlist_function("php_.*")
        .allowlist_function("zend_.*")
        .allowlist_function("sapi_.*")
        .allowlist_function("ts_.*")
        .allowlist_function("tsrm_.*")
        .allowlist_type("zval")
        .allowlist_type("zend_.*")
        .allowlist_type("sapi_.*")
        .allowlist_type("HashTable")
        .allowlist_type("Bucket")
        .allowlist_var("IS_.*")
        .allowlist_var("SUCCESS")
        .allowlist_var("FAILURE")
        .allowlist_var("ZTS")
        .allowlist_var("ZEND_.*")
        .allowlist_var("PHP_.*")
        .allowlist_var("SAPI_.*")
        .allowlist_var("LOG_.*")
        .allowlist_var("E_.*")
        .allowlist_var("HANDLE_.*")
        .allowlist_var("MODULE_.*")
        .allowlist_var("REQUEST_.*")
        .allowlist_var("TRACK_VARS_.*")
        .blocklist_type("FILE")
        .blocklist_type("_IO_.*")
        .opaque_type("zend_.*");

    if !php_includes.is_empty() {
        for inc in php_includes.split(';') {
            builder = builder.clang_arg(format!("-I{}", inc.trim()));
        }
    }

    // Platform-specific flags (mirror cgo.go)
    if cfg!(target_os = "windows") {
        builder = builder
            .clang_arg("-D_WINDOWS")
            .clang_arg("-DWINDOWS=1")
            .clang_arg("-DZEND_WIN32=1")
            .clang_arg("-DPHP_WIN32=1")
            .clang_arg("-DWIN32")
            .clang_arg("-DZTS=1");
    } else if cfg!(target_os = "linux") {
        builder = builder.clang_arg("-D_GNU_SOURCE");
    }

    let bindings = builder.generate().expect("Unable to generate bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings.write_to_file(out_path.join("bindings.rs")).expect("Couldn't write bindings!");
}