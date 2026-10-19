//! The daemon's `FederationService` (`specs/DECISIONS.md` §28): the
//! scheduler syncs due follows and empties the inbox on its own, and
//! `POST /api/v1/federation/sync` starts follows now, once each.

#![allow(clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::Request;
use hyper_util::rt::TokioIo;
use kairo_core::{ActorId, ObjectId, Timestamp};
use kairo_daemon::{serve_with_shutdown, Config, Error};
use kairo_store::{FilesystemStore, Follow, FollowStore, InboxAnnouncement, InboxStore};
use serde_json::Value;
use tokio::net::UnixStream;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

// Crates the lib depends on that this target doesn't name.
use axum as _;
use base64 as _;
use getrandom as _;
use kairo_config as _;
use kairo_daemon_client as _;
use kairo_federation_service as _;
use kairo_identity as _;
use kairo_keystore as _;
use kairo_object as _;
use kairo_statement as _;
use kairo_test_support as _;
use serde as _;
use tokio_util as _;
use tower as _;
use tower_http as _;
use tracing as _;
use tracing_subscriber as _;
use utoipa as _;

struct Daemon {
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<Result<(), Error>>>,
    socket: PathBuf,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

impl Daemon {
    async fn stop(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            task.await.expect("join").expect("serve");
        }
    }
}

async fn spawn(store: &Path) -> Daemon {
    let socket = store.join("daemon.sock");
    let (stop, stopped) = oneshot::channel();
    let task = tokio::spawn(serve_with_shutdown(
        Config {
            store_path: store.to_path_buf(),
            keys_path: None,
        },
        async move {
            let _ = stopped.await;
        },
    ));
    for _ in 0..250 {
        if socket.exists() && UnixStream::connect(&socket).await.is_ok() {
            return Daemon {
                stop: Some(stop),
                task: Some(task),
                socket,
            };
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("daemon never bound its socket");
}

async fn call(socket: &Path, method: &str, path: &str, body: &str) -> (u16, Value) {
    let stream = UnixStream::connect(socket).await.expect("connect");
    let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .expect("handshake");
    let conn = tokio::spawn(async move {
        let _ = conn.await;
    });
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "daemon")
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body.to_owned())))
        .expect("request");
    let response = sender.send_request(request).await.expect("send");
    let status = response.status().as_u16();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    drop(sender);
    let _ = conn.await;
    (status, serde_json::from_slice(&bytes).expect("json"))
}

async fn status(socket: &Path) -> Value {
    let (code, body) = call(socket, "GET", "/api/v1/federation/status", "").await;
    assert_eq!(code, 200, "{body}");
    body["result"].clone()
}

/// Poll status until `done` holds, up to a few seconds.
async fn until(socket: &Path, done: impl Fn(&Value) -> bool) -> Value {
    for _ in 0..100 {
        let current = status(socket).await;
        if done(&current) {
            return current;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("status never got there: {}", status(socket).await);
}

fn store() -> (tempfile::TempDir, FilesystemStore, ObjectId) {
    let dir = tempfile::TempDir::new().expect("tempdir");
    std::fs::write(
        dir.path().join("config.toml"),
        "[federation.sync]\ninterval_secs = 3600\ninbox_secs = 1\n\n\
         [[federation.peers]]\nurl = \"http://127.0.0.1:1\"\n",
    )
    .expect("config");
    let store = FilesystemStore::open(dir.path()).expect("store");
    let object = ObjectId::from_sha256_digest([1; 32]);
    store
        .add_follow(&Follow::Object(object.clone()), Timestamp::from_seconds(1))
        .expect("follow");
    (dir, store, object)
}

#[tokio::test]
async fn the_scheduler_syncs_due_follows_and_empties_the_inbox() {
    let (dir, store, object) = store();
    store
        .put_inbox_announcement(
            &InboxAnnouncement {
                from: ActorId::from_sha256_digest([2; 32]),
                to: ActorId::from_sha256_digest([3; 32]),
                base_url: "https://laptop.example.org".to_owned(),
                objects: vec![object.clone()],
                received_at: Timestamp::from_seconds(1),
            },
            "0a",
            8,
        )
        .expect("announce");
    let daemon = spawn(dir.path()).await;

    // Never tried, so due at the first tick; the one peer is down.
    let synced = until(&daemon.socket, |status| {
        status["follows"][0]["last_attempt"].is_string()
            && status["inbox_entries"] == 0
            && status["runs"]
                .as_array()
                .is_some_and(|runs| runs.iter().all(|run| run["finished_at"].is_string()))
    })
    .await;
    assert_eq!(synced["enabled"], true);
    assert_eq!(synced["inbox_secs"], 1);
    let follow = &synced["follows"][0];
    assert_eq!(follow["follow"], format!("object:{object}"));
    assert_eq!(follow["last_success"], Value::Null);
    assert_eq!(follow["peers"][0]["outcome"], "failed");
    let runs = synced["runs"].as_array().expect("runs");
    let inbox = runs
        .iter()
        .find(|run| run["kind"] == "inbox")
        .expect("an inbox run");
    assert_eq!(inbox["result"], "received=1 not_pulled=1");
    assert!(runs
        .iter()
        .any(|run| run["kind"] == "sync" && run["result"] == "not_current"));
    // Discovery ran on the same schedule; its one source is down.
    assert!(
        runs.iter()
            .any(|run| run["kind"] == "discover" && run["result"] == "outcomes=1 learned=0"),
        "{synced}"
    );
    daemon.stop().await;
}

#[tokio::test]
async fn a_trigger_starts_follows_once_and_refuses_unknown_ones() {
    let (dir, _store, object) = store();
    let daemon = spawn(dir.path()).await;
    // Let the first scheduled run finish, so the trigger is the next.
    until(&daemon.socket, |status| {
        status["follows"][0]["last_attempt"].is_string() && status["follows"][0]["running"] == false
    })
    .await;

    let (code, body) = call(&daemon.socket, "POST", "/api/v1/federation/sync", "").await;
    assert_eq!(code, 200, "{body}");
    let started = body["result"]["started"].as_array().expect("started");
    let already = body["result"]["already_running"]
        .as_array()
        .expect("running");
    assert_eq!(started.len() + already.len(), 1, "{body}");

    let other = ObjectId::from_sha256_digest([9; 32]);
    let (code, body) = call(
        &daemon.socket,
        "POST",
        "/api/v1/federation/sync",
        &format!("{{\"object\":\"{other}\"}}"),
    )
    .await;
    assert_eq!(code, 404, "{body}");
    let (code, body) = call(
        &daemon.socket,
        "POST",
        "/api/v1/federation/sync",
        "{\"scope\":\"general\"}",
    )
    .await;
    assert_eq!(code, 400, "{body}");

    // Local-only mode, read on the next request without a restart.
    std::fs::write(
        dir.path().join("config.toml"),
        "[federation]\nenabled = false\n",
    )
    .expect("config");
    let (code, body) = call(
        &daemon.socket,
        "POST",
        "/api/v1/federation/sync",
        &format!("{{\"object\":\"{object}\"}}"),
    )
    .await;
    assert_eq!(code, 409, "{body}");
    assert_eq!(body["error"]["code"], "federation_disabled");
    daemon.stop().await;
}

#[tokio::test]
async fn an_explicit_fetch_is_queued_and_recorded_as_a_run() {
    let (dir, _store, _object) = store();
    let daemon = spawn(dir.path()).await;
    let wanted = ObjectId::from_sha256_digest([7; 32]);
    let body = format!("{{\"object\":\"{wanted}\"}}");
    let (code, response) = call(&daemon.socket, "POST", "/api/v1/federation/fetch", &body).await;
    assert_eq!(code, 200, "{response}");
    assert_eq!(response["result"]["acquisition"], "queued");

    let done = until(&daemon.socket, |status| {
        status["runs"].as_array().is_some_and(|runs| {
            runs.iter().any(|run| {
                run["kind"] == "fetch"
                    && run["follow"] == format!("object:{wanted}")
                    && run["result"] == "not_acquired"
            })
        })
    })
    .await;
    assert!(done["enabled"].as_bool().unwrap_or(false));

    // Explicit requests skip the cooldown.
    let (code, response) = call(&daemon.socket, "POST", "/api/v1/federation/fetch", &body).await;
    assert_eq!(code, 200, "{response}");
    assert_ne!(response["result"]["acquisition"], "cooling_down");

    let (code, _) = call(
        &daemon.socket,
        "POST",
        "/api/v1/federation/fetch",
        "{\"object\":\"not-an-id\"}",
    )
    .await;
    assert_eq!(code, 400);
    daemon.stop().await;
}

#[tokio::test]
async fn read_triggered_acquisitions_cool_down_and_need_sources() {
    use kairo_daemon::federation::{Acquisition, FederationService};
    use std::sync::Arc;
    let (dir, store, _object) = store();
    let service = FederationService::new(Arc::new(store), dir.path().to_path_buf());
    let wanted = ObjectId::from_sha256_digest([8; 32]);
    assert_eq!(service.acquire(&wanted, false), Ok(Acquisition::Queued));
    // Wait for the attempt against the down peer to finish.
    for _ in 0..100 {
        if service.runs().iter().all(|run| run.finished_at.is_some()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        service.acquire(&wanted, false),
        Ok(Acquisition::CoolingDown)
    );
    assert_eq!(service.acquire(&wanted, true), Ok(Acquisition::Queued));

    std::fs::write(dir.path().join("config.toml"), "").expect("no peers");
    let other = ObjectId::from_sha256_digest([9; 32]);
    assert_eq!(service.acquire(&other, false), Ok(Acquisition::NoSources));
    std::fs::write(
        dir.path().join("config.toml"),
        "[federation]\nenabled = false\n",
    )
    .expect("disabled");
    assert_eq!(service.acquire(&other, true), Ok(Acquisition::Disabled));
}

/// GET `path`, returning the status and the `kairo-acquisition` header.
async fn acquisition_header(socket: &Path, path: &str) -> (u16, Option<String>) {
    let stream = UnixStream::connect(socket).await.expect("connect");
    let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .expect("handshake");
    let conn = tokio::spawn(async move {
        let _ = conn.await;
    });
    let request = Request::builder()
        .method("GET")
        .uri(path)
        .header("host", "daemon")
        .body(Full::new(Bytes::new()))
        .expect("request");
    let response = sender.send_request(request).await.expect("send");
    let status = response.status().as_u16();
    let header = response
        .headers()
        .get("kairo-acquisition")
        .map(|value| value.to_str().expect("ascii").to_owned());
    drop(response);
    drop(sender);
    let _ = conn.await;
    (status, header)
}

#[tokio::test]
async fn reads_of_missing_objects_queue_an_acquisition_only_when_opted_in() {
    let (dir, _store, _object) = store();
    let daemon = spawn(dir.path()).await;
    let missing = ObjectId::from_sha256_digest([5; 32]);
    let object_path = format!("/api/v1/objects/{missing}");
    let verify_path = format!("/api/v1/verify-object/{missing}");

    // Off by default: a plain 404.
    assert_eq!(
        acquisition_header(&daemon.socket, &object_path).await,
        (404, None)
    );

    std::fs::write(
        dir.path().join("config.toml"),
        "[federation.sync]\ninterval_secs = 3600\ninbox_secs = 1\n\n\
         [federation.on_demand]\non_read = true\ncooldown_secs = 3600\n\n\
         [[federation.peers]]\nurl = \"http://127.0.0.1:1\"\n",
    )
    .expect("config");
    assert_eq!(
        acquisition_header(&daemon.socket, &object_path).await,
        (404, Some("queued".to_owned()))
    );
    until(&daemon.socket, |status| {
        status["runs"].as_array().is_some_and(|runs| {
            runs.iter().any(|run| {
                run["kind"] == "fetch"
                    && run["follow"] == format!("object:{missing}")
                    && run["result"].is_string()
            })
        })
    })
    .await;
    // The attempt failed (the peer is down); reads now wait out the
    // cooldown instead of retrying.
    assert_eq!(
        acquisition_header(&daemon.socket, &verify_path).await,
        (404, Some("cooling_down".to_owned()))
    );
    daemon.stop().await;
}
