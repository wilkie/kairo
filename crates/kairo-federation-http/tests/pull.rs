//! End-to-end pull tests (`specs/PHASE_2_FEDERATION.md` slice 3): a
//! real server on a loopback port, the blocking client pulling into a
//! second store. Adversarial cases wrap the real router in a
//! middleware that tampers with one response.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod common;

use std::sync::Arc;

use axum::body::Body;
use axum::extract::Request;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::Router;
use kairo_core::{KairoRef, ObjectId, StatementId, Timestamp};
use kairo_federation_core::FedErrorCode;
use kairo_federation_http::client::{pull_object, Limits, PullError, RecordRejection};
use kairo_statement::{ActorKeyRevocationBody, ObjectVersionTagBody, SemverVersion};
use kairo_store::{
    ActorStore, BlobStore, BranchResolver, FilesystemStore, ObjectStore, ProvenanceStore,
    StatementStore,
};
use serde_json::Value;
use tempfile::TempDir;

use common::{seed, sign, Seeded, Served};

// Crates the lib or `common` depends on that this target doesn't name.
use http_body as _;
use http_body_util as _;
use kairo_federation_core as _;
use kairo_git as _;
use kairo_object as _;
use kairo_test_support as _;
use serde as _;
use tokio as _;
use tower as _;
use tracing as _;
use tracing_subscriber as _;
use ureq as _;

fn local_store() -> (TempDir, FilesystemStore) {
    let dir = TempDir::new().expect("tempdir");
    let store = FilesystemStore::open(dir.path()).expect("open");
    (dir, store)
}

fn serve(seeded: &Seeded) -> Served {
    Served::spawn(common::node(seeded).router())
}

/// Serve `seeded`, letting `tamper` replace any response body: it gets
/// the request path and the real body, and a `Some` is served instead,
/// with status 200.
fn serve_tampered<F>(seeded: &Seeded, tamper: F) -> Served
where
    F: Fn(&str, &[u8]) -> Option<Vec<u8>> + Send + Sync + 'static,
{
    let tamper = Arc::new(tamper);
    let router: Router = common::node(seeded).router().layer(middleware::from_fn(
        move |request: Request, next: Next| {
            let tamper = Arc::clone(&tamper);
            async move {
                let path = request.uri().path().to_owned();
                let response = next.run(request).await;
                let (parts, body) = response.into_parts();
                let bytes = axum::body::to_bytes(body, usize::MAX)
                    .await
                    .expect("body")
                    .to_vec();
                let mut parts = parts;
                match tamper(&path, &bytes) {
                    Some(replaced) => {
                        parts.status = axum::http::StatusCode::OK;
                        parts.headers.remove(axum::http::header::CONTENT_LENGTH);
                        Response::from_parts(parts, Body::from(replaced))
                    }
                    None => Response::from_parts(parts, Body::from(bytes)),
                }
            }
        },
    ));
    Served::spawn(router)
}

fn pull(
    store: &FilesystemStore,
    served: &Served,
    object: &ObjectId,
) -> Result<kairo_federation_http::client::PullReport, PullError> {
    pull_object(store, &served.base_url, object, &Limits::default())
}

fn json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).expect("json")
}

fn statements_path(id: &StatementId) -> String {
    format!("/federation/v1/statements/{id}")
}

// --- happy path -------------------------------------------------------

#[test]
fn pull_copies_the_published_closure() {
    let seeded = seed();
    let served = serve(&seeded);
    let (_dir, local) = local_store();

    let report = pull(&local, &served, &seeded.published).expect("pull");
    assert_eq!(report.peer_url, served.base_url);
    assert_eq!(report.peer_actor, seeded.node_actor);
    assert_eq!(report.already_held, 0);
    assert!(report.skipped.is_empty());
    // alice and bob, the genesis, four object statements, three
    // authority statements, alice's rotation, and the manifest blob.
    assert_eq!(report.fetched, 2 + 1 + 4 + 3 + 1 + 1);

    local
        .get_object_genesis(&seeded.published)
        .expect("genesis");
    for id in seeded
        .statements
        .iter()
        .chain(&seeded.authority)
        .chain(std::iter::once(&seeded.alice_rotation))
    {
        let pulled = local.get_statement_bytes(id).expect("pulled statement");
        let origin = seeded.store.get_statement_bytes(id).expect("origin");
        assert_eq!(json(&pulled), json(&origin), "{id}");
    }
    assert_eq!(
        local.get_blob(&seeded.manifest).expect("blob"),
        seeded
            .store
            .get_blob(&seeded.manifest)
            .expect("origin blob")
    );
    assert_eq!(
        local.list_branches(&seeded.published).expect("branches"),
        seeded
            .store
            .list_branches(&seeded.published)
            .expect("origin branches"),
        "branch heads resolve the same on both nodes"
    );
    assert_eq!(
        local.actor_key_event_ids(&seeded.alice).expect("keys"),
        vec![seeded.alice_rotation.clone()]
    );

    // Nothing outside the closure came along.
    assert!(local.get_statement_bytes(&seeded.trust).is_err());
    for id in &seeded.unpublished_statements {
        assert!(local.get_statement_bytes(id).is_err(), "{id}");
    }
    assert!(
        local.get_actor(&seeded.carol).is_err(),
        "carol signed nothing"
    );

    // Provenance names the peer.
    let entries = local
        .get_provenance(&KairoRef::Statement(seeded.after_rotation_branch.clone()))
        .expect("provenance");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].peer_url, served.base_url);
    assert_eq!(entries[0].peer_actor, seeded.node_actor);
    for record in [
        KairoRef::Object(seeded.published.clone()),
        KairoRef::Actor(seeded.alice.clone()),
        KairoRef::Blob(seeded.manifest.clone()),
    ] {
        assert_eq!(
            local.get_provenance(&record).expect("provenance").len(),
            1,
            "{record}"
        );
    }
}

#[test]
fn re_pull_fetches_nothing_and_keeps_first_provenance() {
    let seeded = seed();
    let served = serve(&seeded);
    let (_dir, local) = local_store();
    let first = pull(&local, &served, &seeded.published).expect("first pull");
    let record = KairoRef::Statement(seeded.alice_rotation.clone());
    let before = local.get_provenance(&record).expect("provenance");

    let second = pull(&local, &served, &seeded.published).expect("second pull");
    assert_eq!(second.fetched, 0);
    assert_eq!(second.already_held, first.fetched);
    assert_eq!(local.get_provenance(&record).expect("provenance"), before);
}

#[test]
fn a_second_peer_appends_provenance() {
    let seeded = seed();
    let first_peer = serve(&seeded);
    let second_peer = serve(&seeded);
    let (_dir, local) = local_store();
    pull(&local, &first_peer, &seeded.published).expect("first peer");
    let report = pull(&local, &second_peer, &seeded.published).expect("second peer");
    assert_eq!(report.fetched, 0);

    let entries = local
        .get_provenance(&KairoRef::Object(seeded.published.clone()))
        .expect("provenance");
    let urls: Vec<_> = entries.iter().map(|entry| entry.peer_url.clone()).collect();
    assert_eq!(
        urls,
        vec![first_peer.base_url.clone(), second_peer.base_url.clone()]
    );
}

#[test]
fn statements_under_a_revoked_key_are_skipped_not_stored() {
    let seeded = seed();
    // bob revokes his only key without rotating, then a tag signed
    // with it shows up dated after the revocation: the verifier
    // reports KeyRevoked, a verified fact rather than tampering.
    let bob = &seeded.bob_actor;
    let revocation = seeded
        .store
        .put_actor_key_revocation(&sign(
            bob,
            &format!("actor:{}", seeded.bob),
            1_700_000_050,
            ActorKeyRevocationBody::new(bob.signing.public_key().key_id(), false, None),
        ))
        .expect("put key revocation");
    let revision = seeded
        .statements
        .iter()
        .find(|id| seeded.store.get_object_revision(id).is_ok())
        .expect("a revision")
        .clone();
    let stale_tag = seeded
        .store
        .put_object_version_tag(&sign(
            bob,
            &format!("object:{}", seeded.published),
            1_700_000_060,
            ObjectVersionTagBody::new(
                seeded.published.clone(),
                SemverVersion::parse("1.0.0").expect("semver"),
                Some(revision),
                None,
            )
            .expect("tag body"),
        ))
        .expect("put stale tag");

    let served = serve(&seeded);
    let (_dir, local) = local_store();
    let report = pull(&local, &served, &seeded.published).expect("pull succeeds");
    assert_eq!(
        report
            .skipped
            .iter()
            .map(|s| s.record.clone())
            .collect::<Vec<_>>(),
        vec![KairoRef::Statement(stale_tag.clone())]
    );
    assert!(local.get_statement_bytes(&stale_tag).is_err(), "not stored");
    assert!(
        local.get_statement_bytes(&revocation).is_ok(),
        "revocation stored"
    );
    assert!(local
        .get_provenance(&KairoRef::Statement(stale_tag))
        .expect("provenance")
        .is_empty());
}

// --- refusals ---------------------------------------------------------

fn record_error(
    result: Result<kairo_federation_http::client::PullReport, PullError>,
) -> (KairoRef, RecordRejection) {
    match result {
        Err(PullError::Record { record, reason }) => (record, reason),
        other => panic!("expected a record rejection, got {other:?}"),
    }
}

#[test]
fn tampered_statement_bytes_fail_fixity() {
    let seeded = seed();
    let target = seeded.after_rotation_branch.clone();
    let path = statements_path(&target);
    let served = serve_tampered(&seeded, move |request_path, bytes| {
        (request_path == path).then(|| {
            let mut value: Value = serde_json::from_slice(bytes).expect("json");
            value["body"]["name"] = Value::from("tampered");
            serde_json::to_vec(&value).expect("json")
        })
    });
    let (_dir, local) = local_store();
    let (record, reason) = record_error(pull(&local, &served, &seeded.published));
    assert_eq!(record, KairoRef::Statement(target.clone()));
    assert!(
        matches!(reason, RecordRejection::Fixity { .. }),
        "{reason:?}"
    );
    assert!(local.get_statement_bytes(&target).is_err());
    // Records ingested before the failure stay.
    assert!(local.get_actor(&seeded.alice).is_ok());
}

#[test]
fn a_different_valid_statement_under_the_requested_id_fails_fixity() {
    let seeded = seed();
    let asked = seeded.after_rotation_branch.clone();
    let substitute = seeded
        .store
        .get_statement_bytes(
            seeded
                .statements
                .iter()
                .find(|id| **id != asked)
                .expect("another"),
        )
        .expect("bytes");
    let path = statements_path(&asked);
    let served = serve_tampered(&seeded, move |request_path, _| {
        (request_path == path).then(|| substitute.clone())
    });
    let (_dir, local) = local_store();
    let (record, reason) = record_error(pull(&local, &served, &seeded.published));
    assert_eq!(record, KairoRef::Statement(asked));
    assert!(
        matches!(reason, RecordRejection::Fixity { .. }),
        "{reason:?}"
    );
}

#[test]
fn withheld_key_events_make_later_signatures_fail() {
    let seeded = seed();
    let path = format!("/federation/v1/actors/{}", seeded.alice);
    let served = serve_tampered(&seeded, move |request_path, bytes| {
        (request_path == path).then(|| {
            let mut value: Value = serde_json::from_slice(bytes).expect("json");
            value["key_events"] = Value::Array(Vec::new());
            serde_json::to_vec(&value).expect("json")
        })
    });
    let (_dir, local) = local_store();
    let (record, reason) = record_error(pull(&local, &served, &seeded.published));
    assert_eq!(
        record,
        KairoRef::Statement(seeded.after_rotation_branch.clone())
    );
    assert!(
        matches!(reason, RecordRejection::Signature(_)),
        "{reason:?}"
    );
    assert!(local
        .get_statement_bytes(&seeded.after_rotation_branch)
        .is_err());
}

#[test]
fn smuggled_trust_statements_are_refused() {
    let seeded = seed();
    let trust = seeded.trust.clone();
    let trust_bytes = seeded
        .store
        .get_statement_bytes(&trust)
        .expect("trust bytes");
    let listing = format!("/federation/v1/objects/{}", seeded.published);
    let trust_path = statements_path(&trust);
    let trust_id = trust.to_string();
    let served = serve_tampered(&seeded, move |request_path, bytes| {
        if request_path == listing {
            let mut value: Value = serde_json::from_slice(bytes).expect("json");
            value["statements"]
                .as_array_mut()
                .expect("array")
                .push(Value::from(trust_id.clone()));
            Some(serde_json::to_vec(&value).expect("json"))
        } else if request_path == trust_path {
            Some(trust_bytes.clone())
        } else {
            None
        }
    });
    let (_dir, local) = local_store();
    let (record, reason) = record_error(pull(&local, &served, &seeded.published));
    assert_eq!(record, KairoRef::Statement(trust.clone()));
    assert!(
        matches!(reason, RecordRejection::WrongSection(_)),
        "{reason:?}"
    );
    assert!(local.get_statement_bytes(&trust).is_err());
}

#[test]
fn a_node_document_claiming_another_actor_is_refused() {
    let seeded = seed();
    let impostor = seeded.alice.to_string();
    let served = serve_tampered(&seeded, move |request_path, bytes| {
        (request_path == "/.well-known/kairo-node").then(|| {
            let mut value: Value = serde_json::from_slice(bytes).expect("json");
            value["node_actor"] = Value::from(impostor.clone());
            serde_json::to_vec(&value).expect("json")
        })
    });
    let (_dir, local) = local_store();
    assert!(matches!(
        pull(&local, &served, &seeded.published),
        Err(PullError::NodeDocument(_))
    ));
}

#[test]
fn oversize_responses_fail_and_keep_what_was_stored() {
    let seeded = seed();
    let served = serve(&seeded);
    let (_dir, local) = local_store();
    let limits = Limits {
        max_blob_bytes: 8,
        ..Limits::default()
    };
    let result = pull_object(&local, &served.base_url, &seeded.published, &limits);
    assert!(
        matches!(result, Err(PullError::TooLarge { limit: 8, .. })),
        "{result:?}"
    );
    assert!(local.get_blob(&seeded.manifest).is_err());
    // Blobs come last, so every statement is already stored.
    for id in seeded.statements.iter().chain(&seeded.authority) {
        assert!(local.get_statement_bytes(id).is_ok(), "{id}");
    }
}

#[test]
fn closures_over_the_record_limit_are_refused() {
    let seeded = seed();
    let served = serve(&seeded);
    let (_dir, local) = local_store();
    let limits = Limits {
        max_records: 3,
        ..Limits::default()
    };
    let result = pull_object(&local, &served.base_url, &seeded.published, &limits);
    assert!(
        matches!(result, Err(PullError::TooManyRecords { limit: 3 })),
        "{result:?}"
    );
}

#[test]
fn unpublished_objects_are_not_found() {
    let seeded = seed();
    let served = serve(&seeded);
    let (_dir, local) = local_store();
    match pull(&local, &served, &seeded.unpublished) {
        Err(PullError::Status {
            status: 404,
            code: Some(FedErrorCode::FedNotFound),
            ..
        }) => {}
        other => panic!("expected 404, got {other:?}"),
    }
    assert!(local.get_object_genesis(&seeded.unpublished).is_err());
}

#[test]
fn plain_http_to_a_remote_host_is_refused_before_connecting() {
    let (_dir, local) = local_store();
    let object = ObjectId::from_bytes(b"test", b"o");
    let result = pull_object(
        &local,
        "http://archive.example.org",
        &object,
        &Limits::default(),
    );
    assert!(
        matches!(result, Err(PullError::InvalidPeerUrl { .. })),
        "{result:?}"
    );
    let _ = Timestamp::now();
}
