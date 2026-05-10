//! Slice 7 integration tests: `GET /api/v1/blobs/{id}` over a
//! real Unix socket, including a multi-MB round-trip to exercise
//! the streaming path.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use axum as _;
use kairo_daemon_client as _;
use kairo_identity as _;
use kairo_object as _;
use kairo_statement as _;
use serde as _;
use serde_json as _;
use tempfile as _;
use tokio_util as _;
use tower as _;
use tower_http as _;
use tracing as _;
use tracing_subscriber as _;
use utoipa as _;

use std::path::{Path, PathBuf};
use std::time::Duration;

use http_body_util::{BodyExt, Empty};
use hyper::body::Bytes;
use hyper::Request;
use hyper_util::rt::TokioIo;
use kairo_core::BlobId;
use kairo_daemon::{serve_with_shutdown, Config, Error};
use kairo_store::BlobStore;
use kairo_test_support::store::StoreFixture;
use tokio::net::UnixStream;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

const STREAM_DOMAIN: &[u8] = b"kairo-daemon-test/blob";

struct DaemonHandle {
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<Result<(), Error>>>,
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
            task.await.expect("daemon join").expect("serve");
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

async fn spawn_daemon(store_path: PathBuf) -> DaemonHandle {
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
    DaemonHandle {
        shutdown: Some(tx),
        task: Some(task),
        socket_path,
    }
}

async fn wait_for_socket(path: &Path, max: Duration) {
    let start = std::time::Instant::now();
    loop {
        if path.exists() && UnixStream::connect(path).await.is_ok() {
            return;
        }
        assert!(start.elapsed() <= max, "socket never bound");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn http_get_full(socket: &Path, path: &str) -> (hyper::StatusCode, hyper::HeaderMap, Bytes) {
    let stream = UnixStream::connect(socket).await.expect("connect");
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
        .await
        .expect("handshake");
    let conn_task = tokio::spawn(async move {
        let _ = conn.await;
    });
    let req = Request::builder()
        .method("GET")
        .uri(path)
        .header("host", "daemon")
        .body(Empty::<Bytes>::new())
        .expect("build");
    let resp = sender.send_request(req).await.expect("send");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body = resp
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    drop(sender);
    let _ = conn_task.await;
    (status, headers, body)
}

#[tokio::test]
async fn blob_round_trips_for_small_payload() {
    let (dir, fixture) = StoreFixture::temp();
    let payload = b"hello, blobs".repeat(100);
    let blob_id = BlobId::from_bytes(STREAM_DOMAIN, &payload);
    fixture
        .store
        .put_blob(&blob_id, &payload)
        .expect("put_blob");
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let (status, headers, body) =
        http_get_full(handle.socket_path(), &format!("/api/v1/blobs/{blob_id}")).await;
    assert_eq!(status, hyper::StatusCode::OK);
    assert_eq!(
        headers.get(hyper::header::CONTENT_TYPE).unwrap(),
        "application/octet-stream"
    );
    assert_eq!(
        headers
            .get(hyper::header::CONTENT_LENGTH)
            .unwrap()
            .to_str()
            .unwrap()
            .parse::<u64>()
            .unwrap(),
        payload.len() as u64
    );
    assert_eq!(body.as_ref(), payload.as_slice());

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn blob_round_trips_for_multi_mb_payload() {
    // 4 MiB — well past the 64 KiB chunk size, far enough that
    // a buffer-everything implementation would obviously show up
    // in resident memory. Pseudo-random content so a degenerate
    // run-length compressor can't paper over a streaming bug.
    const SIZE: usize = 4 * 1024 * 1024;
    let mut payload = Vec::with_capacity(SIZE);
    let mut state: u32 = 0x9E37_79B1; // a seed
    while payload.len() < SIZE {
        state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
        payload.extend_from_slice(&state.to_le_bytes());
    }
    payload.truncate(SIZE);

    let (dir, fixture) = StoreFixture::temp();
    let blob_id = BlobId::from_bytes(STREAM_DOMAIN, &payload);
    fixture
        .store
        .put_blob(&blob_id, &payload)
        .expect("put_blob");
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let (status, _headers, body) =
        http_get_full(handle.socket_path(), &format!("/api/v1/blobs/{blob_id}")).await;
    assert_eq!(status, hyper::StatusCode::OK);
    assert_eq!(body.len(), payload.len());
    // Compare via slice equality. Vec<u8> and Bytes both deref
    // to [u8].
    assert_eq!(body.as_ref(), payload.as_slice());

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn blob_returns_404_for_missing_id() {
    let (dir, _fixture) = StoreFixture::temp();
    let store_path = dir.path().to_path_buf();
    drop(_fixture);

    let handle = spawn_daemon(store_path).await;
    // shape-valid but not on disk
    let absent = BlobId::from_sha256_digest([0xEE; 32]);
    let (status, headers, _) =
        http_get_full(handle.socket_path(), &format!("/api/v1/blobs/{absent}")).await;
    assert_eq!(status, hyper::StatusCode::NOT_FOUND);
    // 404 takes the JSON envelope path, not octet-stream.
    assert_eq!(
        headers
            .get(hyper::header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap(),
        "application/json"
    );

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn blob_returns_400_for_malformed_id() {
    let (dir, _fixture) = StoreFixture::temp();
    let store_path = dir.path().to_path_buf();
    drop(_fixture);

    let handle = spawn_daemon(store_path).await;
    let (status, _, _) = http_get_full(handle.socket_path(), "/api/v1/blobs/!!!").await;
    assert_eq!(status, hyper::StatusCode::BAD_REQUEST);

    handle.shutdown().await;
    drop(dir);
}
