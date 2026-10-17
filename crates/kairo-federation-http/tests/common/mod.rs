//! Fixtures shared by the serve and pull tests.

#![allow(dead_code, clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::thread::JoinHandle;
use std::time::Duration;

use axum::Router;
use kairo_core::canonical::CanonicalEncode;
use kairo_core::{ActorId, KairoRef, ObjectId, StatementId, Timestamp};
use kairo_federation_http::Node;
use kairo_identity::{ActorGenesisBody, ActorKind, SecretSigningKey};
use kairo_statement::{
    ActorCapabilityGrantBody, ActorCapabilityRevocationBody, ActorKeyRotationBody, ActorTrustBody,
    Capability, CapabilityScope, ObjectBranchBody, RevisionId, Signature, SignedStatement,
    StatementBody, StatementKind, TrustDecision, UnsignedStatement,
};
use kairo_store::{ActorStore, FilesystemStore, PublishedObjectStore, StatementStore};
use kairo_test_support::store::{CreatedActor, StoreFixture};
use tempfile::TempDir;

pub(crate) const MANIFEST: &str = r#"
    [kairo]
    schema = 1
    kind = "kairo/object"
    name = "federation-fixture"

    [content]
    kind = "tree"
"#;

pub(crate) const BASE_URL: &str = "http://127.0.0.1:7880";

pub(crate) fn sign<B: StatementBody>(
    signer: &CreatedActor,
    subject: &str,
    seconds: i64,
    body: B,
) -> SignedStatement<B> {
    let subject: KairoRef = subject.parse().expect("subject parses");
    let unsigned = UnsignedStatement::new(
        signer.actor_id.clone(),
        subject,
        Timestamp::from_seconds(seconds),
        body,
    );
    let bytes = signer.signing.sign(&unsigned.canonical_bytes());
    let signature = Signature::new(
        signer.actor_id.clone(),
        signer.signing.public_key().key_id().to_string(),
        "ed25519",
        bytes.bytes().to_vec(),
    );
    SignedStatement::new(unsigned, signature)
}

pub(crate) fn grant(
    store: &FilesystemStore,
    grantor: &CreatedActor,
    grantee: &CreatedActor,
    object: &ObjectId,
    seconds: i64,
) -> StatementId {
    let capability = Capability::new(
        CapabilityScope::Object(object.clone()),
        vec![StatementKind::ObjectBranch],
        false,
        Vec::new(),
    )
    .expect("capability");
    let body = ActorCapabilityGrantBody::new(grantee.actor_id.clone(), capability, None);
    let signed = sign(
        grantor,
        &format!("actor:{}", grantee.actor_id),
        seconds,
        body,
    );
    store
        .put_actor_capability_grant(&signed)
        .expect("put grant")
}

pub(crate) fn node_actor(store: &FilesystemStore, kind: ActorKind) -> ActorId {
    let genesis = ActorGenesisBody::new(
        kind,
        SecretSigningKey::ed25519([200; 32]).public_key(),
        vec![SecretSigningKey::ed25519([201; 32]).public_key()],
        1,
        Timestamp::from_seconds(1_700_000_000),
        [202; 32],
    )
    .expect("genesis");
    store.put_actor(&genesis).expect("put node actor")
}

/// Everything the tests assert against.
pub(crate) struct Seeded {
    pub(crate) _dir: TempDir,
    pub(crate) store: FilesystemStore,
    pub(crate) node_actor: ActorId,
    pub(crate) published: ObjectId,
    pub(crate) unpublished: ObjectId,
    pub(crate) alice: ActorId,
    pub(crate) bob: ActorId,
    pub(crate) carol: ActorId,
    pub(crate) statements: BTreeSet<StatementId>,
    pub(crate) authority: BTreeSet<StatementId>,
    pub(crate) manifest: kairo_core::BlobId,
    pub(crate) alice_rotation: StatementId,
    /// alice, signing with the key she rotated to.
    /// alice with her original signing key.
    pub(crate) alice_actor: CreatedActor,
    pub(crate) bob_actor: CreatedActor,
    pub(crate) alice_rotated: CreatedActor,
    /// A branch alice signed with her rotated key: it only verifies
    /// once her rotation is known.
    pub(crate) after_rotation_branch: StatementId,
    pub(crate) trust: StatementId,
    pub(crate) unpublished_statements: Vec<StatementId>,
}

/// alice owns `published`: one revision, her `main` branch, a grant
/// letting bob move branches (bob then sets `bob`), and a grant to
/// carol that alice revoked. alice rotated her key, then set an
/// `after-rotation` branch with the new key. She trusts bob.
/// alice also owns `unpublished`, with its own revision and a grant
/// to carol.
pub(crate) fn seed() -> Seeded {
    let (dir, fixture) = StoreFixture::temp();
    let alice = fixture.make_actor();
    let bob = fixture.make_actor();
    let carol = fixture.make_actor();
    let store = fixture.store.clone();

    let object = fixture.make_object(&alice, "kairo/object");
    let revision = fixture.make_revision(
        &alice,
        &object,
        RevisionId::new(format!("git:sha1:{}", "1".repeat(40))),
        MANIFEST,
        Vec::new(),
    );
    let alice_main = fixture.set_branch(&alice, &object, &revision, "main");
    let bob_grant = grant(&store, &alice, &bob, &object.object_id, 1_700_000_100);
    let bob_branch = fixture.set_branch(&bob, &object, &revision, "bob");
    let carol_grant = grant(&store, &alice, &carol, &object.object_id, 1_700_000_200);
    let revocation = store
        .put_actor_capability_revocation(&sign(
            &alice,
            &format!("statement:{carol_grant}"),
            1_700_000_300,
            ActorCapabilityRevocationBody::new(carol_grant.clone(), false, None),
        ))
        .expect("put revocation");
    let alice_rotation = store
        .put_actor_key_rotation(&sign(
            &alice,
            &format!("actor:{}", alice.actor_id),
            1_700_000_400,
            ActorKeyRotationBody::new(SecretSigningKey::ed25519([99; 32]).public_key(), None),
        ))
        .expect("put rotation");
    let alice_rotated = CreatedActor {
        signing: SecretSigningKey::ed25519([99; 32]),
        ..alice.clone()
    };
    let after_rotation_branch = store
        .put_object_branch(&sign(
            &alice_rotated,
            &format!("object:{}", object.object_id),
            1_700_000_450,
            ObjectBranchBody::new(
                object.object_id.clone(),
                "after-rotation",
                revision.statement_id.clone(),
                None,
            ),
        ))
        .expect("put after-rotation branch");
    let trust = store
        .put_actor_trust(&sign(
            &alice,
            &format!("actor:{}", bob.actor_id),
            1_700_000_500,
            ActorTrustBody::new(
                bob.actor_id.clone(),
                Some(TrustDecision::Trusted),
                None,
                None,
            )
            .expect("trust body"),
        ))
        .expect("put trust");

    let other = fixture.make_object(&alice, "kairo/other");
    let other_revision = fixture.make_revision(
        &alice,
        &other,
        RevisionId::new(format!("git:sha1:{}", "2".repeat(40))),
        MANIFEST,
        Vec::new(),
    );
    let other_grant = grant(&store, &alice, &carol, &other.object_id, 1_700_000_600);

    let node_actor = node_actor(&store, ActorKind::node());
    store
        .publish_object(&object.object_id, Timestamp::from_seconds(1_700_001_000))
        .expect("publish");

    Seeded {
        _dir: dir,
        node_actor,
        published: object.object_id,
        unpublished: other.object_id,
        alice: alice.actor_id.clone(),
        bob: bob.actor_id.clone(),
        carol: carol.actor_id,
        statements: [
            revision.statement_id,
            alice_main.statement_id,
            bob_branch.statement_id,
            after_rotation_branch.clone(),
        ]
        .into(),
        authority: [bob_grant, carol_grant, revocation].into(),
        manifest: revision.manifest_blob_id,
        alice_rotation,
        alice_actor: alice.clone(),
        bob_actor: bob.clone(),
        alice_rotated,
        after_rotation_branch,
        trust,
        unpublished_statements: vec![other_revision.statement_id, other_grant],
        store,
    }
}

/// A node over `seeded`'s store with a zero view TTL, so every
/// request sees the store as it is now.
pub(crate) fn node(seeded: &Seeded) -> Node {
    Node::from_store(
        seeded.store.clone(),
        &seeded.node_actor,
        BASE_URL,
        Duration::ZERO,
    )
    .expect("node opens")
}

/// A router served on an ephemeral loopback port from a background
/// thread, stopped on drop.
pub(crate) struct Served {
    pub(crate) base_url: String,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Served {
    pub(crate) fn spawn(router: Router) -> Self {
        let (addr_tx, addr_rx) = std::sync::mpsc::channel::<SocketAddr>();
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                    .await
                    .expect("bind");
                addr_tx
                    .send(listener.local_addr().expect("addr"))
                    .expect("send addr");
                axum::serve(listener, router)
                    .with_graceful_shutdown(async {
                        let _ = stopped.await;
                    })
                    .await
                    .expect("serve");
            });
        });
        let addr = addr_rx.recv().expect("server started");
        Self {
            base_url: format!("http://{addr}"),
            stop: Some(stop),
            thread: Some(thread),
        }
    }
}

impl Drop for Served {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
