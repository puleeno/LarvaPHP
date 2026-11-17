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

    /// Show version information
    #[arg(short = 'v', long = "version")]
    version: bool,

    /// Show help information
    #[arg(short = 'h', long = "help")]
    help: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();

    if args.help {
        println!("LarvaPHP {} - a modern PHP application server", VERSION);
        println!();
        println!("Usage:");
        println!("  larva-php <script.php> [args...]    Run a PHP script");
        println!("  larva-php -root <dir>               Serve PHP application from directory");
        println!("  larva-php --version                 Show version");
        println!("  larva-php --help                    Show this help");
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

    // Initialize tracing
    tracing_subscriber::fmt::init();

    if let Some(root) = args.root {
        let addr: std::net::SocketAddr = match args.addr.parse() {
            Ok(a) => a,
            Err(e) => {
                eprintln!("Invalid address '{}': {}", args.addr, e);
                return ExitCode::from(1);
            }
        };

        // Initialize PHP runtime
        let server = larva_php::server::Server::new(&root).unwrap();
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
        eprintln!("LarvaPHP {} - a modern PHP application server", VERSION);
        eprintln!();
        eprintln!("Usage:");
        eprintln!("  larva-php <script.php> [args...]    Run a PHP script");
        eprintln!("  larva-php -root <dir>               Serve PHP application from directory");
        eprintln!("  larva-php --version                 Show version");
        eprintln!("  larva-php --help                    Show this help");
        ExitCode::from(1)
    }
}
