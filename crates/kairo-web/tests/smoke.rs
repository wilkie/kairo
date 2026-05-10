//! Slice 3 (web-client) smoke tests for `kairo-web`:
//!
//! - `/api/v1/version` proxies through to the daemon (typed
//!   round-trip via `kairo_daemon_client::Client` against the
//!   web server's TCP front-end).
//! - `/` returns 200 and the SPA bundle's `index.html`.
//! - An HTML5-route path (e.g., `/objects/abc`) falls back to
//!   `index.html` instead of 404.
//! - Binding to a non-loopback address errors at startup.
//!
//! The harness spins up a kairo-daemon in-process over a tempdir
//! and a kairo-web in-process on `127.0.0.1:0` (ephemeral port)
//! pointing at the daemon's socket. Both are torn down cleanly on
//! drop.

#![allow(clippy::expect_used, clippy::panic)]

use axum as _;
use http_body_util as _;
use hyper as _;
use hyper_util as _;
use kairo_core as _;
use kairo_daemon_client as _;
use kairo_store as _;
use serde_json as _;
use tempfile as _;
use tokio as _;
use tower as _;
use tower_http as _;
use tracing as _;
use tracing_subscriber as _;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use http_body_util::{BodyExt, Empty};
use hyper::body::Bytes;
use hyper::Request;
use hyper_util::rt::TokioIo;
use kairo_daemon::{serve_with_shutdown as daemon_serve, Config as DaemonConfig};
use kairo_test_support::store::StoreFixture;
use kairo_web::{serve_with_shutdown as web_serve, Config as WebConfig};
use tempfile::TempDir;
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

struct ProcHandle<E: std::fmt::Debug + Send + 'static> {
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<Result<(), E>>>,
}

impl<E: std::fmt::Debug + Send + 'static> ProcHandle<E> {
    async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(task) = self.task.take() {
            task.await.expect("join").expect("serve");
        }
    }
}

impl<E: std::fmt::Debug + Send + 'static> Drop for ProcHandle<E> {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

async fn spawn_daemon(store_path: PathBuf) -> ProcHandle<kairo_daemon::Error> {
    let socket_path = store_path.join("daemon.sock");
    let (tx, rx) = oneshot::channel();
    let task = tokio::spawn(daemon_serve(
        DaemonConfig {
            store_path: store_path.clone(),
        },
        async move {
            let _ = rx.await;
        },
    ));
    wait_for_unix_socket(&socket_path, Duration::from_secs(5)).await;
    ProcHandle {
        shutdown: Some(tx),
        task: Some(task),
    }
}

async fn spawn_web(config: WebConfig) -> (SocketAddr, ProcHandle<kairo_web::Error>) {
    // Probe the configured bind address for liveness *after*
    // spawning. Because `--bind` may be `127.0.0.1:0` we have to
    // bind first, read the actual port back, and then start.
    // We accomplish this by binding the listener inside the
    // server itself but reusing the configured port — the
    // ephemeral-port pattern means the test asks for port 0, the
    // server picks one, and we discover it by polling the socket.
    let (tx, rx) = oneshot::channel();
    // We need the actual port the OS picked, so start by probing
    // the configured port (works for explicit ports) — for the
    // ephemeral case we bind a temporary listener, read the port,
    // drop it, and pass the port back to the server.
    let bind = if config.bind_addr.port() == 0 {
        let listener = std::net::TcpListener::bind(config.bind_addr).expect("probe bind");
        let bound = listener.local_addr().expect("probe local_addr");
        drop(listener);
        bound
    } else {
        config.bind_addr
    };

    let mut config = config;
    config.bind_addr = bind;

    let task = tokio::spawn(web_serve(config, async move {
        let _ = rx.await;
    }));
    wait_for_tcp(&bind, Duration::from_secs(5)).await;
    (
        bind,
        ProcHandle {
            shutdown: Some(tx),
            task: Some(task),
        },
    )
}

async fn wait_for_unix_socket(path: &Path, max: Duration) {
    let start = std::time::Instant::now();
    loop {
        if path.exists() && tokio::net::UnixStream::connect(path).await.is_ok() {
            return;
        }
        assert!(start.elapsed() <= max, "daemon socket not bound in {max:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn wait_for_tcp(addr: &SocketAddr, max: Duration) {
    let start = std::time::Instant::now();
    loop {
        if TcpStream::connect(addr).await.is_ok() {
            return;
        }
        assert!(
            start.elapsed() <= max,
            "web port {addr} not bound in {max:?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn http_get_tcp(addr: &SocketAddr, path: &str) -> (hyper::StatusCode, Bytes) {
    let stream = TcpStream::connect(addr).await.expect("connect");
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
        .header("host", "test")
        .body(Empty::<Bytes>::new())
        .expect("build req");
    let resp = sender.send_request(req).await.expect("send");
    let status = resp.status();
    let body = resp
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    drop(sender);
    let _ = conn_task.await;
    (status, body)
}

fn write_spa_dir() -> TempDir {
    let dir = TempDir::new().expect("tempdir");
    std::fs::write(
        dir.path().join("index.html"),
        "<!doctype html><title>spa</title>",
    )
    .expect("write index.html");
    std::fs::write(dir.path().join("app.js"), "// hello").expect("write app.js");
    dir
}

#[tokio::test]
async fn proxies_api_v1_version_through_to_daemon() {
    let (store_dir, _fixture) = StoreFixture::temp();
    let store_path = store_dir.path().to_path_buf();
    drop(_fixture);

    let daemon_handle = spawn_daemon(store_path.clone()).await;
    let spa_dir = write_spa_dir();

    let (web_addr, web_handle) = spawn_web(WebConfig {
        bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
        spa_dir: Some(spa_dir.path().to_path_buf()),
        daemon_socket: store_path.join("daemon.sock"),
        pid_file: None,
    })
    .await;

    let (status, body) = http_get_tcp(&web_addr, "/api/v1/version").await;
    assert_eq!(status, hyper::StatusCode::OK);
    let envelope: serde_json::Value = serde_json::from_slice(&body).expect("parse JSON");
    assert_eq!(envelope["ok"], true, "envelope: {envelope}");
    assert_eq!(envelope["schema"], "kairo.api.result.v1");
    let result = &envelope["result"];
    assert_eq!(result["api_version"], "v1");
    assert!(
        result["daemon_version"]
            .as_str()
            .is_some_and(|s| !s.is_empty()),
        "daemon_version: {result}"
    );

    web_handle.shutdown().await;
    daemon_handle.shutdown().await;
    drop(spa_dir);
    drop(store_dir);
}

#[tokio::test]
async fn serves_index_html_at_root() {
    let (store_dir, _fixture) = StoreFixture::temp();
    let store_path = store_dir.path().to_path_buf();
    drop(_fixture);

    let daemon_handle = spawn_daemon(store_path.clone()).await;
    let spa_dir = write_spa_dir();

    let (web_addr, web_handle) = spawn_web(WebConfig {
        bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
        spa_dir: Some(spa_dir.path().to_path_buf()),
        daemon_socket: store_path.join("daemon.sock"),
        pid_file: None,
    })
    .await;

    let (status, body) = http_get_tcp(&web_addr, "/").await;
    assert_eq!(status, hyper::StatusCode::OK);
    assert!(
        body.windows(b"<title>spa</title>".len())
            .any(|w| w == b"<title>spa</title>"),
        "expected SPA index.html in body: {:?}",
        std::str::from_utf8(&body).unwrap_or("<non-utf8>")
    );

    web_handle.shutdown().await;
    daemon_handle.shutdown().await;
    drop(spa_dir);
    drop(store_dir);
}

#[tokio::test]
async fn serves_spa_static_asset() {
    let (store_dir, _fixture) = StoreFixture::temp();
    let store_path = store_dir.path().to_path_buf();
    drop(_fixture);

    let daemon_handle = spawn_daemon(store_path.clone()).await;
    let spa_dir = write_spa_dir();

    let (web_addr, web_handle) = spawn_web(WebConfig {
        bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
        spa_dir: Some(spa_dir.path().to_path_buf()),
        daemon_socket: store_path.join("daemon.sock"),
        pid_file: None,
    })
    .await;

    let (status, body) = http_get_tcp(&web_addr, "/app.js").await;
    assert_eq!(status, hyper::StatusCode::OK);
    assert_eq!(&body[..], b"// hello");

    web_handle.shutdown().await;
    daemon_handle.shutdown().await;
    drop(spa_dir);
    drop(store_dir);
}

#[tokio::test]
async fn html5_route_falls_back_to_index() {
    let (store_dir, _fixture) = StoreFixture::temp();
    let store_path = store_dir.path().to_path_buf();
    drop(_fixture);

    let daemon_handle = spawn_daemon(store_path.clone()).await;
    let spa_dir = write_spa_dir();

    let (web_addr, web_handle) = spawn_web(WebConfig {
        bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
        spa_dir: Some(spa_dir.path().to_path_buf()),
        daemon_socket: store_path.join("daemon.sock"),
        pid_file: None,
    })
    .await;

    let (status, body) = http_get_tcp(&web_addr, "/objects/abc").await;
    assert_eq!(status, hyper::StatusCode::OK);
    assert!(
        body.windows(b"<title>spa</title>".len())
            .any(|w| w == b"<title>spa</title>"),
        "expected SPA fallback at HTML5 route: {:?}",
        std::str::from_utf8(&body).unwrap_or("<non-utf8>")
    );

    web_handle.shutdown().await;
    daemon_handle.shutdown().await;
    drop(spa_dir);
    drop(store_dir);
}

#[tokio::test]
async fn refuses_non_loopback_bind() {
    let (store_dir, _fixture) = StoreFixture::temp();
    let store_path = store_dir.path().to_path_buf();
    drop(_fixture);

    let daemon_handle = spawn_daemon(store_path.clone()).await;
    let spa_dir = write_spa_dir();

    // 0.0.0.0 is not a loopback address; v1 rejects it at startup.
    let result = web_serve(
        WebConfig {
            bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
            spa_dir: Some(spa_dir.path().to_path_buf()),
            daemon_socket: store_path.join("daemon.sock"),
            pid_file: None,
        },
        async {
            // Future never resolves; serve must error before we get here.
            std::future::pending::<()>().await
        },
    )
    .await;

    let error = result.expect_err("non-loopback bind should error");
    assert!(
        matches!(error, kairo_web::Error::NonLoopbackBind { .. }),
        "expected NonLoopbackBind, got {error:?}"
    );

    daemon_handle.shutdown().await;
    drop(spa_dir);
    drop(store_dir);
}

#[tokio::test]
async fn api_only_mode_proxies_api_and_404s_root() {
    let (store_dir, _fixture) = StoreFixture::temp();
    let store_path = store_dir.path().to_path_buf();
    drop(_fixture);

    let daemon_handle = spawn_daemon(store_path.clone()).await;

    // No spa_dir → API-only mode. Browser hits to / get the
    // helpful 404 fallback; /api/v1/* still proxies through.
    let (web_addr, web_handle) = spawn_web(WebConfig {
        bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
        spa_dir: None,
        daemon_socket: store_path.join("daemon.sock"),
        pid_file: None,
    })
    .await;

    // /api/v1/version still works.
    let (api_status, api_body) = http_get_tcp(&web_addr, "/api/v1/version").await;
    assert_eq!(api_status, hyper::StatusCode::OK);
    let envelope: serde_json::Value = serde_json::from_slice(&api_body).expect("parse JSON");
    assert_eq!(envelope["ok"], true, "envelope: {envelope}");

    // / returns 404 with the helpful body.
    let (root_status, root_body) = http_get_tcp(&web_addr, "/").await;
    assert_eq!(root_status, hyper::StatusCode::NOT_FOUND);
    let body_text = std::str::from_utf8(&root_body).expect("utf-8 body");
    assert!(
        body_text.contains("API-proxy-only mode"),
        "expected helpful 404 body, got: {body_text}"
    );
    assert!(
        body_text.contains("--spa-dir"),
        "expected --spa-dir hint in body, got: {body_text}"
    );

    // An HTML5-route path also returns the same 404 (no
    // index.html fallback when there's no SPA).
    let (subpath_status, _) = http_get_tcp(&web_addr, "/objects/abc").await;
    assert_eq!(subpath_status, hyper::StatusCode::NOT_FOUND);

    web_handle.shutdown().await;
    daemon_handle.shutdown().await;
    drop(store_dir);
}
