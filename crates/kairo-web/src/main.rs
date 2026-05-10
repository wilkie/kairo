//! `kairo-web` binary entry point.
//!
//! Foreground only (DECISIONS.md §12.7). Parses `--bind`,
//! `--spa-dir`, `--daemon-socket`, installs the tracing
//! subscriber, builds a multi-thread tokio runtime, and calls
//! [`serve`]. `serve` runs until `SIGTERM` / `SIGINT`, then
//! drains and exits.

use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use kairo_web::{install_tracing, serve, Config, DEFAULT_PORT};

// All HTTP-stack deps live in the lib; the bin only drives the
// public API. These shims silence `unused_crate_dependencies`
// for crates the bin doesn't reference directly.
use axum as _;
use http_body_util as _;
use hyper as _;
use hyper_util as _;
use tokio as _;
use tower as _;
use tower_http as _;
use tracing_subscriber as _;
// dev-deps are visible to the bin's unit-test target.
#[cfg(test)]
use kairo_core as _;
#[cfg(test)]
use kairo_daemon as _;
#[cfg(test)]
use kairo_daemon_client as _;
#[cfg(test)]
use kairo_store as _;
#[cfg(test)]
use kairo_test_support as _;
#[cfg(test)]
use serde_json as _;
#[cfg(test)]
use tempfile as _;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const USAGE: &str = "\
usage: kairo-web --daemon-socket <path> [--spa-dir <path>] [--bind <addr>] [--pid-file <path>]
       kairo-web --version

When --spa-dir is omitted, kairo-web runs in API-proxy-only mode:
/api/v1/* still proxies to the daemon, but non-API paths return
404. This is the shape the dev workflow uses (Vite serves the
SPA on its own port).
";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();

    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("kairo-web {VERSION}");
        return ExitCode::SUCCESS;
    }
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    let config = match parse_args(&args) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("kairo-web: {message}");
            eprint!("{USAGE}");
            return ExitCode::from(2);
        }
    };

    install_tracing();

    tracing::info!(
        version = VERSION,
        bind = %config.bind_addr,
        spa_dir = ?config.spa_dir.as_deref().map(std::path::Path::display),
        daemon_socket = %config.daemon_socket.display(),
        "kairo-web starting"
    );

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("kairo-web: failed to start async runtime: {error}");
            return ExitCode::from(1);
        }
    };

    match runtime.block_on(serve(config)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("kairo-web: {error}");
            ExitCode::from(1)
        }
    }
}

fn parse_args(args: &[String]) -> Result<Config, String> {
    let mut iter = args.iter().skip(1);
    let mut bind_addr: Option<SocketAddr> = None;
    let mut spa_dir: Option<PathBuf> = None;
    let mut daemon_socket: Option<PathBuf> = None;
    let mut pid_file: Option<PathBuf> = None;

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--bind" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "--bind requires an address argument".to_owned())?;
                let addr: SocketAddr = value
                    .parse()
                    .map_err(|error| format!("--bind {value:?}: {error}"))?;
                bind_addr = Some(addr);
            }
            "--spa-dir" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "--spa-dir requires a path argument".to_owned())?;
                spa_dir = Some(PathBuf::from(value));
            }
            "--daemon-socket" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "--daemon-socket requires a path argument".to_owned())?;
                daemon_socket = Some(PathBuf::from(value));
            }
            "--pid-file" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "--pid-file requires a path argument".to_owned())?;
                pid_file = Some(PathBuf::from(value));
            }
            other => return Err(format!("unrecognized argument: {other}")),
        }
    }

    let bind_addr = bind_addr.unwrap_or_else(|| {
        SocketAddr::new(
            std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            DEFAULT_PORT,
        )
    });
    let daemon_socket =
        daemon_socket.ok_or_else(|| "--daemon-socket <path> is required".to_owned())?;

    Ok(Config {
        bind_addr,
        spa_dir,
        daemon_socket,
        pid_file,
    })
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        std::iter::once("kairo-web".to_owned())
            .chain(values.iter().map(|v| v.to_owned().to_owned()))
            .collect()
    }

    #[test]
    fn parses_full_arg_set() {
        let parsed = parse_args(&args(&[
            "--bind",
            "127.0.0.1:9000",
            "--spa-dir",
            "/tmp/spa",
            "--daemon-socket",
            "/tmp/daemon.sock",
            "--pid-file",
            "/tmp/web.pid",
        ]))
        .expect("parse");
        assert_eq!(parsed.bind_addr.port(), 9000);
        assert_eq!(
            parsed.spa_dir.as_deref(),
            Some(PathBuf::from("/tmp/spa").as_path())
        );
        assert_eq!(parsed.daemon_socket, PathBuf::from("/tmp/daemon.sock"));
        assert_eq!(
            parsed.pid_file.as_deref(),
            Some(PathBuf::from("/tmp/web.pid").as_path())
        );
    }

    #[test]
    fn spa_dir_is_optional() {
        let parsed = parse_args(&args(&["--daemon-socket", "/tmp/daemon.sock"])).expect("parse");
        assert!(parsed.spa_dir.is_none());
    }

    #[test]
    fn pid_file_defaults_to_none() {
        let parsed = parse_args(&args(&[
            "--spa-dir",
            "/tmp/spa",
            "--daemon-socket",
            "/tmp/daemon.sock",
        ]))
        .expect("parse");
        assert!(parsed.pid_file.is_none());
    }

    #[test]
    fn defaults_bind_to_loopback() {
        let parsed = parse_args(&args(&[
            "--spa-dir",
            "/tmp/spa",
            "--daemon-socket",
            "/tmp/daemon.sock",
        ]))
        .expect("parse");
        assert_eq!(parsed.bind_addr.ip().to_string(), "127.0.0.1");
        assert_eq!(parsed.bind_addr.port(), DEFAULT_PORT);
    }

    #[test]
    fn errors_when_daemon_socket_missing() {
        let err = parse_args(&args(&["--spa-dir", "/tmp/spa"])).expect_err("expected error");
        assert!(err.contains("--daemon-socket"), "{err:?}");
    }

    #[test]
    fn errors_on_unknown_flag() {
        let err = parse_args(&args(&["--port", "9000"])).expect_err("expected error");
        assert!(err.contains("--port"), "{err:?}");
    }
}
