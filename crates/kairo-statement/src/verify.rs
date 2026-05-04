//! Generic statement verification.
//!
//! `verify_envelope_statement` resolves the actor declared on the envelope
//! through an [`ActorResolver`], then evaluates the signature against the
//! resolved initial key. It returns a structured [`VerificationReport`] that
//! reports signature status, actor resolution outcome, and trust evaluation
//! independently. Cryptographic validity does not imply trust; trust comes
//! from a separate per-truster opinion published as `ActorTrust`.
//!
//! [`evaluate_trust`] resolves "does `by_actor` trust `of_actor`?" against a
//! [`TrustResolver`]. The resolver returns the chain-leaf `ActorTrust` for
//! the pair (or `None` if no opinion was ever published); `evaluate_trust`
//! folds that into a [`TrustEvaluation`]. Trust is informational — it never
//! makes a cryptographically valid statement invalid; callers compose the
//! two independently.
//!
//! [`evaluate_capability`] resolves "does grantee `B` have authority to
//! issue statements of kind `K` against target `T` at causal position `at`?"
//! against a [`CapabilityResolver`]. It walks the chain leaf for each
//! candidate grantor, checks revocation / expiration / delegation depth,
//! and recursively verifies grantor authority back to the object's root
//! authority. See `specs/CAPABILITIES.md` §6.1.

use std::collections::HashSet;

use kairo_core::{ActorId, ObjectId, StatementId, Timestamp};
use kairo_identity::{ActorResolveError, ActorResolver, SignatureVerificationError};

use crate::{
    ActorCapabilityGrantBody, ActorCapabilityRevocationBody, ActorTrustBody, CapabilityConstraint,
    CapabilityScope, SignedStatement, SigningSurface, StatementBody, StatementKind,
    StatementSignatureError, TrustDecision,
};

/// Outcome of verifying a signed statement against a resolver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationReport {
    pub statement_id: StatementId,
    pub envelope_actor: ActorId,
    pub signature_actor: ActorId,
    pub actor: ActorResolution,
    pub signature: SignatureStatus,
    pub trust: TrustEvaluation,
}

/// Whether the actor declared on the envelope was resolvable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActorResolution {
    /// Actor genesis was found in the resolver.
    Resolved,
    /// Actor genesis was not present in the resolver.
    NotFound,
    /// Resolver could not answer (transient or backend error).
    ResolverUnavailable(String),
    /// `signature.actor` does not match the envelope `actor`. The signature is
    /// not evaluated in this case.
    SignatureActorMismatch,
}

/// Whether the signature verified against the actor's active key at the
/// statement's causal position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureStatus {
    /// Signature verified against the actor's active key at
    /// `created_at`, and that key was not revoked.
    Valid,
    /// Signature did not verify against the resolved active key.
    Invalid,
    /// Signature algorithm is not understood by this implementation.
    UnsupportedAlgorithm(String),
    /// Signature bytes are malformed (e.g. wrong length for the algorithm).
    Malformed {
        expected_len: usize,
        actual_len: usize,
    },
    /// Algorithm declared on the signature does not match the resolved key.
    AlgorithmMismatch,
    /// `signature.key_id` does not match the actor's active key at the
    /// statement's `created_at` per the rotation chain
    /// (`ACTORS.md` §5.5). Carries both ids for diagnostics.
    KeyMismatch {
        signature_key_id: String,
        active_key_id: String,
    },
    /// `signature.key_id` matches the active key at `created_at`, but
    /// that key is revoked for the actor at that time per the
    /// revocation set (`ACTORS.md` §5.5).
    KeyRevoked,
    /// The actor has no active key at the statement's `created_at`
    /// (e.g. they revoked their only key). Verification cannot proceed.
    NoActiveKey,
    /// The statement is an emergency body kind, but `signature.key_id`
    /// is not in the actor's attestation set at `created_at`. Carries
    /// the signature's `key_id` for diagnostics. See `ACTORS.md`
    /// §5.5.2 / §6.1.
    NotInAttestationSet { signature_key_id: String },
    /// Signature was not evaluated (e.g. actor could not be resolved).
    NotEvaluated,
}

/// Local trust evaluation against a chosen truster.
///
/// Trust is first-person and always parameterized by *who* is asking. A
/// statement is `Trusted` from `by_actor`'s perspective when `by_actor` has
/// an active "trusted" opinion about the statement's signing actor;
/// `Untrusted` when the active opinion is "untrusted"; `Unknown` when no
/// active opinion exists (no statement, or the chain leaf is a withdrawal);
/// and `Unevaluated` when the caller did not supply a `by_actor` (e.g.
/// `kairo verify object` was run without `--as`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustEvaluation {
    Trusted,
    Untrusted,
    Unknown,
    Unevaluated,
}

/// Lookup interface for first-person trust opinions.
///
/// Implementors return the chain-leaf `ActorTrust` statement for a
/// `(by_actor, trusted_actor)` pair, or `None` if `by_actor` has never
/// published an opinion about `trusted_actor`. The leaf may be a grant,
/// block, or withdrawal — `evaluate_trust` interprets the decision.
///
/// Mirrors [`ActorResolver`] in shape: a small generic trait that can
/// be satisfied by any backing store (filesystem, in-memory, network),
/// with an associated `Error` so callers don't lock to a single error
/// type.
pub trait TrustResolver {
    type Error: std::error::Error + 'static;

    fn latest_trust(
        &self,
        by_actor: &ActorId,
        trusted_actor: &ActorId,
    ) -> Result<Option<SignedStatement<ActorTrustBody>>, Self::Error>;
}

/// Resolve "does `by_actor` trust `of_actor`?" against `trust_resolver`.
///
/// Returns the active opinion as a [`TrustEvaluation`]. A withdrawal
/// (chain leaf with `decision = None`) is reported as
/// [`TrustEvaluation::Unknown`] — the actor explicitly retracted any
/// prior opinion, so for evaluation purposes it is equivalent to never
/// having published one. The audit history is still preserved on disk
/// via the chain.
pub fn evaluate_trust<R: TrustResolver>(
    by_actor: &ActorId,
    of_actor: &ActorId,
    trust_resolver: &R,
) -> Result<TrustEvaluation, R::Error> {
    match trust_resolver.latest_trust(by_actor, of_actor)? {
        None => Ok(TrustEvaluation::Unknown),
        Some(signed) => Ok(match signed.unsigned().body().decision() {
            Some(TrustDecision::Trusted) => TrustEvaluation::Trusted,
            Some(TrustDecision::Untrusted) => TrustEvaluation::Untrusted,
            None => TrustEvaluation::Unknown,
        }),
    }
}

/// What the capability evaluator is being asked to authorize. See
/// `specs/CAPABILITIES.md` §6.1.
///
/// `Object` is the only target with usable kinds in v1; `Actor`
/// targets exist for forward compatibility (no statement kind is
/// valid for `CapabilityScope::Actor` per §4.3, so the evaluator
/// short-circuits to `NotHeld`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionTarget {
    Object { id: ObjectId, kind: StatementKind },
    Actor { id: ActorId, kind: StatementKind },
}

impl ResolutionTarget {
    pub fn kind(&self) -> StatementKind {
        match self {
            Self::Object { kind, .. } | Self::Actor { kind, .. } => *kind,
        }
    }

    fn scope(&self) -> CapabilityScope {
        match self {
            Self::Object { id, .. } => CapabilityScope::Object(id.clone()),
            Self::Actor { id, .. } => CapabilityScope::Actor(id.clone()),
        }
    }
}

/// Outcome of [`evaluate_capability`]. See `specs/CAPABILITIES.md`
/// §6.1 for the exact decision rules.
///
/// `Revoked` and `Expired` carry the offending grant's `StatementId`
/// so callers can audit *which* grant in the chain failed.
/// `DelegationTooDeep` and `GrantorLacksAuthority` aggregate
/// information about the chain rather than naming a single grant —
/// the chain shape is the failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityEvaluation {
    /// `grantee` is authorized for `(target.kind, target.id)` at the
    /// requested causal position.
    Held,
    /// No covering grant exists.
    NotHeld,
    /// A covering grant exists but is revoked at the requested
    /// position (default revocation when `at >= revocation.created_at`,
    /// or any retroactive revocation regardless of `at`).
    Revoked(StatementId),
    /// A covering grant exists but its `ExpiresAt` constraint is in
    /// the past relative to the requested position.
    Expired(StatementId),
    /// A covering grant exists but the chain that would authorize it
    /// exceeds some grant's `MaxDelegationDepth`.
    DelegationTooDeep,
    /// A covering grant exists but its grantor does not hold the
    /// authority to have issued it (no root-authority match and no
    /// recursive delegable chain reaches the root). Also returned for
    /// cycles in the delegation graph.
    GrantorLacksAuthority,
}

/// Lookup interface for capability evaluation.
///
/// Implementors return the structural primitives the evaluator
/// composes:
///
/// - `latest_capability` — chain leaf for a `(grantor, grantee, scope)`
///   triple.
/// - `latest_capability_revocation` — most-restrictive revocation a
///   grantor has issued against one of their grants.
/// - `capability_grantors_for` — every grantor who has a chain-leaf
///   grant naming `grantee` on `object`.
/// - `object_root_authority` — the actors empowered to originate any
///   capability on `object` (the root authority set; v1 returns a
///   single-element vector containing the object's `created_by`).
///
/// Mirrors [`TrustResolver`] in shape: a small generic trait that
/// can be satisfied by any backing store (filesystem, in-memory,
/// network), with an associated `Error` so callers don't lock to a
/// single error type.
pub trait CapabilityResolver {
    type Error: std::error::Error + 'static;

    fn latest_capability(
        &self,
        grantor: &ActorId,
        grantee: &ActorId,
        scope: &CapabilityScope,
    ) -> Result<Option<SignedStatement<ActorCapabilityGrantBody>>, Self::Error>;

    fn latest_capability_revocation(
        &self,
        grantor: &ActorId,
        revoked_grant: &StatementId,
    ) -> Result<Option<SignedStatement<ActorCapabilityRevocationBody>>, Self::Error>;

    fn capability_grantors_for(
        &self,
        object: &ObjectId,
        grantee: &ActorId,
    ) -> Result<Vec<ActorId>, Self::Error>;

    fn object_root_authority(
        &self,
        object: &ObjectId,
    ) -> Result<Option<Vec<ActorId>>, Self::Error>;

    /// Whether `(actor, key_id)` is revoked at causal position `at`.
    ///
    /// Drives the `KeyPinned` constraint check in
    /// [`evaluate_capability`]: a pinned grant whose pinned key is
    /// revoked collapses to [`CapabilityEvaluation::Revoked`]
    /// (`specs/CAPABILITIES.md` §7.2). The default returns `false`,
    /// preserving the v1 behavior where `KeyPinned` was declarative
    /// only — backing stores override this to consult their per-actor
    /// revocation set.
    fn is_key_revoked_at(
        &self,
        _actor: &ActorId,
        _key_id: &kairo_identity::KeyId,
        _at: Timestamp,
    ) -> Result<bool, Self::Error> {
        Ok(false)
    }
}

/// Resolve "does `grantee` have authority to issue statements of
/// kind `target.kind()` against `target` at causal position `at`?"
/// against `resolver`.
///
/// Returns the structured outcome as a [`CapabilityEvaluation`].
/// See `specs/CAPABILITIES.md` §6.1 for the decision rules; the
/// implementation walks every candidate grantor produced by
/// `capability_grantors_for` and returns the first non-failure
/// outcome (`Held`), or the most informative failure observed.
///
/// Cycle detection: the evaluator tracks the
/// `(grantor, grantee, scope)` triples already on the resolution
/// path and returns `GrantorLacksAuthority` rather than recurring
/// into a cycle. `MaxDelegationDepth` enforces a separate
/// per-constraint bound on chain length.
pub fn evaluate_capability<R: CapabilityResolver>(
    grantee: &ActorId,
    target: &ResolutionTarget,
    at: Timestamp,
    resolver: &R,
) -> Result<CapabilityEvaluation, R::Error> {
    let mut visited = HashSet::new();
    evaluate_inner(grantee, target, at, false, resolver, &mut visited, 0)
}

fn evaluate_inner<R: CapabilityResolver>(
    grantee: &ActorId,
    target: &ResolutionTarget,
    at: Timestamp,
    require_delegable: bool,
    resolver: &R,
    visited: &mut HashSet<(ActorId, ActorId, CapabilityScope)>,
    depth: usize,
) -> Result<CapabilityEvaluation, R::Error> {
    let object_id = match target {
        ResolutionTarget::Object { id, .. } => id.clone(),
        ResolutionTarget::Actor { .. } => {
            // No statement kinds are valid for actor scope in v1
            // (`specs/CAPABILITIES.md` §4.3). Future actor-surface
            // kinds will lift this short-circuit.
            return Ok(CapabilityEvaluation::NotHeld);
        }
    };
    let scope = target.scope();
    let kind = target.kind();

    let candidate_grantors = resolver.capability_grantors_for(&object_id, grantee)?;
    if candidate_grantors.is_empty() {
        return Ok(CapabilityEvaluation::NotHeld);
    }

    let mut best_failure: Option<CapabilityEvaluation> = None;

    for grantor in candidate_grantors {
        let key = (grantor.clone(), grantee.clone(), scope.clone());
        if visited.contains(&key) {
            // Cycle on this resolution path — treat as
            // "grantor authority unproven" per §6.1.
            best_failure.get_or_insert(CapabilityEvaluation::GrantorLacksAuthority);
            continue;
        }

        let Some(grant) = resolver.latest_capability(&grantor, grantee, &scope)? else {
            // Index pointed at a grantor whose chain leaf is gone.
            // Treat as no covering grant from this grantor; the
            // resolver / index inconsistency will surface elsewhere.
            continue;
        };
        let body = grant.unsigned().body();
        let cap = body.capability();
        let grant_id = grant.statement_id();

        if !cap.statement_kinds().contains(&kind) {
            continue;
        }

        if require_delegable && !cap.delegable() {
            // Grant exists but doesn't permit re-grant; cannot
            // satisfy a recursive authority check.
            continue;
        }

        // Revocation (§6.1 condition 4 + §6.3).
        if let Some(revocation) =
            resolver.latest_capability_revocation(&grantor, &grant_id)?
        {
            let rev_at = revocation.unsigned().created_at();
            let retroactive = revocation.unsigned().body().retroactive();
            if retroactive || at >= rev_at {
                best_failure
                    .get_or_insert(CapabilityEvaluation::Revoked(grant_id.clone()));
                continue;
            }
        }

        // Constraints (§6.1 condition 6).
        let mut constraint_failure: Option<CapabilityEvaluation> = None;
        for constraint in cap.constraints() {
            match constraint {
                CapabilityConstraint::ExpiresAt(ts) => {
                    if at > *ts {
                        constraint_failure =
                            Some(CapabilityEvaluation::Expired(grant_id.clone()));
                        break;
                    }
                }
                CapabilityConstraint::MaxDelegationDepth(max) => {
                    if depth > *max as usize {
                        constraint_failure = Some(CapabilityEvaluation::DelegationTooDeep);
                        break;
                    }
                }
                CapabilityConstraint::KeyPinned(key_id) => {
                    // §7.2 — pinned grant collapses to Revoked the
                    // moment the named key is revoked, regardless of
                    // whether the revocation is retroactive. The
                    // grantor is the authority for the pinned key.
                    if resolver.is_key_revoked_at(&grantor, key_id, at)? {
                        constraint_failure =
                            Some(CapabilityEvaluation::Revoked(grant_id.clone()));
                        break;
                    }
                }
            }
        }
        if let Some(failure) = constraint_failure {
            best_failure.get_or_insert(failure);
            continue;
        }

        // Grantor authority (§6.1 condition 5).
        let root = resolver
            .object_root_authority(&object_id)?
            .unwrap_or_default();
        if root.contains(&grantor) {
            return Ok(CapabilityEvaluation::Held);
        }

        // Recurse: grantor must hold a delegable capability covering
        // `kind` on this object at `at`.
        visited.insert(key.clone());
        let recursive_target = ResolutionTarget::Object {
            id: object_id.clone(),
            kind,
        };
        let recursive = evaluate_inner(
            &grantor,
            &recursive_target,
            at,
            true,
            resolver,
            visited,
            depth + 1,
        )?;
        visited.remove(&key);

        match recursive {
            CapabilityEvaluation::Held => return Ok(CapabilityEvaluation::Held),
            // Concrete chain-failure modes propagate verbatim — the
            // chain exists but breaks for a specific reason (a
            // parent grant was revoked / expired / over-deep). The
            // returned StatementId names the parent grant where the
            // chain broke, which is what an auditor needs.
            failure @ (CapabilityEvaluation::Revoked(_)
            | CapabilityEvaluation::Expired(_)
            | CapabilityEvaluation::DelegationTooDeep) => {
                best_failure.get_or_insert(failure);
            }
            // Structural failures (no chain reaches the root, or a
            // cycle) collapse to GrantorLacksAuthority at this
            // level. `NotHeld` from the recursive call means the
            // grantor has no covering grant on this object → they
            // could not have issued G.
            CapabilityEvaluation::NotHeld | CapabilityEvaluation::GrantorLacksAuthority => {
                best_failure.get_or_insert(CapabilityEvaluation::GrantorLacksAuthority);
            }
        }
    }

    Ok(best_failure.unwrap_or(CapabilityEvaluation::NotHeld))
}

/// Verify an envelope-wrapped signed statement against an [`ActorResolver`].
///
/// This never returns an `Err`: every observable outcome is encoded as a
/// variant of [`SignatureStatus`] / [`ActorResolution`] inside the report.
/// Operational errors from the resolver appear as
/// [`ActorResolution::ResolverUnavailable`].
pub fn verify_envelope_statement<B, R>(
    statement: &SignedStatement<B>,
    resolver: &R,
) -> VerificationReport
where
    B: StatementBody,
    R: ActorResolver,
{
    let statement_id = statement.statement_id();
    let envelope_actor = statement.unsigned().actor().clone();
    let signature_actor = statement.signature().actor().clone();
    let created_at = statement.unsigned().created_at();

    if signature_actor != envelope_actor {
        return VerificationReport {
            statement_id,
            envelope_actor,
            signature_actor,
            actor: ActorResolution::SignatureActorMismatch,
            signature: SignatureStatus::NotEvaluated,
            trust: TrustEvaluation::Unevaluated,
        };
    }

    // Confirm the actor exists before consulting the rotation chain.
    match resolver.actor_genesis(&envelope_actor) {
        Ok(Some(_)) => {}
        Ok(None) => {
            return VerificationReport {
                statement_id,
                envelope_actor,
                signature_actor,
                actor: ActorResolution::NotFound,
                signature: SignatureStatus::NotEvaluated,
                trust: TrustEvaluation::Unevaluated,
            };
        }
        Err(ActorResolveError::Unavailable(reason)) => {
            return VerificationReport {
                statement_id,
                envelope_actor,
                signature_actor,
                actor: ActorResolution::ResolverUnavailable(reason),
                signature: SignatureStatus::NotEvaluated,
                trust: TrustEvaluation::Unevaluated,
            };
        }
    }

    // Surface dispatch by statement kind. Operational kinds verify
    // against the active signing key per the rotation chain (§6.1
    // bullet 2a); emergency kinds verify against the attestation key
    // set per §5.5.2 (§6.1 bullet 2b). The two surfaces never overlap.
    let resolved_key = match B::SIGNING_SURFACE {
        SigningSurface::Operational => {
            // Resolve the active key at `created_at` per the rotation
            // chain. Falls back to the genesis-initial key when no
            // rotations precede `created_at`.
            let active_key = match resolver.active_key_at(&envelope_actor, created_at) {
                Ok(Some(key)) => key,
                Ok(None) => {
                    return VerificationReport {
                        statement_id,
                        envelope_actor,
                        signature_actor,
                        actor: ActorResolution::Resolved,
                        signature: SignatureStatus::NoActiveKey,
                        trust: TrustEvaluation::Unevaluated,
                    };
                }
                Err(ActorResolveError::Unavailable(reason)) => {
                    return VerificationReport {
                        statement_id,
                        envelope_actor,
                        signature_actor,
                        actor: ActorResolution::ResolverUnavailable(reason),
                        signature: SignatureStatus::NotEvaluated,
                        trust: TrustEvaluation::Unevaluated,
                    };
                }
            };

            let active_key_id = active_key.key_id();
            if statement.signature().key_id() != active_key_id.as_str() {
                return VerificationReport {
                    statement_id,
                    envelope_actor,
                    signature_actor,
                    actor: ActorResolution::Resolved,
                    signature: SignatureStatus::KeyMismatch {
                        signature_key_id: statement.signature().key_id().to_owned(),
                        active_key_id: active_key_id.to_string(),
                    },
                    trust: TrustEvaluation::Unevaluated,
                };
            }

            match resolver.is_key_revoked_at(&envelope_actor, &active_key_id, created_at) {
                Ok(true) => {
                    return VerificationReport {
                        statement_id,
                        envelope_actor,
                        signature_actor,
                        actor: ActorResolution::Resolved,
                        signature: SignatureStatus::KeyRevoked,
                        trust: TrustEvaluation::Unevaluated,
                    };
                }
                Ok(false) => {}
                Err(ActorResolveError::Unavailable(reason)) => {
                    return VerificationReport {
                        statement_id,
                        envelope_actor,
                        signature_actor,
                        actor: ActorResolution::ResolverUnavailable(reason),
                        signature: SignatureStatus::NotEvaluated,
                        trust: TrustEvaluation::Unevaluated,
                    };
                }
            }

            active_key
        }
        SigningSurface::Attestation => {
            // Look up the actor's attestation set at `created_at`. The
            // signature's `key_id` must be in this set; the matching
            // public key is then used for byte verification.
            let attestation_set = match resolver
                .attestation_keys_at(&envelope_actor, created_at)
            {
                Ok(set) => set,
                Err(ActorResolveError::Unavailable(reason)) => {
                    return VerificationReport {
                        statement_id,
                        envelope_actor,
                        signature_actor,
                        actor: ActorResolution::ResolverUnavailable(reason),
                        signature: SignatureStatus::NotEvaluated,
                        trust: TrustEvaluation::Unevaluated,
                    };
                }
            };

            let signature_key_id = statement.signature().key_id();
            let lookup =
                kairo_identity::KeyId::new(signature_key_id.to_owned());
            match attestation_set.get(&lookup) {
                Some(key) => key.clone(),
                None => {
                    return VerificationReport {
                        statement_id,
                        envelope_actor,
                        signature_actor,
                        actor: ActorResolution::Resolved,
                        signature: SignatureStatus::NotInAttestationSet {
                            signature_key_id: signature_key_id.to_owned(),
                        },
                        trust: TrustEvaluation::Unevaluated,
                    };
                }
            }
        }
    };

    let signature = match statement.verify_signature(&resolved_key) {
        Ok(_) => SignatureStatus::Valid,
        Err(StatementSignatureError::Verification(
            SignatureVerificationError::InvalidSignature,
        )) => SignatureStatus::Invalid,
        Err(StatementSignatureError::Verification(
            SignatureVerificationError::InvalidPublicKey,
        )) => {
            // The resolved key (operational or attestation) is
            // malformed. Surface as an operational resolver issue
            // rather than an invalid statement.
            return VerificationReport {
                statement_id,
                envelope_actor,
                signature_actor,
                actor: ActorResolution::ResolverUnavailable(
                    "resolved key is malformed".to_owned(),
                ),
                signature: SignatureStatus::NotEvaluated,
                trust: TrustEvaluation::Unevaluated,
            };
        }
        Err(StatementSignatureError::Verification(
            SignatureVerificationError::AlgorithmMismatch { .. },
        )) => SignatureStatus::AlgorithmMismatch,
        Err(StatementSignatureError::UnsupportedAlgorithm(algorithm)) => {
            SignatureStatus::UnsupportedAlgorithm(algorithm)
        }
        Err(StatementSignatureError::InvalidSignatureLength { expected, actual }) => {
            SignatureStatus::Malformed {
                expected_len: expected,
                actual_len: actual,
            }
        }
    };

    VerificationReport {
        statement_id,
        envelope_actor,
        signature_actor,
        actor: ActorResolution::Resolved,
        signature,
        trust: TrustEvaluation::Unevaluated,
    }
}

impl VerificationReport {
    /// True only if the signature verified and the actor was resolved.
    /// This says nothing about local trust.
    pub fn is_cryptographically_valid(&self) -> bool {
        matches!(self.actor, ActorResolution::Resolved)
            && matches!(self.signature, SignatureStatus::Valid)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic, clippy::too_many_arguments)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};
    use kairo_core::canonical::CanonicalEncode;
    use kairo_core::{ActorId, BlobId, KairoRef, ObjectId, Timestamp};
    use kairo_identity::{
        ActorGenesisBody, ActorKind, ActorResolveError, ActorResolver, MemoryActorResolver,
        PublicKey,
    };

    use super::*;
    use crate::{ObjectRevisionBody, RevisionId, Signature, SignedStatement, UnsignedStatement};

    const OBJECT_ID: &str = "zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk";
    const BLOB_ID: &str = "zQmfBE2w2UqKJhGZAxK4ZWb4JuCrvxnN9P4YKuvzfbPSvD5";

    fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&[7; 32])
    }

    fn other_signing_key() -> SigningKey {
        SigningKey::from_bytes(&[8; 32])
    }

    fn public_key_for(key: &SigningKey) -> PublicKey {
        PublicKey::ed25519(key.verifying_key().to_bytes())
    }

    fn timestamp() -> Timestamp {
        Timestamp::from_seconds(1_700_000_000)
    }

    fn attestation_key() -> PublicKey {
        PublicKey::ed25519(SigningKey::from_bytes(&[200; 32]).verifying_key().to_bytes())
    }

    fn genesis_for(key: &SigningKey) -> ActorGenesisBody {
        ActorGenesisBody::new(
            ActorKind::person(),
            public_key_for(key),
            vec![attestation_key()],
            timestamp(),
            [9; 32],
        )
        .expect("genesis well-formed")
    }

    fn revision_body() -> Result<ObjectRevisionBody, kairo_core::IdError> {
        Ok(ObjectRevisionBody::new(
            ObjectId::new(OBJECT_ID)?,
            RevisionId::new("git:sha256:revision"),
            vec![RevisionId::new("git:sha256:parent")],
            BlobId::new(BLOB_ID)?,
            true,
        ))
    }

    fn object_ref() -> Result<KairoRef, kairo_core::IdError> {
        format!("object:{OBJECT_ID}").parse()
    }

    fn unsigned_for_actor(
        actor: ActorId,
    ) -> Result<UnsignedStatement<ObjectRevisionBody>, kairo_core::IdError> {
        Ok(UnsignedStatement::new(
            actor,
            object_ref()?,
            timestamp(),
            revision_body()?,
        ))
    }

    fn sign_with(
        unsigned: &UnsignedStatement<ObjectRevisionBody>,
        key: &SigningKey,
        sig_actor: ActorId,
        algorithm: &str,
        bytes_override: Option<Vec<u8>>,
    ) -> Signature {
        let bytes = bytes_override
            .unwrap_or_else(|| key.sign(&unsigned.canonical_bytes()).to_bytes().to_vec());
        Signature::new(
            sig_actor,
            public_key_for(key).key_id().to_string(),
            algorithm,
            bytes,
        )
    }

    fn resolver_with(genesis: ActorGenesisBody) -> MemoryActorResolver {
        let mut resolver = MemoryActorResolver::new();
        resolver.insert(genesis);
        resolver
    }

    #[derive(Debug, Default)]
    struct UnavailableResolver;

    impl ActorResolver for UnavailableResolver {
        fn actor_genesis(
            &self,
            _actor: &ActorId,
        ) -> Result<Option<ActorGenesisBody>, ActorResolveError> {
            Err(ActorResolveError::Unavailable("backend down".to_owned()))
        }
    }

    #[test]
    fn valid_signature_with_resolved_actor() -> Result<(), Box<dyn std::error::Error>> {
        let key = signing_key();
        let genesis = genesis_for(&key);
        let actor_id = genesis.actor_id();
        let unsigned = unsigned_for_actor(actor_id.clone())?;
        let signature = sign_with(&unsigned, &key, actor_id, "ed25519", None);
        let signed = SignedStatement::new(unsigned, signature);

        let report = verify_envelope_statement(&signed, &resolver_with(genesis));

        assert_eq!(report.actor, ActorResolution::Resolved);
        assert_eq!(report.signature, SignatureStatus::Valid);
        assert_eq!(report.trust, TrustEvaluation::Unevaluated);
        assert!(report.is_cryptographically_valid());
        Ok(())
    }

    #[test]
    fn invalid_signature_after_signing() -> Result<(), Box<dyn std::error::Error>> {
        let key = signing_key();
        let genesis = genesis_for(&key);
        let actor_id = genesis.actor_id();
        let unsigned = unsigned_for_actor(actor_id.clone())?;
        // Sign one body, then swap the signed body for a different one.
        let signature = sign_with(&unsigned, &key, actor_id.clone(), "ed25519", None);
        let other_body = ObjectRevisionBody::new(
            ObjectId::new(OBJECT_ID)?,
            RevisionId::new("git:sha256:other-revision"),
            vec![RevisionId::new("git:sha256:parent")],
            BlobId::new(BLOB_ID)?,
            true,
        );
        let tampered = UnsignedStatement::new(actor_id, object_ref()?, timestamp(), other_body);
        let signed = SignedStatement::new(tampered, signature);

        let report = verify_envelope_statement(&signed, &resolver_with(genesis));

        assert_eq!(report.actor, ActorResolution::Resolved);
        assert_eq!(report.signature, SignatureStatus::Invalid);
        assert!(!report.is_cryptographically_valid());
        Ok(())
    }

    #[test]
    fn actor_not_found() -> Result<(), Box<dyn std::error::Error>> {
        let key = signing_key();
        let genesis = genesis_for(&key);
        let actor_id = genesis.actor_id();
        let unsigned = unsigned_for_actor(actor_id.clone())?;
        let signature = sign_with(&unsigned, &key, actor_id, "ed25519", None);
        let signed = SignedStatement::new(unsigned, signature);

        let report = verify_envelope_statement(&signed, &MemoryActorResolver::new());

        assert_eq!(report.actor, ActorResolution::NotFound);
        assert_eq!(report.signature, SignatureStatus::NotEvaluated);
        Ok(())
    }

    #[test]
    fn resolver_unavailable() -> Result<(), Box<dyn std::error::Error>> {
        let key = signing_key();
        let genesis = genesis_for(&key);
        let actor_id = genesis.actor_id();
        let unsigned = unsigned_for_actor(actor_id.clone())?;
        let signature = sign_with(&unsigned, &key, actor_id, "ed25519", None);
        let signed = SignedStatement::new(unsigned, signature);

        let report = verify_envelope_statement(&signed, &UnavailableResolver);

        assert!(matches!(
            report.actor,
            ActorResolution::ResolverUnavailable(_)
        ));
        assert_eq!(report.signature, SignatureStatus::NotEvaluated);
        Ok(())
    }

    #[test]
    fn signature_actor_mismatch() -> Result<(), Box<dyn std::error::Error>> {
        let key = signing_key();
        let envelope_genesis = genesis_for(&key);
        let other_genesis = genesis_for(&other_signing_key());
        let envelope_actor = envelope_genesis.actor_id();
        let other_actor = other_genesis.actor_id();
        assert_ne!(envelope_actor, other_actor);

        let unsigned = unsigned_for_actor(envelope_actor)?;
        // Signature claims a different actor than the envelope.
        let signature = sign_with(&unsigned, &key, other_actor, "ed25519", None);
        let signed = SignedStatement::new(unsigned, signature);

        let report = verify_envelope_statement(&signed, &resolver_with(envelope_genesis));

        assert_eq!(report.actor, ActorResolution::SignatureActorMismatch);
        assert_eq!(report.signature, SignatureStatus::NotEvaluated);
        Ok(())
    }

    #[test]
    fn unsupported_algorithm() -> Result<(), Box<dyn std::error::Error>> {
        let key = signing_key();
        let genesis = genesis_for(&key);
        let actor_id = genesis.actor_id();
        let unsigned = unsigned_for_actor(actor_id.clone())?;
        let signature = sign_with(&unsigned, &key, actor_id, "exotic", None);
        let signed = SignedStatement::new(unsigned, signature);

        let report = verify_envelope_statement(&signed, &resolver_with(genesis));

        assert_eq!(report.actor, ActorResolution::Resolved);
        assert!(
            matches!(report.signature, SignatureStatus::UnsupportedAlgorithm(ref a) if a == "exotic")
        );
        Ok(())
    }

    #[test]
    fn malformed_signature_length() -> Result<(), Box<dyn std::error::Error>> {
        let key = signing_key();
        let genesis = genesis_for(&key);
        let actor_id = genesis.actor_id();
        let unsigned = unsigned_for_actor(actor_id.clone())?;
        let signature = sign_with(&unsigned, &key, actor_id, "ed25519", Some(vec![0; 16]));
        let signed = SignedStatement::new(unsigned, signature);

        let report = verify_envelope_statement(&signed, &resolver_with(genesis));

        assert_eq!(report.actor, ActorResolution::Resolved);
        assert!(matches!(
            report.signature,
            SignatureStatus::Malformed {
                expected_len: 64,
                actual_len: 16
            }
        ));
        Ok(())
    }

    #[test]
    fn wrong_signing_key() -> Result<(), Box<dyn std::error::Error>> {
        // Genesis declares key A; statement is signed with key B and
        // honestly carries B's key_id. Verification rejects at the
        // key-id check before even examining the bytes — the active
        // key for this actor at `created_at` is A, not B.
        let envelope_key = signing_key();
        let genesis = genesis_for(&envelope_key);
        let actor_id = genesis.actor_id();
        let unsigned = unsigned_for_actor(actor_id.clone())?;
        let attacker = other_signing_key();
        let signature = sign_with(&unsigned, &attacker, actor_id, "ed25519", None);
        let signed = SignedStatement::new(unsigned, signature);

        let report = verify_envelope_statement(&signed, &resolver_with(genesis));

        assert_eq!(report.actor, ActorResolution::Resolved);
        assert!(matches!(
            report.signature,
            SignatureStatus::KeyMismatch { .. }
        ));
        Ok(())
    }

    #[test]
    fn forged_key_id_with_attacker_bytes_is_invalid() -> Result<(), Box<dyn std::error::Error>> {
        // Genesis declares key A. Attacker signs with key B but forges
        // A's key_id onto the signature (claiming to be A). The key-id
        // check passes, but the bytes do not verify against A — so the
        // result is `Invalid`, not `KeyMismatch`.
        let envelope_key = signing_key();
        let genesis = genesis_for(&envelope_key);
        let actor_id = genesis.actor_id();
        let unsigned = unsigned_for_actor(actor_id.clone())?;
        let attacker = other_signing_key();
        let attacker_bytes = attacker.sign(&unsigned.canonical_bytes()).to_bytes().to_vec();
        let signature = Signature::new(
            actor_id,
            public_key_for(&envelope_key).key_id().to_string(),
            "ed25519",
            attacker_bytes,
        );
        let signed = SignedStatement::new(unsigned, signature);

        let report = verify_envelope_statement(&signed, &resolver_with(genesis));

        assert_eq!(report.actor, ActorResolution::Resolved);
        assert_eq!(report.signature, SignatureStatus::Invalid);
        Ok(())
    }

    #[test]
    fn report_records_statement_and_actor_ids() -> Result<(), Box<dyn std::error::Error>> {
        let key = signing_key();
        let genesis = genesis_for(&key);
        let actor_id = genesis.actor_id();
        let unsigned = unsigned_for_actor(actor_id.clone())?;
        let expected_id = unsigned.statement_id();
        let signature = sign_with(&unsigned, &key, actor_id.clone(), "ed25519", None);
        let signed = SignedStatement::new(unsigned, signature);

        let report = verify_envelope_statement(&signed, &resolver_with(genesis));

        assert_eq!(report.statement_id, expected_id);
        assert_eq!(report.envelope_actor, actor_id);
        assert_eq!(report.signature_actor, actor_id);
        Ok(())
    }

    use std::collections::HashMap;
    use std::convert::Infallible;

    use crate::ActorTrustBody;

    /// Test-only in-memory `TrustResolver`. Callers pre-populate
    /// `(by_actor, trusted_actor) -> SignedStatement<ActorTrustBody>`.
    #[derive(Debug, Default)]
    struct MemoryTrustResolver {
        entries: HashMap<(String, String), SignedStatement<ActorTrustBody>>,
    }

    impl MemoryTrustResolver {
        fn insert(&mut self, signed: SignedStatement<ActorTrustBody>) {
            let by_actor = signed.unsigned().actor().to_string();
            let trusted_actor = signed.unsigned().body().trusted_actor().to_string();
            self.entries.insert((by_actor, trusted_actor), signed);
        }
    }

    impl TrustResolver for MemoryTrustResolver {
        type Error = Infallible;

        fn latest_trust(
            &self,
            by_actor: &ActorId,
            trusted_actor: &ActorId,
        ) -> Result<Option<SignedStatement<ActorTrustBody>>, Self::Error> {
            Ok(self
                .entries
                .get(&(by_actor.to_string(), trusted_actor.to_string()))
                .cloned())
        }
    }

    fn signed_actor_trust(
        by_actor: &ActorId,
        trusted_actor: &ActorId,
        decision: Option<TrustDecision>,
        supersedes: Option<crate::StatementId>,
    ) -> Result<SignedStatement<ActorTrustBody>, Box<dyn std::error::Error>> {
        let body = ActorTrustBody::new(trusted_actor.clone(), decision, None, supersedes)?;
        let subject: KairoRef = format!("actor:{trusted_actor}").parse()?;
        let unsigned = UnsignedStatement::new(by_actor.clone(), subject, timestamp(), body);
        let key = signing_key();
        let bytes = key.sign(&unsigned.canonical_bytes()).to_bytes().to_vec();
        let signature = Signature::new(
            by_actor.clone(),
            public_key_for(&key).key_id().to_string(),
            "ed25519",
            bytes,
        );
        Ok(SignedStatement::new(unsigned, signature))
    }

    fn fresh_actor(seed: u8) -> ActorId {
        ActorGenesisBody::new(
            ActorKind::person(),
            public_key_for(&SigningKey::from_bytes(&[seed; 32])),
            vec![attestation_key()],
            timestamp(),
            [seed; 32],
        )
        .expect("genesis well-formed")
        .actor_id()
    }

    #[test]
    fn evaluate_trust_returns_unknown_when_no_opinion()
    -> Result<(), Box<dyn std::error::Error>> {
        let by_actor = fresh_actor(1);
        let of_actor = fresh_actor(2);
        let resolver = MemoryTrustResolver::default();
        let evaluation = evaluate_trust(&by_actor, &of_actor, &resolver)?;
        assert_eq!(evaluation, TrustEvaluation::Unknown);
        Ok(())
    }

    #[test]
    fn evaluate_trust_returns_trusted_for_grant() -> Result<(), Box<dyn std::error::Error>> {
        let by_actor = fresh_actor(1);
        let of_actor = fresh_actor(2);
        let mut resolver = MemoryTrustResolver::default();
        resolver.insert(signed_actor_trust(
            &by_actor,
            &of_actor,
            Some(TrustDecision::Trusted),
            None,
        )?);
        let evaluation = evaluate_trust(&by_actor, &of_actor, &resolver)?;
        assert_eq!(evaluation, TrustEvaluation::Trusted);
        Ok(())
    }

    #[test]
    fn evaluate_trust_returns_untrusted_for_block()
    -> Result<(), Box<dyn std::error::Error>> {
        let by_actor = fresh_actor(1);
        let of_actor = fresh_actor(2);
        let mut resolver = MemoryTrustResolver::default();
        resolver.insert(signed_actor_trust(
            &by_actor,
            &of_actor,
            Some(TrustDecision::Untrusted),
            None,
        )?);
        let evaluation = evaluate_trust(&by_actor, &of_actor, &resolver)?;
        assert_eq!(evaluation, TrustEvaluation::Untrusted);
        Ok(())
    }

    #[test]
    fn evaluate_trust_treats_withdrawal_as_unknown()
    -> Result<(), Box<dyn std::error::Error>> {
        // Resolver returns the chain leaf, which here is the
        // withdrawal. evaluate_trust collapses that to Unknown.
        let by_actor = fresh_actor(1);
        let of_actor = fresh_actor(2);
        let grant = signed_actor_trust(
            &by_actor,
            &of_actor,
            Some(TrustDecision::Trusted),
            None,
        )?;
        let withdraw = signed_actor_trust(
            &by_actor,
            &of_actor,
            None,
            Some(grant.statement_id()),
        )?;
        let mut resolver = MemoryTrustResolver::default();
        resolver.insert(withdraw);
        let evaluation = evaluate_trust(&by_actor, &of_actor, &resolver)?;
        assert_eq!(evaluation, TrustEvaluation::Unknown);
        Ok(())
    }

    #[test]
    fn evaluate_trust_is_per_truster() -> Result<(), Box<dyn std::error::Error>> {
        // Truster A grants; truster B has no opinion. Same target.
        let truster_a = fresh_actor(1);
        let truster_b = fresh_actor(2);
        let target = fresh_actor(3);
        let mut resolver = MemoryTrustResolver::default();
        resolver.insert(signed_actor_trust(
            &truster_a,
            &target,
            Some(TrustDecision::Trusted),
            None,
        )?);
        assert_eq!(
            evaluate_trust(&truster_a, &target, &resolver)?,
            TrustEvaluation::Trusted
        );
        assert_eq!(
            evaluate_trust(&truster_b, &target, &resolver)?,
            TrustEvaluation::Unknown
        );
        Ok(())
    }

    use crate::{
        ActorCapabilityGrantBody, ActorCapabilityRevocationBody, Capability,
        CapabilityConstraint, CapabilityScope, StatementKind,
    };

    #[derive(Debug, Default)]
    struct MemoryCapabilityResolver {
        grants: HashMap<(String, String, String), SignedStatement<ActorCapabilityGrantBody>>,
        revocations:
            HashMap<(String, String), SignedStatement<ActorCapabilityRevocationBody>>,
        object_root: HashMap<String, Vec<ActorId>>,
        /// `(actor, key_id) -> (retroactive, created_at)`. Drives
        /// `is_key_revoked_at` for KeyPinned tests.
        revoked_keys: HashMap<(String, String), (bool, Timestamp)>,
    }

    impl MemoryCapabilityResolver {
        fn insert_grant(&mut self, signed: SignedStatement<ActorCapabilityGrantBody>) {
            let grantor = signed.unsigned().actor().to_string();
            let body = signed.unsigned().body();
            let grantee = body.grantee().to_string();
            let scope_key = scope_key_str(body.capability().scope());
            self.grants.insert((grantor, grantee, scope_key), signed);
        }

        fn insert_revocation(
            &mut self,
            signed: SignedStatement<ActorCapabilityRevocationBody>,
        ) {
            let grantor = signed.unsigned().actor().to_string();
            let revoked = signed.unsigned().body().revoked_grant().to_string();
            self.revocations.insert((grantor, revoked), signed);
        }

        fn set_root(&mut self, object: &ObjectId, root: Vec<ActorId>) {
            self.object_root.insert(object.to_string(), root);
        }

        fn revoke_key(
            &mut self,
            actor: &ActorId,
            key_id: &kairo_identity::KeyId,
            retroactive: bool,
            created_at: Timestamp,
        ) {
            self.revoked_keys.insert(
                (actor.to_string(), key_id.to_string()),
                (retroactive, created_at),
            );
        }
    }

    fn scope_key_str(scope: &CapabilityScope) -> String {
        match scope {
            CapabilityScope::Object(id) => format!("object:{id}"),
            CapabilityScope::Actor(id) => format!("actor:{id}"),
        }
    }

    impl CapabilityResolver for MemoryCapabilityResolver {
        type Error = Infallible;

        fn latest_capability(
            &self,
            grantor: &ActorId,
            grantee: &ActorId,
            scope: &CapabilityScope,
        ) -> Result<Option<SignedStatement<ActorCapabilityGrantBody>>, Self::Error> {
            Ok(self
                .grants
                .get(&(
                    grantor.to_string(),
                    grantee.to_string(),
                    scope_key_str(scope),
                ))
                .cloned())
        }

        fn latest_capability_revocation(
            &self,
            grantor: &ActorId,
            revoked_grant: &StatementId,
        ) -> Result<Option<SignedStatement<ActorCapabilityRevocationBody>>, Self::Error>
        {
            Ok(self
                .revocations
                .get(&(grantor.to_string(), revoked_grant.to_string()))
                .cloned())
        }

        fn capability_grantors_for(
            &self,
            object: &ObjectId,
            grantee: &ActorId,
        ) -> Result<Vec<ActorId>, Self::Error> {
            let scope = format!("object:{object}");
            let mut grantors: Vec<ActorId> = self
                .grants
                .keys()
                .filter(|(_, g, s)| g == grantee.as_str() && s == &scope)
                .map(|(grantor, _, _)| {
                    ActorId::new(grantor.clone()).expect("memory resolver holds valid actor ids")
                })
                .collect();
            grantors.sort();
            grantors.dedup();
            Ok(grantors)
        }

        fn object_root_authority(
            &self,
            object: &ObjectId,
        ) -> Result<Option<Vec<ActorId>>, Self::Error> {
            Ok(self.object_root.get(object.as_str()).cloned())
        }

        fn is_key_revoked_at(
            &self,
            actor: &ActorId,
            key_id: &kairo_identity::KeyId,
            at: Timestamp,
        ) -> Result<bool, Self::Error> {
            Ok(match self
                .revoked_keys
                .get(&(actor.to_string(), key_id.to_string()))
            {
                Some((retroactive, created_at)) => *retroactive || at >= *created_at,
                None => false,
            })
        }
    }

    fn signed_capability_grant(
        grantor: &ActorId,
        grantee: &ActorId,
        scope: CapabilityScope,
        kinds: Vec<StatementKind>,
        delegable: bool,
        constraints: Vec<CapabilityConstraint>,
        supersedes: Option<StatementId>,
        created_at: Timestamp,
    ) -> Result<SignedStatement<ActorCapabilityGrantBody>, Box<dyn std::error::Error>> {
        let cap = Capability::new(scope, kinds, delegable, constraints)?;
        let body = ActorCapabilityGrantBody::new(grantee.clone(), cap, supersedes);
        let subject: kairo_core::KairoRef = format!("actor:{grantee}").parse()?;
        let unsigned = UnsignedStatement::new(grantor.clone(), subject, created_at, body);
        let key = signing_key();
        let bytes = key.sign(&unsigned.canonical_bytes()).to_bytes().to_vec();
        let signature = Signature::new(
            grantor.clone(),
            public_key_for(&key).key_id().to_string(),
            "ed25519",
            bytes,
        );
        Ok(SignedStatement::new(unsigned, signature))
    }

    fn signed_capability_revocation(
        grantor: &ActorId,
        revoked_grant: StatementId,
        retroactive: bool,
        created_at: Timestamp,
    ) -> Result<SignedStatement<ActorCapabilityRevocationBody>, Box<dyn std::error::Error>>
    {
        let body =
            ActorCapabilityRevocationBody::new(revoked_grant.clone(), retroactive, None);
        let subject: kairo_core::KairoRef = format!("statement:{revoked_grant}").parse()?;
        let unsigned = UnsignedStatement::new(grantor.clone(), subject, created_at, body);
        let key = signing_key();
        let bytes = key.sign(&unsigned.canonical_bytes()).to_bytes().to_vec();
        let signature = Signature::new(
            grantor.clone(),
            public_key_for(&key).key_id().to_string(),
            "ed25519",
            bytes,
        );
        Ok(SignedStatement::new(unsigned, signature))
    }

    fn fresh_object(seed: u8) -> ObjectId {
        ObjectId::from_sha256_digest([seed; 32])
    }

    fn capability_target(object: &ObjectId, kind: StatementKind) -> ResolutionTarget {
        ResolutionTarget::Object {
            id: object.clone(),
            kind,
        }
    }

    #[test]
    fn evaluate_capability_held_for_root_authority_grantor()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = fresh_actor(1);
        let bob = fresh_actor(2);
        let object = fresh_object(9);
        let mut resolver = MemoryCapabilityResolver::default();
        resolver.set_root(&object, vec![root.clone()]);
        resolver.insert_grant(signed_capability_grant(
            &root,
            &bob,
            CapabilityScope::Object(object.clone()),
            vec![StatementKind::ObjectVersionTag],
            false,
            vec![],
            None,
            timestamp(),
        )?);

        let evaluation = evaluate_capability(
            &bob,
            &capability_target(&object, StatementKind::ObjectVersionTag),
            timestamp(),
            &resolver,
        )?;
        assert_eq!(evaluation, CapabilityEvaluation::Held);
        Ok(())
    }

    #[test]
    fn evaluate_capability_not_held_when_no_grant()
    -> Result<(), Box<dyn std::error::Error>> {
        let bob = fresh_actor(2);
        let object = fresh_object(9);
        let resolver = MemoryCapabilityResolver::default();

        let evaluation = evaluate_capability(
            &bob,
            &capability_target(&object, StatementKind::ObjectVersionTag),
            timestamp(),
            &resolver,
        )?;
        assert_eq!(evaluation, CapabilityEvaluation::NotHeld);
        Ok(())
    }

    #[test]
    fn evaluate_capability_not_held_when_kind_not_in_set()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = fresh_actor(1);
        let bob = fresh_actor(2);
        let object = fresh_object(9);
        let mut resolver = MemoryCapabilityResolver::default();
        resolver.set_root(&object, vec![root.clone()]);
        // Grant covers ObjectRevision; query asks about ObjectVersionTag.
        resolver.insert_grant(signed_capability_grant(
            &root,
            &bob,
            CapabilityScope::Object(object.clone()),
            vec![StatementKind::ObjectRevision],
            false,
            vec![],
            None,
            timestamp(),
        )?);

        let evaluation = evaluate_capability(
            &bob,
            &capability_target(&object, StatementKind::ObjectVersionTag),
            timestamp(),
            &resolver,
        )?;
        assert_eq!(evaluation, CapabilityEvaluation::NotHeld);
        Ok(())
    }

    #[test]
    fn evaluate_capability_revoked_default_after_revocation_time()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = fresh_actor(1);
        let bob = fresh_actor(2);
        let object = fresh_object(9);
        let mut resolver = MemoryCapabilityResolver::default();
        resolver.set_root(&object, vec![root.clone()]);
        let grant = signed_capability_grant(
            &root,
            &bob,
            CapabilityScope::Object(object.clone()),
            vec![StatementKind::ObjectVersionTag],
            false,
            vec![],
            None,
            timestamp(),
        )?;
        let grant_id = grant.statement_id();
        resolver.insert_grant(grant);
        resolver.insert_revocation(signed_capability_revocation(
            &root,
            grant_id.clone(),
            false,
            Timestamp::from_seconds(timestamp().seconds() + 100),
        )?);

        let after = Timestamp::from_seconds(timestamp().seconds() + 200);
        let evaluation = evaluate_capability(
            &bob,
            &capability_target(&object, StatementKind::ObjectVersionTag),
            after,
            &resolver,
        )?;
        assert_eq!(evaluation, CapabilityEvaluation::Revoked(grant_id));
        Ok(())
    }

    #[test]
    fn evaluate_capability_held_before_default_revocation()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = fresh_actor(1);
        let bob = fresh_actor(2);
        let object = fresh_object(9);
        let mut resolver = MemoryCapabilityResolver::default();
        resolver.set_root(&object, vec![root.clone()]);
        let grant = signed_capability_grant(
            &root,
            &bob,
            CapabilityScope::Object(object.clone()),
            vec![StatementKind::ObjectVersionTag],
            false,
            vec![],
            None,
            timestamp(),
        )?;
        let grant_id = grant.statement_id();
        resolver.insert_grant(grant);
        resolver.insert_revocation(signed_capability_revocation(
            &root,
            grant_id,
            false,
            Timestamp::from_seconds(timestamp().seconds() + 100),
        )?);

        let before = Timestamp::from_seconds(timestamp().seconds() + 50);
        let evaluation = evaluate_capability(
            &bob,
            &capability_target(&object, StatementKind::ObjectVersionTag),
            before,
            &resolver,
        )?;
        assert_eq!(evaluation, CapabilityEvaluation::Held);
        Ok(())
    }

    #[test]
    fn evaluate_capability_revoked_retroactive_even_before_revocation_time()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = fresh_actor(1);
        let bob = fresh_actor(2);
        let object = fresh_object(9);
        let mut resolver = MemoryCapabilityResolver::default();
        resolver.set_root(&object, vec![root.clone()]);
        let grant = signed_capability_grant(
            &root,
            &bob,
            CapabilityScope::Object(object.clone()),
            vec![StatementKind::ObjectVersionTag],
            false,
            vec![],
            None,
            timestamp(),
        )?;
        let grant_id = grant.statement_id();
        resolver.insert_grant(grant);
        resolver.insert_revocation(signed_capability_revocation(
            &root,
            grant_id.clone(),
            true,
            Timestamp::from_seconds(timestamp().seconds() + 100),
        )?);

        let before = Timestamp::from_seconds(timestamp().seconds() + 50);
        let evaluation = evaluate_capability(
            &bob,
            &capability_target(&object, StatementKind::ObjectVersionTag),
            before,
            &resolver,
        )?;
        assert_eq!(evaluation, CapabilityEvaluation::Revoked(grant_id));
        Ok(())
    }

    #[test]
    fn evaluate_capability_expired_when_at_after_expires_at()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = fresh_actor(1);
        let bob = fresh_actor(2);
        let object = fresh_object(9);
        let mut resolver = MemoryCapabilityResolver::default();
        resolver.set_root(&object, vec![root.clone()]);
        let expires_at = Timestamp::from_seconds(timestamp().seconds() + 100);
        let grant = signed_capability_grant(
            &root,
            &bob,
            CapabilityScope::Object(object.clone()),
            vec![StatementKind::ObjectVersionTag],
            false,
            vec![CapabilityConstraint::ExpiresAt(expires_at)],
            None,
            timestamp(),
        )?;
        let grant_id = grant.statement_id();
        resolver.insert_grant(grant);

        let after = Timestamp::from_seconds(timestamp().seconds() + 200);
        let evaluation = evaluate_capability(
            &bob,
            &capability_target(&object, StatementKind::ObjectVersionTag),
            after,
            &resolver,
        )?;
        assert_eq!(evaluation, CapabilityEvaluation::Expired(grant_id));
        Ok(())
    }

    #[test]
    fn key_pinned_collapses_to_revoked_when_pinned_key_is_revoked()
    -> Result<(), Box<dyn std::error::Error>> {
        // Root grants Bob a KeyPinned grant. Root then revokes the
        // pinned key. evaluate_capability collapses the grant to
        // Revoked at any time after the revocation, regardless of
        // whether an explicit ActorCapabilityRevocation was issued.
        let root = fresh_actor(1);
        let bob = fresh_actor(2);
        let object = fresh_object(9);
        let pinned_key = kairo_identity::KeyId::new(
            "zQmZ12345678901234567890123456789012345678901234".to_owned(),
        );
        let mut resolver = MemoryCapabilityResolver::default();
        resolver.set_root(&object, vec![root.clone()]);
        let grant = signed_capability_grant(
            &root,
            &bob,
            CapabilityScope::Object(object.clone()),
            vec![StatementKind::ObjectVersionTag],
            false,
            vec![CapabilityConstraint::KeyPinned(pinned_key.clone())],
            None,
            timestamp(),
        )?;
        let grant_id = grant.statement_id();
        resolver.insert_grant(grant);

        // Before the pinned key is revoked: Held.
        let evaluation = evaluate_capability(
            &bob,
            &capability_target(&object, StatementKind::ObjectVersionTag),
            timestamp(),
            &resolver,
        )?;
        assert_eq!(evaluation, CapabilityEvaluation::Held);

        // Revoke the pinned key (non-retroactive, at t+50).
        resolver.revoke_key(
            &root,
            &pinned_key,
            false,
            Timestamp::from_seconds(timestamp().seconds() + 50),
        );

        // Querying at t+100: Revoked.
        let evaluation = evaluate_capability(
            &bob,
            &capability_target(&object, StatementKind::ObjectVersionTag),
            Timestamp::from_seconds(timestamp().seconds() + 100),
            &resolver,
        )?;
        assert_eq!(evaluation, CapabilityEvaluation::Revoked(grant_id));
        Ok(())
    }

    #[test]
    fn key_pinned_retroactive_revocation_invalidates_grant_at_every_timestamp()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = fresh_actor(1);
        let bob = fresh_actor(2);
        let object = fresh_object(9);
        let pinned_key = kairo_identity::KeyId::new(
            "zQmZ12345678901234567890123456789012345678901234".to_owned(),
        );
        let mut resolver = MemoryCapabilityResolver::default();
        resolver.set_root(&object, vec![root.clone()]);
        let grant = signed_capability_grant(
            &root,
            &bob,
            CapabilityScope::Object(object.clone()),
            vec![StatementKind::ObjectVersionTag],
            false,
            vec![CapabilityConstraint::KeyPinned(pinned_key.clone())],
            None,
            timestamp(),
        )?;
        let grant_id = grant.statement_id();
        resolver.insert_grant(grant);

        // Retroactive revocation at t+50 invalidates even queries at t.
        resolver.revoke_key(
            &root,
            &pinned_key,
            true,
            Timestamp::from_seconds(timestamp().seconds() + 50),
        );

        let evaluation = evaluate_capability(
            &bob,
            &capability_target(&object, StatementKind::ObjectVersionTag),
            timestamp(),
            &resolver,
        )?;
        assert_eq!(evaluation, CapabilityEvaluation::Revoked(grant_id));
        Ok(())
    }

    #[test]
    fn evaluate_capability_held_in_delegated_chain()
    -> Result<(), Box<dyn std::error::Error>> {
        // root → A (delegable=true, MDD=2) → B (final user)
        let root = fresh_actor(1);
        let alice = fresh_actor(2);
        let bob = fresh_actor(3);
        let object = fresh_object(9);
        let mut resolver = MemoryCapabilityResolver::default();
        resolver.set_root(&object, vec![root.clone()]);
        // root → alice (delegable, MDD allows 2 deep)
        resolver.insert_grant(signed_capability_grant(
            &root,
            &alice,
            CapabilityScope::Object(object.clone()),
            vec![StatementKind::ObjectVersionTag],
            true,
            vec![CapabilityConstraint::MaxDelegationDepth(2)],
            None,
            timestamp(),
        )?);
        // alice → bob
        resolver.insert_grant(signed_capability_grant(
            &alice,
            &bob,
            CapabilityScope::Object(object.clone()),
            vec![StatementKind::ObjectVersionTag],
            false,
            vec![],
            None,
            timestamp(),
        )?);

        let evaluation = evaluate_capability(
            &bob,
            &capability_target(&object, StatementKind::ObjectVersionTag),
            timestamp(),
            &resolver,
        )?;
        assert_eq!(evaluation, CapabilityEvaluation::Held);
        Ok(())
    }

    #[test]
    fn evaluate_capability_grantor_lacks_authority_when_chain_grant_not_delegable()
    -> Result<(), Box<dyn std::error::Error>> {
        // root → A (delegable=false) → B. A's grant from root cannot
        // be used to re-grant, so B's grant from A is unbacked.
        let root = fresh_actor(1);
        let alice = fresh_actor(2);
        let bob = fresh_actor(3);
        let object = fresh_object(9);
        let mut resolver = MemoryCapabilityResolver::default();
        resolver.set_root(&object, vec![root.clone()]);
        resolver.insert_grant(signed_capability_grant(
            &root,
            &alice,
            CapabilityScope::Object(object.clone()),
            vec![StatementKind::ObjectVersionTag],
            false, // not delegable
            vec![],
            None,
            timestamp(),
        )?);
        resolver.insert_grant(signed_capability_grant(
            &alice,
            &bob,
            CapabilityScope::Object(object.clone()),
            vec![StatementKind::ObjectVersionTag],
            false,
            vec![],
            None,
            timestamp(),
        )?);

        let evaluation = evaluate_capability(
            &bob,
            &capability_target(&object, StatementKind::ObjectVersionTag),
            timestamp(),
            &resolver,
        )?;
        assert_eq!(evaluation, CapabilityEvaluation::GrantorLacksAuthority);
        Ok(())
    }

    #[test]
    fn evaluate_capability_delegation_too_deep_propagates_from_chain()
    -> Result<(), Box<dyn std::error::Error>> {
        // root → A (delegable, MDD=0) → B. A's parent grant says
        // "no further re-grants below me," but A re-granted to B.
        // The recursive evaluator catches MDD violation at depth 1.
        let root = fresh_actor(1);
        let alice = fresh_actor(2);
        let bob = fresh_actor(3);
        let object = fresh_object(9);
        let mut resolver = MemoryCapabilityResolver::default();
        resolver.set_root(&object, vec![root.clone()]);
        resolver.insert_grant(signed_capability_grant(
            &root,
            &alice,
            CapabilityScope::Object(object.clone()),
            vec![StatementKind::ObjectVersionTag],
            true,
            vec![CapabilityConstraint::MaxDelegationDepth(0)],
            None,
            timestamp(),
        )?);
        resolver.insert_grant(signed_capability_grant(
            &alice,
            &bob,
            CapabilityScope::Object(object.clone()),
            vec![StatementKind::ObjectVersionTag],
            false,
            vec![],
            None,
            timestamp(),
        )?);

        let evaluation = evaluate_capability(
            &bob,
            &capability_target(&object, StatementKind::ObjectVersionTag),
            timestamp(),
            &resolver,
        )?;
        assert_eq!(evaluation, CapabilityEvaluation::DelegationTooDeep);
        Ok(())
    }

    #[test]
    fn evaluate_capability_grantor_lacks_authority_when_grantor_not_root_and_no_parent()
    -> Result<(), Box<dyn std::error::Error>> {
        // alice (not root) granted bob, but alice has no parent
        // grant. Resolver returns GrantorLacksAuthority.
        let root = fresh_actor(1);
        let alice = fresh_actor(2);
        let bob = fresh_actor(3);
        let object = fresh_object(9);
        let mut resolver = MemoryCapabilityResolver::default();
        resolver.set_root(&object, vec![root.clone()]);
        resolver.insert_grant(signed_capability_grant(
            &alice,
            &bob,
            CapabilityScope::Object(object.clone()),
            vec![StatementKind::ObjectVersionTag],
            false,
            vec![],
            None,
            timestamp(),
        )?);

        let evaluation = evaluate_capability(
            &bob,
            &capability_target(&object, StatementKind::ObjectVersionTag),
            timestamp(),
            &resolver,
        )?;
        assert_eq!(evaluation, CapabilityEvaluation::GrantorLacksAuthority);
        Ok(())
    }

    #[test]
    fn evaluate_capability_short_circuits_for_actor_target()
    -> Result<(), Box<dyn std::error::Error>> {
        // No statement kinds are valid for actor scope in v1, so
        // an actor target always evaluates to NotHeld regardless
        // of any grants in the resolver.
        let bob = fresh_actor(2);
        let some_actor = fresh_actor(3);
        let resolver = MemoryCapabilityResolver::default();

        let evaluation = evaluate_capability(
            &bob,
            &ResolutionTarget::Actor {
                id: some_actor,
                kind: StatementKind::ActorTrust,
            },
            timestamp(),
            &resolver,
        )?;
        assert_eq!(evaluation, CapabilityEvaluation::NotHeld);
        Ok(())
    }

    #[test]
    fn evaluate_capability_cycle_returns_grantor_lacks_authority()
    -> Result<(), Box<dyn std::error::Error>> {
        // alice ↔ bob: alice granted bob, bob granted alice. Neither
        // is root. evaluate(bob) follows the chain alice → root, but
        // alice's authority comes from bob (cycle) → GrantorLacksAuthority.
        let alice = fresh_actor(2);
        let bob = fresh_actor(3);
        let object = fresh_object(9);
        let mut resolver = MemoryCapabilityResolver::default();
        // Root authority is some other actor; neither alice nor bob.
        resolver.set_root(&object, vec![fresh_actor(1)]);
        resolver.insert_grant(signed_capability_grant(
            &alice,
            &bob,
            CapabilityScope::Object(object.clone()),
            vec![StatementKind::ObjectVersionTag],
            true,
            vec![],
            None,
            timestamp(),
        )?);
        resolver.insert_grant(signed_capability_grant(
            &bob,
            &alice,
            CapabilityScope::Object(object.clone()),
            vec![StatementKind::ObjectVersionTag],
            true,
            vec![],
            None,
            timestamp(),
        )?);

        let evaluation = evaluate_capability(
            &bob,
            &capability_target(&object, StatementKind::ObjectVersionTag),
            timestamp(),
            &resolver,
        )?;
        assert_eq!(evaluation, CapabilityEvaluation::GrantorLacksAuthority);
        Ok(())
    }

    // ---- Key chain integration ----

    fn unsigned_for_actor_at(
        actor: ActorId,
        at: Timestamp,
    ) -> Result<UnsignedStatement<ObjectRevisionBody>, kairo_core::IdError> {
        Ok(UnsignedStatement::new(actor, object_ref()?, at, revision_body()?))
    }

    #[test]
    fn statement_signed_by_rotated_in_key_after_rotation_is_valid()
    -> Result<(), Box<dyn std::error::Error>> {
        // Genesis declares key A. Actor publishes a rotation to key B
        // at t+10. A statement signed by B with created_at = t+20
        // verifies as Valid.
        let key_a = signing_key();
        let key_b = other_signing_key();
        let genesis = genesis_for(&key_a);
        let actor_id = genesis.actor_id();

        let mut resolver = MemoryActorResolver::new();
        resolver.insert(genesis);
        resolver.insert_rotation(
            actor_id.clone(),
            kairo_identity::KeyRotationEntry {
                statement_id: "rot-1".to_owned(),
                next_key: public_key_for(&key_b),
                created_at: Timestamp::from_seconds(timestamp().seconds() + 10),
                supersedes: None,
                surface: kairo_identity::KeySurface::Operational,
            },
        );

        let unsigned = unsigned_for_actor_at(
            actor_id.clone(),
            Timestamp::from_seconds(timestamp().seconds() + 20),
        )?;
        let signature = sign_with(&unsigned, &key_b, actor_id, "ed25519", None);
        let signed = SignedStatement::new(unsigned, signature);

        let report = verify_envelope_statement(&signed, &resolver);
        assert_eq!(report.signature, SignatureStatus::Valid);
        Ok(())
    }

    #[test]
    fn statement_signed_by_rotated_out_key_after_rotation_is_key_mismatch()
    -> Result<(), Box<dyn std::error::Error>> {
        // Genesis declares key A. Rotation to key B at t+10. A
        // statement signed by A (the rotated-out key) with
        // created_at = t+20 fails: at t+20 the active key is B, not A.
        let key_a = signing_key();
        let key_b = other_signing_key();
        let genesis = genesis_for(&key_a);
        let actor_id = genesis.actor_id();

        let mut resolver = MemoryActorResolver::new();
        resolver.insert(genesis);
        resolver.insert_rotation(
            actor_id.clone(),
            kairo_identity::KeyRotationEntry {
                statement_id: "rot-1".to_owned(),
                next_key: public_key_for(&key_b),
                created_at: Timestamp::from_seconds(timestamp().seconds() + 10),
                supersedes: None,
                surface: kairo_identity::KeySurface::Operational,
            },
        );

        let unsigned = unsigned_for_actor_at(
            actor_id.clone(),
            Timestamp::from_seconds(timestamp().seconds() + 20),
        )?;
        let signature = sign_with(&unsigned, &key_a, actor_id, "ed25519", None);
        let signed = SignedStatement::new(unsigned, signature);

        let report = verify_envelope_statement(&signed, &resolver);
        assert!(matches!(
            report.signature,
            SignatureStatus::KeyMismatch { .. }
        ));
        Ok(())
    }

    #[test]
    fn statement_signed_by_revoked_key_is_key_revoked()
    -> Result<(), Box<dyn std::error::Error>> {
        // Genesis declares key A. Actor revokes key A at t+50. A
        // statement signed by A with created_at = t+100 fails as
        // KeyRevoked even though A's signature bytes verify.
        let key_a = signing_key();
        let genesis = genesis_for(&key_a);
        let actor_id = genesis.actor_id();
        let key_a_id = public_key_for(&key_a).key_id();

        let mut resolver = MemoryActorResolver::new();
        resolver.insert(genesis);
        resolver.insert_revocation(
            actor_id.clone(),
            kairo_identity::KeyRevocationEntry {
                statement_id: "rev-1".to_owned(),
                revoked_key: key_a_id,
                retroactive: false,
                created_at: Timestamp::from_seconds(timestamp().seconds() + 50),
                surface: kairo_identity::KeySurface::Operational,
            },
        );

        let unsigned = unsigned_for_actor_at(
            actor_id.clone(),
            Timestamp::from_seconds(timestamp().seconds() + 100),
        )?;
        let signature = sign_with(&unsigned, &key_a, actor_id, "ed25519", None);
        let signed = SignedStatement::new(unsigned, signature);

        let report = verify_envelope_statement(&signed, &resolver);
        assert_eq!(report.signature, SignatureStatus::KeyRevoked);
        Ok(())
    }

    #[test]
    fn retroactive_revocation_invalidates_prior_statement()
    -> Result<(), Box<dyn std::error::Error>> {
        // Statement signed at t+10 with key A; later (at t+50) the
        // actor publishes a retroactive revocation of key A. The
        // earlier statement now flips to KeyRevoked.
        let key_a = signing_key();
        let genesis = genesis_for(&key_a);
        let actor_id = genesis.actor_id();
        let key_a_id = public_key_for(&key_a).key_id();

        let mut resolver = MemoryActorResolver::new();
        resolver.insert(genesis);
        resolver.insert_revocation(
            actor_id.clone(),
            kairo_identity::KeyRevocationEntry {
                statement_id: "rev-1".to_owned(),
                revoked_key: key_a_id,
                retroactive: true,
                created_at: Timestamp::from_seconds(timestamp().seconds() + 50),
                surface: kairo_identity::KeySurface::Operational,
            },
        );

        let unsigned = unsigned_for_actor_at(
            actor_id.clone(),
            Timestamp::from_seconds(timestamp().seconds() + 10),
        )?;
        let signature = sign_with(&unsigned, &key_a, actor_id, "ed25519", None);
        let signed = SignedStatement::new(unsigned, signature);

        let report = verify_envelope_statement(&signed, &resolver);
        assert_eq!(report.signature, SignatureStatus::KeyRevoked);
        Ok(())
    }

    // ---- Surface dispatch (Phase 2 §14) ----

    use crate::ActorEmergencyKeyRotationBody;

    /// Sign an `ActorEmergencyKeyRotation` with the given key (caller's
    /// choice — used to stage both the valid and the invalid signing
    /// surfaces).
    fn signed_emergency_rotation(
        actor: ActorId,
        sig_key: &SigningKey,
        next_key: PublicKey,
        at: Timestamp,
    ) -> Result<SignedStatement<ActorEmergencyKeyRotationBody>, Box<dyn std::error::Error>> {
        let body = ActorEmergencyKeyRotationBody::new(next_key, None);
        let subject: KairoRef = format!("actor:{actor}").parse()?;
        let unsigned = UnsignedStatement::new(actor.clone(), subject, at, body);
        let bytes = sig_key.sign(&unsigned.canonical_bytes()).to_bytes().to_vec();
        let signature = Signature::new(
            actor,
            public_key_for(sig_key).key_id().to_string(),
            "ed25519",
            bytes,
        );
        Ok(SignedStatement::new(unsigned, signature))
    }

    /// Build a `MemoryActorResolver` whose actor genesis declares the
    /// given attestation public key. The genesis's initial signing key
    /// is the conventional `signing_key()` (seed [7; 32]).
    fn resolver_with_attestation(
        signing_seed: SigningKey,
        attestation_seed: SigningKey,
    ) -> Result<(MemoryActorResolver, ActorId), Box<dyn std::error::Error>> {
        let initial = public_key_for(&signing_seed);
        let attestation = public_key_for(&attestation_seed);
        let genesis = ActorGenesisBody::new(
            ActorKind::person(),
            initial,
            vec![attestation],
            timestamp(),
            [9; 32],
        )?;
        let actor_id = genesis.actor_id();
        let mut resolver = MemoryActorResolver::new();
        resolver.insert(genesis);
        Ok((resolver, actor_id))
    }

    #[test]
    fn emergency_rotation_signed_by_attestation_key_verifies()
    -> Result<(), Box<dyn std::error::Error>> {
        // Genesis declares signing key A (the active key) and
        // attestation key Z. An emergency rotation signed by Z must
        // verify under the attestation surface.
        let signing = SigningKey::from_bytes(&[7; 32]);
        let attestation = SigningKey::from_bytes(&[200; 32]);
        let next_key = PublicKey::ed25519(SigningKey::from_bytes(&[8; 32]).verifying_key().to_bytes());
        let (resolver, actor_id) = resolver_with_attestation(signing, attestation.clone())?;

        let signed = signed_emergency_rotation(
            actor_id,
            &attestation,
            next_key,
            Timestamp::from_seconds(timestamp().seconds() + 100),
        )?;

        let report = verify_envelope_statement(&signed, &resolver);
        assert_eq!(report.signature, SignatureStatus::Valid);
        Ok(())
    }

    #[test]
    fn emergency_rotation_signed_by_active_signing_key_is_not_in_attestation_set()
    -> Result<(), Box<dyn std::error::Error>> {
        // Same actor as above. An emergency rotation signed by the
        // active *signing* key A (not by the attestation key Z) must
        // be rejected — the surfaces don't overlap.
        let signing = SigningKey::from_bytes(&[7; 32]);
        let attestation = SigningKey::from_bytes(&[200; 32]);
        let next_key = PublicKey::ed25519(SigningKey::from_bytes(&[8; 32]).verifying_key().to_bytes());
        let (resolver, actor_id) = resolver_with_attestation(signing.clone(), attestation)?;

        let signed = signed_emergency_rotation(
            actor_id,
            &signing,
            next_key,
            Timestamp::from_seconds(timestamp().seconds() + 100),
        )?;

        let report = verify_envelope_statement(&signed, &resolver);
        assert!(matches!(
            report.signature,
            SignatureStatus::NotInAttestationSet { .. }
        ));
        Ok(())
    }

    #[test]
    fn routine_rotation_signed_by_attestation_key_is_key_mismatch()
    -> Result<(), Box<dyn std::error::Error>> {
        // The reverse direction: a routine `ActorKeyRotation` signed
        // by the attestation key (not the active signing key) is
        // rejected as `KeyMismatch` because the active-key chain rule
        // applies and the attestation key is not in that chain.
        let signing = SigningKey::from_bytes(&[7; 32]);
        let attestation = SigningKey::from_bytes(&[200; 32]);
        let next_key = PublicKey::ed25519(SigningKey::from_bytes(&[8; 32]).verifying_key().to_bytes());
        let (resolver, actor_id) = resolver_with_attestation(signing, attestation.clone())?;

        let body = crate::ActorKeyRotationBody::new(next_key, None);
        let subject: KairoRef = format!("actor:{actor_id}").parse()?;
        let unsigned = UnsignedStatement::new(
            actor_id.clone(),
            subject,
            Timestamp::from_seconds(timestamp().seconds() + 100),
            body,
        );
        let bytes = attestation
            .sign(&unsigned.canonical_bytes())
            .to_bytes()
            .to_vec();
        let signature = Signature::new(
            actor_id,
            public_key_for(&attestation).key_id().to_string(),
            "ed25519",
            bytes,
        );
        let signed = SignedStatement::new(unsigned, signature);

        let report = verify_envelope_statement(&signed, &resolver);
        assert!(matches!(
            report.signature,
            SignatureStatus::KeyMismatch { .. }
        ));
        Ok(())
    }
}
