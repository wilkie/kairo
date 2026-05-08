//! `kairo-daemon` binary entry point.
//!
//! Foreground only (DECISIONS.md §10.1). Parses `--store
//! <path>`, installs the tracing subscriber, builds a multi-
//! thread tokio runtime, and calls [`serve`]. `serve` runs until
//! `SIGTERM` or `SIGINT`, then drains and exits.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use kairo_daemon::{install_tracing, serve, Config};

// All HTTP-stack deps live in the lib; the bin only drives the
// public API. These shims silence `unused_crate_dependencies`
// for crates the bin doesn't reference directly.
use axum as _;
use hyper as _;
use hyper_util as _;
use kairo_core as _;
use kairo_daemon_client as _;
use kairo_store as _;
use serde as _;
use tower as _;
use tower_http as _;
use tracing_subscriber as _;
// dev-deps are visible to the bin's unit-test target.
#[cfg(test)]
use http_body_util as _;
#[cfg(test)]
use serde_json as _;
#[cfg(test)]
use tempfile as _;

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
        "kairo-daemon starting"
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

    match runtime.block_on(serve(Config { store_path })) {
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
