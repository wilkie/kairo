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
use flate2 as _;
use kairo_core::{KairoRef, ObjectId, StatementId, Timestamp};
use kairo_federation_core::{FedErrorCode, NodeDocument, NodeDocumentError};
use kairo_federation_http::client::{
    pull_object, IncompleteChain, Limits, PullError, PullOptions, RecordRejection,
};
use kairo_statement::{ActorKeyRevocationBody, ChainScope, ObjectVersionTagBody, SemverVersion};
use kairo_store::{
    ActorStore, BlobStore, BranchResolver, FilesystemStore, ObjectStore, ProvenanceStore,
    StatementStore,
};
use serde_json::Value;
use tempfile::TempDir;

use common::{node_key, seed, sign, sign_chained, Seeded, Served};

// Crates the lib or `common` depends on that this target doesn't name.
use http_body as _;
use http_body_util as _;
use kairo_config as _;
use kairo_federation_core as _;
use kairo_git as _;
use kairo_keystore as _;
use kairo_object as _;
use kairo_test_support as _;
use serde as _;
use sha2 as _;
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
    Served::spawn(|base_url| common::node_at(seeded, base_url).router())
}

/// Serve `seeded`, letting `tamper` replace any response body: it gets
/// the request path and the real body, and a `Some` is served instead,
/// with status 200.
fn serve_tampered<F>(seeded: &Seeded, tamper: F) -> Served
where
    F: Fn(&str, &[u8]) -> Option<Vec<u8>> + Send + Sync + 'static,
{
    let tamper = Arc::new(tamper);
    Served::spawn(|base_url| tamper_router(common::node_at(seeded, base_url).router(), tamper))
}

fn tamper_router<F>(router: Router, tamper: Arc<F>) -> Router
where
    F: Fn(&str, &[u8]) -> Option<Vec<u8>> + Send + Sync + 'static,
{
    router.layer(middleware::from_fn(move |request: Request, next: Next| {
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
    }))
}

fn pull(
    store: &FilesystemStore,
    served: &Served,
    object: &ObjectId,
) -> Result<kairo_federation_http::client::PullReport, PullError> {
    pull_object(store, &served.base_url, object, &PullOptions::default())
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
    assert!(report.flagged.is_empty());
    // The node actor, alice and bob, the genesis, four object
    // statements, three authority statements, alice's rotation, and
    // the manifest blob.
    assert_eq!(report.fetched, 1 + 2 + 1 + 4 + 3 + 1 + 1);
    local
        .get_actor(&seeded.node_actor)
        .expect("the node actor comes along to check the document");

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
fn statements_under_a_revoked_key_are_stored_and_flagged() {
    let seeded = seed();
    // bob revokes his only key without rotating, then a tag signed
    // with it shows up dated after the revocation. The verifier reports
    // KeyRevoked, a verified fact rather than tampering, so the pull
    // stores it like the origin does and flags it; read-time
    // verification is what reports it invalid.
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
            .flagged
            .iter()
            .map(|f| f.record.clone())
            .collect::<Vec<_>>(),
        vec![KairoRef::Statement(stale_tag.clone())]
    );
    assert_eq!(
        local.get_statement_bytes(&stale_tag).map(|b| json(&b)).ok(),
        seeded
            .store
            .get_statement_bytes(&stale_tag)
            .map(|b| json(&b))
            .ok(),
        "stored, same as the origin"
    );
    assert!(
        local.get_statement_bytes(&revocation).is_ok(),
        "revocation stored"
    );
    assert_eq!(
        local
            .get_provenance(&KairoRef::Statement(stale_tag))
            .expect("provenance")
            .len(),
        1
    );
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

/// Serve `seeded`, replacing the statement `target` with
/// `replace(stored bytes)` on every path that carries it: its
/// per-record `GET` and its entry in any statement batch (DECISIONS.md
/// §24). The replacement must be JSON.
fn serve_replacing_statement<F>(seeded: &Seeded, target: &StatementId, replace: F) -> Served
where
    F: Fn(&[u8]) -> Vec<u8> + Send + Sync + 'static,
{
    let path = statements_path(target);
    let target = target.to_string();
    serve_tampered(seeded, move |request_path, bytes| {
        if request_path == path {
            return Some(replace(bytes));
        }
        if request_path != "/federation/v1/statement-batch" {
            return None;
        }
        let mut batch: Value = serde_json::from_slice(bytes).expect("batch json");
        for entry in batch["statements"].as_array_mut()? {
            if entry["id"].as_str() == Some(target.as_str()) {
                let original = serde_json::to_vec(&entry["statement"]).expect("json");
                entry["statement"] =
                    serde_json::from_slice(&replace(&original)).expect("replacement is json");
            }
        }
        Some(serde_json::to_vec(&batch).expect("json"))
    })
}

#[test]
fn tampered_statement_bytes_fail_fixity() {
    let seeded = seed();
    let target = seeded.after_rotation_branch.clone();
    let served = serve_replacing_statement(&seeded, &target, |bytes| {
        let mut value: Value = serde_json::from_slice(bytes).expect("json");
        value["body"]["name"] = Value::from("tampered");
        serde_json::to_vec(&value).expect("json")
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
    let served = serve_replacing_statement(&seeded, &asked, move |_| substitute.clone());
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

/// A served node document changed by `change`, then signed again with
/// the node's key, as an honest node with those fields would serve it.
fn resign(bytes: &[u8], change: impl FnOnce(&mut NodeDocument)) -> Vec<u8> {
    let mut document: NodeDocument = serde_json::from_slice(bytes).expect("document");
    change(&mut document);
    document.sign(&node_key(), Timestamp::now());
    serde_json::to_vec(&document).expect("json")
}

#[test]
fn a_node_document_changed_after_signing_is_refused() {
    let seeded = seed();
    let served = serve_tampered(&seeded, |request_path, bytes| {
        (request_path == "/.well-known/kairo-node").then(|| {
            let mut value: Value = serde_json::from_slice(bytes).expect("json");
            value["supports"] = serde_json::json!(["object-pull"]);
            serde_json::to_vec(&value).expect("json")
        })
    });
    let (_dir, local) = local_store();
    assert!(matches!(
        pull(&local, &served, &seeded.published),
        Err(PullError::NodeDocument(NodeDocumentError::BadSignature))
    ));
    assert!(local.get_object_genesis(&seeded.published).is_err());
}

#[test]
fn a_node_document_signed_by_another_key_is_refused() {
    let seeded = seed();
    let served = serve_tampered(&seeded, |request_path, bytes| {
        (request_path == "/.well-known/kairo-node").then(|| {
            let mut document: NodeDocument = serde_json::from_slice(bytes).expect("document");
            document.sign(
                &kairo_identity::SecretSigningKey::ed25519([77; 32]),
                Timestamp::now(),
            );
            serde_json::to_vec(&document).expect("json")
        })
    });
    let (_dir, local) = local_store();
    assert!(matches!(
        pull(&local, &served, &seeded.published),
        Err(PullError::NodeDocument(
            NodeDocumentError::KeyMismatch { .. }
        ))
    ));
    assert!(local.get_object_genesis(&seeded.published).is_err());
}

#[test]
fn a_node_document_for_another_url_is_refused_before_fetching() {
    let seeded = seed();
    // Signed honestly, for the URL the node really has, and served at
    // a different one (a mirror, or a replay of its document).
    let served =
        Served::spawn(|_| common::node_at(&seeded, "https://archive.example.org").router());
    let (_dir, local) = local_store();
    match pull(&local, &served, &seeded.published) {
        Err(PullError::BaseUrlMismatch { pulled, document }) => {
            assert_eq!(pulled, served.base_url);
            assert_eq!(document, "https://archive.example.org");
        }
        other => panic!("expected BaseUrlMismatch, got {other:?}"),
    }
    assert!(local.get_actor(&seeded.node_actor).is_err());
}

#[test]
fn a_rotated_node_key_is_learned_from_the_node_actor_listing() {
    let seeded = seed();
    let node_signer = kairo_test_support::store::CreatedActor {
        actor_id: seeded.node_actor.clone(),
        signing: node_key(),
        attestation_keys: Vec::new(),
        genesis: seeded.store.get_actor(&seeded.node_actor).expect("genesis"),
    };
    let rotated = kairo_identity::SecretSigningKey::ed25519([78; 32]);
    let rotation = seeded
        .store
        .put_actor_key_rotation(&sign_chained(
            &seeded.store,
            &node_signer,
            &format!("actor:{}", seeded.node_actor),
            1_700_000_500,
            kairo_statement::ActorKeyRotationBody::new(rotated.public_key(), None),
        ))
        .expect("rotate");
    let served = Served::spawn(|base_url| {
        kairo_federation_http::Node::from_store(
            seeded.store.clone(),
            &seeded.node_actor,
            &rotated,
            base_url,
            std::time::Duration::ZERO,
        )
        .expect("node")
        .router()
    });
    let (_dir, local) = local_store();
    let report = pull(&local, &served, &seeded.published).expect("pull");
    assert_eq!(report.peer_actor, seeded.node_actor);
    assert_eq!(
        local.actor_key_event_ids(&seeded.node_actor).expect("keys"),
        vec![rotation]
    );
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
    let result = pull_object(
        &local,
        &served.base_url,
        &seeded.published,
        &PullOptions {
            limits,
            ..PullOptions::default()
        },
    );
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
    let result = pull_object(
        &local,
        &served.base_url,
        &seeded.published,
        &PullOptions {
            limits,
            ..PullOptions::default()
        },
    );
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
        &PullOptions::default(),
    );
    assert!(
        matches!(result, Err(PullError::InvalidPeerUrl { .. })),
        "{result:?}"
    );
    let _ = Timestamp::now();
}

#[test]
fn a_pinned_node_actor_must_match_before_anything_is_fetched() {
    let seeded = seed();
    let served = serve(&seeded);
    let (_dir, local) = local_store();

    let wrong = PullOptions {
        expected_node_actor: Some(seeded.alice.clone()),
        ..PullOptions::default()
    };
    match pull_object(&local, &served.base_url, &seeded.published, &wrong) {
        Err(PullError::NodeActorMismatch { expected, claimed }) => {
            assert_eq!(expected, seeded.alice);
            assert_eq!(claimed, seeded.node_actor);
        }
        other => panic!("expected NodeActorMismatch, got {other:?}"),
    }
    assert!(local.get_actor(&seeded.alice).is_err(), "nothing stored");
    assert!(local.get_object_genesis(&seeded.published).is_err());

    let pinned = PullOptions {
        expected_node_actor: Some(seeded.node_actor.clone()),
        ..PullOptions::default()
    };
    pull_object(&local, &served.base_url, &seeded.published, &pinned).expect("pinned pull");
}

#[test]
fn revisions_no_ref_reaches_stay_on_the_origin() {
    // Git-like closure (DECISIONS.md §20): a revision signed on top of
    // the published history, but not yet pointed at by any branch or
    // tag, is like a dangling commit and does not federate.
    let seeded = seed();
    let parent = seeded
        .statements
        .iter()
        .find_map(|id| seeded.store.get_object_revision(id).ok())
        .expect("a revision");
    let dangling = seeded
        .store
        .put_object_revision(&sign(
            &seeded.alice_actor,
            &format!("object:{}", seeded.published),
            1_700_000_700,
            kairo_statement::ObjectRevisionBody::new(
                seeded.published.clone(),
                kairo_statement::RevisionId::new(format!("git:sha1:{}", "9".repeat(40))),
                vec![parent.unsigned().body().revision().clone()],
                seeded.manifest.clone(),
                true,
            ),
        ))
        .expect("put dangling revision");

    let served = serve(&seeded);
    let (_dir, local) = local_store();
    pull(&local, &served, &seeded.published).expect("pull");
    assert!(
        local.get_statement_bytes(&dangling).is_err(),
        "not federated"
    );
    // Everything the refs reach still arrives.
    for id in &seeded.statements {
        assert!(local.get_statement_bytes(id).is_ok(), "{id}");
    }
}

// --- actor chains (DECISIONS.md §23) ----------------------------------

#[test]
fn a_complete_pull_reports_no_incomplete_chains() {
    let seeded = seed();
    let served = serve(&seeded);
    let (_dir, local) = local_store();
    let report = pull(&local, &served, &seeded.published).expect("pull");
    assert!(report.incomplete.is_empty(), "{:?}", report.incomplete);
    // alice's after-rotation branch is chained after her revocation.
    let branch = local
        .get_object_branch(&seeded.after_rotation_branch)
        .expect("branch");
    assert_eq!(
        branch.unsigned().previous().ids(),
        std::slice::from_ref(&seeded.revocation)
    );
}

/// The listing a peer serves, without alice's revocation of carol's
/// grant: the one record a peer would want to hide.
fn serve_withholding_the_revocation(seeded: &Seeded) -> Served {
    let path = format!("/federation/v1/objects/{}", seeded.published);
    let revocation = seeded.revocation.to_string();
    serve_tampered(seeded, move |request_path, bytes| {
        (request_path == path).then(|| {
            let mut value: Value = serde_json::from_slice(bytes).expect("json");
            if let Some(authority) = value["authority"].as_array_mut() {
                authority.retain(|id| id.as_str() != Some(revocation.as_str()));
            }
            serde_json::to_vec(&value).expect("json")
        })
    })
}

#[test]
fn a_withheld_revocation_is_a_reported_gap() {
    let seeded = seed();
    let served = serve_withholding_the_revocation(&seeded);
    let (_dir, local) = local_store();
    let report = pull(&local, &served, &seeded.published).expect("pull still succeeds");

    assert!(local.get_statement_bytes(&seeded.revocation).is_err());
    assert_eq!(
        report.incomplete,
        vec![IncompleteChain {
            signer: seeded.alice.clone(),
            scope: ChainScope::Object(seeded.published.clone()),
            missing: vec![seeded.revocation.clone()],
        }]
    );
}

#[test]
fn require_complete_fails_on_a_gap_but_keeps_what_verified() {
    let seeded = seed();
    let served = serve_withholding_the_revocation(&seeded);
    let (_dir, local) = local_store();
    let options = PullOptions {
        require_complete: true,
        ..PullOptions::default()
    };
    match pull_object(&local, &served.base_url, &seeded.published, &options) {
        Err(PullError::IncompleteChains { chains }) => {
            assert_eq!(chains.len(), 1);
            assert_eq!(chains[0].missing, vec![seeded.revocation.clone()]);
        }
        other => panic!("expected IncompleteChains, got {other:?}"),
    }
    assert!(local
        .get_statement_bytes(&seeded.after_rotation_branch)
        .is_ok());

    // The same pull from an honest peer completes the chain.
    let honest = serve(&seeded);
    let report =
        pull_object(&local, &honest.base_url, &seeded.published, &options).expect("complete pull");
    assert!(report.incomplete.is_empty());
}

// --- statement batch (DECISIONS.md §24) -------------------------------

/// Counts of per-record statement `GET`s and batch `POST`s a pull made.
#[derive(Default)]
struct Requests {
    per_record: std::sync::atomic::AtomicUsize,
    batches: std::sync::atomic::AtomicUsize,
}

fn count(requests: &Requests, path: &str) {
    use std::sync::atomic::Ordering;
    if path.starts_with("/federation/v1/statements/") {
        requests.per_record.fetch_add(1, Ordering::SeqCst);
    } else if path == "/federation/v1/statement-batch" {
        requests.batches.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn a_pull_fetches_statements_in_batches() {
    use std::sync::atomic::Ordering;
    let seeded = seed();
    let requests = Arc::new(Requests::default());
    let seen = Arc::clone(&requests);
    let served = serve_tampered(&seeded, move |path, _| {
        count(&seen, path);
        None
    });
    let (_dir, local) = local_store();
    let report = pull(&local, &served, &seeded.published).expect("pull");
    assert!(report.incomplete.is_empty());
    assert_eq!(requests.per_record.load(Ordering::SeqCst), 0);
    assert!(requests.batches.load(Ordering::SeqCst) >= 1);
    for id in seeded.statements.iter().chain(&seeded.authority) {
        assert!(local.get_statement_bytes(id).is_ok(), "{id}");
    }
}

#[test]
fn a_node_without_batch_support_is_pulled_record_by_record() {
    use std::sync::atomic::Ordering;
    let seeded = seed();
    let requests = Arc::new(Requests::default());
    let seen = Arc::clone(&requests);
    let served = serve_tampered(&seeded, move |path, bytes| {
        count(&seen, path);
        (path == "/.well-known/kairo-node").then(|| {
            resign(bytes, |document| {
                document.supports = vec!["object-pull".to_owned()];
            })
        })
    });
    let (_dir, local) = local_store();
    let report = pull(&local, &served, &seeded.published).expect("pull");
    assert!(report.incomplete.is_empty());
    assert_eq!(requests.batches.load(Ordering::SeqCst), 0);
    assert!(requests.per_record.load(Ordering::SeqCst) > 0);
}

#[test]
fn a_statement_a_batch_leaves_out_is_fetched_on_its_own() {
    use std::sync::atomic::Ordering;
    let seeded = seed();
    let left_out = seeded.after_rotation_branch.to_string();
    let requests = Arc::new(Requests::default());
    let seen = Arc::clone(&requests);
    let served = serve_tampered(&seeded, move |path, bytes| {
        count(&seen, path);
        (path == "/federation/v1/statement-batch").then(|| {
            let mut value: Value = serde_json::from_slice(bytes).expect("json");
            if let Some(statements) = value["statements"].as_array_mut() {
                statements.retain(|entry| entry["id"].as_str() != Some(left_out.as_str()));
            }
            serde_json::to_vec(&value).expect("json")
        })
    });
    let (_dir, local) = local_store();
    pull(&local, &served, &seeded.published).expect("pull");
    assert!(local
        .get_statement_bytes(&seeded.after_rotation_branch)
        .is_ok());
    assert_eq!(requests.per_record.load(Ordering::SeqCst), 1);
}

// --- inventories (DECISIONS.md §29) -----------------------------------

#[test]
fn a_peers_inventory_is_fetched_and_verified() {
    let seeded = seed();
    let served = serve(&seeded);
    let (_dir, local) = local_store();
    let fetched = kairo_federation_http::client::fetch_inventory(
        &local,
        &served.base_url,
        &PullOptions::default(),
    )
    .expect("inventory");
    assert_eq!(fetched.inventory.node_actor, seeded.node_actor);
    assert_eq!(fetched.inventory.objects, vec![seeded.published.clone()]);
    assert_eq!(fetched.peer_url, served.base_url);
    assert!(
        local.get_actor(&seeded.node_actor).is_ok(),
        "the node is authenticated first"
    );
}

#[test]
fn an_inventory_changed_after_signing_or_for_another_node_is_refused() {
    let seeded = seed();
    let extra = seeded.unpublished.to_string();
    let added = serve_tampered(&seeded, move |path, bytes| {
        (path == "/federation/v1/inventory").then(|| {
            let mut value: Value = serde_json::from_slice(bytes).expect("json");
            let mut objects: Vec<String> = value["objects"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_owned())
                .collect();
            objects.push(extra.clone());
            objects.sort();
            value["objects"] = serde_json::json!(objects);
            serde_json::to_vec(&value).expect("json")
        })
    });
    let (_dir, local) = local_store();
    assert!(matches!(
        kairo_federation_http::client::fetch_inventory(
            &local,
            &added.base_url,
            &PullOptions::default()
        ),
        Err(PullError::NodeDocument(NodeDocumentError::BadSignature))
    ));

    let alice = seeded.alice.to_string();
    let other_node = serve_tampered(&seeded, move |path, bytes| {
        (path == "/federation/v1/inventory").then(|| {
            let mut value: Value = serde_json::from_slice(bytes).expect("json");
            value["node_actor"] = Value::from(alice.clone());
            serde_json::to_vec(&value).expect("json")
        })
    });
    let (_dir, local) = local_store();
    assert!(matches!(
        kairo_federation_http::client::fetch_inventory(
            &local,
            &other_node.base_url,
            &PullOptions::default()
        ),
        Err(PullError::NodeActorMismatch { .. })
    ));
}
