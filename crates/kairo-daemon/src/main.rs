//! `kairo-daemon` binary entry point.
//!
//! Foreground only (DECISIONS.md §10.1). Parses `--store
//! <path>`, installs the tracing subscriber, builds a multi-
//! thread tokio runtime, and calls [`serve`]. `serve` runs until
//! `SIGTERM` or `SIGINT`, then drains and exits.

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use kairo_daemon::{install_tracing, serve, ApiDoc, Config};

// All HTTP-stack deps live in the lib; the bin only drives the
// public API. These shims silence `unused_crate_dependencies`
// for crates the bin doesn't reference directly.
use axum as _;
use hyper as _;
use hyper_util as _;
use kairo_core as _;
use kairo_daemon_client as _;
use kairo_identity as _;
use kairo_statement as _;
use kairo_store as _;
use serde as _;
use serde_json as _;
use tokio_util as _;
use tower as _;
use tower_http as _;
use tracing_subscriber as _;
use utoipa as _;
// dev-deps are visible to the bin's unit-test target.
#[cfg(test)]
use http_body_util as _;
#[cfg(test)]
use kairo_test_support as _;
#[cfg(test)]
use tempfile as _;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();

    if args.get(1).map(String::as_str) == Some("dump-openapi") {
        return run_dump_openapi(&args[2..]);
    }

    let store_path = match parse_store_path(&args) {
        Ok(path) => path,
        Err(message) => {
            eprintln!("kairo-daemon: {message}");
            eprintln!("usage: kairo-daemon --store <path>");
            eprintln!("       kairo-daemon dump-openapi [--out <path>]");
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

/// `kairo-daemon dump-openapi [--out <path>]` writes the OpenAPI
/// schema to disk (or stdout when `--out` is omitted). Used for
/// regenerating the checked-in `openapi/kairo-daemon.json`.
fn run_dump_openapi(args: &[String]) -> ExitCode {
    let mut iter = args.iter();
    let mut out: Option<PathBuf> = None;
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--out" => match iter.next() {
                Some(path) => out = Some(PathBuf::from(path)),
                None => {
                    eprintln!("kairo-daemon: --out requires a path argument");
                    return ExitCode::from(2);
                }
            },
            other => {
                eprintln!("kairo-daemon: unrecognized argument to dump-openapi: {other}");
                return ExitCode::from(2);
            }
        }
    }

    let json = match ApiDoc::pretty_json() {
        Ok(json) => json,
        Err(error) => {
            eprintln!("kairo-daemon: failed to serialize OpenAPI schema: {error}");
            return ExitCode::from(1);
        }
    };

    match out {
        Some(path) => {
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    if let Err(error) = fs::create_dir_all(parent) {
                        eprintln!(
                            "kairo-daemon: failed to create {}: {error}",
                            parent.display()
                        );
                        return ExitCode::from(1);
                    }
                }
            }
            if let Err(error) = fs::write(&path, json.as_bytes()) {
                eprintln!("kairo-daemon: failed to write {}: {error}", path.display());
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        None => {
            let mut stdout = io::stdout().lock();
            if let Err(error) = stdout.write_all(json.as_bytes()) {
                eprintln!("kairo-daemon: failed to write to stdout: {error}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
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
