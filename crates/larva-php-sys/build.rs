use std::env;
use std::path::PathBuf;

fn main() {
    // ---- PHP library directory ----
    let php_lib_dir = PathBuf::from(env::var("PHP_LIB_DIR").unwrap_or_else(|_| {
        r"C:\Program Files\PHP\current\dev".to_string()
    }));

    // ---- Stub C source directory ----
    let stub_src = PathBuf::from(env::var("STUB_SRC_DIR").unwrap_or_else(|_| {
        r"C:\Users\Administrator\Projects\LarvaPHP-Web-Server\larva-php\crates\larva-php-sys\src".to_string()
    }));

    println!("cargo:rerun-if-changed={}", stub_src.join("stubs.c").display());

    // ---- Link against php8.lib ----
    println!(
        "cargo:rustc-link-search={}",
        php_lib_dir.display()
    );
    println!("cargo:rustc-link-lib=php8");

    // ---- Windows system libraries ----
    if cfg!(target_os = "windows") {
        let sdk_lib_base = PathBuf::from(r"C:\Program Files (x86)\Windows Kits\10\Lib");
        if let Ok(versions) = sdk_lib_base.read_dir() {
            let ver_dirs: Vec<_> = versions
                .filter_map(|e| e.ok())
                .filter(|e| e.file_name().to_string_lossy().starts_with("10.0."))
                .collect();
            if let Some(latest) = ver_dirs.iter().max_by_key(|e| e.file_name()) {
                let sdk_lib = latest.path().join("um").join("x64");
                println!("cargo:rustc-link-search={}", sdk_lib.display());
            }
        }
        for lib in &["shell32", "user32", "advapi32", "ws2_32", "bcrypt", "crypt32"] {
            println!("cargo:rustc-link-lib={}", lib);
        }
    }

    // ---- Compile stubs.c using cc crate ----
    let stub_path = stub_src.join("stubs.c");
    let mut build = cc::Build::new();
    if stub_path.exists() {
        build.file(stub_path);
    }

    // Add VC include directory explicitly
    if cfg!(target_os = "windows") {
        let vc_include = PathBuf::from(env::var("INCLUDE").unwrap_or_default());
        for inc in vc_include.to_string_lossy().split(';') {
            let inc = inc.trim();
            if !inc.is_empty() {
                build.include(PathBuf::from(inc));
            }
        }
        // Add Windows SDK UCRT for stddef.h, etc.
        let win_ucrt = PathBuf::from(r"C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\ucrt");
        if win_ucrt.exists() {
            build.include(win_ucrt);
        }
        build.define("_WINDOWS", None);
        build.define("WINDOWS", Some("1"));
        build.define("ZEND_WIN32", Some("1"));
        build.define("PHP_WIN32", Some("1"));
        build.define("WIN32", None);
        build.define("_CRT_SECURE_NO_WARNINGS", None);
        build.define("_CRT_NONSTDC_NO_DEPRECATE", None);
        build.compiler("cl.exe");
    }

    build.compile("larva_php_bridge");

    // ---- Bindgen (optional, skipped - using manual ffi.rs) ----
    let php_includes = env::var("PHP_INCLUDES").unwrap_or_default();
    if !php_includes.is_empty() {
        println!("cargo:warning=PHP_INCLUDES set but bindgen skipped; using manual ffi.rs.");
    }
}
