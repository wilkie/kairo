//! Property tests for `ActorGenesisBody`'s canonical encoding and JSON
//! DTO round-trip. Mirrors the `kairo-statement` test layout.
//!
//! ActorGenesisBody is the only body type owned by this crate; its
//! shape participates directly in the actor identifier hash, so any
//! drift between `CanonicalEncode` and the JSON DTO would produce
//! split actor IDs across consumers.

// Silence unused-crate warnings for dev-deps the lib needs but this
// integration test doesn't touch directly.
use base64 as _;
use ed25519_dalek as _;
use getrandom as _;
use serde as _;
use utoipa as _;

use kairo_core::canonical::CanonicalEncode;
use kairo_core::Timestamp;
use kairo_identity::json::ActorGenesisJson;
use kairo_identity::{ActorGenesisBody, ActorKind, PublicKey};
use proptest::prelude::*;

fn arb_timestamp() -> impl Strategy<Value = Timestamp> {
    (0_i64..32_503_680_000_i64).prop_map(Timestamp::from_seconds)
}

fn arb_public_key() -> impl Strategy<Value = PublicKey> {
    any::<[u8; 32]>().prop_map(PublicKey::ed25519)
}

fn arb_actor_kind() -> impl Strategy<Value = ActorKind> {
    "[a-z][a-z0-9_-]{0,16}".prop_map(ActorKind::new)
}

/// `ActorGenesisBody::new` enforces non-empty attestation set,
/// disjointness from the initial key, and `1 ≤ threshold ≤ |set|`.
/// Generate compatible inputs by sampling the threshold from
/// `1..=set.len()` after deduping.
fn arb_actor_genesis_body() -> impl Strategy<Value = ActorGenesisBody> {
    (
        arb_actor_kind(),
        arb_public_key(),
        proptest::collection::vec(arb_public_key(), 1..=4),
        arb_timestamp(),
        any::<[u8; 32]>(),
    )
        .prop_filter_map(
            "attestation set must be non-empty and disjoint from initial key",
            |(kind, initial, attestation, at, nonce)| {
                let mut deduped: Vec<PublicKey> = attestation
                    .into_iter()
                    .filter(|key| key.bytes() != initial.bytes())
                    .collect();
                deduped.sort_by(|a, b| a.bytes().cmp(b.bytes()));
                deduped.dedup_by(|a, b| a.bytes() == b.bytes());
                if deduped.is_empty() {
                    return None;
                }
                #[allow(clippy::cast_possible_truncation)]
                let threshold = u8::try_from(deduped.len()).unwrap_or(u8::MAX).max(1);
                Some((kind, initial, deduped, threshold, at, nonce))
            },
        )
        .prop_flat_map(|(kind, initial, attestation, max_threshold, at, nonce)| {
            (1u8..=max_threshold).prop_map(move |threshold| {
                ActorGenesisBody::new(
                    kind.clone(),
                    initial.clone(),
                    attestation.clone(),
                    threshold,
                    at,
                    nonce,
                )
                .expect("constructor inputs satisfy invariants")
            })
        })
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 64,
        ..ProptestConfig::default()
    })]

    /// Random body → JSON DTO → JSON string → DTO → body' must match
    /// both structurally and by canonical bytes (ActorId derivation).
    #[test]
    fn actor_genesis_body_round_trip(body in arb_actor_genesis_body()) {
        let canonical_a = body.canonical_bytes();
        let dto = ActorGenesisJson::from_body(&body);
        let json = serde_json::to_string(&dto).expect("serialize");
        let reparsed: ActorGenesisJson = serde_json::from_str(&json).expect("deserialize");
        let body_back = reparsed.to_body().expect("to_body");
        prop_assert_eq!(canonical_a, body_back.canonical_bytes());
        prop_assert_eq!(body.actor_id(), body_back.actor_id());
        // Structural equality: every getter must match. Body itself
        // doesn't derive PartialEq publicly, so check field-by-field.
        prop_assert_eq!(body.actor_kind().as_str(), body_back.actor_kind().as_str());
        prop_assert_eq!(body.initial_key().bytes(), body_back.initial_key().bytes());
        prop_assert_eq!(body.attestation_threshold(), body_back.attestation_threshold());
        prop_assert_eq!(body.attestation_keys().len(), body_back.attestation_keys().len());
        prop_assert_eq!(body.created_at(), body_back.created_at());
        prop_assert_eq!(body.nonce(), body_back.nonce());
    }

    /// Canonical bytes must be deterministic — encoding the same body
    /// twice must yield identical output.
    #[test]
    fn actor_genesis_body_canonical_bytes_are_deterministic(body in arb_actor_genesis_body()) {
        let mut a = Vec::new();
        let mut b = Vec::new();
        body.encode_canonical(&mut a);
        body.encode_canonical(&mut b);
        prop_assert_eq!(a, b);
    }
}
