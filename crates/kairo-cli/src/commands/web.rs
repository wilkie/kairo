//! Runners for `kairo web start | status | stop`.
//!
//! `start` runs the web server in the foreground (foreground only
//! in v1 — `specs/DECISIONS.md` §12.7). `status` probes the
//! configured TCP address by hitting `/api/v1/version`. `stop`
//! reads the PID file and sends SIGTERM, optionally waiting for
//! the PID file to disappear.
//!
//! All three runners are sync wrappers that build a tokio runtime
//! locally for the async work — same shape as `commands::daemon`.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::time::{Duration, Instant};

use http_body_util::{BodyExt, Empty};
use hyper::body::Bytes;
use hyper::Request;
use hyper_util::rt::TokioIo;
use kairo_web::{install_tracing, serve, Config, DEFAULT_PORT};
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use tokio::net::TcpStream;
use tokio::time::timeout;

use crate::cli::WebCommand;
use crate::error::CliError;
use crate::store_paths::StorePaths;

const PID_FILE: &str = "web.pid";
const SOCKET_FILE: &str = "daemon.sock";
const STATUS_PROBE_TIMEOUT: Duration = Duration::from_millis(500);

/// Dispatch entry point for `kairo web ...`.
pub(crate) fn run_web_command(command: WebCommand, paths: &StorePaths) -> Result<String, CliError> {
    match command {
        WebCommand::Start { spa_dir, bind } => {
            let bind_addr = parse_bind(bind.as_deref())?;
            run_start(paths, spa_dir, bind_addr)
        }
        WebCommand::Status { bind } => {
            let bind_addr = parse_bind(bind.as_deref())?;
            run_status(bind_addr)
        }
        WebCommand::Stop { wait, wait_timeout } => {
            run_stop(paths, wait, Duration::from_secs(wait_timeout))
        }
    }
}

/// Run `kairo_web::serve` in the foreground until SIGTERM/
/// SIGINT. Installs the structured-text tracing subscriber on
/// stderr so the CLI shares the web server's log shape.
fn run_start(
    paths: &StorePaths,
    spa_dir: Option<std::path::PathBuf>,
    bind_addr: SocketAddr,
) -> Result<String, CliError> {
    install_tracing();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(CliError::WebRuntime)?;

    let config = Config {
        bind_addr,
        spa_dir,
        daemon_socket: paths.store.join(SOCKET_FILE),
        pid_file: Some(paths.store.join(PID_FILE)),
    };

    runtime
        .block_on(serve(config))
        .map_err(|error| CliError::WebServe {
            source: Box::new(error),
        })?;

    Ok(String::new())
}

fn run_status(bind_addr: SocketAddr) -> Result<String, CliError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(CliError::WebRuntime)?;

    let probe_result = runtime.block_on(async {
        match timeout(STATUS_PROBE_TIMEOUT, probe_version(&bind_addr)).await {
            Ok(Ok(version)) => Some(version),
            _ => None,
        }
    });

    match probe_result {
        Some(version) => Ok(format!(
            "kairo-web: running\nbind          = {bind_addr}\ndaemon_version = {}\napi_version    = {}\n",
            version.daemon_version, version.api_version,
        )),
        None => Ok(format!(
            "kairo-web: not running\nbind = {bind_addr}\n"
        )),
    }
}

fn run_stop(paths: &StorePaths, wait: bool, wait_timeout: Duration) -> Result<String, CliError> {
    let pid_path = paths.store.join(PID_FILE);
    let pid = read_pid(&pid_path)?;

    kill(Pid::from_raw(pid), Signal::SIGTERM)
        .map_err(|error| CliError::DaemonKill { pid, source: error })?;

    if !wait {
        return Ok(format!("kairo-web: SIGTERM sent (pid {pid})\n"));
    }

    let waited = wait_for_pid_gone(&pid_path, wait_timeout)?;
    Ok(format!(
        "kairo-web: stopped (pid {pid}, PID file gone after {:.1}s)\n",
        waited.as_secs_f64()
    ))
}

fn parse_bind(value: Option<&str>) -> Result<SocketAddr, CliError> {
    match value {
        None => Ok(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            DEFAULT_PORT,
        )),
        Some(value) => value.parse().map_err(|source| CliError::ParseWebBind {
            value: value.to_owned(),
            source,
        }),
    }
}

fn read_pid(path: &Path) -> Result<i32, CliError> {
    let contents = std::fs::read_to_string(path).map_err(|source| CliError::ReadPid {
        path: path.to_path_buf(),
        source,
    })?;
    let trimmed = contents.trim();
    trimmed.parse::<i32>().map_err(|_| CliError::InvalidPid {
        path: path.to_path_buf(),
        contents: trimmed.to_owned(),
    })
}

fn wait_for_pid_gone(pid_path: &Path, max: Duration) -> Result<Duration, CliError> {
    let start = Instant::now();
    loop {
        if !pid_path.exists() {
            return Ok(start.elapsed());
        }
        if start.elapsed() >= max {
            return Err(CliError::WebStopTimeout {
                pid_file: pid_path.to_path_buf(),
                waited: max,
            });
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Lightweight probe: hit `/api/v1/version`, parse the wrapped
/// envelope's `result` field. Mirrors the daemon-client's
/// envelope shape but avoids a typed dependency — the daemon
/// might not be reachable at all (web could be running with a
/// dead daemon), so the field-by-field check is what proves the
/// proxy is alive end-to-end.
async fn probe_version(addr: &SocketAddr) -> Result<VersionPair, ()> {
    let stream = TcpStream::connect(addr).await.map_err(|_| ())?;
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
        .await
        .map_err(|_| ())?;
    let conn_task = tokio::spawn(async move {
        let _ = conn.await;
    });

    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/version")
        .header("host", "kairo-web")
        .body(Empty::<Bytes>::new())
        .map_err(|_| ())?;
    let resp = sender.send_request(req).await.map_err(|_| ())?;
    let status = resp.status();
    let body = resp.into_body().collect().await.map_err(|_| ())?.to_bytes();
    drop(sender);
    let _ = conn_task.await;

    if !status.is_success() {
        return Err(());
    }

    let value: serde_json::Value = serde_json::from_slice(&body).map_err(|_| ())?;
    if value["ok"] != serde_json::Value::Bool(true) {
        return Err(());
    }
    let result = &value["result"];
    Ok(VersionPair {
        daemon_version: result["daemon_version"].as_str().ok_or(())?.to_owned(),
        api_version: result["api_version"].as_str().ok_or(())?.to_owned(),
    })
}

struct VersionPair {
    daemon_version: String,
    api_version: String,
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn parse_bind_defaults_to_loopback() {
        let addr = parse_bind(None).expect("default");
        assert_eq!(addr.ip().to_string(), "127.0.0.1");
        assert_eq!(addr.port(), DEFAULT_PORT);
    }

    #[test]
    fn parse_bind_accepts_explicit_loopback() {
        let addr = parse_bind(Some("127.0.0.1:9000")).expect("parse");
        assert_eq!(addr.port(), 9000);
    }

    #[test]
    fn parse_bind_rejects_garbage() {
        match parse_bind(Some("not-an-addr")) {
            Err(CliError::ParseWebBind { value, .. }) => assert_eq!(value, "not-an-addr"),
            other => panic!("expected ParseWebBind, got {other:?}"),
        }
    }

    #[test]
    fn run_status_returns_not_running_for_unbound_port() {
        // 127.0.0.1 with a port that's almost certainly free.
        let addr: SocketAddr = "127.0.0.1:1".parse().expect("parse");
        let output = run_status(addr).expect("status");
        assert!(output.contains("not running"), "got: {output}");
    }

    #[test]
    fn run_stop_errors_when_pid_file_missing() {
        let dir = TempDir::new().expect("tempdir");
        let paths = StorePaths {
            store: dir.path().to_path_buf(),
            keys: dir.path().join("keys"),
        };

        match run_stop(&paths, false, Duration::from_secs(1)) {
            Err(CliError::ReadPid { path, .. }) => {
                assert!(path.ends_with("web.pid"), "path: {}", path.display());
            }
            other => panic!("expected ReadPid, got {other:?}"),
        }
    }

    #[test]
    fn run_stop_errors_when_pid_is_garbage() {
        let dir = TempDir::new().expect("tempdir");
        let paths = StorePaths {
            store: dir.path().to_path_buf(),
            keys: dir.path().join("keys"),
        };
        std::fs::write(paths.store.join(PID_FILE), "nonsense").expect("write");

        match run_stop(&paths, false, Duration::from_secs(1)) {
            Err(CliError::InvalidPid { contents, .. }) => assert_eq!(contents, "nonsense"),
            other => panic!("expected InvalidPid, got {other:?}"),
        }
    }
}
