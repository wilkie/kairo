//! Uploads (`specs/DECISIONS.md` §26 sub-decisions 6 and 8): the plan
//! keeps an upload to the delta, the receiver checks everything but the
//! signature before reading the body, and the closure bundle ingests
//! through the pull verifier.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod common;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use kairo_bundle::closure::unpack_tar;
use kairo_core::{KairoRef, StatementId, Timestamp};
use kairo_federation_core::{
    FedErrorCode, FedErrorEnvelope, NodeAuthorization, NodeRequest, RequestBody,
};
use kairo_federation_http::client::{
    closure_inventory, ingest_closure_bundle, push, NodeSigner, PullError, PullOptions,
    PushReceipt, RecordRejection,
};
use kairo_statement::ObjectBranchBody;
use kairo_store::{
    ActorStore, FilesystemStore, InboxItem, InboxStore, ProvenanceStore, StatementStore,
};
use tempfile::TempDir;
use tower::ServiceExt;

use common::{node_key, receiver, seed, sign_chained, Receiver, Seeded};

// Crates the lib or `common` depends on that this target doesn't name.
use flate2 as _;
use http_body as _;
use http_body_util as _;
use kairo_config as _;
use kairo_git as _;
use kairo_identity as _;
use kairo_keystore as _;
use kairo_object as _;
use serde as _;
use sha2 as _;
use tokio as _;
use tracing as _;
use tracing_subscriber as _;
use ureq as _;

fn push_from(seeded: &Seeded, to: &Receiver) -> Result<PushReceipt, PullError> {
    push(
        &seeded.store,
        &to.served.base_url,
        &seeded.published,
        NodeSigner {
            node: &seeded.node_actor,
            key: &node_key(),
        },
        &PullOptions::default(),
    )
}

/// A trusted receiver that accepts the seeded object.
fn trusting_receiver(seeded: &Seeded, max_push_bytes: Option<u64>) -> Receiver {
    let receiver = receiver(&seeded.published, max_push_bytes);
    receiver.trusts(&seeded.store, &seeded.node_actor);
    receiver
}

/// A revision of the published object, to point a new branch at.
fn a_revision(seeded: &Seeded) -> StatementId {
    seeded
        .statements
        .iter()
        .find(|id| seeded.store.get_object_revision(id).is_ok())
        .expect("a revision")
        .clone()
}

#[test]
fn a_first_push_sends_the_closure_and_ingests_as_a_pull_would() {
    let seeded = seed();
    let receiver = trusting_receiver(&seeded, None);
    let receipt = push_from(&seeded, &receiver).expect("push");
    assert_eq!(receipt.peer_actor, receiver.signer.actor_id);
    let inventory = closure_inventory(&seeded.store, &seeded.published).expect("inventory");
    let statements = inventory.statement_ids().count();
    assert!(receipt.sent.genesis);
    assert_eq!(receipt.sent.statements, statements);
    assert_eq!(receipt.sent.blobs, inventory.blobs.len());
    // alice and bob, but not the pusher, which the receiver learned.
    assert_eq!(receipt.sent.actors, inventory.actors.len());
    assert!(receipt.entry.is_some());

    let reports = receiver.receive();
    assert_eq!(reports.len(), 1);
    let report = reports.into_iter().next().unwrap().expect("ingest");
    assert!(report.incomplete.is_empty(), "{:?}", report.incomplete);
    for id in seeded.statements.iter().chain(&seeded.authority) {
        assert!(receiver.store.get_statement_bytes(id).is_ok(), "{id}");
        let provenance = receiver
            .store
            .get_provenance(&KairoRef::Statement(id.clone()))
            .expect("provenance");
        assert_eq!(
            provenance[0].peer_url,
            format!("push:{}", seeded.node_actor)
        );
    }
    assert!(receiver
        .store
        .get_statement_bytes(&seeded.alice_rotation)
        .is_ok());
    // Nothing outside the closure came along.
    assert!(receiver.store.get_statement_bytes(&seeded.trust).is_err());
}

#[test]
fn later_pushes_send_only_the_delta() {
    let seeded = seed();
    let receiver = trusting_receiver(&seeded, None);
    push_from(&seeded, &receiver).expect("push");
    receiver.receive();

    let again = push_from(&seeded, &receiver).expect("push again");
    assert_eq!(
        again.entry, None,
        "nothing to send, so nothing uploaded: {again:?}"
    );
    assert_eq!(again.sent, Default::default());
    assert!(receiver.store.list_inbox().expect("inbox").is_empty());

    let branch = seeded
        .store
        .put_object_branch(&sign_chained(
            &seeded.store,
            &seeded.alice_rotated,
            &format!("object:{}", seeded.published),
            1_700_000_900,
            ObjectBranchBody::new(seeded.published.clone(), "later", a_revision(&seeded), None),
        ))
        .expect("new branch");
    let delta = push_from(&seeded, &receiver).expect("push the delta");
    assert!(!delta.sent.genesis);
    assert_eq!(
        (delta.sent.statements, delta.sent.actors, delta.sent.blobs),
        (1, 0, 0)
    );
    let report = receiver
        .receive()
        .into_iter()
        .next()
        .unwrap()
        .expect("ingest");
    assert_eq!(report.fetched, 1);
    assert!(receiver.store.get_statement_bytes(&branch).is_ok());
}

#[test]
fn an_untrusted_or_unaccepted_push_is_refused_at_the_plan() {
    let seeded = seed();
    let receiver = receiver(&seeded.published, None);
    // Learned but not trusted.
    receiver
        .store
        .put_actor(&seeded.store.get_actor(&seeded.node_actor).unwrap())
        .unwrap();
    match push_from(&seeded, &receiver) {
        Err(PullError::Status {
            status: 403,
            code: Some(FedErrorCode::FedNotTrusted),
            ..
        }) => {}
        other => panic!("expected 403 fed_not_trusted, got {other:?}"),
    }

    let other = common::receiver(&seeded.unpublished, None);
    other.trusts(&seeded.store, &seeded.node_actor);
    match push_from(&seeded, &other) {
        Err(PullError::Status {
            status: 403,
            code: Some(FedErrorCode::FedNotAccepted),
            ..
        }) => {}
        other => panic!("expected 403 fed_not_accepted, got {other:?}"),
    }
    assert!(other.store.list_inbox().expect("inbox").is_empty());
}

#[test]
fn an_upload_over_max_push_bytes_is_refused_and_leaves_nothing() {
    let seeded = seed();
    let receiver = trusting_receiver(&seeded, Some(1024));
    match push_from(&seeded, &receiver) {
        Err(PullError::Status {
            status: 413,
            code: Some(FedErrorCode::FedTooLarge),
            ..
        }) => {}
        other => panic!("expected 413, got {other:?}"),
    }
    assert!(receiver.store.list_inbox().expect("inbox").is_empty());
    let inbox = receiver.store.root().join("federation/inbox");
    let leftovers: Vec<_> = std::fs::read_dir(&inbox)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|name| name.ends_with(".tar"))
                .collect()
        })
        .unwrap_or_default();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[tokio::test]
async fn an_upload_whose_body_does_not_match_its_signature_is_deleted() {
    let seeded = seed();
    let receiver_store = TempDir::new().expect("tempdir");
    let store = FilesystemStore::open(receiver_store.path()).expect("store");
    let receiving = common::node_actor_with(&store, 220);
    store
        .put_actor(&seeded.store.get_actor(&seeded.node_actor).unwrap())
        .unwrap();
    common::trust_push(&store, &receiving, &seeded.node_actor);
    let router = kairo_federation_http::Node::from_store(
        store.clone(),
        &receiving.actor_id,
        &receiving.signing,
        common::BASE_URL,
        std::time::Duration::ZERO,
    )
    .expect("node")
    .with_federation_config(kairo_config::FederationConfig {
        accept: vec![kairo_config::AcceptConfig {
            object: seeded.published.clone(),
            from: None,
            publish: false,
        }],
        ..Default::default()
    })
    .router();

    let path = format!("/federation/v1/push?object={}", seeded.published);
    let request = NodeRequest {
        method: "POST",
        receiver_base_url: common::BASE_URL,
        path_and_query: &path,
        body: RequestBody::Bytes(b"what was signed"),
    };
    let authorization = NodeAuthorization::sign(
        &request,
        seeded.node_actor.clone(),
        &node_key(),
        Timestamp::now().seconds(),
        [5; 16],
    )
    .format();
    let response = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(&path)
                .header(header::AUTHORIZATION, authorization)
                .body(Body::from("something else"))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body = http_body_util::BodyExt::collect(response.into_body())
        .await
        .expect("body")
        .to_bytes();
    let envelope: FedErrorEnvelope = serde_json::from_slice(&body).expect("envelope");
    assert_eq!(envelope.error.code, FedErrorCode::FedUnauthenticated);
    assert!(store.list_inbox().expect("inbox").is_empty());
    let incoming = store
        .inbox_incoming_path("05050505050505050505050505050505")
        .unwrap();
    assert!(!incoming.exists(), "the unverified body is deleted");
}

#[test]
fn a_delta_bundle_ingests_only_where_the_rest_is_held() {
    let seeded = seed();
    let receiver = trusting_receiver(&seeded, None);
    push_from(&seeded, &receiver).expect("push");
    receiver.receive();
    seeded
        .store
        .put_object_branch(&sign_chained(
            &seeded.store,
            &seeded.alice_rotated,
            &format!("object:{}", seeded.published),
            1_700_000_900,
            ObjectBranchBody::new(seeded.published.clone(), "later", a_revision(&seeded), None),
        ))
        .expect("new branch");
    push_from(&seeded, &receiver).expect("push the delta");
    let entry = receiver.store.list_inbox().expect("inbox").remove(0);
    let InboxItem::Upload(upload, tar) = entry.item else {
        panic!("expected an upload");
    };

    // Elsewhere, the delta names records it doesn't carry.
    let elsewhere = TempDir::new().expect("tempdir");
    let other = FilesystemStore::open(elsewhere.path()).expect("store");
    other
        .put_actor(&seeded.store.get_actor(&seeded.node_actor).unwrap())
        .unwrap();
    let unpacked = TempDir::new().expect("tempdir");
    let bundle = unpack_tar(std::fs::File::open(&tar).unwrap(), unpacked.path()).expect("unpack");
    match ingest_closure_bundle(&other, bundle, &upload.from, &PullOptions::default()) {
        Err(PullError::NotInBundle { .. }) => {}
        other => panic!("expected NotInBundle, got {other:?}"),
    }

    // A record changed after the pusher sent it fails fixity.
    let unpacked = TempDir::new().expect("tempdir");
    let bundle = unpack_tar(std::fs::File::open(&tar).unwrap(), unpacked.path()).expect("unpack");
    let statements = unpacked.path().join("statements");
    let file = std::fs::read_dir(&statements)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    value["body"]["name"] = serde_json::Value::from("tampered");
    std::fs::write(&file, serde_json::to_vec(&value).unwrap()).unwrap();
    match ingest_closure_bundle(
        &receiver.store,
        bundle,
        &upload.from,
        &PullOptions::default(),
    ) {
        Err(PullError::Record {
            reason: RecordRejection::Fixity { .. },
            ..
        }) => {}
        other => panic!("expected a fixity failure, got {other:?}"),
    }
}
