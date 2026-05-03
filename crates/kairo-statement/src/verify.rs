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

use kairo_core::{ActorId, StatementId};
use kairo_identity::{ActorResolveError, ActorResolver, SignatureVerificationError};

use crate::{ActorTrustBody, SignedStatement, StatementBody, StatementSignatureError, TrustDecision};

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

/// Whether the signature verified against the resolved initial key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureStatus {
    /// Signature verified against the resolved key.
    Valid,
    /// Signature did not verify against the resolved key.
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

    let genesis = match resolver.actor_genesis(&envelope_actor) {
        Ok(Some(genesis)) => genesis,
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
    };

    let signature = match statement.verify_signature(genesis.initial_key()) {
        Ok(_) => SignatureStatus::Valid,
        Err(StatementSignatureError::Verification(
            SignatureVerificationError::InvalidSignature,
        )) => SignatureStatus::Invalid,
        Err(StatementSignatureError::Verification(
            SignatureVerificationError::InvalidPublicKey,
        )) => {
            // The resolved genesis carries a malformed key. Surface as an
            // operational resolver issue rather than an invalid statement.
            return VerificationReport {
                statement_id,
                envelope_actor,
                signature_actor,
                actor: ActorResolution::ResolverUnavailable(
                    "actor genesis carries a malformed initial key".to_owned(),
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

    fn genesis_for(key: &SigningKey) -> ActorGenesisBody {
        ActorGenesisBody::new(
            ActorKind::person(),
            public_key_for(key),
            timestamp(),
            [9; 32],
        )
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
        // Genesis declares key A; statement is signed with key B.
        let envelope_key = signing_key();
        let genesis = genesis_for(&envelope_key);
        let actor_id = genesis.actor_id();
        let unsigned = unsigned_for_actor(actor_id.clone())?;
        let attacker = other_signing_key();
        let signature = sign_with(&unsigned, &attacker, actor_id, "ed25519", None);
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
            timestamp(),
            [seed; 32],
        )
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
}
