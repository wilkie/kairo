//! Property tests over each body type's canonical encoding and JSON DTO
//! round-trip. Catches drift between `CanonicalEncode` impls and the
//! corresponding `*BodyJson::from_body` / `to_body` pair: random body →
//! JSON → back must yield an equal body and identical canonical bytes.
//!
//! These tests intentionally avoid signing/envelope work; they target
//! the body shape only, which is what the StatementId hash is computed
//! over. A fuzz-style failure here would mean a wire-shape regression
//! that could split actor IDs or statement IDs across consumers.
//!
//! Strategies generate every variant where possible (Optionals
//! exercised both Some and None; vectors exercised at non-trivial
//! sizes). IDs are derived from random 32-byte digests so they pass
//! the multihash format check.

// Silence unused-crate warnings for dev-deps the lib needs for its
// own tests but this integration test doesn't touch directly.
use base64 as _;
use ed25519_dalek as _;
use semver as _;
use serde as _;

use kairo_core::canonical::CanonicalEncode;
use kairo_core::{ActorId, BlobId, ObjectId, StatementId, Timestamp};
use kairo_identity::{KeyId, PublicKey};
use kairo_statement::json::{
    ActorAttestationKeyAddBodyJson, ActorAttestationKeyRevocationBodyJson,
    ActorAttestationThresholdChangeBodyJson, ActorCapabilityGrantBodyJson,
    ActorCapabilityRevocationBodyJson, ActorEmergencyKeyRevocationBodyJson,
    ActorEmergencyKeyRotationBodyJson, ActorKeyRevocationBodyJson, ActorKeyRotationBodyJson,
    ActorTrustBodyJson, ObjectBranchBodyJson, ObjectGenesisBodyJson, ObjectRevisionBodyJson,
    ObjectVersionTagBodyJson,
};
use kairo_statement::{
    ActorAttestationKeyAddBody, ActorAttestationKeyRevocationBody,
    ActorAttestationThresholdChangeBody, ActorCapabilityGrantBody, ActorCapabilityRevocationBody,
    ActorEmergencyKeyRevocationBody, ActorEmergencyKeyRotationBody, ActorKeyRevocationBody,
    ActorKeyRotationBody, ActorTrustBody, Capability, CapabilityConstraint, CapabilityScope,
    ObjectBranchBody, ObjectGenesisBody, ObjectKind, ObjectRevisionBody, ObjectVersionTagBody,
    RevisionId, SemverVersion, StatementKind, TrustDecision,
};
use proptest::collection::{hash_set, vec};
use proptest::prelude::*;

// ---- Primitive strategies. ----

fn arb_actor_id() -> impl Strategy<Value = ActorId> {
    any::<[u8; 32]>().prop_map(ActorId::from_sha256_digest)
}

fn arb_object_id() -> impl Strategy<Value = ObjectId> {
    any::<[u8; 32]>().prop_map(ObjectId::from_sha256_digest)
}

fn arb_blob_id() -> impl Strategy<Value = BlobId> {
    any::<[u8; 32]>().prop_map(BlobId::from_sha256_digest)
}

fn arb_statement_id() -> impl Strategy<Value = StatementId> {
    any::<[u8; 32]>().prop_map(StatementId::from_sha256_digest)
}

/// Constrain to the year range 1970..3000 so RFC 3339 round-trips
/// cleanly (the `Display` impl uses `{year:04}`, breaking on years
/// outside `0..=9999`).
fn arb_timestamp() -> impl Strategy<Value = Timestamp> {
    (0_i64..32_503_680_000_i64).prop_map(Timestamp::from_seconds)
}

fn arb_public_key() -> impl Strategy<Value = PublicKey> {
    any::<[u8; 32]>().prop_map(PublicKey::ed25519)
}

fn arb_key_id() -> impl Strategy<Value = KeyId> {
    arb_public_key().prop_map(|key| key.key_id())
}

fn arb_object_kind() -> impl Strategy<Value = ObjectKind> {
    "[a-z][a-z0-9_-]{0,16}".prop_map(ObjectKind::new)
}

fn arb_revision_id() -> impl Strategy<Value = RevisionId> {
    "git:sha256:[0-9a-f]{40}".prop_map(RevisionId::new)
}

fn arb_branch_name() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_-]{0,16}"
}

fn arb_reason() -> impl Strategy<Value = Option<String>> {
    proptest::option::of("[ -~]{0,32}".prop_map(|s| s.to_owned()))
}

fn arb_semver() -> impl Strategy<Value = SemverVersion> {
    (0u32..1000, 0u32..1000, 0u32..1000).prop_map(|(major, minor, patch)| {
        SemverVersion::parse(format!("{major}.{minor}.{patch}"))
            .expect("constructed semver always parses")
    })
}

fn arb_trust_decision() -> impl Strategy<Value = TrustDecision> {
    prop_oneof![
        Just(TrustDecision::Trusted),
        Just(TrustDecision::Untrusted),
    ]
}

/// Object scope only — Actor scope has no valid statement kinds in v1
/// (`CapabilityScope::is_kind_valid` returns `false` for every kind),
/// so a `Capability` over it cannot be constructed.
fn arb_capability_scope() -> impl Strategy<Value = CapabilityScope> {
    arb_object_id().prop_map(CapabilityScope::Object)
}

/// Only the three statement kinds that are legal under `Object` scope
/// per `CapabilityScope::is_kind_valid`. Future actor-surface kinds
/// will widen this set.
fn arb_object_scope_kind() -> impl Strategy<Value = StatementKind> {
    prop_oneof![
        Just(StatementKind::ObjectRevision),
        Just(StatementKind::ObjectBranch),
        Just(StatementKind::ObjectVersionTag),
    ]
}

/// `Capability::new` rejects duplicate constraint tags, so generate a
/// constraint vector with at most one of each variant by independently
/// sampling each.
fn arb_capability_constraints() -> impl Strategy<Value = Vec<CapabilityConstraint>> {
    (
        proptest::option::of(arb_timestamp().prop_map(CapabilityConstraint::ExpiresAt)),
        proptest::option::of((1u8..=8).prop_map(CapabilityConstraint::MaxDelegationDepth)),
        proptest::option::of(arb_key_id().prop_map(CapabilityConstraint::KeyPinned)),
    )
        .prop_map(|(expires, depth, pinned)| {
            [expires, depth, pinned].into_iter().flatten().collect()
        })
}

fn arb_capability() -> impl Strategy<Value = Capability> {
    (
        arb_capability_scope(),
        // Distinct kinds (constructor dedupes; post-dedupe must be ≥ 1).
        hash_set(arb_object_scope_kind(), 1..=3)
            .prop_map(|set| set.into_iter().collect::<Vec<_>>()),
        any::<bool>(),
        arb_capability_constraints(),
    )
        .prop_map(|(scope, kinds, delegable, constraints)| {
            Capability::new(scope, kinds, delegable, constraints).expect("well-formed capability")
        })
}

// ---- Body-level strategies. ----

fn arb_object_genesis_body() -> impl Strategy<Value = ObjectGenesisBody> {
    (
        arb_object_kind(),
        arb_actor_id(),
        arb_timestamp(),
        any::<[u8; 32]>(),
        proptest::option::of(arb_revision_id()),
    )
        .prop_map(|(kind, by, at, nonce, revision)| {
            ObjectGenesisBody::new(kind, by, at, nonce, revision)
        })
}

fn arb_object_revision_body() -> impl Strategy<Value = ObjectRevisionBody> {
    (
        arb_object_id(),
        arb_revision_id(),
        vec(arb_revision_id(), 0..=3),
        arb_blob_id(),
        any::<bool>(),
    )
        .prop_map(|(object, revision, parents, manifest_hash, attests)| {
            ObjectRevisionBody::new(object, revision, parents, manifest_hash, attests)
        })
}

fn arb_object_branch_body() -> impl Strategy<Value = ObjectBranchBody> {
    (
        arb_object_id(),
        arb_branch_name(),
        arb_statement_id(),
        proptest::option::of(arb_statement_id()),
    )
        .prop_map(|(object, name, revision, supersedes)| {
            ObjectBranchBody::new(object, name, revision, supersedes)
        })
}

fn arb_object_version_tag_body() -> impl Strategy<Value = ObjectVersionTagBody> {
    // The shape rule says target=None and supersedes=None is invalid.
    // Generate variants that always have at least one populated.
    (
        arb_object_id(),
        arb_semver(),
        proptest::option::of(arb_statement_id()),
        proptest::option::of(arb_statement_id()),
    )
        .prop_filter(
            "tag must have target or supersedes",
            |(_, _, target, supersedes)| target.is_some() || supersedes.is_some(),
        )
        .prop_map(|(object, version, target, supersedes)| {
            ObjectVersionTagBody::new(object, version, target, supersedes).expect("well-formed tag")
        })
}

fn arb_actor_trust_body() -> impl Strategy<Value = ActorTrustBody> {
    // Same withdraw-without-supersedes rule as version tags.
    (
        arb_actor_id(),
        proptest::option::of(arb_trust_decision()),
        arb_reason(),
        proptest::option::of(arb_statement_id()),
    )
        .prop_filter(
            "trust must have decision or supersedes",
            |(_, decision, _, supersedes)| decision.is_some() || supersedes.is_some(),
        )
        .prop_map(|(trusted, decision, reason, supersedes)| {
            ActorTrustBody::new(trusted, decision, reason, supersedes)
                .expect("well-formed trust body")
        })
}

fn arb_actor_key_rotation_body() -> impl Strategy<Value = ActorKeyRotationBody> {
    (arb_public_key(), proptest::option::of(arb_statement_id()))
        .prop_map(|(next, supersedes)| ActorKeyRotationBody::new(next, supersedes))
}

fn arb_actor_key_revocation_body() -> impl Strategy<Value = ActorKeyRevocationBody> {
    (arb_key_id(), any::<bool>(), arb_reason())
        .prop_map(|(key, retroactive, reason)| {
            ActorKeyRevocationBody::new(key, retroactive, reason)
        })
}

fn arb_actor_emergency_key_rotation_body() -> impl Strategy<Value = ActorEmergencyKeyRotationBody> {
    (arb_public_key(), proptest::option::of(arb_statement_id()))
        .prop_map(|(next, supersedes)| ActorEmergencyKeyRotationBody::new(next, supersedes))
}

fn arb_actor_emergency_key_revocation_body() -> impl Strategy<Value = ActorEmergencyKeyRevocationBody>
{
    (arb_key_id(), any::<bool>(), arb_reason())
        .prop_map(|(key, retroactive, reason)| {
            ActorEmergencyKeyRevocationBody::new(key, retroactive, reason)
        })
}

fn arb_actor_attestation_key_add_body() -> impl Strategy<Value = ActorAttestationKeyAddBody> {
    arb_public_key().prop_map(ActorAttestationKeyAddBody::new)
}

fn arb_actor_attestation_key_revocation_body(
) -> impl Strategy<Value = ActorAttestationKeyRevocationBody> {
    (arb_key_id(), arb_reason())
        .prop_map(|(key, reason)| ActorAttestationKeyRevocationBody::new(key, reason))
}

fn arb_actor_attestation_threshold_change_body(
) -> impl Strategy<Value = ActorAttestationThresholdChangeBody> {
    (1u8..=64).prop_map(|threshold| {
        ActorAttestationThresholdChangeBody::new(threshold).expect("threshold ≥ 1")
    })
}

fn arb_actor_capability_grant_body() -> impl Strategy<Value = ActorCapabilityGrantBody> {
    (
        arb_actor_id(),
        arb_capability(),
        proptest::option::of(arb_statement_id()),
    )
        .prop_map(|(grantee, capability, supersedes)| {
            ActorCapabilityGrantBody::new(grantee, capability, supersedes)
        })
}

fn arb_actor_capability_revocation_body() -> impl Strategy<Value = ActorCapabilityRevocationBody> {
    (arb_statement_id(), any::<bool>(), arb_reason())
        .prop_map(|(grant, retroactive, reason)| {
            ActorCapabilityRevocationBody::new(grant, retroactive, reason)
        })
}

// ---- Round-trip helper. ----
//
// Pattern is identical for every body type:
//   1. body → BodyJson (via from_body)
//   2. BodyJson → JSON (via serde_json)
//   3. JSON → BodyJson (via serde_json)
//   4. BodyJson → body' (via to_body)
//   5. assert canonical_bytes(body) == canonical_bytes(body')
//
// We assert canonical-bytes equality rather than `body == body'`
// because canonical bytes are what StatementIds and signatures are
// computed over — that's the invariant the wire-shape pair is supposed
// to preserve. Where structural equality is meaningful, we assert it
// too.
//
// `body_json_round_trip` is a macro because the closure form would
// require `Body: Clone + PartialEq` and identical `BodyJson` type
// names; the macro just substitutes both at call time.
macro_rules! body_round_trip {
    ($body:expr, $body_json:ty) => {{
        let body = $body;
        let canonical_a = body.canonical_bytes();
        let dto = <$body_json>::from_body(&body);
        let json = serde_json::to_string(&dto).expect("serialize");
        let reparsed: $body_json = serde_json::from_str(&json).expect("deserialize");
        let body_back = reparsed.to_body().expect("to_body");
        let canonical_b = body_back.canonical_bytes();
        prop_assert_eq!(canonical_a, canonical_b);
        prop_assert_eq!(body, body_back);
    }};
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 64,
        ..ProptestConfig::default()
    })]

    #[test]
    fn object_genesis_body_round_trip(body in arb_object_genesis_body()) {
        body_round_trip!(body, ObjectGenesisBodyJson);
    }

    #[test]
    fn object_revision_body_round_trip(body in arb_object_revision_body()) {
        body_round_trip!(body, ObjectRevisionBodyJson);
    }

    #[test]
    fn object_branch_body_round_trip(body in arb_object_branch_body()) {
        body_round_trip!(body, ObjectBranchBodyJson);
    }

    #[test]
    fn object_version_tag_body_round_trip(body in arb_object_version_tag_body()) {
        body_round_trip!(body, ObjectVersionTagBodyJson);
    }

    #[test]
    fn actor_trust_body_round_trip(body in arb_actor_trust_body()) {
        body_round_trip!(body, ActorTrustBodyJson);
    }

    #[test]
    fn actor_key_rotation_body_round_trip(body in arb_actor_key_rotation_body()) {
        body_round_trip!(body, ActorKeyRotationBodyJson);
    }

    #[test]
    fn actor_key_revocation_body_round_trip(body in arb_actor_key_revocation_body()) {
        body_round_trip!(body, ActorKeyRevocationBodyJson);
    }

    #[test]
    fn actor_emergency_key_rotation_body_round_trip(
        body in arb_actor_emergency_key_rotation_body(),
    ) {
        body_round_trip!(body, ActorEmergencyKeyRotationBodyJson);
    }

    #[test]
    fn actor_emergency_key_revocation_body_round_trip(
        body in arb_actor_emergency_key_revocation_body(),
    ) {
        body_round_trip!(body, ActorEmergencyKeyRevocationBodyJson);
    }

    #[test]
    fn actor_attestation_key_add_body_round_trip(body in arb_actor_attestation_key_add_body()) {
        body_round_trip!(body, ActorAttestationKeyAddBodyJson);
    }

    #[test]
    fn actor_attestation_key_revocation_body_round_trip(
        body in arb_actor_attestation_key_revocation_body(),
    ) {
        body_round_trip!(body, ActorAttestationKeyRevocationBodyJson);
    }

    #[test]
    fn actor_attestation_threshold_change_body_round_trip(
        body in arb_actor_attestation_threshold_change_body(),
    ) {
        body_round_trip!(body, ActorAttestationThresholdChangeBodyJson);
    }

    #[test]
    fn actor_capability_grant_body_round_trip(body in arb_actor_capability_grant_body()) {
        body_round_trip!(body, ActorCapabilityGrantBodyJson);
    }

    #[test]
    fn actor_capability_revocation_body_round_trip(body in arb_actor_capability_revocation_body()) {
        body_round_trip!(body, ActorCapabilityRevocationBodyJson);
    }

    /// Two bodies that are structurally equal must produce identical
    /// canonical bytes. This is the determinism property the
    /// `CanonicalEncode` contract is supposed to guarantee; we exercise
    /// it directly so a stray `HashMap::iter()` ordering bug would show
    /// up here rather than as a downstream signature failure.
    #[test]
    fn capability_canonical_bytes_are_deterministic(capability in arb_capability()) {
        let mut a = Vec::new();
        let mut b = Vec::new();
        capability.encode_canonical(&mut a);
        capability.clone().encode_canonical(&mut b);
        prop_assert_eq!(a, b);
    }
}
