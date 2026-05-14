//! Integration tests for the slice-1 session machinery
//! (`PHASE_2_WEB_CLIENT_AUTH.md` §6 Slice 1).
//!
//! Boots the real daemon over a real Unix socket and drives the
//! four new endpoints through `kairo_daemon_client::Client`. The
//! in-process unit tests in `crates/kairo-daemon/src/sessions.rs`
//! already cover idle and absolute timeouts against a fake clock;
//! the integration suite focuses on the HTTP round-trip and on
//! the cross-endpoint workflows (sign in, current, sign out,
//! replace).

#![allow(clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use kairo_daemon::{serve_with_shutdown, Config, Error};
use kairo_daemon_client::{Client, ClientError};
use kairo_test_support::store::StoreFixture;
use tokio::net::UnixStream;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

// dev-deps not directly referenced by this test.
use axum as _;
use base64 as _;
use getrandom as _;
use http_body_util as _;
use hyper as _;
use hyper_util as _;
use kairo_core as _;
use kairo_identity as _;
use kairo_keystore as _;
use kairo_object as _;
use kairo_statement as _;
use kairo_store as _;
use serde as _;
use serde_json as _;
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
async fn keystore_actors_lists_seeded_actors() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();

    let daemon = spawn_daemon(dir.path().to_path_buf()).await;
    let client = Client::new(daemon.socket_path());

    let actors = client.keystore_actors().await.expect("keystore actors");
    let mut ids: Vec<String> = actors.into_iter().map(|a| a.actor_id).collect();
    ids.sort();
    let mut expected = vec![alice.actor_id.to_string(), bob.actor_id.to_string()];
    expected.sort();
    assert_eq!(ids, expected);

    daemon.shutdown().await;
}

#[tokio::test]
async fn keystore_actors_returns_empty_when_no_keys() {
    let (dir, _fixture) = StoreFixture::temp();
    // No make_actor calls — keystore directory exists (the
    // fixture created it) but is empty.

    let daemon = spawn_daemon(dir.path().to_path_buf()).await;
    let client = Client::new(daemon.socket_path());

    let actors = client.keystore_actors().await.expect("keystore actors");
    assert!(actors.is_empty(), "expected empty list, got {actors:?}");

    daemon.shutdown().await;
}

#[tokio::test]
async fn sign_in_and_out_round_trip() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();

    let daemon = spawn_daemon(dir.path().to_path_buf()).await;
    let client = Client::new(daemon.socket_path());

    let (info, cookie) = client
        .open_session(&alice.actor_id.to_string(), None)
        .await
        .expect("open session");
    assert_eq!(info.actor_id, alice.actor_id.to_string());
    assert!(!cookie.is_empty(), "expected a cookie value");
    assert_eq!(
        cookie.len(),
        43,
        "session id should be 43 base64url chars, got {} ({cookie:?})",
        cookie.len()
    );

    let current = client.current_session(&cookie).await.expect("current");
    assert_eq!(current.actor_id, info.actor_id);
    assert_eq!(current.created_at, info.created_at);
    assert_eq!(current.expires_at, info.expires_at);

    client.close_session(&cookie).await.expect("close");

    // Post-close, the same cookie is rejected.
    match client.current_session(&cookie).await {
        Err(ClientError::Http { status, code, .. }) => {
            assert_eq!(status, 401);
            assert_eq!(code, "no_session");
        }
        other => panic!("expected 401 no_session, got {other:?}"),
    }

    daemon.shutdown().await;
}

#[tokio::test]
async fn open_session_rejects_unknown_actor() {
    let (dir, _fixture) = StoreFixture::temp();
    let daemon = spawn_daemon(dir.path().to_path_buf()).await;
    let client = Client::new(daemon.socket_path());

    let unknown = kairo_core::ActorId::from_sha256_digest([0x42; 32]).to_string();
    match client.open_session(&unknown, None).await {
        Err(ClientError::Http { status, code, .. }) => {
            assert_eq!(status, 404);
            assert_eq!(code, "actor_not_in_keystore");
        }
        other => panic!("expected 404 actor_not_in_keystore, got {other:?}"),
    }

    daemon.shutdown().await;
}

#[tokio::test]
async fn open_session_rejects_malformed_actor_id() {
    let (dir, _fixture) = StoreFixture::temp();
    let daemon = spawn_daemon(dir.path().to_path_buf()).await;
    let client = Client::new(daemon.socket_path());

    match client.open_session("not-a-valid-actor-id", None).await {
        Err(ClientError::Http { status, code, .. }) => {
            assert_eq!(status, 400);
            assert_eq!(code, "bad_request");
        }
        other => panic!("expected 400 bad_request, got {other:?}"),
    }

    daemon.shutdown().await;
}

#[tokio::test]
async fn second_sign_in_replaces_prior_session() {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();

    let daemon = spawn_daemon(dir.path().to_path_buf()).await;
    let client = Client::new(daemon.socket_path());

    let actor = alice.actor_id.to_string();
    let (_first_info, first_cookie) = client
        .open_session(&actor, None)
        .await
        .expect("first open");
    // Simulate the browser's automatic cookie attachment by
    // forwarding the prior session id to the daemon. The handler
    // deletes the prior session before creating the new one.
    let (_second_info, second_cookie) = client
        .open_session(&actor, Some(&first_cookie))
        .await
        .expect("second open");

    assert_ne!(
        first_cookie, second_cookie,
        "second sign-in must return a fresh session id"
    );

    // The first cookie is invalidated; the second is live.
    match client.current_session(&first_cookie).await {
        Err(ClientError::Http { status, code, .. }) => {
            assert_eq!(status, 401);
            assert_eq!(code, "no_session");
        }
        other => panic!("first cookie should be rejected, got {other:?}"),
    }
    let current = client
        .current_session(&second_cookie)
        .await
        .expect("second cookie still live");
    assert_eq!(current.actor_id, actor);

    daemon.shutdown().await;
}

#[tokio::test]
async fn current_session_without_cookie_is_401() {
    let (dir, _fixture) = StoreFixture::temp();
    let daemon = spawn_daemon(dir.path().to_path_buf()).await;
    let client = Client::new(daemon.socket_path());

    match client.current_session("").await {
        Err(ClientError::Http { status, code, .. }) => {
            assert_eq!(status, 401);
            assert_eq!(code, "no_session");
        }
        other => panic!("expected 401 no_session, got {other:?}"),
    }

    daemon.shutdown().await;
}

#[tokio::test]
async fn close_session_is_idempotent() {
    let (dir, _fixture) = StoreFixture::temp();
    let daemon = spawn_daemon(dir.path().to_path_buf()).await;
    let client = Client::new(daemon.socket_path());

    // Closing with no cookie and with a bogus cookie both succeed.
    client.close_session("").await.expect("close empty");
    client
        .close_session(&"A".repeat(43))
        .await
        .expect("close bogus");

    daemon.shutdown().await;
}
