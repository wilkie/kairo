//! Slice 3 (web-client) smoke tests for `kairo-web`:
//!
//! - `/api/v1/version` proxies through to the daemon (typed
//!   round-trip via `kairo_daemon_client::Client` against the
//!   web server's TCP front-end).
//! - `/` returns 200 and the SPA bundle's `index.html`.
//! - An HTML5-route path (e.g., `/objects/abc`) falls back to
//!   `index.html` instead of 404.
//! - Binding to a non-loopback address errors at startup.
//! - The browser write surface (`DECISIONS.md §15`): session
//!   sign-in / cookie-mode write / sign-out round-trip through
//!   the proxy to a real daemon, and rebound-`Host`,
//!   cross-origin, and off-allowlist requests are refused.
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

use http_body_util::{BodyExt, Full};
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
            keys_path: None,
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
    let (status, _, body) = http_request_tcp(addr, "GET", path, &[], Bytes::new()).await;
    (status, body)
}

/// Send one request to the web front-end. `Host` is always the
/// dialed loopback address (the proxy rejects anything else);
/// `extra_headers` may override it.
async fn http_request_tcp(
    addr: &SocketAddr,
    method: &str,
    path: &str,
    extra_headers: &[(&str, &str)],
    body: Bytes,
) -> (hyper::StatusCode, hyper::HeaderMap, Bytes) {
    let stream = TcpStream::connect(addr).await.expect("connect");
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
        .await
        .expect("handshake");
    let conn_task = tokio::spawn(async move {
        let _ = conn.await;
    });
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("host", addr.to_string());
    if let Some(headers) = builder.headers_mut() {
        for (name, value) in extra_headers {
            headers.insert(
                hyper::header::HeaderName::from_bytes(name.as_bytes()).expect("header name"),
                hyper::header::HeaderValue::from_str(value).expect("header value"),
            );
        }
    }
    let req = builder.body(Full::new(body)).expect("build req");
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

/// `kairo_session=<id>` from a `Set-Cookie` header, ready to
/// send back as a `Cookie` header.
fn session_cookie(headers: &hyper::HeaderMap) -> String {
    headers
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find_map(|v| {
            v.split(';')
                .next()
                .filter(|kv| kv.starts_with("kairo_session="))
        })
        .expect("kairo_session Set-Cookie")
        .to_owned()
}

#[tokio::test]
async fn browser_session_write_round_trips_through_proxy() {
    // DECISIONS.md §15: the proxy forwards the session lifecycle
    // and cookie-mode statement writes end-to-end to a real
    // daemon, with the cookie riding through in both directions.
    let (store_dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let store_path = store_dir.path().to_path_buf();
    drop(fixture);

    let daemon_handle = spawn_daemon(store_path.clone()).await;
    let (web_addr, web_handle) = spawn_web(WebConfig {
        bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
        spa_dir: None,
        daemon_socket: store_path.join("daemon.sock"),
        pid_file: None,
    })
    .await;
    let origin = format!("http://{web_addr}");
    let json_write = [
        ("origin", origin.as_str()),
        ("content-type", "application/json"),
    ];

    // Sign in as Alice.
    let sign_in = serde_json::json!({ "actor_id": alice.actor_id.to_string() });
    let (status, headers, body) = http_request_tcp(
        &web_addr,
        "POST",
        "/api/v1/sessions",
        &json_write,
        Bytes::from(sign_in.to_string()),
    )
    .await;
    assert_eq!(
        status,
        hyper::StatusCode::CREATED,
        "sign-in: {}",
        String::from_utf8_lossy(&body)
    );
    let cookie = session_cookie(&headers);

    // The session reads back through the proxy.
    let (status, _, body) = http_request_tcp(
        &web_addr,
        "GET",
        "/api/v1/sessions/current",
        &[("cookie", &cookie)],
        Bytes::new(),
    )
    .await;
    assert_eq!(status, hyper::StatusCode::OK);
    let envelope: serde_json::Value = serde_json::from_slice(&body).expect("parse JSON");
    assert_eq!(envelope["result"]["actor_id"], alice.actor_id.to_string());

    // Cookie-mode write: the daemon signs an ActorTrust as Alice.
    let write = serde_json::json!({
        "body_kind": "ActorTrust",
        "body": {
            "trusted_actor": bob.actor_id.to_string(),
            "scope": "general",
            "decision": "trusted",
            "reason": "kairo-web proxy round trip",
        },
    });
    let mut write_headers = json_write.to_vec();
    write_headers.push(("cookie", &cookie));
    let (status, _, body) = http_request_tcp(
        &web_addr,
        "POST",
        "/api/v1/statements",
        &write_headers,
        Bytes::from(write.to_string()),
    )
    .await;
    assert_eq!(
        status,
        hyper::StatusCode::OK,
        "write: {}",
        String::from_utf8_lossy(&body)
    );
    let envelope: serde_json::Value = serde_json::from_slice(&body).expect("parse JSON");
    let statement = &envelope["result"]["statement"];
    assert_eq!(statement["type"], "ActorTrust");
    assert_eq!(statement["actor"], alice.actor_id.to_string());

    // Sign out, then the same cookie can no longer write.
    let (status, _, _) = http_request_tcp(
        &web_addr,
        "DELETE",
        "/api/v1/sessions/current",
        &[("origin", origin.as_str()), ("cookie", &cookie)],
        Bytes::new(),
    )
    .await;
    assert_eq!(status, hyper::StatusCode::NO_CONTENT);
    let (status, _, _) = http_request_tcp(
        &web_addr,
        "POST",
        "/api/v1/statements",
        &write_headers,
        Bytes::from(write.to_string()),
    )
    .await;
    assert_eq!(status, hyper::StatusCode::UNAUTHORIZED);

    web_handle.shutdown().await;
    daemon_handle.shutdown().await;
    drop(store_dir);
}

#[tokio::test]
async fn rejects_rebound_cross_origin_and_off_allowlist_requests() {
    let (store_dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let store_path = store_dir.path().to_path_buf();
    drop(fixture);

    let daemon_handle = spawn_daemon(store_path.clone()).await;
    let (web_addr, web_handle) = spawn_web(WebConfig {
        bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
        spa_dir: None,
        daemon_socket: store_path.join("daemon.sock"),
        pid_file: None,
    })
    .await;
    let sign_in =
        Bytes::from(serde_json::json!({ "actor_id": alice.actor_id.to_string() }).to_string());

    // DNS rebind: the page's own hostname arrives in `Host`.
    let (status, _, body) = http_request_tcp(
        &web_addr,
        "POST",
        "/api/v1/sessions",
        &[
            ("host", "rebind.example"),
            ("origin", "http://rebind.example"),
        ],
        sign_in.clone(),
    )
    .await;
    assert_eq!(status, hyper::StatusCode::FORBIDDEN);
    assert!(String::from_utf8_lossy(&body).contains("web_forbidden_host"));

    // Another localhost port: same site, different origin.
    let (status, headers, body) = http_request_tcp(
        &web_addr,
        "POST",
        "/api/v1/sessions",
        &[("origin", "http://localhost:3000")],
        sign_in,
    )
    .await;
    assert_eq!(status, hyper::StatusCode::FORBIDDEN);
    assert!(String::from_utf8_lossy(&body).contains("web_cross_origin"));
    assert!(headers.get("set-cookie").is_none(), "no session minted");

    // Off-allowlist method.
    let origin = format!("http://{web_addr}");
    let (status, headers, _) = http_request_tcp(
        &web_addr,
        "PUT",
        "/api/v1/statements",
        &[("origin", origin.as_str())],
        Bytes::new(),
    )
    .await;
    assert_eq!(status, hyper::StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(
        headers.get("allow").and_then(|v| v.to_str().ok()),
        Some("GET, HEAD, POST")
    );

    // Reads need no Origin and still pass.
    let (status, _) = http_get_tcp(&web_addr, "/api/v1/version").await;
    assert_eq!(status, hyper::StatusCode::OK);

    web_handle.shutdown().await;
    daemon_handle.shutdown().await;
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
