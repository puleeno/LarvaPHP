//! PHP CLI execution.
//!
//! Mirrors `cli.go` from the Go source.

use std::ffi::CString;
use std::os::raw::c_char;

use crate::ffi;
use crate::options::{Opt, Option};

/// Execute a PHP script via the CLI SAPI.
///
/// This mirrors `frankenphp.ExecuteScriptCLI()`.
pub fn execute_script_cli(
    script: &str,
    args: &[String],
    options: Vec<Box<dyn Option>>,
) -> i32 {
    // Apply options
    let mut opt = Opt::default();
    for option in &options {
        if option.apply(&mut opt).is_err() {
            return 1;
        }
    }

    let c_script = match CString::new(script) {
        Ok(s) => s,
        Err(_) => return 1,
    };

    let argc = args.len() as i32;
    let mut argv: Vec<*mut c_char> = args
        .iter()
        .filter_map(|a| CString::new(a.as_str()).ok())
        .map(|s| s.into_raw())
        .collect();

    if argv.is_empty() {
        return 1;
    }

    let result = unsafe {
        ffi::frankenphp_execute_script_cli(
            c_script.as_ptr() as *mut c_char,
            argc,
            argv.as_mut_ptr(),
            false,
        )
    };

    // Free argv
    for ptr in argv {
        unsafe {
            let _ = CString::from_raw(ptr);
        }
    }

    result
}