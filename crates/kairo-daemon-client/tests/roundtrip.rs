//! Integration tests for the client transport. Each test spins
//! up a real daemon over a tempdir and exercises the client
//! against the live socket.

#![allow(clippy::expect_used, clippy::panic)]

// Production deps the integration test doesn't reference directly
// — silence `unused_crate_dependencies` for them.
use http_body_util as _;
use hyper as _;
use hyper_util as _;
use serde as _;
use serde_json as _;

use std::path::{Path, PathBuf};
use std::time::Duration;

use kairo_daemon::{serve_with_shutdown, Config};
use kairo_daemon_client::{Client, ClientError};
use tempfile::TempDir;
use tokio::net::UnixStream;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

struct DaemonHandle {
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<Result<(), kairo_daemon::Error>>>,
    socket_path: PathBuf,
}

impl DaemonHandle {
    fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(task) = self.task.take() {
            task.await.expect("daemon join").expect("daemon serve");
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

#[tokio::test]
async fn version_round_trips_through_client() {
    let (handle, _dir) = spawn_daemon().await;
    let client = Client::new(handle.socket_path());

    let version = client.version().await.expect("version call");
    assert_eq!(version.api_version, "v1");
    assert!(!version.daemon_version.is_empty());
    assert!(!version.core_version.is_empty());
    assert!(!version.store_version.is_empty());

    handle.shutdown().await;
}

#[tokio::test]
async fn status_round_trips_through_client() {
    let (handle, dir) = spawn_daemon().await;
    let client = Client::new(handle.socket_path());

    let status = client.status().await.expect("status call");
    assert!(status.daemon_running);
    assert_eq!(status.store_path, dir.path().display().to_string());
    assert_eq!(status.store_schema_version, "1");
    assert_eq!(status.pid, std::process::id());
    assert!(!status.daemon_version.is_empty());

    handle.shutdown().await;
}

#[tokio::test]
async fn probe_returns_true_for_live_daemon() {
    let (handle, _dir) = spawn_daemon().await;
    let client = Client::new(handle.socket_path());

    assert!(client.probe(None).await);

    handle.shutdown().await;
}

#[tokio::test]
async fn probe_returns_false_for_missing_socket() {
    let dir = TempDir::new().expect("tempdir");
    let socket = dir.path().join("nonexistent.sock");
    let client = Client::new(&socket);

    assert!(!client.probe(Some(Duration::from_millis(200))).await);
}

#[tokio::test]
async fn probe_returns_false_after_daemon_shuts_down() {
    let (handle, _dir) = spawn_daemon().await;
    let client = Client::new(handle.socket_path());
    assert!(client.probe(None).await);

    handle.shutdown().await;

    assert!(!client.probe(Some(Duration::from_millis(200))).await);
}

#[tokio::test]
async fn probe_returns_false_when_socket_is_a_regular_file() {
    let dir = TempDir::new().expect("tempdir");
    let fake = dir.path().join("not-a-socket");
    std::fs::write(&fake, b"i am a file").expect("write fake socket");
    let client = Client::new(&fake);

    assert!(!client.probe(Some(Duration::from_millis(200))).await);
}

#[tokio::test]
async fn version_returns_connect_error_when_daemon_is_absent() {
    let dir = TempDir::new().expect("tempdir");
    let socket = dir.path().join("nonexistent.sock");
    let client = Client::new(&socket);

    let result = client.version().await;
    match result {
        Err(ClientError::Connect(_)) => {}
        other => panic!("expected ClientError::Connect, got {other:?}"),
    }
}

#[tokio::test]
async fn client_handle_is_clone_and_concurrent_safe() {
    let (handle, _dir) = spawn_daemon().await;
    let client = Client::new(handle.socket_path());

    let mut tasks = Vec::new();
    for _ in 0..8 {
        let client = client.clone();
        tasks.push(tokio::spawn(
            async move { client.version().await.map(|v| v.api_version) },
        ));
    }

    for task in tasks {
        let api_version = task.await.expect("join").expect("version call");
        assert_eq!(api_version, "v1");
    }

    handle.shutdown().await;
}
