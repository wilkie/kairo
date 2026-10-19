//! Trust transport (`specs/DECISIONS.md` §27): only published
//! (truster, scope) chains are served, a fetch pulls one chain through
//! the pull verifier, and fetched opinions stay their truster's.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod common;

use kairo_core::{ActorId, KairoRef, StatementId, Timestamp};
use kairo_federation_core::{FedErrorCode, PublishedTrust, TrustListing};
use kairo_federation_http::client::{fetch_trust, PullError, PullOptions, RecordRejection};
use kairo_statement::{ActorTrustBody, TrustDecision, TrustScope};
use kairo_store::{
    ActorStore, FilesystemStore, ProvenanceStore, StatementStore, TrustResolver,
    TrustTransportStore,
};
use serde_json::Value;
use tempfile::TempDir;

use common::{seed, sign_chained, Seeded, Served};

// Crates the lib or `common` depends on that this target doesn't name.
use axum as _;
use flate2 as _;
use http_body as _;
use http_body_util as _;
use kairo_bundle as _;
use kairo_config as _;
use kairo_git as _;
use kairo_identity as _;
use kairo_keystore as _;
use kairo_object as _;
use serde as _;
use sha2 as _;
use tokio as _;
use tower as _;
use tracing as _;
use tracing_subscriber as _;
use ureq as _;

/// alice's opinion of `of` in `scope`, signed with her current key.
fn opinion(
    seeded: &Seeded,
    of: &ActorId,
    scope: TrustScope,
    decision: Option<TrustDecision>,
    supersedes: Option<StatementId>,
    seconds: i64,
) -> StatementId {
    seeded
        .store
        .put_actor_trust(&sign_chained(
            &seeded.store,
            &seeded.alice_rotated,
            &format!("actor:{of}"),
            seconds,
            ActorTrustBody::new(of.clone(), scope, decision, None, supersedes).expect("body"),
        ))
        .expect("put trust")
}

struct Chains {
    trusted: StatementId,
    withdrawn: StatementId,
    carol: StatementId,
    pushes: StatementId,
}

fn index() -> TrustScope {
    TrustScope::federation_index()
}

/// alice, in `federation:index`, trusts bob then withdraws and trusts
/// carol; in `federation:push` she trusts carol. Only
/// `federation:index` is published.
fn chains(seeded: &Seeded) -> Chains {
    let trusted = opinion(
        seeded,
        &seeded.bob,
        index(),
        Some(TrustDecision::Trusted),
        None,
        1_700_000_700,
    );
    let withdrawn = opinion(
        seeded,
        &seeded.bob,
        index(),
        None,
        Some(trusted.clone()),
        1_700_000_710,
    );
    let carol = opinion(
        seeded,
        &seeded.carol,
        index(),
        Some(TrustDecision::Trusted),
        None,
        1_700_000_715,
    );
    let pushes = opinion(
        seeded,
        &seeded.carol,
        TrustScope::federation_push(),
        Some(TrustDecision::Trusted),
        None,
        1_700_000_720,
    );
    seeded
        .store
        .publish_trust(&seeded.alice, &index(), Timestamp::now())
        .expect("publish");
    Chains {
        trusted,
        withdrawn,
        carol,
        pushes,
    }
}

fn get(served: &Served, path: &str) -> (u16, Vec<u8>) {
    let mut response = ureq::get(format!("{}{path}", served.base_url))
        .config()
        .http_status_as_error(false)
        .build()
        .call()
        .expect("request");
    let status = response.status().as_u16();
    (status, response.body_mut().read_to_vec().expect("body"))
}

fn local() -> (TempDir, FilesystemStore) {
    let dir = TempDir::new().expect("tempdir");
    let store = FilesystemStore::open(dir.path()).expect("store");
    (dir, store)
}

fn serve(seeded: &Seeded) -> Served {
    Served::spawn(|base_url| common::node_at(seeded, base_url).router())
}

fn fetch(
    store: &FilesystemStore,
    served: &Served,
    by: &ActorId,
    scope: &TrustScope,
) -> Result<kairo_federation_http::client::TrustFetchReport, PullError> {
    fetch_trust(store, &served.base_url, by, scope, &PullOptions::default())
}

#[test]
fn only_published_chains_are_served() {
    let seeded = seed();
    let chains = chains(&seeded);
    let served = serve(&seeded);

    let (status, body) = get(&served, "/federation/v1/trust");
    assert_eq!(status, 200);
    let published: PublishedTrust = serde_json::from_slice(&body).expect("list");
    assert_eq!(published.chains.len(), 1);
    assert_eq!(published.chains[0].by, seeded.alice);
    assert_eq!(published.chains[0].scope, "federation:index");

    let path = format!(
        "/federation/v1/trust/{}?scope=federation:index",
        seeded.alice
    );
    let (status, body) = get(&served, &path);
    assert_eq!(status, 200);
    let listing: TrustListing = serde_json::from_slice(&body).expect("listing");
    let mut ids = listing.statements.clone();
    ids.sort();
    let mut expected = vec![
        chains.trusted.clone(),
        chains.withdrawn.clone(),
        chains.carol.clone(),
    ];
    expected.sort();
    assert_eq!(ids, expected, "withdrawals are served with the chain");
    for id in &listing.statements {
        let (status, _) = get(&served, &format!("/federation/v1/statements/{id}"));
        assert_eq!(status, 200, "{id}");
    }

    // The unpublished scope is indistinguishable from nothing.
    let push_path = format!(
        "/federation/v1/trust/{}?scope=federation:push",
        seeded.alice
    );
    let (status, body) = get(&served, &push_path);
    assert_eq!(status, 404);
    let envelope: kairo_federation_core::FedErrorEnvelope =
        serde_json::from_slice(&body).expect("envelope");
    assert_eq!(envelope.error.code, FedErrorCode::FedNotFound);
    let (status, _) = get(
        &served,
        &format!("/federation/v1/statements/{}", chains.pushes),
    );
    assert_eq!(status, 404);
}

#[test]
fn a_fetched_chain_stays_its_trusters_opinion() {
    let seeded = seed();
    let chains = chains(&seeded);
    let served = serve(&seeded);
    let (_dir, store) = local();
    let report = fetch(&store, &served, &seeded.alice, &index()).expect("fetch");
    assert_eq!(report.chain, 3);
    assert!(report.flagged.is_empty());

    // alice's opinions, resolved as alice's: bob withdrawn, carol trusted.
    let bob = store
        .latest_trust(&seeded.alice, &seeded.bob, &index())
        .expect("read")
        .expect("an opinion");
    assert_eq!(bob.statement_id(), chains.withdrawn);
    let carol = store
        .latest_trust(&seeded.alice, &seeded.carol, &index())
        .expect("read")
        .expect("an opinion");
    assert_eq!(
        carol.unsigned().body().decision(),
        Some(TrustDecision::Trusted)
    );
    // Nothing unpublished came along, and no one else's view changed:
    // the peer's node actor has no opinion here.
    assert!(store.get_statement_bytes(&chains.pushes).is_err());
    assert!(store
        .list_trust(&report.peer_actor)
        .expect("list")
        .is_empty());
    let provenance = store
        .get_provenance(&KairoRef::Statement(chains.carol.clone()))
        .expect("provenance");
    assert_eq!(provenance[0].peer_url, served.base_url);
    assert!(
        store.get_actor(&seeded.alice).is_ok(),
        "the truster's key state comes first"
    );

    let again = fetch(&store, &served, &seeded.alice, &index()).expect("refetch");
    assert_eq!(again.fetched, 0);
}

#[test]
fn an_unpublished_chain_is_not_found() {
    let seeded = seed();
    chains(&seeded);
    let served = serve(&seeded);
    let (_dir, store) = local();
    match fetch(
        &store,
        &served,
        &seeded.alice,
        &TrustScope::federation_push(),
    ) {
        Err(PullError::Status { status: 404, .. }) => {}
        other => panic!("expected 404, got {other:?}"),
    }
}

#[test]
fn an_opinion_outside_the_requested_chain_is_refused() {
    let seeded = seed();
    let chains = chains(&seeded);
    // Publish push too so its statement is servable, then list it under
    // the index chain.
    seeded
        .store
        .publish_trust(
            &seeded.alice,
            &TrustScope::federation_push(),
            Timestamp::now(),
        )
        .unwrap();
    let path = format!("/federation/v1/trust/{}", seeded.alice);
    let smuggled = chains.pushes.to_string();
    let served = Served::spawn(|base_url| {
        let router = common::node_at(&seeded, base_url).router();
        router.layer(axum::middleware::from_fn(
            move |request: axum::extract::Request, next: axum::middleware::Next| {
                let path = path.clone();
                let smuggled = smuggled.clone();
                async move {
                    let is_index = request.uri().path() == path
                        && request.uri().query() == Some("scope=federation:index");
                    let response = next.run(request).await;
                    if !is_index {
                        return response;
                    }
                    let (parts, body) = response.into_parts();
                    let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
                    let mut value: Value = serde_json::from_slice(&bytes).unwrap();
                    value["statements"]
                        .as_array_mut()
                        .unwrap()
                        .push(Value::from(smuggled));
                    let mut parts = parts;
                    parts.headers.remove(axum::http::header::CONTENT_LENGTH);
                    axum::response::Response::from_parts(
                        parts,
                        axum::body::Body::from(serde_json::to_vec(&value).unwrap()),
                    )
                }
            },
        ))
    });
    let (_dir, store) = local();
    match fetch(&store, &served, &seeded.alice, &index()) {
        Err(PullError::Record {
            record,
            reason: RecordRejection::WrongSection(_),
        }) => assert_eq!(record, KairoRef::Statement(chains.pushes.clone())),
        other => panic!("expected a wrong-section refusal, got {other:?}"),
    }
    assert!(store.get_statement_bytes(&chains.pushes).is_err());
}

#[test]
fn an_opinion_signed_with_a_stale_key_is_refused() {
    // The fixture's general opinion is signed with alice's key from
    // before her rotation; publishing it doesn't make it verify.
    let seeded = seed();
    seeded
        .store
        .publish_trust(&seeded.alice, &TrustScope::general(), Timestamp::now())
        .unwrap();
    let served = serve(&seeded);
    let (_dir, store) = local();
    match fetch(&store, &served, &seeded.alice, &TrustScope::general()) {
        Err(PullError::Record {
            record,
            reason: RecordRejection::Signature(_),
        }) => assert_eq!(record, KairoRef::Statement(seeded.trust.clone())),
        other => panic!("expected a signature refusal, got {other:?}"),
    }
}
