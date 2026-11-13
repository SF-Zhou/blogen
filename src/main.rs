//! blogen binary entry point.

use clap::Parser;
use tracing_subscriber::EnvFilter;

use blogen::cli::{run, Cli};
use blogen::error::format_error_chain;

fn main() {
    let cli = Cli::parse();
    init_tracing(&cli);
    if let Err(error) = run(cli) {
        eprintln!("error: {}", format_error_chain(&error));
        std::process::exit(1);
    }
}

fn init_tracing(cli: &Cli) {
    let level = if cli.quiet {
        "warn"
    } else {
        match cli.verbose {
            0 => "info",
            1 => "debug",
            _ => "trace",
        }
    };
    let filter = std::env::var("RUST_LOG")
        .ok()
        .and_then(|value| EnvFilter::try_new(value).ok())
        .unwrap_or_else(|| EnvFilter::new(format!("blogen={level},warn")));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(false)
        .without_time()
        .try_init();
}
