//! Slice 5 integration tests: `GET /api/v1/{actors,objects,
//! statements}/{id}` over a real Unix socket against an
//! in-process daemon backed by a populated `FilesystemStore`.

#![allow(clippy::expect_used, clippy::panic)]

use axum as _;
use kairo_daemon_client as _;
use kairo_identity as _;
use kairo_object as _;
use utoipa as _;
use kairo_store as _;
use serde as _;
use tempfile as _;
use tokio_util as _;
use tower as _;
use tower_http as _;
use tracing as _;
use tracing_subscriber as _;

use std::path::{Path, PathBuf};
use std::time::Duration;

use http_body_util::{BodyExt, Empty};
use hyper::body::Bytes;
use hyper::Request;
use hyper_util::rt::TokioIo;
use kairo_core::{ActorId, ObjectId, StatementId};
use kairo_daemon::{serve_with_shutdown, Config, Error};
use kairo_test_support::store::StoreFixture;
use serde_json::Value;
use tokio::net::UnixStream;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

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

/// Spawn a daemon over the supplied store dir. Caller is
/// responsible for keeping the `TempDir` alive.
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
        assert!(
            start.elapsed() <= max,
            "daemon never bound socket at {} within {:?}",
            path.display(),
            max
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn http_get(socket: &Path, path: &str) -> (hyper::StatusCode, Bytes) {
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

#[tokio::test]
async fn actors_endpoint_returns_genesis_for_known_id() {
    let (dir, fixture) = StoreFixture::temp();
    let actor = fixture.make_actor();
    let store_path = dir.path().to_path_buf();
    drop(fixture); // close store handle before daemon opens its own

    let handle = spawn_daemon(store_path).await;
    let (status, body) = http_get(
        handle.socket_path(),
        &format!("/api/v1/actors/{}", actor.actor_id),
    )
    .await;
    assert_eq!(status, hyper::StatusCode::OK);

    let json: Value = serde_json::from_slice(&body).expect("parse");
    assert_eq!(json["ok"], Value::Bool(true));
    assert_eq!(json["schema"], "kairo.api.result.v1");
    let result = &json["result"];
    // ActorGenesisJson serializes the body fields directly; the
    // id is derived, not stored. Sanity-check the type tag and a
    // known body field.
    assert_eq!(result["type"], "ActorGenesis", "actor result: {result:#}");
    assert_eq!(result["version"], 1);
    assert!(result["initial_key"].is_object());

    handle.shutdown().await;
    drop(dir);
}

/// Shape-valid ID derived from a fixed digest. Won't collide
/// with any fixture (digests are domain-tagged hashes of bodies
/// — a synthetic constant-byte digest can't represent a real
/// body). Used to drive the not-found path.
fn absent_actor_id() -> ActorId {
    ActorId::from_sha256_digest([0x42; 32])
}
fn absent_object_id() -> ObjectId {
    ObjectId::from_sha256_digest([0x43; 32])
}
fn absent_statement_id() -> StatementId {
    StatementId::from_sha256_digest([0x44; 32])
}

#[tokio::test]
async fn actors_endpoint_returns_404_for_missing_id() {
    let (dir, _fixture) = StoreFixture::temp();
    let store_path = dir.path().to_path_buf();
    drop(_fixture);

    let handle = spawn_daemon(store_path).await;
    let id = absent_actor_id();
    let (status, body) =
        http_get(handle.socket_path(), &format!("/api/v1/actors/{id}")).await;
    assert_eq!(status, hyper::StatusCode::NOT_FOUND);

    let json: Value = serde_json::from_slice(&body).expect("parse");
    assert_eq!(json["ok"], Value::Bool(false));
    assert_eq!(json["error"]["code"], "not_found");

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn actors_endpoint_returns_400_for_malformed_id() {
    let (dir, _fixture) = StoreFixture::temp();
    let store_path = dir.path().to_path_buf();
    drop(_fixture);

    let handle = spawn_daemon(store_path).await;
    let (status, body) =
        http_get(handle.socket_path(), "/api/v1/actors/not!valid@id").await;
    assert_eq!(status, hyper::StatusCode::BAD_REQUEST);
    let json: Value = serde_json::from_slice(&body).expect("parse");
    assert_eq!(json["error"]["code"], "bad_request");

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn objects_endpoint_returns_genesis_statement() {
    let (dir, fixture) = StoreFixture::temp();
    let actor = fixture.make_actor();
    let object = fixture.make_object(&actor, "kairo/object");
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let (status, body) = http_get(
        handle.socket_path(),
        &format!("/api/v1/objects/{}", object.object_id),
    )
    .await;
    assert_eq!(status, hyper::StatusCode::OK);

    let json: Value = serde_json::from_slice(&body).expect("parse");
    let result = &json["result"];
    // ObjectGenesisStatementJson has a `body` field with the
    // genesis body, plus a `signature` field.
    assert!(result["body"].is_object(), "result: {result:#}");
    assert!(result["signature"].is_object(), "result: {result:#}");
    assert_eq!(
        result["body"]["created_by"].as_str(),
        Some(actor.actor_id.as_str())
    );

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn objects_endpoint_returns_404_for_missing_id() {
    let (dir, _fixture) = StoreFixture::temp();
    let store_path = dir.path().to_path_buf();
    drop(_fixture);

    let handle = spawn_daemon(store_path).await;
    let id = absent_object_id();
    let (status, _) =
        http_get(handle.socket_path(), &format!("/api/v1/objects/{id}")).await;
    assert_eq!(status, hyper::StatusCode::NOT_FOUND);

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn statements_endpoint_returns_revision_by_id() {
    let (dir, fixture) = StoreFixture::temp();
    let actor = fixture.make_actor();
    let object = fixture.make_object(&actor, "kairo/object");
    let manifest_text = r#"
        [kairo]
        schema = 1
        kind = "kairo/object"
        name = "fixture"

        [content]
        kind = "tree"
    "#;
    let revision = fixture.make_revision(
        &actor,
        &object,
        kairo_statement::RevisionId::new("git:sha256:r1"),
        manifest_text,
        Vec::new(),
    );
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let (status, body) = http_get(
        handle.socket_path(),
        &format!("/api/v1/statements/{}", revision.statement_id),
    )
    .await;
    assert_eq!(status, hyper::StatusCode::OK);

    let json: Value = serde_json::from_slice(&body).expect("parse");
    let result = &json["result"];
    // Polymorphic shape: revision JSON has body + signature.
    assert!(result["body"].is_object(), "result: {result:#}");

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn statements_endpoint_returns_404_for_missing_id() {
    let (dir, _fixture) = StoreFixture::temp();
    let store_path = dir.path().to_path_buf();
    drop(_fixture);

    let handle = spawn_daemon(store_path).await;
    let id = absent_statement_id();
    let (status, _) =
        http_get(handle.socket_path(), &format!("/api/v1/statements/{id}")).await;
    assert_eq!(status, hyper::StatusCode::NOT_FOUND);

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn statements_endpoint_returns_400_for_malformed_id() {
    let (dir, _fixture) = StoreFixture::temp();
    let store_path = dir.path().to_path_buf();
    drop(_fixture);

    let handle = spawn_daemon(store_path).await;
    let (status, _) = http_get(handle.socket_path(), "/api/v1/statements/!!!").await;
    assert_eq!(status, hyper::StatusCode::BAD_REQUEST);

    handle.shutdown().await;
    drop(dir);
}
