use std::process::ExitCode;

use larva_php::{self, VERSION};

fn main() -> ExitCode {
    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("LarvaPHP {} - a modern PHP application server", VERSION);
        eprintln!();
        eprintln!("Usage:");
        eprintln!("  larva-php <script.php> [args...]    Run a PHP script");
        eprintln!("  larva-php --version                Show version");
        eprintln!("  larva-php --help                   Show this help");
        return ExitCode::from(1);
    }

    match args[1].as_str() {
        "--version" | "-v" => {
            println!("LarvaPHP {}", VERSION);
            let version = larva_php::version();
            let version_str = unsafe {
                std::ffi::CStr::from_ptr(version.version)
                    .to_string_lossy()
                    .into_owned()
            };
            println!("PHP {}", version_str);
            return ExitCode::SUCCESS;
        }
        "--help" | "-h" => {
            println!("LarvaPHP {} - a modern PHP application server", VERSION);
            println!();
            println!("Usage:");
            println!("  larva-php <script.php> [args...]    Run a PHP script");
            println!("  larva-php --version                Show version");
            println!("  larva-php --help                   Show this help");
            return ExitCode::SUCCESS;
        }
        _ => {}
    }

    // Execute PHP script
    let script = &args[1];
    let script_args = &args[1..];

    let status = larva_php::execute_script_cli(script, script_args, Vec::new());

    ExitCode::from(status as u8)
}