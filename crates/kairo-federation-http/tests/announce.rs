//! `POST /federation/v1/announce` (`specs/DECISIONS.md` §26): the
//! receiver's checks in order (authentication, `federation:push` trust,
//! the accept list), the inbox entry, and the client's `learn_peer` and
//! `announce` against real servers.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod common;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use kairo_config::{AcceptConfig, FederationConfig, LimitsConfig};
use kairo_core::{ActorId, ObjectId, Timestamp};
use kairo_federation_core::{
    Announcement, AnnouncementAccepted, FedErrorCode, FedErrorEnvelope, NodeAuthorization,
    NodeDocument, NodeRequest,
};
use kairo_federation_http::client::{announce, learn_peer, NodeSigner, PullError, PullOptions};
use kairo_federation_http::Node;
use kairo_identity::{ActorGenesisBody, ActorKind, SecretSigningKey};
use kairo_statement::{ActorTrustBody, TrustDecision, TrustScope};
use kairo_store::{ActorStore, FilesystemStore, InboxItem, InboxStore, StatementStore};
use kairo_test_support::store::CreatedActor;
use tempfile::TempDir;
use tower::ServiceExt;

use common::{node_key, seed, sign_chained, Seeded, Served, BASE_URL};

// Crates the lib or `common` depends on that this target doesn't name.
use flate2 as _;
use http_body as _;
use kairo_git as _;
use kairo_keystore as _;
use kairo_object as _;
use serde as _;
use serde_json as _;
use tokio as _;
use tracing as _;
use tracing_subscriber as _;
use ureq as _;

const PATH: &str = "/federation/v1/announce";
const SENDER_URL: &str = "https://laptop.example.org";

fn node_actor_with(store: &FilesystemStore, seed: u8) -> CreatedActor {
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
    let actor_id = store.put_actor(&genesis).expect("put node actor");
    CreatedActor {
        actor_id,
        signing,
        attestation_keys: Vec::new(),
        genesis,
    }
}

/// `by`'s opinion of `of` in `scope`.
fn trust(store: &FilesystemStore, by: &CreatedActor, of: &ActorId, scope: TrustScope) {
    let body = ActorTrustBody::new(of.clone(), scope, Some(TrustDecision::Trusted), None, None)
        .expect("trust body");
    store
        .put_actor_trust(&sign_chained(
            store,
            by,
            &format!("actor:{of}"),
            1_700_000_600,
            body,
        ))
        .expect("put trust");
}

fn receiver_signer(seeded: &Seeded) -> CreatedActor {
    CreatedActor {
        actor_id: seeded.node_actor.clone(),
        signing: node_key(),
        attestation_keys: Vec::new(),
        genesis: seeded.store.get_actor(&seeded.node_actor).expect("genesis"),
    }
}

fn accepting(
    object: &ObjectId,
    from: Option<&ActorId>,
    max_inbox_entries: u64,
) -> FederationConfig {
    FederationConfig {
        accept: vec![AcceptConfig {
            object: object.clone(),
            from: from.cloned(),
            publish: false,
        }],
        limits: LimitsConfig {
            max_inbox_entries: Some(max_inbox_entries),
            ..LimitsConfig::default()
        },
        ..FederationConfig::default()
    }
}

/// A receiver over `seeded` that trusts a sender with `federation:push`
/// and accepts `seeded.published` from anyone, with an inbox share of
/// `max`.
fn setup(max: u64) -> (Seeded, Router, CreatedActor) {
    let seeded = seed();
    let sender = node_actor_with(&seeded.store, 210);
    trust(
        &seeded.store,
        &receiver_signer(&seeded),
        &sender.actor_id,
        TrustScope::federation_push(),
    );
    let router = common::node(&seeded)
        .with_federation_config(accepting(&seeded.published, None, max))
        .router();
    (seeded, router, sender)
}

fn announcement(objects: Vec<ObjectId>) -> Vec<u8> {
    serde_json::to_vec(&Announcement::new(SENDER_URL.to_owned(), objects)).expect("json")
}

fn signed(sender: &CreatedActor, body: &[u8], nonce: u8) -> String {
    let request = NodeRequest {
        method: "POST",
        receiver_base_url: BASE_URL,
        path_and_query: PATH,
        body,
    };
    NodeAuthorization::sign(
        &request,
        sender.actor_id.clone(),
        &sender.signing,
        Timestamp::now().seconds(),
        [nonce; 16],
    )
    .format()
}

async fn post(
    router: &Router,
    authorization: Option<&str>,
    body: Vec<u8>,
) -> (StatusCode, Vec<u8>, Option<String>) {
    let mut request = Request::builder()
        .method(Method::POST)
        .uri(PATH)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(authorization) = authorization {
        request = request.header(header::AUTHORIZATION, authorization);
    }
    let response = router
        .clone()
        .oneshot(request.body(Body::from(body)).expect("request"))
        .await
        .expect("response");
    let status = response.status();
    let retry_after = response
        .headers()
        .get(header::RETRY_AFTER)
        .map(|value| value.to_str().expect("ascii").to_owned());
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    (status, body.to_vec(), retry_after)
}

fn code(body: &[u8]) -> FedErrorCode {
    serde_json::from_slice::<FedErrorEnvelope>(body)
        .expect("error envelope")
        .error
        .code
}

#[tokio::test]
async fn an_accepted_announcement_lands_in_the_inbox() {
    let (seeded, router, sender) = setup(8);
    let body = announcement(vec![seeded.published.clone()]);
    let (status, response, _) = post(&router, Some(&signed(&sender, &body, 1)), body).await;
    assert_eq!(
        status,
        StatusCode::ACCEPTED,
        "{}",
        String::from_utf8_lossy(&response)
    );
    let accepted: AnnouncementAccepted = serde_json::from_slice(&response).expect("accepted");

    let entries = seeded.store.list_inbox().expect("inbox");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, accepted.entry);
    let InboxItem::Announcement(entry) = &entries[0].item;
    assert_eq!(entry.from, sender.actor_id);
    assert_eq!(entry.to, seeded.node_actor);
    assert_eq!(entry.base_url, SENDER_URL);
    assert_eq!(entry.objects, vec![seeded.published.clone()]);
}

#[tokio::test]
async fn an_unauthenticated_announcement_is_refused_before_anything_else() {
    let (seeded, router, sender) = setup(8);
    let (status, response, _) = post(&router, None, b"not json".to_vec()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(code(&response), FedErrorCode::FedUnauthenticated);

    // Signed over other bytes than were sent.
    let body = announcement(vec![seeded.published.clone()]);
    let header = signed(&sender, b"{}", 1);
    let (status, _, _) = post(&router, Some(&header), body).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(seeded.store.list_inbox().expect("inbox").is_empty());
}

#[tokio::test]
async fn a_node_without_federation_push_trust_is_refused() {
    let seeded = seed();
    let sender = node_actor_with(&seeded.store, 210);
    // `general` trust is a different chain: no fallback (§18).
    trust(
        &seeded.store,
        &receiver_signer(&seeded),
        &sender.actor_id,
        TrustScope::general(),
    );
    let router = common::node(&seeded)
        .with_federation_config(accepting(&seeded.published, None, 8))
        .router();
    let body = announcement(vec![seeded.published.clone()]);
    let (status, response, _) = post(&router, Some(&signed(&sender, &body, 1)), body).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(code(&response), FedErrorCode::FedNotTrusted);
    assert!(seeded.store.list_inbox().expect("inbox").is_empty());
}

#[tokio::test]
async fn objects_off_the_accept_list_are_refused() {
    let (seeded, router, sender) = setup(8);
    let body = announcement(vec![seeded.published.clone(), seeded.unpublished.clone()]);
    let (status, response, _) = post(&router, Some(&signed(&sender, &body, 1)), body).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(code(&response), FedErrorCode::FedNotAccepted);
    let message = serde_json::from_slice::<FedErrorEnvelope>(&response)
        .expect("envelope")
        .error
        .message;
    assert!(message.contains(seeded.unpublished.as_str()), "{message}");

    // An entry for one other node doesn't cover this one.
    let other = ActorId::from_sha256_digest([0x44; 32]);
    let router = common::node(&seeded)
        .with_federation_config(accepting(&seeded.published, Some(&other), 8))
        .router();
    let body = announcement(vec![seeded.published.clone()]);
    let (status, response, _) = post(&router, Some(&signed(&sender, &body, 2)), body).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(code(&response), FedErrorCode::FedNotAccepted);
    assert!(seeded.store.list_inbox().expect("inbox").is_empty());
}

#[tokio::test]
async fn a_malformed_announcement_from_a_trusted_node_is_a_bad_request() {
    let (seeded, router, sender) = setup(8);
    for (nonce, body) in [
        (1, b"{\"schema\":\"x\"}".to_vec()),
        (2, announcement(Vec::new())),
    ] {
        let (status, response, _) = post(&router, Some(&signed(&sender, &body, nonce)), body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(code(&response), FedErrorCode::FedBadAnnouncement);
    }
    assert!(seeded.store.list_inbox().expect("inbox").is_empty());
}

#[tokio::test]
async fn each_node_has_a_bounded_share_of_the_inbox() {
    let (seeded, router, sender) = setup(1);
    let body = announcement(vec![seeded.published.clone()]);
    let (status, _, _) = post(&router, Some(&signed(&sender, &body, 1)), body.clone()).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let (status, response, retry_after) =
        post(&router, Some(&signed(&sender, &body, 2)), body).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(code(&response), FedErrorCode::FedBusy);
    assert_eq!(retry_after.as_deref(), Some("60"));
    assert_eq!(seeded.store.list_inbox().expect("inbox").len(), 1);
}

#[tokio::test]
async fn the_node_document_advertises_announce() {
    let seeded = seed();
    let response = common::node(&seeded)
        .router()
        .oneshot(
            Request::builder()
                .uri("/.well-known/kairo-node")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let document: NodeDocument = serde_json::from_slice(&body).expect("document");
    assert!(document.supports("announce"));
}

// --- client, against real servers -------------------------------------

/// A receiving node in its own store, served on a loopback port.
struct Receiver {
    _dir: TempDir,
    store: FilesystemStore,
    signer: CreatedActor,
    served: Served,
}

fn receiver(accept: &ObjectId) -> Receiver {
    let dir = TempDir::new().expect("tempdir");
    let store = FilesystemStore::open(dir.path()).expect("store");
    let signer = node_actor_with(&store, 220);
    let config = accepting(accept, None, 8);
    let served = Served::spawn(|base_url| {
        Node::from_store(
            store.clone(),
            &signer.actor_id,
            &signer.signing,
            base_url,
            std::time::Duration::ZERO,
        )
        .expect("receiver node")
        .with_federation_config(config)
        .router()
    });
    Receiver {
        _dir: dir,
        store,
        signer,
        served,
    }
}

#[test]
fn a_learned_and_trusted_peer_announces_and_the_receiver_can_pull() {
    let origin = seed();
    let origin_served = Served::spawn(|base_url| common::node_at(&origin, base_url).router());
    let receiver = receiver(&origin.published);

    // Unknown to the receiver: it can't check the signature.
    let options = PullOptions::default();
    let signer = NodeSigner {
        node: &origin.node_actor,
        key: &node_key(),
    };
    let to_announce = Announcement::new(
        origin_served.base_url.clone(),
        vec![origin.published.clone()],
    );
    match announce(
        &origin.store,
        &receiver.served.base_url,
        &to_announce,
        signer,
        &options,
    ) {
        Err(PullError::Status {
            status: 401,
            code: Some(FedErrorCode::FedUnauthenticated),
            ..
        }) => {}
        other => panic!("expected 401, got {other:?}"),
    }

    // Learned, but not yet trusted.
    let learned = learn_peer(&receiver.store, &origin_served.base_url, &options).expect("learn");
    assert_eq!(learned.node_actor, origin.node_actor);
    assert!(receiver.store.get_actor(&origin.node_actor).is_ok());
    match announce(
        &origin.store,
        &receiver.served.base_url,
        &to_announce,
        signer,
        &options,
    ) {
        Err(PullError::Status {
            status: 403,
            code: Some(FedErrorCode::FedNotTrusted),
            ..
        }) => {}
        other => panic!("expected 403, got {other:?}"),
    }

    trust(
        &receiver.store,
        &receiver.signer,
        &origin.node_actor,
        TrustScope::federation_push(),
    );
    let receipt = announce(
        &origin.store,
        &receiver.served.base_url,
        &to_announce,
        signer,
        &options,
    )
    .expect("announce");
    assert_eq!(receipt.peer_actor, receiver.signer.actor_id);

    let entries = receiver.store.list_inbox().expect("inbox");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, receipt.entry);
    let InboxItem::Announcement(entry) = &entries[0].item;
    // What `kairo federation receive` does with it.
    let pulled = kairo_federation_http::client::pull_object(
        &receiver.store,
        &entry.base_url,
        &origin.published,
        &PullOptions {
            expected_node_actor: Some(entry.from.clone()),
            ..PullOptions::default()
        },
    )
    .expect("pull the announced object");
    assert_eq!(pulled.peer_actor, origin.node_actor);
    for id in &origin.statements {
        assert!(receiver.store.get_statement_bytes(id).is_ok(), "{id}");
    }
}
