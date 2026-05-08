//! Integration tests for the client transport. Each test spins
//! up a real daemon over a tempdir and exercises the client
//! against the live socket.

#![allow(clippy::expect_used, clippy::panic)]

// Production deps the integration test doesn't reference directly
// — silence `unused_crate_dependencies` for them.
use futures_util as _;
use http_body_util as _;
use hyper as _;
use hyper_util as _;
use kairo_identity as _;
use serde as _;
use tokio_util as _;

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

// ---------------------------------------------------------------------------
// Slice 6: branch / version-tag / trust / capability methods.

const SAMPLE_MANIFEST: &str = r#"
[kairo]
schema = 1
kind = "kairo/object"
name = "fixture"

[content]
kind = "tree"
"#;

#[tokio::test]
async fn client_list_branches_round_trips() {
    use kairo_statement::RevisionId;

    let (dir, fixture) = StoreFixture::temp();
    let actor = fixture.make_actor();
    let object = fixture.make_object(&actor, "kairo/object");
    let revision = fixture.make_revision(
        &actor,
        &object,
        RevisionId::new("git:sha256:r1"),
        SAMPLE_MANIFEST,
        Vec::new(),
    );
    fixture.set_branch(&actor, &object, &revision, "head");
    let object_id = object.object_id.to_string();
    let actor_id = actor.actor_id.to_string();
    drop(fixture);

    let (handle, _dir) = spawn_daemon_at(dir).await;
    let client = Client::new(handle.socket_path());

    let tips = client.list_branches(&object_id).await.expect("list");
    assert_eq!(tips.len(), 1);
    assert_eq!(tips[0].name, "head");
    assert_eq!(tips[0].actor, actor_id);
    assert_eq!(tips[0].object, object_id);

    handle.shutdown().await;
}

#[tokio::test]
async fn client_latest_branch_round_trips() {
    use kairo_statement::RevisionId;

    let (dir, fixture) = StoreFixture::temp();
    let actor = fixture.make_actor();
    let object = fixture.make_object(&actor, "kairo/object");
    let revision = fixture.make_revision(
        &actor,
        &object,
        RevisionId::new("git:sha256:r1"),
        SAMPLE_MANIFEST,
        Vec::new(),
    );
    fixture.set_branch(&actor, &object, &revision, "head");
    let object_id = object.object_id.to_string();
    drop(fixture);

    let (handle, _dir) = spawn_daemon_at(dir).await;
    let client = Client::new(handle.socket_path());

    let signed = client
        .latest_branch(&object_id, "head", None)
        .await
        .expect("latest branch");
    let raw = serde_json::to_value(&signed).expect("serialize");
    assert_eq!(raw["body"]["name"], "head");

    handle.shutdown().await;
}

#[tokio::test]
async fn client_latest_branch_returns_404_when_missing() {
    let (dir, fixture) = StoreFixture::temp();
    let actor = fixture.make_actor();
    let object = fixture.make_object(&actor, "kairo/object");
    // No branch set.
    let object_id = object.object_id.to_string();
    drop(fixture);

    let (handle, _dir) = spawn_daemon_at(dir).await;
    let client = Client::new(handle.socket_path());

    match client.latest_branch(&object_id, "head", None).await {
        Err(ClientError::Http { status, code, .. }) => {
            assert_eq!(status, 404);
            assert_eq!(code, "not_found");
        }
        other => panic!("expected 404 not_found, got {other:?}"),
    }

    handle.shutdown().await;
}

#[tokio::test]
async fn client_trust_round_trips() {
    use kairo_core::canonical::CanonicalEncode;
    use kairo_statement::{
        ActorTrustBody, Signature, SignedStatement, TrustDecision, UnsignedStatement,
    };
    use kairo_store::StatementStore;

    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();

    // Inline trust signing.
    let body = ActorTrustBody::new(bob.actor_id.clone(), Some(TrustDecision::Trusted), None, None)
        .expect("trust body");
    let subject: kairo_core::KairoRef = format!("actor:{}", bob.actor_id)
        .parse()
        .expect("subject parse");
    let unsigned = UnsignedStatement::new(
        alice.actor_id.clone(),
        subject,
        kairo_core::Timestamp::from_seconds(1_700_000_000),
        body,
    );
    let bytes = alice.signing.sign(&unsigned.canonical_bytes());
    let signature = Signature::new(
        alice.actor_id.clone(),
        alice.signing.public_key().key_id().to_string(),
        "ed25519",
        bytes.bytes().to_vec(),
    );
    let signed = SignedStatement::new(unsigned, signature);
    fixture.store.put_actor_trust(&signed).expect("put_actor_trust");

    let alice_id = alice.actor_id.to_string();
    let bob_id = bob.actor_id.to_string();
    drop(fixture);

    let (handle, _dir) = spawn_daemon_at(dir).await;
    let client = Client::new(handle.socket_path());

    let trust = client.trust(&alice_id, &bob_id).await.expect("trust call");
    let raw = serde_json::to_value(&trust).expect("serialize");
    assert_eq!(raw["body"]["trusted_actor"], bob_id);
    assert_eq!(raw["body"]["decision"], "trusted");

    handle.shutdown().await;
}

#[tokio::test]
async fn client_trust_returns_404_for_missing_opinion() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let alice_id = alice.actor_id.to_string();
    let bob_id = bob.actor_id.to_string();
    drop(fixture);

    let (handle, _dir) = spawn_daemon_at(dir).await;
    let client = Client::new(handle.socket_path());

    match client.trust(&alice_id, &bob_id).await {
        Err(ClientError::Http { status, code, .. }) => {
            assert_eq!(status, 404);
            assert_eq!(code, "not_found");
        }
        other => panic!("expected 404 not_found, got {other:?}"),
    }

    handle.shutdown().await;
}

#[tokio::test]
async fn client_list_capabilities_round_trips() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let alice_id = alice.actor_id.to_string();
    drop(fixture);

    let (handle, _dir) = spawn_daemon_at(dir).await;
    let client = Client::new(handle.socket_path());

    // Empty — alice has no grants. Smoke-test the empty-list path
    // — populating capabilities here would re-do everything in the
    // daemon's own resolved_handlers.rs; the daemon-side test
    // already exercises a populated grantor.
    let heads = client
        .list_capabilities_from(&alice_id)
        .await
        .expect("list");
    assert!(heads.is_empty());

    handle.shutdown().await;
}

// ---------------------------------------------------------------------------
// Slice 7: streaming blob.

#[tokio::test]
async fn client_blob_round_trips_via_async_read() {
    use kairo_core::BlobId;
    use kairo_store::BlobStore;
    use tokio::io::AsyncReadExt;

    const DOMAIN: &[u8] = b"kairo-daemon-client-test/blob";
    const SIZE: usize = 2 * 1024 * 1024;
    let mut payload = Vec::with_capacity(SIZE);
    let mut state: u32 = 0xDEAD_BEEF;
    while payload.len() < SIZE {
        state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
        payload.extend_from_slice(&state.to_le_bytes());
    }
    payload.truncate(SIZE);

    let (dir, fixture) = StoreFixture::temp();
    let blob_id = BlobId::from_bytes(DOMAIN, &payload);
    fixture.store.put_blob(&blob_id, &payload).expect("put_blob");
    let id_str = blob_id.to_string();
    drop(fixture);

    let (handle, _dir) = spawn_daemon_at(dir).await;
    let client = Client::new(handle.socket_path());

    let mut reader = client.blob(&id_str).await.expect("blob open");
    let mut received = Vec::with_capacity(SIZE);
    reader
        .read_to_end(&mut received)
        .await
        .expect("read_to_end");

    assert_eq!(received.len(), payload.len());
    assert_eq!(received, payload);

    handle.shutdown().await;
}

#[tokio::test]
async fn client_blob_returns_404_for_missing_id() {
    use kairo_core::BlobId;

    let (dir, _fixture) = StoreFixture::temp();
    drop(_fixture);
    let (handle, _dir) = spawn_daemon_at(dir).await;
    let client = Client::new(handle.socket_path());

    let absent = BlobId::from_sha256_digest([0xFF; 32]).to_string();
    match client.blob(&absent).await {
        Err(ClientError::Http { status, code, .. }) => {
            assert_eq!(status, 404);
            assert_eq!(code, "not_found");
        }
        other => panic!("expected 404 not_found, got {other:?}"),
    }

    handle.shutdown().await;
}

#[tokio::test]
async fn client_blob_returns_400_for_malformed_id() {
    let (dir, _fixture) = StoreFixture::temp();
    drop(_fixture);
    let (handle, _dir) = spawn_daemon_at(dir).await;
    let client = Client::new(handle.socket_path());

    match client.blob("not-an-id").await {
        Err(ClientError::Http { status, code, .. }) => {
            assert_eq!(status, 400);
            assert_eq!(code, "bad_request");
        }
        other => panic!("expected 400 bad_request, got {other:?}"),
    }

    handle.shutdown().await;
}
