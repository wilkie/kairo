//! Session-mode write tests for `POST /api/v1/statements`
//! (`PHASE_2_WEB_CLIENT_AUTH.md` §2.3 + the daemon-side body
//! signing slice).
//!
//! Mirrors `post_statements.rs` shape: spawn an in-process
//! daemon over a Unix socket, drive `kairo-daemon-client` over
//! it, assert wire-level behaviour. These tests exercise the
//! cookie-mode entry point — body is `{ body_kind, body }`
//! unsigned, the daemon constructs and signs the envelope
//! before dispatching through the same per-kind persist path
//! as Phase B's `Authorization: KairoActor1` mode.

#![allow(clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::Request;
use hyper_util::rt::TokioIo;
use kairo_daemon::{serve_with_shutdown, Config, Error};
use kairo_daemon_client::{Client, ClientError};
use kairo_test_support::store::StoreFixture;
use serde_json::{json, Value};
use tokio::net::UnixStream;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

// dev-deps that don't have direct call sites in this test.
use axum as _;
use base64 as _;
use getrandom as _;
use kairo_core as _;
use kairo_identity as _;
use kairo_keystore as _;
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

#[tokio::test]
async fn session_mode_actor_trust_round_trips() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path.clone()).await;
    let client = Client::new(handle.socket_path());

    // Alice opens a session, then expresses a trust opinion
    // about Bob via cookie-mode write.
    let (_info, cookie) = client
        .open_session(&alice.actor_id.to_string(), None)
        .await
        .expect("open session");

    let body = json!({
        "trusted_actor": bob.actor_id.to_string(),
        "decision": "trusted",
        "reason": "cookie-mode smoke test",
    });

    let result = client
        .write_via_session(&cookie, "ActorTrust", body)
        .await
        .expect("write via session");

    assert_eq!(
        result.statement["type"], "ActorTrust",
        "echoed statement type"
    );
    assert_eq!(
        result.statement["actor"],
        alice.actor_id.to_string(),
        "daemon signed as the session actor"
    );
    let trusted_actor = result.statement["body"]["trusted_actor"]
        .as_str()
        .expect("trusted_actor present");
    assert_eq!(trusted_actor, bob.actor_id.to_string());

    // The daemon stamped `created_at` rather than echoing a
    // caller-supplied value — confirm the field is populated
    // with a valid RFC 3339 string.
    let created_at = result.statement["created_at"]
        .as_str()
        .expect("created_at present");
    assert!(created_at.ends_with('Z'), "created_at: {created_at}");

    handle.shutdown().await;
}

#[tokio::test]
async fn session_mode_capability_grant_round_trips() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let object = fixture.make_object(&alice, "kairo/object");
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path.clone()).await;
    let client = Client::new(handle.socket_path());

    let (_info, cookie) = client
        .open_session(&alice.actor_id.to_string(), None)
        .await
        .expect("open session");

    // Alice grants Bob an ObjectVersionTag capability on her
    // object. Cookie-mode write; the daemon signs the
    // capability statement on Alice's behalf.
    let body = json!({
        "grantee": bob.actor_id.to_string(),
        "capability": {
            "scope": { "object": object.object_id.to_string() },
            "statement_kinds": ["ObjectVersionTag"],
            "delegable": false,
            "constraints": [],
        },
    });

    let result = client
        .write_via_session(&cookie, "ActorCapabilityGrant", body)
        .await
        .expect("write via session");

    assert_eq!(result.statement["type"], "ActorCapabilityGrant");
    assert_eq!(result.statement["actor"], alice.actor_id.to_string());
    assert_eq!(
        result.statement["body"]["grantee"],
        bob.actor_id.to_string()
    );

    handle.shutdown().await;
}

#[tokio::test]
async fn session_mode_capability_denied_for_object_revision_returns_403() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    // Alice owns the object; Bob has no capability for it.
    let object = fixture.make_object(&alice, "kairo/object");
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path.clone()).await;
    let client = Client::new(handle.socket_path());

    // Bob signs in and tries to publish a revision on Alice's
    // object. Capability check rejects with 403.
    let (_info, cookie) = client
        .open_session(&bob.actor_id.to_string(), None)
        .await
        .expect("open session");

    // Any syntactically valid blob id works — the capability
    // check runs before the daemon would resolve the blob.
    let manifest_hash = kairo_core::BlobId::from_sha256_digest([0xAB; 32]).to_string();
    let body = json!({
        "object": object.object_id.to_string(),
        "revision": "git:sha256:0000000000000000000000000000000000000000",
        "parents": [],
        "manifest_hash": manifest_hash,
        "attests_reachable_history": false,
    });

    match client.write_via_session(&cookie, "ObjectRevision", body).await {
        Err(ClientError::Http { status, code, message }) => {
            assert_eq!(status, 403, "code={code}, message={message}");
            assert_eq!(code, "not_authorized");
        }
        other => panic!("expected 403 not_authorized, got {other:?}"),
    }

    handle.shutdown().await;
}

#[tokio::test]
async fn ambiguous_auth_when_both_headers_present_returns_400() {
    // Driven via raw HTTP — the client API doesn't let us send
    // both an Authorization header and a Cookie at the same
    // time. The daemon's auth-mode detection runs before any
    // body decoding, so we don't even need a valid body.

    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path.clone()).await;
    let client = Client::new(handle.socket_path());

    let (_info, cookie) = client
        .open_session(&alice.actor_id.to_string(), None)
        .await
        .expect("open session");

    let (status, body) = raw_post_json(
        handle.socket_path(),
        "/api/v1/statements",
        b"{}",
        Some("KairoActor1 actor=zXxx,key=ed25519:zYyy,t=0,n=zZz,sig=zWww"),
        Some(&cookie),
    )
    .await;
    assert_eq!(status, 400, "body: {}", String::from_utf8_lossy(&body));
    let envelope: Value = serde_json::from_slice(&body).expect("parse envelope");
    assert_eq!(envelope["ok"], Value::Bool(false));
    assert_eq!(envelope["error"]["code"], "ambiguous_auth");

    handle.shutdown().await;
}

#[tokio::test]
async fn session_mode_with_expired_cookie_returns_401() {
    let (dir, fixture) = StoreFixture::temp();
    let _alice = fixture.make_actor();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path.clone()).await;
    let client = Client::new(handle.socket_path());

    let bogus_cookie = "A".repeat(43); // 43-char base64url, never minted

    match client
        .write_via_session(&bogus_cookie, "ActorTrust", json!({}))
        .await
    {
        Err(ClientError::Http { status, code, .. }) => {
            assert_eq!(status, 401);
            assert_eq!(code, "no_session");
        }
        other => panic!("expected 401 no_session, got {other:?}"),
    }

    handle.shutdown().await;
}

#[tokio::test]
async fn session_mode_object_genesis_returns_415() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let store_path = dir.path().to_path_buf();
    drop(fixture);

    let handle = spawn_daemon(store_path.clone()).await;
    let client = Client::new(handle.socket_path());

    let (_info, cookie) = client
        .open_session(&alice.actor_id.to_string(), None)
        .await
        .expect("open session");

    match client
        .write_via_session(&cookie, "ObjectGenesis", json!({ "object_kind": "kairo/object" }))
        .await
    {
        Err(ClientError::Http { status, code, .. }) => {
            assert_eq!(status, 415);
            assert_eq!(code, "unsupported_statement_kind");
        }
        other => panic!("expected 415 unsupported_statement_kind, got {other:?}"),
    }

    handle.shutdown().await;
}

#[tokio::test]
async fn status_auth_mode_lists_both_modes() {
    let (dir, _fixture) = StoreFixture::temp();
    let handle = spawn_daemon(dir.path().to_path_buf()).await;
    let client = Client::new(handle.socket_path());

    let status = client.status().await.expect("status");
    let modes: Vec<&str> = status.auth_mode.split(',').collect();
    assert!(modes.contains(&"signed-request"));
    assert!(modes.contains(&"session-cookie"));

    handle.shutdown().await;
}

// ---------------------------------------------------------------------------
// Raw HTTP helper for the ambiguous-auth case (both headers).
// The kairo-daemon-client API doesn't compose Authorization +
// Cookie on the same request, so we build the request directly
// with hyper for that one scenario.
// ---------------------------------------------------------------------------

async fn raw_post_json(
    socket_path: &Path,
    path: &str,
    body: &[u8],
    authorization: Option<&str>,
    cookie_value: Option<&str>,
) -> (u16, Bytes) {
    let stream = UnixStream::connect(socket_path)
        .await
        .expect("connect daemon");
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
        .await
        .expect("handshake");
    let conn_task = tokio::spawn(async move {
        let _ = conn.await;
    });

    let mut builder = Request::builder()
        .method("POST")
        .uri(path)
        .header("host", "daemon")
        .header("content-type", "application/json");
    if let Some(auth) = authorization {
        builder = builder.header("authorization", auth);
    }
    if let Some(cookie) = cookie_value {
        builder = builder.header("cookie", format!("kairo_session={cookie}"));
    }
    let req = builder
        .body(Full::new(Bytes::copy_from_slice(body)))
        .expect("request build");
    let response = sender.send_request(req).await.expect("send_request");
    let status = response.status().as_u16();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    drop(sender);
    let _ = conn_task.await;
    (status, body)
}
