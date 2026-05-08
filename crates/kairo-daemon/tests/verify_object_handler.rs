//! Slice 2 (web-client) integration tests:
//! `GET /api/v1/verify-object/{id}` against a real Unix socket.
//!
//! Covers:
//! - genesis-only object → `valid` with an info issue.
//! - object with revision + branch tip → `indeterminate` because
//!   manifest/content layer can't be resolved server-side.
//! - missing object → 404.
//! - malformed id → 400.

#![allow(clippy::expect_used, clippy::panic)]

use axum as _;
use kairo_core as _;
use kairo_identity as _;
use kairo_object as _;
use kairo_statement as _;
use kairo_store as _;
use serde as _;
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
use kairo_core::ObjectId;
use kairo_daemon::{serve_with_shutdown, Config, Error};
use kairo_daemon_client::dto::{ValidationIssueSeverity, ValidationStatus};
use kairo_daemon_client::Client;
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
            "daemon never bound socket within {:?}",
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

const SAMPLE_MANIFEST: &str = r#"
[kairo]
schema = 1
kind = "kairo/object"
name = "fixture"

[content]
kind = "tree"
"#;

#[tokio::test]
async fn verify_object_returns_valid_for_genesis_only_object() {
    let (dir, fixture) = StoreFixture::temp();
    let actor = fixture.make_actor();
    let object = fixture.make_object(&actor, "kairo/object");
    let object_id = object.object_id.clone();
    // No revision, no branch tip — genesis-only state.
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let client = Client::new(handle.socket_path());
    let result = client
        .verify_object(object_id.as_str())
        .await
        .expect("verify_object");

    assert_eq!(result.object_id, object_id.as_str());
    assert_eq!(result.status, ValidationStatus::Valid);
    assert!(result.revision_statement_id.is_none());
    assert!(result.branch_name.is_none());
    assert_eq!(result.issues.len(), 1);
    let issue = &result.issues[0];
    assert_eq!(issue.kind, "branch_head_missing");
    assert_eq!(issue.severity, ValidationIssueSeverity::Info);
    assert_eq!(issue.actor_id.as_deref(), Some(actor.actor_id.as_str()));

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn verify_object_returns_indeterminate_for_object_with_revision() {
    let (dir, fixture) = StoreFixture::temp();
    let actor = fixture.make_actor();
    let object = fixture.make_object(&actor, "kairo/object");
    let revision = fixture.make_revision(
        &actor,
        &object,
        kairo_statement::RevisionId::new("git:sha256:r1"),
        SAMPLE_MANIFEST,
        Vec::new(),
    );
    fixture.set_branch(&actor, &object, &revision, "head");
    let object_id = object.object_id.clone();
    let revision_id = revision.statement_id.clone();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let client = Client::new(handle.socket_path());
    let result = client
        .verify_object(object_id.as_str())
        .await
        .expect("verify_object");

    assert_eq!(result.object_id, object_id.as_str());
    assert_eq!(
        result.status,
        ValidationStatus::Indeterminate,
        "issues: {:#?}",
        result.issues
    );
    assert_eq!(
        result.revision_statement_id.as_deref(),
        Some(revision_id.as_str())
    );
    assert_eq!(result.branch_name.as_deref(), Some("head"));

    // Both manifest and content-layer must be flagged as indeterminate.
    let kinds: Vec<&str> = result.issues.iter().map(|i| i.kind.as_str()).collect();
    assert!(
        kinds.contains(&"manifest_not_provided"),
        "expected manifest_not_provided in {kinds:?}"
    );
    assert!(
        kinds.contains(&"content_layer_indeterminate"),
        "expected content_layer_indeterminate in {kinds:?}"
    );
    // No errors for a freshly-signed valid revision.
    assert!(
        result
            .issues
            .iter()
            .all(|i| !matches!(i.severity, ValidationIssueSeverity::Error)),
        "no error issues expected: {:#?}",
        result.issues
    );

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn verify_object_returns_404_when_object_missing() {
    let (dir, _fixture) = StoreFixture::temp();
    let store_path = dir.path().to_path_buf();
    drop(_fixture);

    let handle = spawn_daemon(store_path).await;
    let absent = ObjectId::from_sha256_digest([0x43; 32]);
    let (status, body) = http_get(
        handle.socket_path(),
        &format!("/api/v1/verify-object/{absent}"),
    )
    .await;

    assert_eq!(status, hyper::StatusCode::NOT_FOUND);
    let json: Value = serde_json::from_slice(&body).expect("parse");
    assert_eq!(json["ok"], false);
    assert_eq!(json["error"]["code"], "not_found");

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn verify_object_returns_400_for_malformed_id() {
    let (dir, _fixture) = StoreFixture::temp();
    let store_path = dir.path().to_path_buf();
    drop(_fixture);

    let handle = spawn_daemon(store_path).await;
    let (status, body) = http_get(handle.socket_path(), "/api/v1/verify-object/not-an-id").await;

    assert_eq!(status, hyper::StatusCode::BAD_REQUEST);
    let json: Value = serde_json::from_slice(&body).expect("parse");
    assert_eq!(json["ok"], false);
    assert_eq!(json["error"]["code"], "bad_request");

    handle.shutdown().await;
    drop(dir);
}
