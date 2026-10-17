//! Slice 2 (daemon writes) integration tests for `POST
//! /api/v1/statements`. Exercises Phase A's only opened-up
//! statement kind (`ActorTrust`) end-to-end over a real Unix
//! socket against an in-process daemon backed by a
//! `StoreFixture`-built `FilesystemStore`.
//!
//! Cases covered (see `PHASE_2_DAEMON_WRITES.md` Slice 2 exit
//! criteria):
//!
//! - Signature-valid trust grant → 200 + `PutStatementResult` +
//!   the row actually persisted in the store.
//! - Signature-invalid body → 422 `bad_signature`; nothing
//!   persisted.
//! - Envelope actor unknown to the store → 422 `actor_not_found`.
//! - Envelope/signature actor disagree → 422
//!   `signature_actor_mismatch`.
//! - Body's `type` discriminator is not yet opened up (e.g.,
//!   `ObjectRevision`) → 415 `unsupported_statement_kind`.
//! - Body missing the `type` field → 400 `bad_request`.
//! - Byte-identical re-delivery → 200 with the same
//!   `statement_id`; per `PHASE_2_DAEMON_WRITES.md §2.3`, the
//!   write is idempotent against retried deliveries of the same
//!   signed envelope.
//!
//! Skipped intentionally (documented in the slice plan):
//!
//! - `supersedes_not_found`: the current `FilesystemStore` does
//!   not enforce that `supersedes` references an existing
//!   statement at write time, so the daemon has no failure mode
//!   to surface here today. Pre-existing behavior in both direct
//!   and daemon modes; revisit if/when the store starts
//!   enforcing.
//! - `already_exists`: byte-different bodies cannot collide on
//!   the same `statement_id` because the id is the canonical
//!   hash of the bytes. Same-id implies same-bytes (the case
//!   covered by the idempotency test above). The 409 mapping
//!   exists in the spec for completeness; no realistic path
//!   reaches it without breaking content-addressing.

#![allow(clippy::expect_used, clippy::panic)]

use axum as _;
use base64 as _;
use getrandom as _;
use hyper as _;
use kairo_daemon_client::auth::{random_nonce, sign_request, TIMESTAMP_WINDOW_SECONDS};
use kairo_identity as _;
use kairo_keystore as _;
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

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::Request;
use hyper_util::rt::TokioIo;
use kairo_core::canonical::CanonicalEncode;
use kairo_core::{KairoRef, Timestamp};
use kairo_daemon::{serve_with_shutdown, Config, Error};
use kairo_object::ObjectManifest;
use kairo_statement::json::{
    ActorCapabilityGrantStatementJson, ActorCapabilityRevocationStatementJson,
    ActorTrustStatementJson, ObjectBranchStatementJson, ObjectGenesisStatementJson,
    ObjectRevisionStatementJson, ObjectVersionTagStatementJson,
};
use kairo_statement::{
    ActorCapabilityGrantBody, ActorCapabilityRevocationBody, ActorTrustBody, Capability,
    CapabilityConstraint, CapabilityScope, ObjectBranchBody, ObjectGenesisBody,
    ObjectGenesisStatement, ObjectKind, ObjectRevisionBody, ObjectVersionTagBody, RevisionId,
    SemverVersion, Signature, SignedStatement, StatementKind, TrustDecision, UnsignedStatement,
};
use kairo_store::{BlobStore, BranchResolver, TrustResolver, VersionTagResolver};
use kairo_test_support::store::{CreatedActor, CreatedObject, CreatedRevision, StoreFixture};
use serde_json::{json, Value};
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
            keys_path: None,
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

/// POST a JSON body, optionally with a precomputed Authorization
/// header value. `auth = None` is used by the missing-auth-header
/// test; tests that want to inject a specific (malformed,
/// expired, etc.) auth value pass `Some(value)`. Tests that just
/// want a valid signed request should call [`signed_post_json`].
async fn http_post_json(
    socket: &Path,
    path: &str,
    body: &Value,
    auth: Option<&str>,
) -> (hyper::StatusCode, Bytes) {
    let body_bytes = serde_json::to_vec(body).expect("serialize body");
    http_post_raw(socket, path, body_bytes, auth).await
}

async fn http_post_raw(
    socket: &Path,
    path: &str,
    body_bytes: Vec<u8>,
    auth: Option<&str>,
) -> (hyper::StatusCode, Bytes) {
    let stream = UnixStream::connect(socket).await.expect("connect");
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
        .await
        .expect("handshake");
    let conn_task = tokio::spawn(async move {
        let _ = conn.await;
    });

    let mut req_builder = Request::builder()
        .method("POST")
        .uri(path)
        .header("host", "daemon")
        .header("content-type", "application/json");
    if let Some(auth_value) = auth {
        req_builder = req_builder.header("authorization", auth_value);
    }
    let req = req_builder
        .body(Full::new(Bytes::from(body_bytes)))
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

/// POST a JSON body signed by `signer`. The request's
/// `Authorization` envelope's actor is `signer.actor_id`; the
/// signature is produced by `signer.signing`. Use this for any
/// test that wants the Phase B authn pipeline to pass — happy
/// path or any *body-level* failure (`bad_signature`,
/// `signature_actor_mismatch`, etc.) that needs to reach the
/// dispatcher.
async fn signed_post_json(
    socket: &Path,
    path: &str,
    body: &Value,
    signer: &CreatedActor,
) -> (hyper::StatusCode, Bytes) {
    let body_bytes = serde_json::to_vec(body).expect("serialize body");
    let nonce = random_nonce().expect("nonce");
    let timestamp = Timestamp::now().seconds();
    let envelope = sign_request(
        "POST",
        path,
        &body_bytes,
        signer.actor_id.clone(),
        &signer.signing,
        timestamp,
        nonce,
    );
    let auth = envelope.format();
    http_post_raw(socket, path, body_bytes, Some(&auth)).await
}

/// Recursive filename search under `root`. Returns the first
/// match or `None`. Used by the actor-not-found test to delete a
/// genesis file without pinning the store's shard layout.
fn find_in_tree(root: &Path, filename: &str) -> Option<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).ok()?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if entry.file_name() == filename {
                return Some(path);
            }
        }
    }
    None
}

const FIXTURE_MANIFEST: &str = r#"
    [kairo]
    schema = 1
    kind = "kairo/object"
    name = "post-statements-fixture"

    [content]
    kind = "tree"
"#;

/// Build a signature-valid `ObjectRevision` for `object` signed
/// by `actor`. The manifest hash is computed locally; the test
/// is responsible for putting the manifest blob into the store
/// before posting (otherwise `verify_envelope_statement` would
/// still succeed — the store doesn't enforce manifest-blob
/// reachability at write time — but the daemon's followup read
/// path would surface a missing blob).
fn sign_revision(
    actor: &CreatedActor,
    object: &CreatedObject,
    revision_id: RevisionId,
    parents: Vec<RevisionId>,
    manifest_hash: kairo_core::BlobId,
    created_at_seconds: i64,
) -> SignedStatement<ObjectRevisionBody> {
    let body = ObjectRevisionBody::new(
        object.object_id.clone(),
        revision_id,
        parents,
        manifest_hash,
        true,
    );
    let subject: KairoRef = format!("object:{}", object.object_id)
        .parse()
        .expect("subject parse");
    let unsigned = UnsignedStatement::new(
        actor.actor_id.clone(),
        subject,
        Timestamp::from_seconds(created_at_seconds),
        body,
    );
    let signature_bytes = actor.signing.sign(&unsigned.canonical_bytes());
    let signature = Signature::new(
        actor.actor_id.clone(),
        actor.signing.public_key().key_id().to_string(),
        "ed25519",
        signature_bytes.bytes().to_vec(),
    );
    SignedStatement::new(unsigned, signature)
}

/// Build a signature-valid `ObjectBranch` head pointing `name`
/// at `revision.statement_id`, signed by `actor`. `supersedes` is
/// `None` for a genesis branch set; supply a prior head's
/// statement_id to chain.
fn sign_branch(
    actor: &CreatedActor,
    object: &CreatedObject,
    revision: &CreatedRevision,
    name: &str,
    supersedes: Option<kairo_core::StatementId>,
    created_at_seconds: i64,
) -> SignedStatement<ObjectBranchBody> {
    let body = ObjectBranchBody::new(
        object.object_id.clone(),
        name,
        revision.statement_id.clone(),
        supersedes,
    );
    let subject: KairoRef = format!("object:{}", object.object_id)
        .parse()
        .expect("subject parse");
    let unsigned = UnsignedStatement::new(
        actor.actor_id.clone(),
        subject,
        Timestamp::from_seconds(created_at_seconds),
        body,
    );
    let signature_bytes = actor.signing.sign(&unsigned.canonical_bytes());
    let signature = Signature::new(
        actor.actor_id.clone(),
        actor.signing.public_key().key_id().to_string(),
        "ed25519",
        signature_bytes.bytes().to_vec(),
    );
    SignedStatement::new(unsigned, signature)
}

/// Build a signature-valid `ObjectVersionTag` bind pointing
/// `version` at `revision.statement_id`, signed by `actor`.
fn sign_version_tag(
    actor: &CreatedActor,
    object: &CreatedObject,
    revision: &CreatedRevision,
    version: &str,
    supersedes: Option<kairo_core::StatementId>,
    created_at_seconds: i64,
) -> SignedStatement<ObjectVersionTagBody> {
    let semver = SemverVersion::parse(version).expect("semver");
    let body = ObjectVersionTagBody::new(
        object.object_id.clone(),
        semver,
        Some(revision.statement_id.clone()),
        supersedes,
    )
    .expect("tag body shape");
    let subject: KairoRef = format!("object:{}", object.object_id)
        .parse()
        .expect("subject parse");
    let unsigned = UnsignedStatement::new(
        actor.actor_id.clone(),
        subject,
        Timestamp::from_seconds(created_at_seconds),
        body,
    );
    let signature_bytes = actor.signing.sign(&unsigned.canonical_bytes());
    let signature = Signature::new(
        actor.actor_id.clone(),
        actor.signing.public_key().key_id().to_string(),
        "ed25519",
        signature_bytes.bytes().to_vec(),
    );
    SignedStatement::new(unsigned, signature)
}

/// Build a signature-valid `ActorTrust` statement (genesis
/// opinion: no `supersedes`) from `by` about `of`.
fn sign_trust_grant(
    by: &CreatedActor,
    of: &CreatedActor,
    decision: Option<TrustDecision>,
    reason: Option<&str>,
) -> SignedStatement<ActorTrustBody> {
    let body = ActorTrustBody::new(
        of.actor_id.clone(),
        decision,
        reason.map(str::to_owned),
        None,
    )
    .expect("trust body shape");
    let subject: KairoRef = format!("actor:{}", of.actor_id)
        .parse()
        .expect("subject ref");
    let unsigned = UnsignedStatement::new(
        by.actor_id.clone(),
        subject,
        Timestamp::from_seconds(1_700_000_100),
        body,
    );
    let signature_bytes = by.signing.sign(&unsigned.canonical_bytes());
    let signature = Signature::new(
        by.actor_id.clone(),
        by.signing.public_key().key_id().to_string(),
        "ed25519",
        signature_bytes.bytes().to_vec(),
    );
    SignedStatement::new(unsigned, signature)
}

#[tokio::test]
async fn accepts_signature_valid_trust_grant_and_persists_it() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path.clone()).await;

    let signed = sign_trust_grant(&alice, &bob, Some(TrustDecision::Trusted), Some("ci-smoke"));
    let dto = ActorTrustStatementJson::from_statement(&signed);
    let body = serde_json::to_value(&dto).expect("serialize dto");

    let (status, response_body) =
        signed_post_json(handle.socket_path(), "/api/v1/statements", &body, &alice).await;
    assert_eq!(status, hyper::StatusCode::OK, "body: {response_body:?}");

    let envelope: Value = serde_json::from_slice(&response_body).expect("parse envelope");
    assert_eq!(envelope["ok"], Value::Bool(true), "envelope: {envelope:#}");
    let result = &envelope["result"];
    assert_eq!(
        result["statement_id"].as_str(),
        Some(signed.statement_id().to_string().as_str())
    );
    assert_eq!(result["statement"]["type"], "ActorTrust");

    // The post-condition is *persistence*: re-open the store and
    // verify the latest_trust head matches the statement_id we
    // just POSTed. Open a fresh handle because the daemon owns
    // the in-process one.
    handle.shutdown().await;
    let store_reread = kairo_store::FilesystemStore::open(&store_path).expect("re-open store");
    let head = store_reread
        .latest_trust(&alice.actor_id, &bob.actor_id)
        .expect("read latest trust")
        .expect("trust head present");
    assert_eq!(head.statement_id(), signed.statement_id());

    drop(dir);
}

#[tokio::test]
async fn rejects_signature_invalid_body_with_422_bad_signature() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path.clone()).await;

    // Sign correctly, then rebuild the envelope with one bit of
    // the signature flipped — the canonical bytes still verify
    // alice's identity, but the signature no longer matches them.
    // Forces verify_envelope_statement onto the `Invalid` path.
    let signed = sign_trust_grant(&alice, &bob, Some(TrustDecision::Trusted), None);
    let mut bad_bytes = signed.signature().bytes().to_vec();
    bad_bytes[0] ^= 0x01;
    let bad_signature = Signature::new(
        alice.actor_id.clone(),
        alice.signing.public_key().key_id().to_string(),
        "ed25519",
        bad_bytes,
    );
    let bad_signed = SignedStatement::new(signed.unsigned().clone(), bad_signature);
    let body = serde_json::to_value(ActorTrustStatementJson::from_statement(&bad_signed))
        .expect("serialize");
    let (status, response_body) =
        signed_post_json(handle.socket_path(), "/api/v1/statements", &body, &alice).await;
    assert_eq!(
        status,
        hyper::StatusCode::UNPROCESSABLE_ENTITY,
        "body: {response_body:?}"
    );
    let envelope: Value = serde_json::from_slice(&response_body).expect("parse envelope");
    assert_eq!(envelope["ok"], Value::Bool(false));
    assert_eq!(envelope["error"]["code"], "bad_signature");

    // Side effect: head still absent — the daemon refused the
    // write before reaching the store.
    handle.shutdown().await;
    let store_reread = kairo_store::FilesystemStore::open(&store_path).expect("re-open store");
    assert!(
        store_reread
            .latest_trust(&alice.actor_id, &bob.actor_id)
            .expect("read")
            .is_none(),
        "no trust head should exist after a rejected write",
    );
    drop(dir);
}

#[tokio::test]
async fn rejects_actor_not_found_for_request_with_401() {
    // Phase B (DECISIONS.md §14): the request's `actor`
    // parameter is resolved against the store before the
    // dispatcher sees the body. If the actor's genesis is
    // missing, the request fails at the authentication layer
    // with 401 `actor_not_found_for_request` — long before any
    // body-level `actor_not_found` (422) could fire.
    //
    // The CLI signs with the same key it signs the statement,
    // so request-actor and body-actor are always equal in
    // practice. This test exercises the layer that catches
    // that case.
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let store_path = dir.path().to_path_buf();
    drop(fixture);
    let alice_filename = format!("{}.json", alice.actor_id);
    let alice_path = find_in_tree(&store_path.join("actors"), &alice_filename)
        .expect("alice's genesis must exist on disk before this test");
    std::fs::remove_file(&alice_path).expect("remove alice's genesis");

    let handle = spawn_daemon(store_path.clone()).await;
    let signed = sign_trust_grant(&alice, &bob, Some(TrustDecision::Trusted), None);
    let body =
        serde_json::to_value(ActorTrustStatementJson::from_statement(&signed)).expect("serialize");

    let (status, response_body) =
        signed_post_json(handle.socket_path(), "/api/v1/statements", &body, &alice).await;
    assert_eq!(
        status,
        hyper::StatusCode::UNAUTHORIZED,
        "body: {response_body:?}"
    );
    let envelope: Value = serde_json::from_slice(&response_body).expect("parse envelope");
    assert_eq!(envelope["error"]["code"], "actor_not_found_for_request");

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn rejects_envelope_signature_actor_mismatch() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path.clone()).await;

    // Build alice-signs-about-bob, then rewrite the JSON's
    // `signature.actor` to bob — the envelope actor (alice) and
    // signature actor (bob) now disagree.
    let signed = sign_trust_grant(&alice, &bob, Some(TrustDecision::Trusted), None);
    let mut dto = ActorTrustStatementJson::from_statement(&signed);
    dto.signature.actor = bob.actor_id.to_string();
    let body = serde_json::to_value(&dto).expect("serialize");

    let (status, response_body) =
        signed_post_json(handle.socket_path(), "/api/v1/statements", &body, &alice).await;
    assert_eq!(
        status,
        hyper::StatusCode::UNPROCESSABLE_ENTITY,
        "body: {response_body:?}"
    );
    let envelope: Value = serde_json::from_slice(&response_body).expect("parse envelope");
    assert_eq!(envelope["error"]["code"], "signature_actor_mismatch");

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn rejects_unsupported_statement_kind_with_415() {
    // POST a body whose `type` is not yet in Phase A's
    // dispatcher allowlist. The handler short-circuits with 415
    // before touching the store or trying to decode the body
    // into a typed shape. Phase B's authn pipeline runs first,
    // so we need a real actor in the store to sign the request
    // — the 415 only fires once auth has passed.
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let body = json!({
        // No `type` discriminator value is on Phase A's allowlist
        // here — Slice 4 closed the original set (ActorTrust,
        // ObjectRevision/Branch/VersionTag, ObjectGenesis,
        // ActorCapabilityGrant/Revocation). Future kinds (e.g.,
        // attestation key surfaces, ActorKeyRotation) are still
        // 415 until a later slice opens them.
        "type": "ActorKeyRotation",
        "version": 1,
        // Other fields intentionally absent — the handler must
        // reject on `type` alone, before attempting body decode.
    });

    let (status, response_body) =
        signed_post_json(handle.socket_path(), "/api/v1/statements", &body, &alice).await;
    assert_eq!(
        status,
        hyper::StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "body: {response_body:?}"
    );
    let envelope: Value = serde_json::from_slice(&response_body).expect("parse envelope");
    assert_eq!(envelope["error"]["code"], "unsupported_statement_kind");
    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn rejects_body_missing_type_discriminator() {
    // Phase B's authn pipeline runs before the body shape is
    // inspected, so we need a real actor to get a 400 on the
    // body rather than a 401 on the auth header.
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let body = json!({ "version": 1, "actor": "kairo:actor:zNothing" });
    let (status, response_body) =
        signed_post_json(handle.socket_path(), "/api/v1/statements", &body, &alice).await;
    assert_eq!(
        status,
        hyper::StatusCode::BAD_REQUEST,
        "body: {response_body:?}"
    );
    let envelope: Value = serde_json::from_slice(&response_body).expect("parse envelope");
    assert_eq!(envelope["error"]["code"], "bad_request");
    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn byte_identical_redelivery_is_idempotent() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path.clone()).await;
    let signed = sign_trust_grant(&alice, &bob, Some(TrustDecision::Trusted), Some("retry"));
    let body =
        serde_json::to_value(ActorTrustStatementJson::from_statement(&signed)).expect("serialize");

    // POST once.
    let (status1, body1) =
        signed_post_json(handle.socket_path(), "/api/v1/statements", &body, &alice).await;
    assert_eq!(status1, hyper::StatusCode::OK);
    let env1: Value = serde_json::from_slice(&body1).expect("parse");
    let id1 = env1["result"]["statement_id"]
        .as_str()
        .expect("id1")
        .to_owned();

    // POST the same envelope a second time — same canonical
    // bytes, same statement_id, store overwrites with byte-
    // identical content. Per PHASE_2_DAEMON_WRITES.md §2.3, this
    // is idempotent success.
    let (status2, body2) =
        signed_post_json(handle.socket_path(), "/api/v1/statements", &body, &alice).await;
    assert_eq!(status2, hyper::StatusCode::OK, "body: {body2:?}");
    let env2: Value = serde_json::from_slice(&body2).expect("parse");
    let id2 = env2["result"]["statement_id"]
        .as_str()
        .expect("id2")
        .to_owned();

    assert_eq!(
        id1, id2,
        "byte-identical re-delivery must yield the same id"
    );

    handle.shutdown().await;
    drop(dir);
}

// --- Slice 3: ObjectRevision -------------------------------------------------

#[tokio::test]
async fn accepts_object_revision_and_persists_it() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let object = fixture.make_object(&alice, "kairo/object");

    // Manifest blob lives in the store before the revision so
    // the daemon's followup read path can resolve it. Mirrors
    // what `kairo revision create` does direct.
    let manifest = ObjectManifest::parse_toml(FIXTURE_MANIFEST).expect("manifest");
    let manifest_hash = manifest.manifest_hash();
    fixture
        .store
        .put_blob(&manifest_hash, &manifest.canonical_bytes())
        .expect("put_blob");
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path.clone()).await;
    let signed = sign_revision(
        &alice,
        &object,
        RevisionId::new("git:sha256:slice3r1"),
        vec![],
        manifest_hash.clone(),
        1_700_001_000,
    );
    let body = serde_json::to_value(ObjectRevisionStatementJson::from_statement(&signed))
        .expect("serialize");

    let (status, response_body) =
        signed_post_json(handle.socket_path(), "/api/v1/statements", &body, &alice).await;
    assert_eq!(status, hyper::StatusCode::OK, "body: {response_body:?}");

    let envelope: Value = serde_json::from_slice(&response_body).expect("parse");
    assert_eq!(envelope["ok"], Value::Bool(true));
    assert_eq!(
        envelope["result"]["statement_id"].as_str(),
        Some(signed.statement_id().to_string().as_str())
    );
    assert_eq!(envelope["result"]["statement"]["type"], "ObjectRevision");

    handle.shutdown().await;
    let store_reread = kairo_store::FilesystemStore::open(&store_path).expect("re-open store");
    let persisted: SignedStatement<ObjectRevisionBody> =
        kairo_store::StatementStore::get_object_revision(&store_reread, &signed.statement_id())
            .expect("get_object_revision");
    assert_eq!(persisted.statement_id(), signed.statement_id());

    drop(dir);
}

#[tokio::test]
async fn rejects_object_revision_with_bad_signature() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let object = fixture.make_object(&alice, "kairo/object");
    let manifest = ObjectManifest::parse_toml(FIXTURE_MANIFEST).expect("manifest");
    let manifest_hash = manifest.manifest_hash();
    fixture
        .store
        .put_blob(&manifest_hash, &manifest.canonical_bytes())
        .expect("put_blob");
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let signed = sign_revision(
        &alice,
        &object,
        RevisionId::new("git:sha256:slice3r2"),
        vec![],
        manifest_hash,
        1_700_001_001,
    );
    let mut bad_bytes = signed.signature().bytes().to_vec();
    bad_bytes[0] ^= 0x01;
    let bad_signature = Signature::new(
        alice.actor_id.clone(),
        alice.signing.public_key().key_id().to_string(),
        "ed25519",
        bad_bytes,
    );
    let bad_signed = SignedStatement::new(signed.unsigned().clone(), bad_signature);
    let body = serde_json::to_value(ObjectRevisionStatementJson::from_statement(&bad_signed))
        .expect("serialize");

    let (status, response_body) =
        signed_post_json(handle.socket_path(), "/api/v1/statements", &body, &alice).await;
    assert_eq!(status, hyper::StatusCode::UNPROCESSABLE_ENTITY);
    let envelope: Value = serde_json::from_slice(&response_body).expect("parse");
    assert_eq!(envelope["error"]["code"], "bad_signature");

    handle.shutdown().await;
    drop(dir);
}

// --- Slice 3: ObjectBranch ---------------------------------------------------

#[tokio::test]
async fn accepts_object_branch_set_and_persists_head() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let object = fixture.make_object(&alice, "kairo/object");
    let revision = fixture.make_revision(
        &alice,
        &object,
        RevisionId::new("git:sha256:slice3-branch-r1"),
        FIXTURE_MANIFEST,
        vec![],
    );
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path.clone()).await;
    let signed = sign_branch(&alice, &object, &revision, "head", None, 1_700_001_100);
    let body = serde_json::to_value(ObjectBranchStatementJson::from_statement(&signed))
        .expect("serialize");

    let (status, response_body) =
        signed_post_json(handle.socket_path(), "/api/v1/statements", &body, &alice).await;
    assert_eq!(status, hyper::StatusCode::OK, "body: {response_body:?}");

    let envelope: Value = serde_json::from_slice(&response_body).expect("parse");
    assert_eq!(envelope["result"]["statement"]["type"], "ObjectBranch");

    handle.shutdown().await;
    let store_reread = kairo_store::FilesystemStore::open(&store_path).expect("re-open store");
    let head = store_reread
        .latest_branch(&alice.actor_id, &object.object_id, "head")
        .expect("latest_branch")
        .expect("head present");
    assert_eq!(head.statement_id(), signed.statement_id());

    drop(dir);
}

#[tokio::test]
async fn rejects_object_branch_with_bad_signature() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let object = fixture.make_object(&alice, "kairo/object");
    let revision = fixture.make_revision(
        &alice,
        &object,
        RevisionId::new("git:sha256:slice3-branch-r2"),
        FIXTURE_MANIFEST,
        vec![],
    );
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let signed = sign_branch(&alice, &object, &revision, "head", None, 1_700_001_101);
    let mut bad_bytes = signed.signature().bytes().to_vec();
    bad_bytes[0] ^= 0x01;
    let bad_signature = Signature::new(
        alice.actor_id.clone(),
        alice.signing.public_key().key_id().to_string(),
        "ed25519",
        bad_bytes,
    );
    let bad_signed = SignedStatement::new(signed.unsigned().clone(), bad_signature);
    let body = serde_json::to_value(ObjectBranchStatementJson::from_statement(&bad_signed))
        .expect("serialize");

    let (status, response_body) =
        signed_post_json(handle.socket_path(), "/api/v1/statements", &body, &alice).await;
    assert_eq!(status, hyper::StatusCode::UNPROCESSABLE_ENTITY);
    let envelope: Value = serde_json::from_slice(&response_body).expect("parse");
    assert_eq!(envelope["error"]["code"], "bad_signature");

    handle.shutdown().await;
    drop(dir);
}

// --- Slice 3: ObjectVersionTag -----------------------------------------------

#[tokio::test]
async fn accepts_version_tag_bind_and_persists_head() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let object = fixture.make_object(&alice, "kairo/object");
    let revision = fixture.make_revision(
        &alice,
        &object,
        RevisionId::new("git:sha256:slice3-tag-r1"),
        FIXTURE_MANIFEST,
        vec![],
    );
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path.clone()).await;
    let signed = sign_version_tag(&alice, &object, &revision, "1.0.0", None, 1_700_001_200);
    let body = serde_json::to_value(ObjectVersionTagStatementJson::from_statement(&signed))
        .expect("serialize");

    let (status, response_body) =
        signed_post_json(handle.socket_path(), "/api/v1/statements", &body, &alice).await;
    assert_eq!(status, hyper::StatusCode::OK, "body: {response_body:?}");

    let envelope: Value = serde_json::from_slice(&response_body).expect("parse");
    assert_eq!(envelope["result"]["statement"]["type"], "ObjectVersionTag");

    handle.shutdown().await;
    let store_reread = kairo_store::FilesystemStore::open(&store_path).expect("re-open store");
    let head = store_reread
        .latest_version_tag(&alice.actor_id, &object.object_id, "1.0.0")
        .expect("latest_version_tag")
        .expect("head present");
    assert_eq!(head.statement_id(), signed.statement_id());

    drop(dir);
}

#[tokio::test]
async fn version_tag_byte_identical_redelivery_is_idempotent() {
    // One re-delivery test per kind would be redundant — the
    // idempotency machinery lives in the shared `persist_signed`
    // helper, and Slice 2's ActorTrust covers the contract. The
    // VersionTag case exists here as a smoke check that the
    // store-side `atomic_write` overwrite still produces a
    // matching statement_id when re-POSTed.
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let object = fixture.make_object(&alice, "kairo/object");
    let revision = fixture.make_revision(
        &alice,
        &object,
        RevisionId::new("git:sha256:slice3-tag-r2"),
        FIXTURE_MANIFEST,
        vec![],
    );
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let signed = sign_version_tag(&alice, &object, &revision, "1.1.0", None, 1_700_001_201);
    let body = serde_json::to_value(ObjectVersionTagStatementJson::from_statement(&signed))
        .expect("serialize");

    let (s1, b1) =
        signed_post_json(handle.socket_path(), "/api/v1/statements", &body, &alice).await;
    let (s2, b2) =
        signed_post_json(handle.socket_path(), "/api/v1/statements", &body, &alice).await;
    assert_eq!(s1, hyper::StatusCode::OK);
    assert_eq!(s2, hyper::StatusCode::OK);
    let env1: Value = serde_json::from_slice(&b1).expect("parse");
    let env2: Value = serde_json::from_slice(&b2).expect("parse");
    assert_eq!(
        env1["result"]["statement_id"], env2["result"]["statement_id"],
        "byte-identical re-delivery must yield the same id across kinds"
    );

    handle.shutdown().await;
    drop(dir);
}

// --- Slice 4: ObjectGenesis --------------------------------------------------

/// Build a signature-valid `ObjectGenesisStatement` signed by
/// `creator`. The signature covers `body.canonical_bytes()`
/// directly (no envelope wrapping — see
/// `project_genesis_signature_asymmetry`).
fn sign_object_genesis(
    creator: &CreatedActor,
    kind: &str,
    nonce_seed: u8,
    created_at_seconds: i64,
) -> ObjectGenesisStatement {
    let nonce = [nonce_seed; 32];
    let body = ObjectGenesisBody::new(
        ObjectKind::new(kind.to_owned()),
        creator.actor_id.clone(),
        Timestamp::from_seconds(created_at_seconds),
        nonce,
        None,
    );
    let signature_bytes = creator.signing.sign(&body.canonical_bytes());
    let signature = Signature::new(
        creator.actor_id.clone(),
        creator.signing.public_key().key_id().to_string(),
        "ed25519",
        signature_bytes.bytes().to_vec(),
    );
    ObjectGenesisStatement::new(body, signature)
}

#[tokio::test]
async fn accepts_object_genesis_and_persists_it() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path.clone()).await;
    let statement = sign_object_genesis(&alice, "kairo/object", 0xA1, 1_700_002_000);
    let expected_object_id = statement.object_id();
    let body = serde_json::to_value(ObjectGenesisStatementJson::from_statement(&statement))
        .expect("serialize");

    let (status, response_body) =
        signed_post_json(handle.socket_path(), "/api/v1/statements", &body, &alice).await;
    assert_eq!(status, hyper::StatusCode::OK, "body: {response_body:?}");

    let envelope: Value = serde_json::from_slice(&response_body).expect("parse");
    assert_eq!(envelope["ok"], Value::Bool(true));
    assert_eq!(
        envelope["result"]["statement_id"].as_str(),
        Some(expected_object_id.to_string().as_str()),
        "ObjectGenesis writes echo the object_id in the statement_id field",
    );
    assert_eq!(envelope["result"]["statement"]["type"], "ObjectGenesis");

    handle.shutdown().await;
    let store_reread = kairo_store::FilesystemStore::open(&store_path).expect("re-open store");
    let persisted =
        kairo_store::ObjectStore::get_object_genesis(&store_reread, &expected_object_id)
            .expect("get_object_genesis");
    assert_eq!(persisted.body(), statement.body());

    drop(dir);
}

#[tokio::test]
async fn rejects_object_genesis_with_bad_signature() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let statement = sign_object_genesis(&alice, "kairo/object", 0xA2, 1_700_002_001);
    let mut bad_bytes = statement.signature().bytes().to_vec();
    bad_bytes[0] ^= 0x01;
    let bad_signature = Signature::new(
        alice.actor_id.clone(),
        alice.signing.public_key().key_id().to_string(),
        "ed25519",
        bad_bytes,
    );
    let bad = ObjectGenesisStatement::new(statement.body().clone(), bad_signature);
    let body =
        serde_json::to_value(ObjectGenesisStatementJson::from_statement(&bad)).expect("serialize");

    let (status, response_body) =
        signed_post_json(handle.socket_path(), "/api/v1/statements", &body, &alice).await;
    assert_eq!(status, hyper::StatusCode::UNPROCESSABLE_ENTITY);
    let envelope: Value = serde_json::from_slice(&response_body).expect("parse");
    assert_eq!(envelope["error"]["code"], "bad_signature");

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn rejects_object_genesis_with_signature_actor_mismatch() {
    // The genesis body's `created_by` is one actor but the
    // signature is presented under a different one. Caught
    // before any resolver lookup or signature math runs.
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let statement = sign_object_genesis(&alice, "kairo/object", 0xA3, 1_700_002_002);
    let mismatched = ObjectGenesisStatement::new(
        statement.body().clone(),
        Signature::new(
            bob.actor_id.clone(),
            bob.signing.public_key().key_id().to_string(),
            "ed25519",
            statement.signature().bytes().to_vec(),
        ),
    );
    let body = serde_json::to_value(ObjectGenesisStatementJson::from_statement(&mismatched))
        .expect("serialize");

    let (status, response_body) =
        signed_post_json(handle.socket_path(), "/api/v1/statements", &body, &alice).await;
    assert_eq!(status, hyper::StatusCode::UNPROCESSABLE_ENTITY);
    let envelope: Value = serde_json::from_slice(&response_body).expect("parse");
    assert_eq!(envelope["error"]["code"], "signature_actor_mismatch");

    handle.shutdown().await;
    drop(dir);
}

// --- Slice 4: capability grant + revocation ---------------------------------

fn sign_capability_grant(
    grantor: &CreatedActor,
    grantee: &CreatedActor,
    object_id: &kairo_core::ObjectId,
    created_at_seconds: i64,
) -> SignedStatement<ActorCapabilityGrantBody> {
    let scope = CapabilityScope::Object(object_id.clone());
    let capability = Capability::new(
        scope,
        vec![StatementKind::ObjectVersionTag],
        true,
        Vec::new(),
    )
    .expect("capability shape");
    let body = ActorCapabilityGrantBody::new(grantee.actor_id.clone(), capability, None);
    let subject: KairoRef = format!("actor:{}", grantee.actor_id)
        .parse()
        .expect("subject parse");
    let unsigned = UnsignedStatement::new(
        grantor.actor_id.clone(),
        subject,
        Timestamp::from_seconds(created_at_seconds),
        body,
    );
    let signature_bytes = grantor.signing.sign(&unsigned.canonical_bytes());
    let signature = Signature::new(
        grantor.actor_id.clone(),
        grantor.signing.public_key().key_id().to_string(),
        "ed25519",
        signature_bytes.bytes().to_vec(),
    );
    SignedStatement::new(unsigned, signature)
}

#[tokio::test]
async fn accepts_capability_grant_and_persists_head() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let object = fixture.make_object(&alice, "kairo/object");
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path.clone()).await;
    let signed = sign_capability_grant(&alice, &bob, &object.object_id, 1_700_002_100);
    let body = serde_json::to_value(ActorCapabilityGrantStatementJson::from_statement(&signed))
        .expect("serialize");

    let (status, response_body) =
        signed_post_json(handle.socket_path(), "/api/v1/statements", &body, &alice).await;
    assert_eq!(status, hyper::StatusCode::OK, "body: {response_body:?}");

    let envelope: Value = serde_json::from_slice(&response_body).expect("parse");
    assert_eq!(
        envelope["result"]["statement"]["type"],
        "ActorCapabilityGrant"
    );

    handle.shutdown().await;
    let store_reread = kairo_store::FilesystemStore::open(&store_path).expect("re-open store");
    let scope = CapabilityScope::Object(object.object_id.clone());
    let head = kairo_store::CapabilityResolver::latest_capability(
        &store_reread,
        &alice.actor_id,
        &bob.actor_id,
        &scope,
    )
    .expect("latest_capability")
    .expect("head present");
    assert_eq!(head.statement_id(), signed.statement_id());

    drop(dir);
}

#[tokio::test]
async fn rejects_capability_grant_with_bad_signature() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let object = fixture.make_object(&alice, "kairo/object");
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let signed = sign_capability_grant(&alice, &bob, &object.object_id, 1_700_002_101);
    let mut bad_bytes = signed.signature().bytes().to_vec();
    bad_bytes[0] ^= 0x01;
    let bad_signature = Signature::new(
        alice.actor_id.clone(),
        alice.signing.public_key().key_id().to_string(),
        "ed25519",
        bad_bytes,
    );
    let bad = SignedStatement::new(signed.unsigned().clone(), bad_signature);
    let body = serde_json::to_value(ActorCapabilityGrantStatementJson::from_statement(&bad))
        .expect("serialize");

    let (status, response_body) =
        signed_post_json(handle.socket_path(), "/api/v1/statements", &body, &alice).await;
    assert_eq!(status, hyper::StatusCode::UNPROCESSABLE_ENTITY);
    let envelope: Value = serde_json::from_slice(&response_body).expect("parse");
    assert_eq!(envelope["error"]["code"], "bad_signature");

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn accepts_capability_revocation_and_persists_it() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let object = fixture.make_object(&alice, "kairo/object");
    // Persist a grant directly so we have something to revoke.
    let scope = CapabilityScope::Object(object.object_id.clone());
    let capability = Capability::new(
        scope.clone(),
        vec![StatementKind::ObjectVersionTag],
        true,
        Vec::new(),
    )
    .expect("capability shape");
    let grant_body = ActorCapabilityGrantBody::new(bob.actor_id.clone(), capability, None);
    let grant_subject: KairoRef = format!("actor:{}", bob.actor_id).parse().expect("subject");
    let grant_unsigned = UnsignedStatement::new(
        alice.actor_id.clone(),
        grant_subject,
        Timestamp::from_seconds(1_700_002_200),
        grant_body,
    );
    let grant_sig_bytes = alice.signing.sign(&grant_unsigned.canonical_bytes());
    let grant_sig = Signature::new(
        alice.actor_id.clone(),
        alice.signing.public_key().key_id().to_string(),
        "ed25519",
        grant_sig_bytes.bytes().to_vec(),
    );
    let grant = SignedStatement::new(grant_unsigned, grant_sig);
    let grant_id = kairo_store::StatementStore::put_actor_capability_grant(&fixture.store, &grant)
        .expect("put grant");
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path.clone()).await;

    // Sign a default (non-retroactive) revocation pointing at
    // grant_id.
    let rev_body = ActorCapabilityRevocationBody::new(grant_id.clone(), false, None);
    let rev_subject: KairoRef = format!("statement:{grant_id}").parse().expect("subject");
    let rev_unsigned = UnsignedStatement::new(
        alice.actor_id.clone(),
        rev_subject,
        Timestamp::from_seconds(1_700_002_201),
        rev_body,
    );
    let rev_sig_bytes = alice.signing.sign(&rev_unsigned.canonical_bytes());
    let rev_sig = Signature::new(
        alice.actor_id.clone(),
        alice.signing.public_key().key_id().to_string(),
        "ed25519",
        rev_sig_bytes.bytes().to_vec(),
    );
    let revocation = SignedStatement::new(rev_unsigned, rev_sig);

    let body = serde_json::to_value(ActorCapabilityRevocationStatementJson::from_statement(
        &revocation,
    ))
    .expect("serialize");

    let (status, response_body) =
        signed_post_json(handle.socket_path(), "/api/v1/statements", &body, &alice).await;
    assert_eq!(status, hyper::StatusCode::OK, "body: {response_body:?}");

    let envelope: Value = serde_json::from_slice(&response_body).expect("parse");
    assert_eq!(
        envelope["result"]["statement"]["type"],
        "ActorCapabilityRevocation"
    );

    handle.shutdown().await;
    let store_reread = kairo_store::FilesystemStore::open(&store_path).expect("re-open store");
    let head = kairo_store::CapabilityResolver::latest_capability_revocation(
        &store_reread,
        &alice.actor_id,
        &grant_id,
    )
    .expect("latest_capability_revocation")
    .expect("revocation present");
    assert_eq!(head.statement_id(), revocation.statement_id());

    drop(dir);
}

// --- Phase B Slice 2: per-401 authn pipeline coverage --------------------
//
// These tests exercise the request-auth pipeline introduced by
// `DECISIONS.md §14`. Each tests one failure mode of
// `verify_request_auth` independently; the happy path is
// already covered by every Phase A test above.

#[tokio::test]
async fn rejects_post_with_missing_authorization() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let signed = sign_trust_grant(&alice, &bob, Some(TrustDecision::Trusted), None);
    let body =
        serde_json::to_value(ActorTrustStatementJson::from_statement(&signed)).expect("serialize");

    // `auth = None` deliberately sends no Authorization header.
    let (status, response_body) =
        http_post_json(handle.socket_path(), "/api/v1/statements", &body, None).await;
    assert_eq!(status, hyper::StatusCode::UNAUTHORIZED);
    let envelope: Value = serde_json::from_slice(&response_body).expect("parse envelope");
    assert_eq!(envelope["error"]["code"], "missing_authorization");

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn rejects_post_with_malformed_authorization() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let signed = sign_trust_grant(&alice, &bob, Some(TrustDecision::Trusted), None);
    let body =
        serde_json::to_value(ActorTrustStatementJson::from_statement(&signed)).expect("serialize");

    // Right scheme tag, gibberish params.
    let (status, response_body) = http_post_json(
        handle.socket_path(),
        "/api/v1/statements",
        &body,
        Some("KairoActor1 not_a_param"),
    )
    .await;
    assert_eq!(status, hyper::StatusCode::UNAUTHORIZED);
    let envelope: Value = serde_json::from_slice(&response_body).expect("parse envelope");
    assert_eq!(envelope["error"]["code"], "malformed_authorization");

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn rejects_post_with_unsupported_auth_scheme() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let signed = sign_trust_grant(&alice, &bob, Some(TrustDecision::Trusted), None);
    let body =
        serde_json::to_value(ActorTrustStatementJson::from_statement(&signed)).expect("serialize");

    let (status, response_body) = http_post_json(
        handle.socket_path(),
        "/api/v1/statements",
        &body,
        Some("Bearer abc.def.ghi"),
    )
    .await;
    assert_eq!(status, hyper::StatusCode::UNAUTHORIZED);
    let envelope: Value = serde_json::from_slice(&response_body).expect("parse envelope");
    assert_eq!(envelope["error"]["code"], "unsupported_auth_scheme");

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn rejects_post_with_timestamp_out_of_window() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let signed = sign_trust_grant(&alice, &bob, Some(TrustDecision::Trusted), None);
    let body =
        serde_json::to_value(ActorTrustStatementJson::from_statement(&signed)).expect("serialize");
    let body_bytes = serde_json::to_vec(&body).expect("serialize");

    // Sign with a timestamp that is just outside the window.
    let stale_ts = Timestamp::now().seconds() - (TIMESTAMP_WINDOW_SECONDS + 10);
    let nonce = random_nonce().expect("nonce");
    let envelope = sign_request(
        "POST",
        "/api/v1/statements",
        &body_bytes,
        alice.actor_id.clone(),
        &alice.signing,
        stale_ts,
        nonce,
    );
    let auth = envelope.format();

    let (status, response_body) = http_post_raw(
        handle.socket_path(),
        "/api/v1/statements",
        body_bytes,
        Some(&auth),
    )
    .await;
    assert_eq!(status, hyper::StatusCode::UNAUTHORIZED);
    let envelope: Value = serde_json::from_slice(&response_body).expect("parse envelope");
    assert_eq!(envelope["error"]["code"], "timestamp_out_of_window");

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn rejects_post_with_bad_request_signature() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let signed = sign_trust_grant(&alice, &bob, Some(TrustDecision::Trusted), None);
    let body =
        serde_json::to_value(ActorTrustStatementJson::from_statement(&signed)).expect("serialize");
    let body_bytes = serde_json::to_vec(&body).expect("serialize");

    // Build a valid envelope, then flip one bit of the
    // signature so it no longer verifies against alice's key.
    let nonce = random_nonce().expect("nonce");
    let timestamp = Timestamp::now().seconds();
    let mut envelope = sign_request(
        "POST",
        "/api/v1/statements",
        &body_bytes,
        alice.actor_id.clone(),
        &alice.signing,
        timestamp,
        nonce,
    );
    envelope.signature[0] ^= 0x01;
    let auth = envelope.format();

    let (status, response_body) = http_post_raw(
        handle.socket_path(),
        "/api/v1/statements",
        body_bytes,
        Some(&auth),
    )
    .await;
    assert_eq!(status, hyper::StatusCode::UNAUTHORIZED);
    let envelope: Value = serde_json::from_slice(&response_body).expect("parse envelope");
    assert_eq!(envelope["error"]["code"], "bad_request_signature");

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn rejects_post_with_request_key_id_unknown() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let signed = sign_trust_grant(&alice, &bob, Some(TrustDecision::Trusted), None);
    let body =
        serde_json::to_value(ActorTrustStatementJson::from_statement(&signed)).expect("serialize");
    let body_bytes = serde_json::to_vec(&body).expect("serialize");

    // Sign with bob's key but claim alice as the actor. The
    // daemon resolves alice's active key, sees the envelope's
    // key_id doesn't match, and returns 401.
    let nonce = random_nonce().expect("nonce");
    let timestamp = Timestamp::now().seconds();
    let envelope = sign_request(
        "POST",
        "/api/v1/statements",
        &body_bytes,
        alice.actor_id.clone(),
        &bob.signing,
        timestamp,
        nonce,
    );
    let auth = envelope.format();

    let (status, response_body) = http_post_raw(
        handle.socket_path(),
        "/api/v1/statements",
        body_bytes,
        Some(&auth),
    )
    .await;
    assert_eq!(status, hyper::StatusCode::UNAUTHORIZED);
    let envelope: Value = serde_json::from_slice(&response_body).expect("parse envelope");
    assert_eq!(envelope["error"]["code"], "request_key_id_unknown");

    handle.shutdown().await;
    drop(dir);
}

// --- Phase B Slice 3: capability enforcement on object writes ----------------
//
// The daemon now runs `evaluate_capability` between authn and
// the dispatcher arms for `ObjectRevision`/`Branch`/`VersionTag`.
// Same-actor (root-authority) writes still pass — every Phase A
// test above is a same-actor case and still green. These tests
// drive the cross-actor cases the gate is meant to catch.

/// Build, sign, and persist (directly through the store) an
/// `ActorCapabilityGrant` from `grantor` to `grantee` with the
/// supplied capability shape. Used as a fixture builder so the
/// daemon-side gate has something to evaluate against.
#[allow(clippy::too_many_arguments)]
fn persist_grant(
    fixture: &StoreFixture,
    grantor: &CreatedActor,
    grantee: &CreatedActor,
    object_id: &kairo_core::ObjectId,
    kinds: Vec<StatementKind>,
    delegable: bool,
    constraints: Vec<CapabilityConstraint>,
    created_at_seconds: i64,
) -> kairo_core::StatementId {
    let scope = CapabilityScope::Object(object_id.clone());
    let capability =
        Capability::new(scope, kinds, delegable, constraints).expect("capability shape");
    let body = ActorCapabilityGrantBody::new(grantee.actor_id.clone(), capability, None);
    let subject: KairoRef = format!("actor:{}", grantee.actor_id)
        .parse()
        .expect("subject parse");
    let unsigned = UnsignedStatement::new(
        grantor.actor_id.clone(),
        subject,
        Timestamp::from_seconds(created_at_seconds),
        body,
    );
    let signature_bytes = grantor.signing.sign(&unsigned.canonical_bytes());
    let signature = Signature::new(
        grantor.actor_id.clone(),
        grantor.signing.public_key().key_id().to_string(),
        "ed25519",
        signature_bytes.bytes().to_vec(),
    );
    let signed = SignedStatement::new(unsigned, signature);
    kairo_store::StatementStore::put_actor_capability_grant(&fixture.store, &signed)
        .expect("put grant")
}

/// Build + persist a revocation pointing at `grant_id`.
fn persist_revocation(
    fixture: &StoreFixture,
    grantor: &CreatedActor,
    grant_id: &kairo_core::StatementId,
    retroactive: bool,
    created_at_seconds: i64,
) -> kairo_core::StatementId {
    let body = ActorCapabilityRevocationBody::new(grant_id.clone(), retroactive, None);
    let subject: KairoRef = format!("statement:{grant_id}")
        .parse()
        .expect("subject parse");
    let unsigned = UnsignedStatement::new(
        grantor.actor_id.clone(),
        subject,
        Timestamp::from_seconds(created_at_seconds),
        body,
    );
    let signature_bytes = grantor.signing.sign(&unsigned.canonical_bytes());
    let signature = Signature::new(
        grantor.actor_id.clone(),
        grantor.signing.public_key().key_id().to_string(),
        "ed25519",
        signature_bytes.bytes().to_vec(),
    );
    let signed = SignedStatement::new(unsigned, signature);
    kairo_store::StatementStore::put_actor_capability_revocation(&fixture.store, &signed)
        .expect("put revocation")
}

#[tokio::test]
async fn accepts_cross_actor_version_tag_with_valid_grant() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let object = fixture.make_object(&alice, "kairo/object");
    let revision = fixture.make_revision(
        &alice,
        &object,
        RevisionId::new("git:sha256:slice3-tag-cross-r1"),
        FIXTURE_MANIFEST,
        vec![],
    );
    // Alice grants Bob ObjectVersionTag on the object.
    persist_grant(
        &fixture,
        &alice,
        &bob,
        &object.object_id,
        vec![StatementKind::ObjectVersionTag],
        true,
        Vec::new(),
        1_700_003_000,
    );
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let signed = sign_version_tag(&bob, &object, &revision, "2.0.0", None, 1_700_003_100);
    let body = serde_json::to_value(ObjectVersionTagStatementJson::from_statement(&signed))
        .expect("serialize");

    let (status, response_body) =
        signed_post_json(handle.socket_path(), "/api/v1/statements", &body, &bob).await;
    assert_eq!(status, hyper::StatusCode::OK, "body: {response_body:?}");

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn rejects_cross_actor_revision_without_grant_with_403() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let object = fixture.make_object(&alice, "kairo/object");
    let manifest = ObjectManifest::parse_toml(FIXTURE_MANIFEST).expect("manifest");
    let manifest_hash = manifest.manifest_hash();
    fixture
        .store
        .put_blob(&manifest_hash, &manifest.canonical_bytes())
        .expect("put_blob");
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    // Bob signs a revision on Alice's object without holding any
    // grant — capability gate should refuse before the dispatcher.
    let signed = sign_revision(
        &bob,
        &object,
        RevisionId::new("git:sha256:slice3-cross-r-nope"),
        vec![],
        manifest_hash,
        1_700_003_200,
    );
    let body = serde_json::to_value(ObjectRevisionStatementJson::from_statement(&signed))
        .expect("serialize");

    let (status, response_body) =
        signed_post_json(handle.socket_path(), "/api/v1/statements", &body, &bob).await;
    assert_eq!(
        status,
        hyper::StatusCode::FORBIDDEN,
        "body: {response_body:?}"
    );
    let envelope: Value = serde_json::from_slice(&response_body).expect("parse envelope");
    assert_eq!(envelope["error"]["code"], "not_authorized");
    let message = envelope["error"]["message"].as_str().unwrap_or("");
    assert!(
        message.contains("ObjectRevision") && message.contains("does not hold"),
        "expected NotHeld diagnostic in message: {message}",
    );

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn rejects_cross_actor_branch_with_revoked_grant_with_403() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let object = fixture.make_object(&alice, "kairo/object");
    let revision = fixture.make_revision(
        &alice,
        &object,
        RevisionId::new("git:sha256:slice3-cross-branch-r1"),
        FIXTURE_MANIFEST,
        vec![],
    );
    let grant_id = persist_grant(
        &fixture,
        &alice,
        &bob,
        &object.object_id,
        vec![StatementKind::ObjectBranch],
        true,
        Vec::new(),
        1_700_003_300,
    );
    // Revoke it default (i.e. only `at >= revocation.created_at`)
    // — we'll request at a timestamp after the revocation.
    persist_revocation(&fixture, &alice, &grant_id, false, 1_700_003_350);
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let signed = sign_branch(&bob, &object, &revision, "head", None, 1_700_003_400);
    let body = serde_json::to_value(ObjectBranchStatementJson::from_statement(&signed))
        .expect("serialize");

    let (status, response_body) =
        signed_post_json(handle.socket_path(), "/api/v1/statements", &body, &bob).await;
    assert_eq!(status, hyper::StatusCode::FORBIDDEN);
    let envelope: Value = serde_json::from_slice(&response_body).expect("parse envelope");
    assert_eq!(envelope["error"]["code"], "not_authorized");
    let message = envelope["error"]["message"].as_str().unwrap_or("");
    assert!(
        message.contains("revoked"),
        "expected revoked diagnostic in message: {message}",
    );

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn rejects_cross_actor_version_tag_with_expired_grant_with_403() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let object = fixture.make_object(&alice, "kairo/object");
    let revision = fixture.make_revision(
        &alice,
        &object,
        RevisionId::new("git:sha256:slice3-cross-tag-r1"),
        FIXTURE_MANIFEST,
        vec![],
    );
    // Grant expires before the request timestamp.
    persist_grant(
        &fixture,
        &alice,
        &bob,
        &object.object_id,
        vec![StatementKind::ObjectVersionTag],
        true,
        vec![CapabilityConstraint::ExpiresAt(Timestamp::from_seconds(
            1_700_000_000,
        ))],
        1_699_000_000,
    );
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    // Request signed for "now"; the daemon's authn pipeline
    // pins the evaluator to `request.timestamp`, so as long as
    // the test runs after the grant expired (it always will),
    // the evaluator returns Expired.
    let signed = sign_version_tag(&bob, &object, &revision, "3.0.0", None, 1_700_003_500);
    let body = serde_json::to_value(ObjectVersionTagStatementJson::from_statement(&signed))
        .expect("serialize");

    let (status, response_body) =
        signed_post_json(handle.socket_path(), "/api/v1/statements", &body, &bob).await;
    assert_eq!(status, hyper::StatusCode::FORBIDDEN);
    let envelope: Value = serde_json::from_slice(&response_body).expect("parse envelope");
    assert_eq!(envelope["error"]["code"], "not_authorized");
    let message = envelope["error"]["message"].as_str().unwrap_or("");
    assert!(
        message.contains("expired"),
        "expected expired diagnostic in message: {message}",
    );

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn rejects_cross_actor_version_tag_with_delegation_depth_violation_with_403() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let carol = fixture.make_actor();
    let object = fixture.make_object(&alice, "kairo/object");
    let revision = fixture.make_revision(
        &alice,
        &object,
        RevisionId::new("git:sha256:slice3-cross-depth-r1"),
        FIXTURE_MANIFEST,
        vec![],
    );
    // Alice grants Bob with MaxDelegationDepth(0) — Bob can
    // use it himself but cannot re-delegate.
    persist_grant(
        &fixture,
        &alice,
        &bob,
        &object.object_id,
        vec![StatementKind::ObjectVersionTag],
        true,
        vec![CapabilityConstraint::MaxDelegationDepth(0)],
        1_700_003_600,
    );
    // Bob re-delegates to Carol anyway. The store accepts the
    // grant (Phase A doesn't validate); the evaluator surfaces
    // the violation when Carol tries to use it.
    persist_grant(
        &fixture,
        &bob,
        &carol,
        &object.object_id,
        vec![StatementKind::ObjectVersionTag],
        false,
        Vec::new(),
        1_700_003_650,
    );
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let signed = sign_version_tag(&carol, &object, &revision, "4.0.0", None, 1_700_003_700);
    let body = serde_json::to_value(ObjectVersionTagStatementJson::from_statement(&signed))
        .expect("serialize");

    let (status, response_body) =
        signed_post_json(handle.socket_path(), "/api/v1/statements", &body, &carol).await;
    assert_eq!(status, hyper::StatusCode::FORBIDDEN);
    let envelope: Value = serde_json::from_slice(&response_body).expect("parse envelope");
    assert_eq!(envelope["error"]["code"], "not_authorized");
    let message = envelope["error"]["message"].as_str().unwrap_or("");
    assert!(
        message.contains("MaxDelegationDepth") || message.contains("depth"),
        "expected depth diagnostic in message: {message}",
    );

    handle.shutdown().await;
    drop(dir);
}

// --- Phase B Slice 4: capability gate on grant + revocation ------------------
//
// Recursive delegable check on `ActorCapabilityGrant`:
//   signer must be the object's root authority OR hold the
//   exact capability being granted with `delegable = true`.
// Grantor-identity check on `ActorCapabilityRevocation`:
//   signer must be the original grantor of the revoked grant.

#[tokio::test]
async fn accepts_delegable_regrant_by_holder() {
    // Alice creates the object and grants Bob ObjectVersionTag
    // delegable. Bob then re-grants Carol — the daemon's gate
    // walks the chain Bob → Alice (root) and accepts.
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let carol = fixture.make_actor();
    let object = fixture.make_object(&alice, "kairo/object");
    persist_grant(
        &fixture,
        &alice,
        &bob,
        &object.object_id,
        vec![StatementKind::ObjectVersionTag],
        true,
        Vec::new(),
        1_700_004_000,
    );
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let signed = sign_capability_grant(&bob, &carol, &object.object_id, 1_700_004_100);
    let body = serde_json::to_value(ActorCapabilityGrantStatementJson::from_statement(&signed))
        .expect("serialize");

    let (status, response_body) =
        signed_post_json(handle.socket_path(), "/api/v1/statements", &body, &bob).await;
    assert_eq!(status, hyper::StatusCode::OK, "body: {response_body:?}");

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn rejects_non_delegable_regrant_with_403() {
    // Alice grants Bob ObjectVersionTag NON-delegable. Bob's
    // attempt to re-grant Carol must be refused — Bob has the
    // capability for himself, but not the authority to pass it
    // on.
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let carol = fixture.make_actor();
    let object = fixture.make_object(&alice, "kairo/object");
    persist_grant(
        &fixture,
        &alice,
        &bob,
        &object.object_id,
        vec![StatementKind::ObjectVersionTag],
        false, // non-delegable
        Vec::new(),
        1_700_004_200,
    );
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let signed = sign_capability_grant(&bob, &carol, &object.object_id, 1_700_004_300);
    let body = serde_json::to_value(ActorCapabilityGrantStatementJson::from_statement(&signed))
        .expect("serialize");

    let (status, response_body) =
        signed_post_json(handle.socket_path(), "/api/v1/statements", &body, &bob).await;
    assert_eq!(status, hyper::StatusCode::FORBIDDEN);
    let envelope: Value = serde_json::from_slice(&response_body).expect("parse envelope");
    assert_eq!(envelope["error"]["code"], "not_authorized");
    let message = envelope["error"]["message"].as_str().unwrap_or("");
    assert!(
        message.contains("does not hold a delegable"),
        "expected 'does not hold a delegable' diagnostic: {message}",
    );

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn rejects_grant_by_non_holder_with_403() {
    // Bob has no grant at all on Alice's object. Bob's attempt
    // to grant Carol is refused.
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let carol = fixture.make_actor();
    let object = fixture.make_object(&alice, "kairo/object");
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;
    let signed = sign_capability_grant(&bob, &carol, &object.object_id, 1_700_004_400);
    let body = serde_json::to_value(ActorCapabilityGrantStatementJson::from_statement(&signed))
        .expect("serialize");

    let (status, response_body) =
        signed_post_json(handle.socket_path(), "/api/v1/statements", &body, &bob).await;
    assert_eq!(status, hyper::StatusCode::FORBIDDEN);
    let envelope: Value = serde_json::from_slice(&response_body).expect("parse envelope");
    assert_eq!(envelope["error"]["code"], "not_authorized");

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn accepts_revocation_by_original_grantor() {
    // Alice grants Bob, then revokes — daemon-mode revocation
    // by the original grantor (root authority for the grant
    // identity, irrespective of object scope).
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let object = fixture.make_object(&alice, "kairo/object");
    let grant_id = persist_grant(
        &fixture,
        &alice,
        &bob,
        &object.object_id,
        vec![StatementKind::ObjectVersionTag],
        true,
        Vec::new(),
        1_700_004_500,
    );
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;

    // Sign a revocation as Alice (the original grantor).
    let body_rev = ActorCapabilityRevocationBody::new(grant_id.clone(), false, None);
    let subject: KairoRef = format!("statement:{grant_id}").parse().expect("subject");
    let unsigned = UnsignedStatement::new(
        alice.actor_id.clone(),
        subject,
        Timestamp::from_seconds(1_700_004_600),
        body_rev,
    );
    let signature_bytes = alice.signing.sign(&unsigned.canonical_bytes());
    let signature = Signature::new(
        alice.actor_id.clone(),
        alice.signing.public_key().key_id().to_string(),
        "ed25519",
        signature_bytes.bytes().to_vec(),
    );
    let signed = SignedStatement::new(unsigned, signature);
    let body = serde_json::to_value(ActorCapabilityRevocationStatementJson::from_statement(
        &signed,
    ))
    .expect("serialize");

    let (status, response_body) =
        signed_post_json(handle.socket_path(), "/api/v1/statements", &body, &alice).await;
    assert_eq!(status, hyper::StatusCode::OK, "body: {response_body:?}");

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn rejects_revocation_by_non_grantor_with_403() {
    // Alice grants Bob. Carol (a third party) signs a revocation
    // for that grant. The grant body's `actor` is Alice, so
    // the daemon refuses regardless of any capability Carol
    // might hold.
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let carol = fixture.make_actor();
    let object = fixture.make_object(&alice, "kairo/object");
    let grant_id = persist_grant(
        &fixture,
        &alice,
        &bob,
        &object.object_id,
        vec![StatementKind::ObjectVersionTag],
        true,
        Vec::new(),
        1_700_004_700,
    );
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;

    let body_rev = ActorCapabilityRevocationBody::new(grant_id.clone(), false, None);
    let subject: KairoRef = format!("statement:{grant_id}").parse().expect("subject");
    let unsigned = UnsignedStatement::new(
        carol.actor_id.clone(),
        subject,
        Timestamp::from_seconds(1_700_004_800),
        body_rev,
    );
    let signature_bytes = carol.signing.sign(&unsigned.canonical_bytes());
    let signature = Signature::new(
        carol.actor_id.clone(),
        carol.signing.public_key().key_id().to_string(),
        "ed25519",
        signature_bytes.bytes().to_vec(),
    );
    let signed = SignedStatement::new(unsigned, signature);
    let body = serde_json::to_value(ActorCapabilityRevocationStatementJson::from_statement(
        &signed,
    ))
    .expect("serialize");

    let (status, response_body) =
        signed_post_json(handle.socket_path(), "/api/v1/statements", &body, &carol).await;
    assert_eq!(status, hyper::StatusCode::FORBIDDEN);
    let envelope: Value = serde_json::from_slice(&response_body).expect("parse envelope");
    assert_eq!(envelope["error"]["code"], "not_authorized");
    let message = envelope["error"]["message"].as_str().unwrap_or("");
    assert!(
        message.contains("not the original grantor"),
        "expected 'not the original grantor' diagnostic: {message}",
    );

    // Unused — silence the bob warning.
    let _ = &bob;

    handle.shutdown().await;
    drop(dir);
}

#[tokio::test]
async fn rejects_revocation_of_unknown_grant_with_403() {
    // Revoke a grant id that doesn't exist locally. The daemon
    // cannot verify the signer's authority either way and
    // refuses with 403 carrying a "grant not found" diagnostic.
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path).await;

    let fake_grant_id = kairo_core::StatementId::from_sha256_digest([0xCD; 32]);
    let body_rev = ActorCapabilityRevocationBody::new(fake_grant_id.clone(), false, None);
    let subject: KairoRef = format!("statement:{fake_grant_id}")
        .parse()
        .expect("subject");
    let unsigned = UnsignedStatement::new(
        alice.actor_id.clone(),
        subject,
        Timestamp::from_seconds(1_700_004_900),
        body_rev,
    );
    let signature_bytes = alice.signing.sign(&unsigned.canonical_bytes());
    let signature = Signature::new(
        alice.actor_id.clone(),
        alice.signing.public_key().key_id().to_string(),
        "ed25519",
        signature_bytes.bytes().to_vec(),
    );
    let signed = SignedStatement::new(unsigned, signature);
    let body = serde_json::to_value(ActorCapabilityRevocationStatementJson::from_statement(
        &signed,
    ))
    .expect("serialize");

    let (status, response_body) =
        signed_post_json(handle.socket_path(), "/api/v1/statements", &body, &alice).await;
    assert_eq!(status, hyper::StatusCode::FORBIDDEN);
    let envelope: Value = serde_json::from_slice(&response_body).expect("parse envelope");
    assert_eq!(envelope["error"]["code"], "not_authorized");
    let message = envelope["error"]["message"].as_str().unwrap_or("");
    assert!(
        message.contains("not in the store"),
        "expected 'not in the store' diagnostic: {message}",
    );

    handle.shutdown().await;
    drop(dir);
}
