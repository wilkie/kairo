//! `kairo-daemon` binary entry point.
//!
//! Slice 1: parses `--store <path>`, installs the tracing
//! subscriber, prints a banner, and exits. Slice 2 replaces the
//! body with a real `serve` invocation, signal handling, and
//! lifecycle.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use kairo_daemon::{install_tracing, serve, Config};

// `tracing-subscriber` is a direct dep so the lib's `install_tracing`
// can install a real fmt subscriber; the bin reaches it through the
// lib API rather than depending on subscriber types directly.
use tracing_subscriber as _;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let store_path = match parse_store_path(&args) {
        Ok(path) => path,
        Err(message) => {
            eprintln!("kairo-daemon: {message}");
            eprintln!("usage: kairo-daemon --store <path>");
            return ExitCode::from(2);
        }
    };

    install_tracing();

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        store = %store_path.display(),
        "kairo-daemon starting (slice 1 stub)"
    );

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("kairo-daemon: failed to start async runtime: {error}");
            return ExitCode::from(1);
        }
    };

    let result = runtime.block_on(serve(Config { store_path }));

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("kairo-daemon: {error}");
            ExitCode::from(1)
        }
    }
}

fn parse_store_path(args: &[String]) -> Result<PathBuf, String> {
    let mut iter = args.iter().skip(1);
    let mut store: Option<PathBuf> = None;
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--store" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "--store requires a path argument".to_owned())?;
                store = Some(PathBuf::from(value));
            }
            other => return Err(format!("unrecognized argument: {other}")),
        }
    }
    store.ok_or_else(|| "--store <path> is required".to_owned())
}

