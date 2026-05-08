//! Integration tests for the client transport. Each test spins
//! up a real daemon over a tempdir and exercises the client
//! against the live socket.

#![allow(clippy::expect_used, clippy::panic)]

// Production deps the integration test doesn't reference directly
// — silence `unused_crate_dependencies` for them.
use http_body_util as _;
use hyper as _;
use hyper_util as _;
use kairo_identity as _;
use serde as _;

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
    spawn_daemon_at(dir).await
}

async fn spawn_daemon_at(dir: TempDir) -> (DaemonHandle, TempDir) {
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

// ---------------------------------------------------------------------------
// Slice 5: by-id endpoints. Use a populated `StoreFixture` for fixture data.

use kairo_test_support::store::StoreFixture;

async fn spawn_daemon_with_fixture<F>(populate: F) -> (DaemonHandle, TempDir, FixtureIds)
where
    F: FnOnce(&StoreFixture) -> FixtureIds,
{
    let (dir, fixture) = StoreFixture::temp();
    let ids = populate(&fixture);
    drop(fixture); // release the store handle before the daemon opens its own
    let (handle, dir) = spawn_daemon_at(dir).await;
    (handle, dir, ids)
}

#[derive(Default)]
struct FixtureIds {
    actor_id: Option<String>,
    object_id: Option<String>,
    statement_id: Option<String>,
}

#[tokio::test]
async fn client_actor_method_round_trips() {
    let (handle, _dir, ids) = spawn_daemon_with_fixture(|fx| {
        let actor = fx.make_actor();
        FixtureIds {
            actor_id: Some(actor.actor_id.to_string()),
            ..FixtureIds::default()
        }
    })
    .await;

    let client = Client::new(handle.socket_path());
    let id = ids.actor_id.expect("actor id");
    let genesis = client.actor(&id).await.expect("actor call");
    let raw = serde_json::to_value(&genesis).expect("serialize");
    assert_eq!(raw["type"], "ActorGenesis");

    handle.shutdown().await;
}

#[tokio::test]
async fn client_actor_method_returns_404_for_missing_id() {
    let (handle, _dir, _ids) =
        spawn_daemon_with_fixture(|_| FixtureIds::default()).await;
    let client = Client::new(handle.socket_path());
    // shape-valid but absent
    let id = kairo_core::ActorId::from_sha256_digest([0xAB; 32]).to_string();

    match client.actor(&id).await {
        Err(ClientError::Http {
            status, code, ..
        }) => {
            assert_eq!(status, 404);
            assert_eq!(code, "not_found");
        }
        other => panic!("expected 404 not_found, got {other:?}"),
    }

    handle.shutdown().await;
}

#[tokio::test]
async fn client_actor_method_returns_400_for_malformed_id() {
    let (handle, _dir, _ids) =
        spawn_daemon_with_fixture(|_| FixtureIds::default()).await;
    let client = Client::new(handle.socket_path());

    match client.actor("not-a-real-id").await {
        Err(ClientError::Http {
            status, code, ..
        }) => {
            assert_eq!(status, 400);
            assert_eq!(code, "bad_request");
        }
        other => panic!("expected 400 bad_request, got {other:?}"),
    }

    handle.shutdown().await;
}

#[tokio::test]
async fn client_object_method_round_trips() {
    let (handle, _dir, ids) = spawn_daemon_with_fixture(|fx| {
        let actor = fx.make_actor();
        let object = fx.make_object(&actor, "kairo/object");
        FixtureIds {
            object_id: Some(object.object_id.to_string()),
            ..FixtureIds::default()
        }
    })
    .await;

    let client = Client::new(handle.socket_path());
    let id = ids.object_id.expect("object id");
    let object = client.object(&id).await.expect("object call");
    let raw = serde_json::to_value(&object).expect("serialize");
    assert!(raw["body"].is_object());
    assert!(raw["signature"].is_object());

    handle.shutdown().await;
}

#[tokio::test]
async fn client_statement_method_round_trips() {
    use kairo_statement::RevisionId;

    const MANIFEST_TEXT: &str = r#"
        [kairo]
        schema = 1
        kind = "kairo/object"
        name = "fixture"

        [content]
        kind = "tree"
    "#;

    let (handle, _dir, ids) = spawn_daemon_with_fixture(|fx| {
        let actor = fx.make_actor();
        let object = fx.make_object(&actor, "kairo/object");
        let revision = fx.make_revision(
            &actor,
            &object,
            RevisionId::new("git:sha256:r1"),
            MANIFEST_TEXT,
            Vec::new(),
        );
        FixtureIds {
            statement_id: Some(revision.statement_id.to_string()),
            ..FixtureIds::default()
        }
    })
    .await;

    let client = Client::new(handle.socket_path());
    let id = ids.statement_id.expect("statement id");
    let value = client.statement(&id).await.expect("statement call");
    assert!(value["body"].is_object(), "value: {value:#}");

    handle.shutdown().await;
}

#[tokio::test]
async fn client_statement_method_returns_404_for_missing_id() {
    let (handle, _dir, _ids) =
        spawn_daemon_with_fixture(|_| FixtureIds::default()).await;
    let client = Client::new(handle.socket_path());
    let id = kairo_core::StatementId::from_sha256_digest([0xCD; 32]).to_string();

    match client.statement(&id).await {
        Err(ClientError::Http { status, code, .. }) => {
            assert_eq!(status, 404);
            assert_eq!(code, "not_found");
        }
        other => panic!("expected 404 not_found, got {other:?}"),
    }

    handle.shutdown().await;
}
