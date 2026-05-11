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
use hyper as _;
use kairo_daemon_client as _;
use kairo_identity as _;
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
use kairo_statement::json::ActorTrustStatementJson;
use kairo_statement::{ActorTrustBody, Signature, SignedStatement, TrustDecision, UnsignedStatement};
use kairo_store::TrustResolver;
use kairo_test_support::store::{CreatedActor, StoreFixture};
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

async fn http_post_json(
    socket: &Path,
    path: &str,
    body: &Value,
) -> (hyper::StatusCode, Bytes) {
    let stream = UnixStream::connect(socket).await.expect("connect");
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
        .await
        .expect("handshake");
    let conn_task = tokio::spawn(async move {
        let _ = conn.await;
    });

    let body_bytes = serde_json::to_vec(body).expect("serialize body");
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("host", "daemon")
        .header("content-type", "application/json")
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
        http_post_json(handle.socket_path(), "/api/v1/statements", &body).await;
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
    let store_reread =
        kairo_store::FilesystemStore::open(&store_path).expect("re-open store");
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
    let body =
        serde_json::to_value(ActorTrustStatementJson::from_statement(&bad_signed)).expect("serialize");
    let (status, response_body) =
        http_post_json(handle.socket_path(), "/api/v1/statements", &body).await;
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
    let store_reread =
        kairo_store::FilesystemStore::open(&store_path).expect("re-open store");
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
async fn rejects_unknown_actor_with_422_actor_not_found() {
    let (dir, fixture) = StoreFixture::temp();
    // Build an actor pair, but make alice unresolvable in the
    // store. The daemon's resolver should then return NotFound
    // on the envelope-actor lookup during verification. We
    // delete alice's genesis file by walking `<store>/actors/`
    // and matching on filename — the shard layout (positions
    // [3..5) and [5..7) of the bare id) is an internal kairo-
    // store detail that this test shouldn't pin.
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
        http_post_json(handle.socket_path(), "/api/v1/statements", &body).await;
    assert_eq!(
        status,
        hyper::StatusCode::UNPROCESSABLE_ENTITY,
        "body: {response_body:?}"
    );
    let envelope: Value = serde_json::from_slice(&response_body).expect("parse envelope");
    assert_eq!(envelope["error"]["code"], "actor_not_found");

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
        http_post_json(handle.socket_path(), "/api/v1/statements", &body).await;
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
    // POST a body whose `type` is not in Phase A's dispatcher
    // allowlist (Slice 2: only ActorTrust). The handler short-
    // circuits with 415 before touching the store or trying to
    // decode the body into a typed shape.
    let (dir, _fixture) = StoreFixture::temp();
    let store_path = dir.path().to_path_buf();
    drop(_fixture);

    let handle = spawn_daemon(store_path).await;
    let body = json!({
        "type": "ObjectRevision",
        "version": 1,
        // Other fields intentionally absent — the handler must
        // reject on `type` alone, before attempting body decode.
    });

    let (status, response_body) =
        http_post_json(handle.socket_path(), "/api/v1/statements", &body).await;
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
    let (dir, _fixture) = StoreFixture::temp();
    let store_path = dir.path().to_path_buf();
    drop(_fixture);

    let handle = spawn_daemon(store_path).await;
    let body = json!({ "version": 1, "actor": "kairo:actor:zNothing" });
    let (status, response_body) =
        http_post_json(handle.socket_path(), "/api/v1/statements", &body).await;
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
        http_post_json(handle.socket_path(), "/api/v1/statements", &body).await;
    assert_eq!(status1, hyper::StatusCode::OK);
    let env1: Value = serde_json::from_slice(&body1).expect("parse");
    let id1 = env1["result"]["statement_id"].as_str().expect("id1").to_owned();

    // POST the same envelope a second time — same canonical
    // bytes, same statement_id, store overwrites with byte-
    // identical content. Per PHASE_2_DAEMON_WRITES.md §2.3, this
    // is idempotent success.
    let (status2, body2) =
        http_post_json(handle.socket_path(), "/api/v1/statements", &body).await;
    assert_eq!(status2, hyper::StatusCode::OK, "body: {body2:?}");
    let env2: Value = serde_json::from_slice(&body2).expect("parse");
    let id2 = env2["result"]["statement_id"].as_str().expect("id2").to_owned();

    assert_eq!(id1, id2, "byte-identical re-delivery must yield the same id");

    handle.shutdown().await;
    drop(dir);
}
