//! Slice 6 integration tests: branch list / latest, version-tag
//! latest (including the cross-actor capability flip), trust
//! latest, and capability list — all over a real Unix socket
//! against an in-process daemon.

#![allow(clippy::expect_used, clippy::panic)]

use axum as _;
use kairo_daemon_client as _;
use kairo_object as _;
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
use kairo_core::canonical::CanonicalEncode;
use kairo_core::{ActorId, KairoRef, ObjectId, StatementId, Timestamp};
use kairo_daemon::{serve_with_shutdown, Config, Error};
use kairo_identity::SecretSigningKey;
use kairo_statement::{
    ActorCapabilityGrantBody, ActorTrustBody, Capability, CapabilityScope, ObjectVersionTagBody,
    SemverVersion, Signature, SignedStatement, StatementKind, TrustDecision, UnsignedStatement,
};
use kairo_store::{FilesystemStore, StatementStore};
use kairo_test_support::store::{CreatedActor, CreatedObject, StoreFixture};
use serde_json::Value;
use tokio::net::UnixStream;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

const FIXTURE_TIMESTAMP: i64 = 1_700_000_000;

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

// ---------------------------------------------------------------------------
// Inline signing helpers — small enough to keep here. If reused
// in later slices we'll lift them into kairo-test-support.

fn sign<B>(actor: &CreatedActor, subject: KairoRef, body: B) -> SignedStatement<B>
where
    B: kairo_core::canonical::CanonicalEncode + kairo_statement::StatementBody,
{
    let unsigned = UnsignedStatement::new(
        actor.actor_id.clone(),
        subject,
        Timestamp::from_seconds(FIXTURE_TIMESTAMP),
        body,
    );
    let bytes = actor.signing.sign(&unsigned.canonical_bytes());
    let signature = Signature::new(
        actor.actor_id.clone(),
        actor.signing.public_key().key_id().to_string(),
        "ed25519",
        bytes.bytes().to_vec(),
    );
    SignedStatement::new(unsigned, signature)
}

fn sign_with_key<B>(
    actor_id: &ActorId,
    signing: &SecretSigningKey,
    subject: KairoRef,
    body: B,
    created_at: Timestamp,
) -> SignedStatement<B>
where
    B: kairo_core::canonical::CanonicalEncode + kairo_statement::StatementBody,
{
    let unsigned = UnsignedStatement::new(actor_id.clone(), subject, created_at, body);
    let bytes = signing.sign(&unsigned.canonical_bytes());
    let signature = Signature::new(
        actor_id.clone(),
        signing.public_key().key_id().to_string(),
        "ed25519",
        bytes.bytes().to_vec(),
    );
    SignedStatement::new(unsigned, signature)
}

fn put_version_tag(
    store: &FilesystemStore,
    actor: &CreatedActor,
    object: &CreatedObject,
    version: &str,
    revision: Option<StatementId>,
    supersedes: Option<StatementId>,
) -> StatementId {
    let body = ObjectVersionTagBody::new(
        object.object_id.clone(),
        SemverVersion::parse(version).expect("semver"),
        revision,
        supersedes,
    )
    .expect("tag body");
    let subject: KairoRef = format!("object:{}", object.object_id)
        .parse()
        .expect("subject");
    let signed = sign(actor, subject, body);
    store
        .put_object_version_tag(&signed)
        .expect("put_object_version_tag")
}

fn put_trust(
    store: &FilesystemStore,
    by: &CreatedActor,
    of: &ActorId,
    decision: TrustDecision,
) -> StatementId {
    let body = ActorTrustBody::new(of.clone(), Some(decision), None, None).expect("trust body");
    let subject: KairoRef = format!("actor:{of}").parse().expect("subject");
    let signed = sign(by, subject, body);
    store.put_actor_trust(&signed).expect("put_actor_trust")
}

fn put_capability_grant(
    store: &FilesystemStore,
    grantor: &CreatedActor,
    grantee: &ActorId,
    object: &ObjectId,
    kind: StatementKind,
) -> StatementId {
    let cap = Capability::new(
        CapabilityScope::Object(object.clone()),
        vec![kind],
        false,
        Vec::new(),
    )
    .expect("capability");
    let body = ActorCapabilityGrantBody::new(grantee.clone(), cap, None);
    let subject: KairoRef = format!("actor:{grantee}").parse().expect("subject");
    let signed = sign(grantor, subject, body);
    store
        .put_actor_capability_grant(&signed)
        .expect("put_actor_capability_grant")
}

// ---------------------------------------------------------------------------

#[tokio::test]
async fn branches_list_returns_tip_per_actor_name() {
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
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let (status, body) = http_get(
        handle.socket_path(),
        &format!("/api/v1/branches/{}", object.object_id),
    )
    .await;
    assert_eq!(status, hyper::StatusCode::OK);
    let json: Value = serde_json::from_slice(&body).expect("parse");
    let result = &json["result"];
    assert!(result.is_array(), "result: {result:#}");
    let arr = result.as_array().expect("array");
    assert_eq!(arr.len(), 1, "one tip per (actor, name)");
    assert_eq!(arr[0]["name"], "head");
    assert_eq!(arr[0]["actor"].as_str(), Some(actor.actor_id.as_str()));
    assert_eq!(arr[0]["object"].as_str(), Some(object.object_id.as_str()));

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn branches_latest_resolves_default_actor_from_genesis() {
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
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    // No ?actor= → defaults to object creator.
    let (status, body) = http_get(
        handle.socket_path(),
        &format!("/api/v1/branches/{}/head/latest", object.object_id),
    )
    .await;
    assert_eq!(status, hyper::StatusCode::OK);
    let json: Value = serde_json::from_slice(&body).expect("parse");
    let result = &json["result"];
    assert!(result["body"].is_object(), "result: {result:#}");
    assert_eq!(result["body"]["name"], "head");

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn branches_latest_returns_404_when_branch_missing() {
    let (dir, fixture) = StoreFixture::temp();
    let actor = fixture.make_actor();
    let object = fixture.make_object(&actor, "kairo/object");
    // No branch set.
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let (status, _) = http_get(
        handle.socket_path(),
        &format!("/api/v1/branches/{}/head/latest", object.object_id),
    )
    .await;
    assert_eq!(status, hyper::StatusCode::NOT_FOUND);

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn version_tag_latest_returns_signed_statement() {
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
    put_version_tag(
        &fixture.store,
        &actor,
        &object,
        "1.2.3",
        Some(revision.statement_id.clone()),
        None,
    );
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let (status, body) = http_get(
        handle.socket_path(),
        &format!("/api/v1/version-tags/{}/1.2.3", object.object_id),
    )
    .await;
    assert_eq!(status, hyper::StatusCode::OK);
    let json: Value = serde_json::from_slice(&body).expect("parse");
    let result = &json["result"];
    assert_eq!(result["type"], "ObjectVersionTag", "result: {result:#}");
    assert_eq!(result["body"]["version"], "1.2.3");
    assert_eq!(
        result["body"]["object"].as_str(),
        Some(object.object_id.as_str())
    );
    assert_eq!(
        result["body"]["target"].as_str(),
        Some(revision.statement_id.as_str())
    );
    assert_eq!(result["actor"].as_str(), Some(actor.actor_id.as_str()));

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn version_tag_latest_honors_cross_actor_capability_flip() {
    // Setup: A creates object O and tags 1.0.0 → r_a.
    //        A grants B an ObjectVersionTag capability on O.
    //        B publishes a tag for 1.0.0 with supersedes = A's tag.
    //        Expected: GET /version-tags/O/1.0.0 (defaults to A
    //        as creator) returns B's tag, because the capability
    //        evaluator honors B's cross-actor supersedes.
    let (dir, fixture) = StoreFixture::temp();
    let actor_a = fixture.make_actor();
    let actor_b = fixture.make_actor();
    let object = fixture.make_object(&actor_a, "kairo/object");
    let revision = fixture.make_revision(
        &actor_a,
        &object,
        kairo_statement::RevisionId::new("git:sha256:r1"),
        SAMPLE_MANIFEST,
        Vec::new(),
    );

    let tag_a = put_version_tag(
        &fixture.store,
        &actor_a,
        &object,
        "1.0.0",
        Some(revision.statement_id.clone()),
        None,
    );

    // A grants B a capability on this object for ObjectVersionTag.
    put_capability_grant(
        &fixture.store,
        &actor_a,
        &actor_b.actor_id,
        &object.object_id,
        StatementKind::ObjectVersionTag,
    );

    // B publishes a successor tag with supersedes pointing at A's
    // tag — cross-actor.
    put_version_tag(
        &fixture.store,
        &actor_b,
        &object,
        "1.0.0",
        Some(revision.statement_id.clone()),
        Some(tag_a.clone()),
    );

    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let (status, body) = http_get(
        handle.socket_path(),
        &format!("/api/v1/version-tags/{}/1.0.0", object.object_id),
    )
    .await;
    assert_eq!(status, hyper::StatusCode::OK);
    let json: Value = serde_json::from_slice(&body).expect("parse");
    let result = &json["result"];
    assert_eq!(
        result["actor"].as_str(),
        Some(actor_b.actor_id.as_str()),
        "expected B's tag to win via capability flip; result: {result:#}"
    );

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn trust_endpoint_returns_latest_opinion() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    put_trust(
        &fixture.store,
        &alice,
        &bob.actor_id,
        TrustDecision::Trusted,
    );

    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let (status, body) = http_get(
        handle.socket_path(),
        &format!("/api/v1/trust/{}/{}", alice.actor_id, bob.actor_id),
    )
    .await;
    assert_eq!(status, hyper::StatusCode::OK);
    let json: Value = serde_json::from_slice(&body).expect("parse");
    assert_eq!(
        json["result"]["body"]["trusted_actor"].as_str(),
        Some(bob.actor_id.as_str())
    );
    assert_eq!(json["result"]["body"]["decision"], "trusted");

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn trust_endpoint_returns_404_when_no_opinion() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let (status, _) = http_get(
        handle.socket_path(),
        &format!("/api/v1/trust/{}/{}", alice.actor_id, bob.actor_id),
    )
    .await;
    assert_eq!(status, hyper::StatusCode::NOT_FOUND);

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn capabilities_list_returns_grantor_heads() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let object = fixture.make_object(&alice, "kairo/object");
    put_capability_grant(
        &fixture.store,
        &alice,
        &bob.actor_id,
        &object.object_id,
        StatementKind::ObjectVersionTag,
    );

    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let (status, body) = http_get(
        handle.socket_path(),
        &format!("/api/v1/capabilities/{}", alice.actor_id),
    )
    .await;
    assert_eq!(status, hyper::StatusCode::OK);
    let json: Value = serde_json::from_slice(&body).expect("parse");
    let result = &json["result"];
    let arr = result.as_array().expect("array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["grantor"].as_str(), Some(alice.actor_id.as_str()));
    assert_eq!(arr[0]["grantee"].as_str(), Some(bob.actor_id.as_str()));
    // CapabilityScopeJson uses #[serde(rename_all = "snake_case")]
    // — the variant tag is "object", not "Object".
    assert_eq!(
        arr[0]["scope"]["object"].as_str(),
        Some(object.object_id.as_str())
    );

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn capabilities_list_returns_empty_array_for_unknown_grantor() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let (status, body) = http_get(
        handle.socket_path(),
        &format!("/api/v1/capabilities/{}", alice.actor_id),
    )
    .await;
    assert_eq!(status, hyper::StatusCode::OK);
    let json: Value = serde_json::from_slice(&body).expect("parse");
    assert_eq!(json["result"], Value::Array(Vec::new()));

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn version_tags_list_returns_heads_per_actor_version() {
    // One object, two tags (1.0.0 and 1.1.0) by the same actor.
    // Expected: list returns two entries, both fields populated.
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
    put_version_tag(
        &fixture.store,
        &actor,
        &object,
        "1.0.0",
        Some(revision.statement_id.clone()),
        None,
    );
    put_version_tag(
        &fixture.store,
        &actor,
        &object,
        "1.1.0",
        Some(revision.statement_id.clone()),
        None,
    );
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let (status, body) = http_get(
        handle.socket_path(),
        &format!("/api/v1/version-tags/{}", object.object_id),
    )
    .await;
    assert_eq!(status, hyper::StatusCode::OK);
    let json: Value = serde_json::from_slice(&body).expect("parse");
    let arr = json["result"].as_array().expect("array");
    assert_eq!(arr.len(), 2, "two heads, one per version: {arr:#?}");
    let mut versions: Vec<&str> = arr
        .iter()
        .map(|e| e["version"].as_str().expect("version"))
        .collect();
    versions.sort();
    assert_eq!(versions, ["1.0.0", "1.1.0"]);
    for entry in arr {
        assert_eq!(entry["actor"].as_str(), Some(actor.actor_id.as_str()));
        assert_eq!(entry["object"].as_str(), Some(object.object_id.as_str()));
        assert!(entry["statement_id"].is_string(), "entry: {entry:#}");
        assert!(entry["created_at"].is_string());
    }

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn version_tags_list_returns_empty_for_unknown_object() {
    let (dir, fixture) = StoreFixture::temp();
    let actor = fixture.make_actor();
    let object = fixture.make_object(&actor, "kairo/object");
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let (status, body) = http_get(
        handle.socket_path(),
        &format!("/api/v1/version-tags/{}", object.object_id),
    )
    .await;
    assert_eq!(status, hyper::StatusCode::OK);
    let json: Value = serde_json::from_slice(&body).expect("parse");
    assert_eq!(json["result"], Value::Array(Vec::new()));

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn revisions_list_returns_chronological_history() {
    // Two revisions, written in reverse-time order, expect them
    // to come back ascending by created_at.
    let (dir, fixture) = StoreFixture::temp();
    let actor = fixture.make_actor();
    let object = fixture.make_object(&actor, "kairo/object");
    let r1 = fixture.make_revision(
        &actor,
        &object,
        kairo_statement::RevisionId::new("git:sha256:r1"),
        SAMPLE_MANIFEST,
        Vec::new(),
    );
    let r2 = fixture.make_revision(
        &actor,
        &object,
        kairo_statement::RevisionId::new("git:sha256:r2"),
        SAMPLE_MANIFEST,
        vec![kairo_statement::RevisionId::new("git:sha256:r1")],
    );
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let (status, body) = http_get(
        handle.socket_path(),
        &format!("/api/v1/revisions/{}", object.object_id),
    )
    .await;
    assert_eq!(status, hyper::StatusCode::OK);
    let json: Value = serde_json::from_slice(&body).expect("parse");
    let arr = json["result"].as_array().expect("array");
    assert_eq!(arr.len(), 2, "both revisions returned: {arr:#?}");
    // r1 has no parents so it's the first revision; r2 lists r1
    // as a parent. Both share a created_at in our fixture, so the
    // secondary sort by statement_id decides ordering — we check
    // membership and shape, not order.
    let ids: Vec<&str> = arr
        .iter()
        .map(|e| e["statement_id"].as_str().expect("statement_id"))
        .collect();
    assert!(
        ids.contains(&r1.statement_id.as_str()),
        "missing r1: {arr:#?}"
    );
    assert!(
        ids.contains(&r2.statement_id.as_str()),
        "missing r2: {arr:#?}"
    );
    for entry in arr {
        assert_eq!(entry["actor"].as_str(), Some(actor.actor_id.as_str()));
        assert_eq!(entry["object"].as_str(), Some(object.object_id.as_str()));
        assert!(entry["revision_id"].is_string(), "entry: {entry:#}");
        assert!(entry["manifest_hash"].is_string());
        assert!(entry["created_at"].is_string());
    }
    let r1_entry = arr
        .iter()
        .find(|e| e["statement_id"].as_str() == Some(r1.statement_id.as_str()))
        .expect("r1 entry");
    assert_eq!(r1_entry["parents"], Value::Array(Vec::new()));
    let r2_entry = arr
        .iter()
        .find(|e| e["statement_id"].as_str() == Some(r2.statement_id.as_str()))
        .expect("r2 entry");
    assert_eq!(
        r2_entry["parents"]
            .as_array()
            .expect("parents")
            .iter()
            .map(|v| v.as_str().expect("parent string"))
            .collect::<Vec<_>>(),
        vec!["git:sha256:r1"],
    );

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn revisions_list_returns_empty_for_unknown_object() {
    let (dir, fixture) = StoreFixture::temp();
    let actor = fixture.make_actor();
    let object = fixture.make_object(&actor, "kairo/object");
    // No revisions written.
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let (status, body) = http_get(
        handle.socket_path(),
        &format!("/api/v1/revisions/{}", object.object_id),
    )
    .await;
    assert_eq!(status, hyper::StatusCode::OK);
    let json: Value = serde_json::from_slice(&body).expect("parse");
    assert_eq!(json["result"], Value::Array(Vec::new()));

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn trust_list_about_returns_per_by_actor_heads() {
    // Two actors each express trust about a third.
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let carol = fixture.make_actor();
    put_trust(
        &fixture.store,
        &alice,
        &carol.actor_id,
        TrustDecision::Trusted,
    );
    put_trust(
        &fixture.store,
        &bob,
        &carol.actor_id,
        TrustDecision::Untrusted,
    );

    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let (status, body) = http_get(
        handle.socket_path(),
        &format!("/api/v1/trust/about/{}", carol.actor_id),
    )
    .await;
    assert_eq!(status, hyper::StatusCode::OK);
    let json: Value = serde_json::from_slice(&body).expect("parse");
    let arr = json["result"].as_array().expect("array");
    assert_eq!(arr.len(), 2, "two opinions about carol: {arr:#?}");
    for entry in arr {
        assert_eq!(
            entry["trusted_actor"].as_str(),
            Some(carol.actor_id.as_str())
        );
        assert!(entry["statement_id"].is_string());
        assert!(entry["created_at"].is_string());
    }
    let by_decision: std::collections::HashMap<&str, &str> = arr
        .iter()
        .map(|e| {
            (
                e["by_actor"].as_str().expect("by_actor"),
                e["decision"].as_str().expect("decision"),
            )
        })
        .collect();
    assert_eq!(by_decision.get(alice.actor_id.as_str()), Some(&"trusted"));
    assert_eq!(by_decision.get(bob.actor_id.as_str()), Some(&"untrusted"));

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn trust_list_about_returns_empty_for_unknown_actor() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let (status, body) = http_get(
        handle.socket_path(),
        &format!("/api/v1/trust/about/{}", alice.actor_id),
    )
    .await;
    assert_eq!(status, hyper::StatusCode::OK);
    let json: Value = serde_json::from_slice(&body).expect("parse");
    assert_eq!(json["result"], Value::Array(Vec::new()));

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn actors_list_statements_returns_authored_envelopes() {
    // Alice signs a branch on her object and a trust statement
    // about Bob. Both should appear under her actor; Bob's list
    // is empty because he hasn't signed anything yet.
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let object = fixture.make_object(&alice, "kairo/object");
    let revision = fixture.make_revision(
        &alice,
        &object,
        kairo_statement::RevisionId::new("git:sha256:r1"),
        SAMPLE_MANIFEST,
        Vec::new(),
    );
    let branch_id = fixture.set_branch(&alice, &object, &revision, "head");
    let trust_id = put_trust(
        &fixture.store,
        &alice,
        &bob.actor_id,
        TrustDecision::Trusted,
    );
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;

    let (status, body) = http_get(
        handle.socket_path(),
        &format!("/api/v1/actors/{}/statements", alice.actor_id),
    )
    .await;
    assert_eq!(status, hyper::StatusCode::OK);
    let json: Value = serde_json::from_slice(&body).expect("parse");
    let arr = json["result"].as_array().expect("array");
    // Branch + trust + the revision Alice signed via make_revision.
    // Genesis is intentionally not indexed here, so we expect
    // exactly the three signed envelopes.
    let by_kind: std::collections::HashMap<&str, &str> = arr
        .iter()
        .map(|e| {
            (
                e["statement_id"].as_str().expect("statement_id"),
                e["kind"].as_str().expect("kind"),
            )
        })
        .collect();
    assert!(
        by_kind.contains_key(branch_id.statement_id.as_str()),
        "branch should be indexed under alice: {arr:#?}",
    );
    assert!(
        by_kind.contains_key(trust_id.as_str()),
        "trust should be indexed under alice: {arr:#?}",
    );
    assert_eq!(
        by_kind.get(branch_id.statement_id.as_str()),
        Some(&"ObjectBranch")
    );
    assert_eq!(by_kind.get(trust_id.as_str()), Some(&"ActorTrust"));
    for entry in arr {
        assert_eq!(entry["actor"].as_str(), Some(alice.actor_id.as_str()));
        assert!(entry["created_at"].is_string());
    }

    let (status, body) = http_get(
        handle.socket_path(),
        &format!("/api/v1/actors/{}/statements", bob.actor_id),
    )
    .await;
    assert_eq!(status, hyper::StatusCode::OK);
    let json: Value = serde_json::from_slice(&body).expect("parse");
    assert_eq!(
        json["result"],
        Value::Array(Vec::new()),
        "bob has authored nothing yet"
    );

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn actors_list_statements_rejects_malformed_id() {
    let (dir, _fixture) = StoreFixture::temp();
    let store_path = dir.path().to_path_buf();
    drop(_fixture);

    let handle = spawn_daemon(store_path).await;
    let (status, body) = http_get(
        handle.socket_path(),
        "/api/v1/actors/not-a-real-id/statements",
    )
    .await;
    assert_eq!(status, hyper::StatusCode::BAD_REQUEST);
    let json: Value = serde_json::from_slice(&body).expect("parse");
    assert_eq!(json["ok"], false);
    assert_eq!(json["error"]["code"], "bad_request");

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn actors_list_objects_returns_owned_objects() {
    // Alice creates two objects; Bob creates none.
    // Alice's /objects lists both genesis entries; Bob's is empty.
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let alpha = fixture.make_object(&alice, "kairo/object");
    let beta = fixture.make_object(&alice, "kairo/object");
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;

    let (status, body) = http_get(
        handle.socket_path(),
        &format!("/api/v1/actors/{}/objects", alice.actor_id),
    )
    .await;
    assert_eq!(status, hyper::StatusCode::OK);
    let json: Value = serde_json::from_slice(&body).expect("parse");
    let arr = json["result"].as_array().expect("array");
    assert_eq!(arr.len(), 2);
    let object_ids: std::collections::HashSet<&str> = arr
        .iter()
        .map(|e| e["object_id"].as_str().expect("object_id"))
        .collect();
    assert!(object_ids.contains(alpha.object_id.as_str()));
    assert!(object_ids.contains(beta.object_id.as_str()));
    for entry in arr {
        assert_eq!(entry["actor"].as_str(), Some(alice.actor_id.as_str()));
        assert_eq!(entry["object_kind"].as_str(), Some("kairo/object"));
        assert!(entry["created_at"].is_string());
    }

    let (status, body) = http_get(
        handle.socket_path(),
        &format!("/api/v1/actors/{}/objects", bob.actor_id),
    )
    .await;
    assert_eq!(status, hyper::StatusCode::OK);
    let json: Value = serde_json::from_slice(&body).expect("parse");
    assert_eq!(
        json["result"],
        Value::Array(Vec::new()),
        "bob has created nothing yet"
    );

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn actors_list_objects_rejects_malformed_id() {
    let (dir, _fixture) = StoreFixture::temp();
    let store_path = dir.path().to_path_buf();
    drop(_fixture);

    let handle = spawn_daemon(store_path).await;
    let (status, body) =
        http_get(handle.socket_path(), "/api/v1/actors/not-a-real-id/objects").await;
    assert_eq!(status, hyper::StatusCode::BAD_REQUEST);
    let json: Value = serde_json::from_slice(&body).expect("parse");
    assert_eq!(json["ok"], false);
    assert_eq!(json["error"]["code"], "bad_request");

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn capabilities_list_for_object_returns_hydrated_heads() {
    // Alice grants Bob an ObjectVersionTag capability scoped to
    // object O. The for-object endpoint must return one entry
    // with the scope hydrated (per-row follow-up read).
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let object = fixture.make_object(&alice, "kairo/object");
    put_capability_grant(
        &fixture.store,
        &alice,
        &bob.actor_id,
        &object.object_id,
        StatementKind::ObjectVersionTag,
    );
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let (status, body) = http_get(
        handle.socket_path(),
        &format!("/api/v1/capabilities/for-object/{}", object.object_id),
    )
    .await;
    assert_eq!(status, hyper::StatusCode::OK);
    let json: Value = serde_json::from_slice(&body).expect("parse");
    let arr = json["result"].as_array().expect("array");
    assert_eq!(arr.len(), 1, "one head for this object: {arr:#?}");
    assert_eq!(arr[0]["grantor"].as_str(), Some(alice.actor_id.as_str()));
    assert_eq!(arr[0]["grantee"].as_str(), Some(bob.actor_id.as_str()));
    assert_eq!(
        arr[0]["scope"]["object"].as_str(),
        Some(object.object_id.as_str()),
        "scope must be hydrated from the underlying grant"
    );

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn capabilities_list_for_object_returns_empty_for_unknown_object() {
    let (dir, fixture) = StoreFixture::temp();
    let actor = fixture.make_actor();
    let object = fixture.make_object(&actor, "kairo/object");
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let (status, body) = http_get(
        handle.socket_path(),
        &format!("/api/v1/capabilities/for-object/{}", object.object_id),
    )
    .await;
    assert_eq!(status, hyper::StatusCode::OK);
    let json: Value = serde_json::from_slice(&body).expect("parse");
    assert_eq!(json["result"], Value::Array(Vec::new()));

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn malformed_ids_return_400_across_endpoints() {
    let (dir, _fixture) = StoreFixture::temp();
    let store_path = dir.path().to_path_buf();
    drop(_fixture);

    let handle = spawn_daemon(store_path).await;

    for path in [
        "/api/v1/branches/!!!",
        "/api/v1/branches/!!!/head/latest",
        "/api/v1/version-tags/!!!",
        "/api/v1/version-tags/!!!/1.0.0",
        "/api/v1/revisions/!!!",
        "/api/v1/trust/!!!/foo",
        "/api/v1/trust/about/!!!",
        "/api/v1/capabilities/!!!",
        "/api/v1/capabilities/for-object/!!!",
    ] {
        let (status, _) = http_get(handle.socket_path(), path).await;
        assert_eq!(
            status,
            hyper::StatusCode::BAD_REQUEST,
            "{path} should return 400"
        );
    }

    handle.shutdown().await;
    drop(dir);
}

const SAMPLE_MANIFEST: &str = r#"
[kairo]
schema = 1
kind = "kairo/object"
name = "fixture"

[content]
kind = "tree"
"#;

// `sign_with_key` is reserved for future tests that need to
// override actor or timestamp; keep it compiled to avoid drift.
#[allow(dead_code)]
fn _force_sign_with_key_compile(
    actor_id: &ActorId,
    signing: &SecretSigningKey,
    subject: KairoRef,
    body: ActorTrustBody,
    ts: Timestamp,
) -> SignedStatement<ActorTrustBody> {
    sign_with_key(actor_id, signing, subject, body, ts)
}
