//! Generic statement verification.
//!
//! `verify_envelope_statement` resolves the actor declared on the envelope
//! through an [`ActorResolver`], then evaluates the signature against the
//! resolved initial key. It returns a structured [`VerificationReport`] that
//! reports signature status, actor resolution outcome, and trust evaluation
//! independently. Cryptographic validity does not imply trust; trust is only
//! filled in once `kairo-trust` exists (see TODO §10).

use kairo_core::{ActorId, StatementId};
use kairo_identity::{ActorResolveError, ActorResolver, SignatureVerificationError};

use crate::{SignedStatement, StatementBody, StatementSignatureError};

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

/// Local trust evaluation.
///
/// Always [`TrustEvaluation::Unevaluated`] in the MVP. A future revision will
/// add `Trusted` / `Untrusted` once `kairo-trust` lands. The placeholder
/// exists so `VerificationReport` does not change shape when trust evaluation
/// is wired up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustEvaluation {
    Unevaluated,
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
}
