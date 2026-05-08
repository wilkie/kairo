//! Integration tests for slice 2: socket bind, double-start
//! refusal, graceful shutdown, and the `/api/v1/version` and
//! `/api/v1/status` round trips over the real Unix socket.

#![allow(clippy::expect_used, clippy::panic)]

// All HTTP-stack deps live in the lib; these shims silence
// `unused_crate_dependencies` for crates the test doesn't
// reference directly.
use axum as _;
use kairo_core as _;
use kairo_daemon_client as _;
use kairo_identity as _;
use kairo_object as _;
use kairo_statement as _;
use kairo_store as _;
use kairo_test_support as _;
use serde as _;
use tokio_util as _;
use tower as _;
use tower_http as _;
use tracing as _;
use tracing_subscriber as _;
use utoipa as _;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use http_body_util::{BodyExt, Empty};
use hyper::body::Bytes;
use hyper::Request;
use hyper_util::rt::TokioIo;
use kairo_daemon::{serve, serve_with_shutdown, Config, Error};
use serde_json::Value;
use tempfile::TempDir;
use tokio::net::UnixStream;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// Drop guard that signals shutdown and joins the daemon task.
struct DaemonHandle {
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<Result<(), Error>>>,
    socket_path: PathBuf,
}

impl DaemonHandle {
    fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    async fn shutdown(mut self) -> Result<(), Error> {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(task) = self.task.take() {
            task.await.expect("daemon task join")
        } else {
            Ok(())
        }
    }
}

impl Drop for DaemonHandle {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

/// Start a daemon over a fresh tempdir and wait for the socket
/// to appear.
async fn spawn_daemon() -> (DaemonHandle, TempDir) {
    let dir = TempDir::new().expect("tempdir");
    let store_path = dir.path().to_path_buf();
    let socket_path = store_path.join("daemon.sock");

    let (tx, rx) = oneshot::channel();
    let task = tokio::spawn(serve_with_shutdown(
        Config {
            store_path: store_path.clone(),
        },
        async move {
            let _ = rx.await;
        },
    ));

    wait_for_socket(&socket_path, Duration::from_secs(5)).await;

    let handle = DaemonHandle {
        shutdown: Some(tx),
        task: Some(task),
        socket_path,
    };
    (handle, dir)
}

async fn wait_for_socket(path: &Path, max: Duration) {
    let start = std::time::Instant::now();
    loop {
        if path.exists() && UnixStream::connect(path).await.is_ok() {
            return;
        }
        assert!(
            start.elapsed() <= max,
            "daemon never bound socket at {} within {:?}",
            path.display(),
            max
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// HTTP-over-Unix-socket GET. Drives a fresh hyper::client::conn
/// handshake per call; that's slow but simple, which is what
/// integration tests need.
async fn http_get(socket: &Path, path: &str) -> (hyper::StatusCode, Bytes) {
    let stream = UnixStream::connect(socket).await.expect("connect socket");
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
        .await
        .expect("http1 handshake");
    let conn_task = tokio::spawn(async move {
        if let Err(error) = conn.await {
            eprintln!("client connection error: {error}");
        }
    });

    let req = Request::builder()
        .method("GET")
        .uri(path)
        .header("host", "daemon")
        .body(Empty::<Bytes>::new())
        .expect("build req");
    let resp = sender.send_request(req).await.expect("send req");
    let status = resp.status();
    let body = resp
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();

    drop(sender);
    let _ = conn_task.await;

    (status, body)
}

#[tokio::test]
async fn binds_socket_with_0600_permissions() {
    let (handle, _dir) = spawn_daemon().await;

    let metadata = std::fs::metadata(handle.socket_path()).expect("socket metadata");
    let mode = metadata.permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "socket mode should be 0600");

    handle.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn version_endpoint_returns_envelope() {
    let (handle, _dir) = spawn_daemon().await;

    let (status, body) = http_get(handle.socket_path(), "/api/v1/version").await;
    assert_eq!(status, hyper::StatusCode::OK);

    let json: Value = serde_json::from_slice(&body).expect("parse json");
    assert_eq!(json["ok"], Value::Bool(true));
    assert_eq!(json["schema"], "kairo.api.result.v1");

    let result = &json["result"];
    assert_eq!(result["api_version"], "v1");
    assert!(result["daemon_version"].is_string());
    assert!(result["core_version"].is_string());
    assert!(result["store_version"].is_string());

    handle.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn status_endpoint_returns_envelope() {
    let (handle, dir) = spawn_daemon().await;

    let (status, body) = http_get(handle.socket_path(), "/api/v1/status").await;
    assert_eq!(status, hyper::StatusCode::OK);

    let json: Value = serde_json::from_slice(&body).expect("parse json");
    assert_eq!(json["ok"], Value::Bool(true));
    assert_eq!(json["schema"], "kairo.api.result.v1");

    let result = &json["result"];
    assert_eq!(result["daemon_running"], Value::Bool(true));
    assert_eq!(
        result["store_path"],
        Value::String(dir.path().display().to_string())
    );
    assert_eq!(result["store_schema_version"], "1");
    assert!(result["pid"].as_u64().is_some());
    assert!(result["daemon_version"].is_string());

    handle.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn double_start_refuses_when_socket_is_live() {
    let (handle, dir) = spawn_daemon().await;

    let result = serve(Config {
        store_path: dir.path().to_path_buf(),
    })
    .await;

    match result {
        Err(Error::AlreadyRunning { socket }) => {
            assert_eq!(socket, dir.path().join("daemon.sock"));
        }
        other => panic!("expected AlreadyRunning error, got {other:?}"),
    }

    handle.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn stale_socket_is_unlinked_and_replaced() {
    let dir = TempDir::new().expect("tempdir");
    let store_path = dir.path().to_path_buf();
    let socket_path = store_path.join("daemon.sock");

    // Pre-create a stale socket file by writing to the path
    // directly. UnixStream::connect on this will fail (it's a
    // regular file), which triggers the unlink-and-rebind path.
    std::fs::write(&socket_path, b"stale").expect("write stale socket");

    let (tx, rx) = oneshot::channel();
    let task = tokio::spawn(serve_with_shutdown(
        Config {
            store_path: store_path.clone(),
        },
        async move {
            let _ = rx.await;
        },
    ));
    wait_for_socket(&socket_path, Duration::from_secs(5)).await;

    let (status, _) = http_get(&socket_path, "/api/v1/version").await;
    assert_eq!(status, hyper::StatusCode::OK);

    let _ = tx.send(());
    task.await.expect("join").expect("serve");
}

#[tokio::test]
async fn shutdown_unlinks_socket_and_pid_file() {
    let (handle, dir) = spawn_daemon().await;
    let socket_path = handle.socket_path().to_path_buf();
    let pid_path = dir.path().join("daemon.pid");

    assert!(socket_path.exists(), "socket exists while running");
    assert!(pid_path.exists(), "PID file exists while running");

    handle.shutdown().await.expect("shutdown");

    assert!(!socket_path.exists(), "socket cleaned up after shutdown");
    assert!(!pid_path.exists(), "PID file cleaned up after shutdown");
}

#[tokio::test]
async fn pid_file_contains_process_id() {
    let (handle, dir) = spawn_daemon().await;
    let pid_path = dir.path().join("daemon.pid");

    let contents = std::fs::read_to_string(&pid_path).expect("read pid");
    let pid: u32 = contents.trim().parse().expect("pid is u32");
    assert_eq!(pid, std::process::id());

    handle.shutdown().await.expect("shutdown");
}
