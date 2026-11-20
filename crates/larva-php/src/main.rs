use std::process::ExitCode;

use clap::Parser;
use larva_php::{self, VERSION};

/// LarvaPHP - a modern PHP application server
#[derive(Parser, Debug)]
#[command(version, about, long_about = None, disable_version_flag = true, disable_help_flag = true)]
struct Args {
    /// Run a PHP script file
    #[arg(value_name = "SCRIPT")]
    script: Option<String>,

    /// Document root directory for serving PHP applications
    #[arg(short = 'r', long = "root", value_name = "DIR")]
    root: Option<String>,

    /// Address to bind the server to
    #[arg(short = 'a', long = "addr", default_value = "127.0.0.1:8000")]
    addr: String,

    /// Set a php.ini directive (repeatable), e.g. --php-ini precision=14
    #[arg(long = "php-ini", value_name = "KEY=VALUE")]
    php_ini: Vec<String>,

    /// Enable debug logging (verbose output)
    #[arg(long = "debug")]
    debug: bool,

    /// Generate a shell completion script and exit (bash|zsh|fish|powershell)
    #[arg(long = "completion", value_name = "SHELL")]
    completion: Option<String>,

    /// Show version information
    #[arg(short = 'v', long = "version")]
    version: bool,

    /// Show help information
    #[arg(short = 'h', long = "help")]
    help: bool,
}

fn print_usage(out: &mut dyn std::io::Write) {
    let _ = writeln!(
        out,
        "LarvaPHP {} - a modern PHP application server",
        VERSION
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "Usage:");
    let _ = writeln!(
        out,
        "  larva-php <script.php> [args...]    Run a PHP script"
    );
    let _ = writeln!(
        out,
        "  larva-php --root <dir>              Serve PHP application from directory"
    );
    let _ = writeln!(
        out,
        "  larva-php --php-ini key=value       Set a php.ini directive (repeatable)"
    );
    let _ = writeln!(out, "  larva-php --debug                   Enable debug logging");
    let _ = writeln!(
        out,
        "  larva-php --completion <shell>      Generate shell completion (bash|zsh|fish|powershell)"
    );
    let _ = writeln!(out, "  larva-php --version                 Show version");
    let _ = writeln!(out, "  larva-php --help                    Show this help");
}

fn main() -> ExitCode {
    let args = Args::parse();

    if args.help {
        print_usage(&mut std::io::stdout());
        return ExitCode::SUCCESS;
    }

    if args.version {
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

    // Shell completion generation (exits early).
    if let Some(shell_name) = &args.completion {
        let bin = std::env::args()
            .next()
            .and_then(|p| {
                std::path::Path::new(&p)
                    .file_stem()
                    .map(|n| n.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| "larva-php".to_string());
        match larva_php::Shell::from_name(shell_name) {
            Some(shell) => {
                shell.write_to(&bin, &mut std::io::stdout()).ok();
                return ExitCode::SUCCESS;
            }
            None => {
                eprintln!(
                    "Unsupported shell '{}'. Supported: bash, zsh, fish, powershell",
                    shell_name
                );
                return ExitCode::from(1);
            }
        }
    }

    // Initialize tracing (DEBUG when --debug is set).
    if args.debug {
        use tracing::Level;
        tracing_subscriber::fmt()
            .with_max_level(Level::DEBUG)
            .init();
    } else {
        tracing_subscriber::fmt::init();
    }

    if let Some(root) = args.root {
        let addr: std::net::SocketAddr = match args.addr.parse() {
            Ok(a) => a,
            Err(e) => {
                eprintln!("Invalid address '{}': {}", args.addr, e);
                return ExitCode::from(1);
            }
        };

        // Initialize PHP runtime
        let mut server = larva_php::server::Server::new(&root).unwrap();

        // Apply php.ini overrides from --php-ini KEY=VALUE flags.
        if !args.php_ini.is_empty() {
            let mut ini = std::collections::HashMap::new();
            for kv in &args.php_ini {
                match kv.split_once('=') {
                    Some((k, v)) => {
                        ini.insert(k.trim().to_string(), v.trim().to_string());
                    }
                    None => {
                        eprintln!("Invalid --php-ini value '{}' (expected KEY=VALUE)", kv);
                        return ExitCode::from(1);
                    }
                }
            }
            server.set_php_ini(ini);
        }

        server.activate();

        // Start PHP main thread
        if let Err(e) = larva_php::thread::init(Vec::new()) {
            eprintln!("Failed to initialize PHP runtime: {}", e);
            return ExitCode::from(1);
        }

        // Start HTTP server
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            if let Err(e) = server.start_server(addr).await {
                eprintln!("Server error: {}", e);
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            }
        })
    } else if let Some(script) = args.script {
        let script_args: Vec<String> = std::env::args().skip(2).collect();
        let status = larva_php::execute_script_cli(&script, &script_args, Vec::new());
        ExitCode::from(status as u8)
    } else {
        print_usage(&mut std::io::stderr());
        ExitCode::from(1)
    }
}