//! `KairoNode1` request authentication (`specs/DECISIONS.md` §25
//! sub-decision 3), driven through `Node::authenticate_request` against
//! a seeded store.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod common;

use kairo_core::{ActorId, Timestamp};
use kairo_federation_core::{FedErrorCode, NodeAuthorization, NodeRequest, RequestBody};
use kairo_federation_http::{AuthenticatedNode, Node, NodeAuthError};
use kairo_identity::{ActorGenesisBody, ActorKind, SecretSigningKey};
use kairo_statement::{ActorKeyRevocationBody, ActorKeyRotationBody};
use kairo_store::{ActorStore, FilesystemStore, StatementStore};
use kairo_test_support::store::CreatedActor;

use common::{seed, sign, sign_chained, Seeded, BASE_URL};

// Crates the lib or `common` depends on that this target doesn't name.
use axum as _;
use flate2 as _;
use http_body as _;
use http_body_util as _;
use kairo_config as _;
use kairo_git as _;
use kairo_keystore as _;
use kairo_object as _;
use serde as _;
use serde_json as _;
use sha2 as _;
use tempfile as _;
use tokio as _;
use tower as _;
use tracing as _;
use tracing_subscriber as _;
use ureq as _;

const PATH: &str = "/federation/v1/announce";
const BODY: &[u8] = br#"{"object":"z"}"#;

/// Another node, known to the receiver: its genesis is in the store.
fn sender(store: &FilesystemStore, seed: u8) -> CreatedActor {
    let signing = SecretSigningKey::ed25519([seed; 32]);
    let genesis = ActorGenesisBody::new(
        ActorKind::node(),
        signing.public_key(),
        vec![SecretSigningKey::ed25519([seed.wrapping_add(1); 32]).public_key()],
        1,
        Timestamp::from_seconds(1_700_000_000),
        [seed; 32],
    )
    .expect("genesis");
    let actor_id = store.put_actor(&genesis).expect("put sender");
    CreatedActor {
        actor_id,
        signing,
        attestation_keys: Vec::new(),
        genesis,
    }
}

fn header(
    signer: &ActorId,
    key: &SecretSigningKey,
    receiver: &str,
    body: &[u8],
    timestamp: i64,
    nonce: u8,
) -> String {
    let request = NodeRequest {
        method: "POST",
        receiver_base_url: receiver,
        path_and_query: PATH,
        body: RequestBody::Bytes(body),
    };
    NodeAuthorization::sign(&request, signer.clone(), key, timestamp, [nonce; 16]).format()
}

fn now() -> i64 {
    Timestamp::now().seconds()
}

fn authenticate(node: &Node, header: &str) -> Result<AuthenticatedNode, NodeAuthError> {
    node.authenticate_request("POST", PATH, Some(header), BODY)
}

fn setup() -> (Seeded, Node, CreatedActor) {
    let seeded = seed();
    let node = common::node(&seeded);
    let sender = sender(&seeded.store, 210);
    (seeded, node, sender)
}

#[test]
fn a_signed_request_from_a_known_node_authenticates() {
    let (_seeded, node, sender) = setup();
    let signed = header(&sender.actor_id, &sender.signing, BASE_URL, BODY, now(), 1);
    let authenticated = authenticate(&node, &signed).expect("authenticates");
    assert_eq!(authenticated.node, sender.actor_id);
    assert_eq!(authenticated.key_id, sender.signing.public_key().key_id());
}

#[test]
fn a_nonce_is_accepted_once() {
    let (seeded, node, sender) = setup();
    let at = now();
    let first = header(&sender.actor_id, &sender.signing, BASE_URL, BODY, at, 1);
    authenticate(&node, &first).expect("first use");
    assert_eq!(authenticate(&node, &first), Err(NodeAuthError::Replayed));
    let fresh = header(&sender.actor_id, &sender.signing, BASE_URL, BODY, at, 2);
    authenticate(&node, &fresh).expect("a new nonce");

    // The same nonce from another node is that node's own.
    let other = self::sender(&seeded.store, 220);
    let theirs = header(&other.actor_id, &other.signing, BASE_URL, BODY, at, 1);
    authenticate(&node, &theirs).expect("nonces are per node");
}

#[test]
fn every_signed_part_of_the_request_is_covered() {
    let (_seeded, node, sender) = setup();
    let key = &sender.signing;
    let actor = &sender.actor_id;

    // Signed for another receiver: not replayable here.
    let elsewhere = header(actor, key, "https://other.example.org", BODY, now(), 1);
    assert_eq!(
        authenticate(&node, &elsewhere),
        Err(NodeAuthError::BadSignature)
    );

    let signed = header(actor, key, BASE_URL, BODY, now(), 2);
    assert_eq!(
        node.authenticate_request("POST", PATH, Some(&signed), b"{}"),
        Err(NodeAuthError::BadSignature)
    );
    assert_eq!(
        node.authenticate_request("POST", "/federation/v1/push", Some(&signed), BODY),
        Err(NodeAuthError::BadSignature)
    );
    assert_eq!(
        node.authenticate_request("PUT", PATH, Some(&signed), BODY),
        Err(NodeAuthError::BadSignature)
    );
    // The failures above didn't burn the nonce.
    authenticate(&node, &signed).expect("the untouched request");
}

#[test]
fn requests_outside_the_window_are_refused() {
    let (_seeded, node, sender) = setup();
    for timestamp in [now() - 301, now() + 301] {
        let signed = header(
            &sender.actor_id,
            &sender.signing,
            BASE_URL,
            BODY,
            timestamp,
            1,
        );
        assert!(matches!(
            authenticate(&node, &signed),
            Err(NodeAuthError::OutOfWindow { .. })
        ));
    }
    let skewed = header(
        &sender.actor_id,
        &sender.signing,
        BASE_URL,
        BODY,
        now() - 250,
        2,
    );
    authenticate(&node, &skewed).expect("within the window");
}

#[test]
fn unknown_and_non_node_signers_are_refused() {
    let (seeded, node, _sender) = setup();
    let stranger = SecretSigningKey::ed25519([230; 32]);
    let unknown = ActorId::from_sha256_digest([0x55; 32]);
    let signed = header(&unknown, &stranger, BASE_URL, BODY, now(), 1);
    assert_eq!(
        authenticate(&node, &signed),
        Err(NodeAuthError::UnknownNode(unknown))
    );

    let alice = &seeded.alice_actor;
    let signed = header(&alice.actor_id, &alice.signing, BASE_URL, BODY, now(), 2);
    assert!(matches!(
        authenticate(&node, &signed),
        Err(NodeAuthError::NotANode { ref kind, .. }) if kind == "person"
    ));
}

#[test]
fn the_signers_key_chain_decides_which_key_is_good() {
    let (seeded, node, sender) = setup();
    let wrong = SecretSigningKey::ed25519([231; 32]);
    let signed = header(&sender.actor_id, &wrong, BASE_URL, BODY, now(), 1);
    assert!(matches!(
        authenticate(&node, &signed),
        Err(NodeAuthError::KeyMismatch { .. })
    ));

    let rotated = SecretSigningKey::ed25519([232; 32]);
    seeded
        .store
        .put_actor_key_rotation(&sign_chained(
            &seeded.store,
            &sender,
            &format!("actor:{}", sender.actor_id),
            1_700_000_500,
            ActorKeyRotationBody::new(rotated.public_key(), None),
        ))
        .expect("rotate");
    let stale = header(&sender.actor_id, &sender.signing, BASE_URL, BODY, now(), 2);
    assert!(matches!(
        authenticate(&node, &stale),
        Err(NodeAuthError::KeyMismatch { .. })
    ));
    let fresh = header(&sender.actor_id, &rotated, BASE_URL, BODY, now(), 3);
    authenticate(&node, &fresh).expect("the rotated key");
}

#[test]
fn a_revoked_key_is_refused() {
    let (seeded, node, _sender) = setup();
    let sender = sender(&seeded.store, 240);
    seeded
        .store
        .put_actor_key_revocation(&sign(
            &sender,
            &format!("actor:{}", sender.actor_id),
            1_700_000_050,
            ActorKeyRevocationBody::new(sender.signing.public_key().key_id(), false, None),
        ))
        .expect("revoke");
    assert!(seeded
        .store
        .actor_key_event_ids(&sender.actor_id)
        .is_ok_and(|ids| ids.len() == 1));
    let signed = header(&sender.actor_id, &sender.signing, BASE_URL, BODY, now(), 1);
    assert!(matches!(
        authenticate(&node, &signed),
        Err(NodeAuthError::RevokedKey { .. })
    ));
}

#[test]
fn missing_and_malformed_headers_are_unauthenticated() {
    let (_seeded, node, _sender) = setup();
    let missing = node.authenticate_request("POST", PATH, None, BODY);
    assert_eq!(missing, Err(NodeAuthError::Missing));
    let bearer = node.authenticate_request("POST", PATH, Some("Bearer abc"), BODY);
    assert!(matches!(bearer, Err(NodeAuthError::Header(_))));
    for error in [missing.unwrap_err(), bearer.unwrap_err()] {
        assert_eq!(error.code(), FedErrorCode::FedUnauthenticated);
        assert_eq!(error.code().status(), 401);
    }
}
