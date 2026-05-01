//! Actor identity and signature primitives.

pub mod json;

use std::error::Error;
use std::fmt;

use std::collections::BTreeMap;

use ed25519_dalek::{Verifier, VerifyingKey};
use kairo_core::canonical::{encode_bytes, encode_str, encode_u8, CanonicalEncode};
pub use kairo_core::ActorId;
use kairo_core::{BlobId, Timestamp};

/// Canonical ActorGenesis v1 encoding is documented at
/// `schemas/canonical/actor-genesis-v1.md`.
const ACTOR_GENESIS_DOMAIN: &[u8] = b"kairo.actor.genesis.v1";
const ACTOR_KEY_DOMAIN: &[u8] = b"kairo.actor.key.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorGenesisBody {
    actor_kind: ActorKind,
    initial_key: PublicKey,
    created_at: Timestamp,
    nonce: [u8; 32],
}

impl ActorGenesisBody {
    pub fn new(
        actor_kind: ActorKind,
        initial_key: PublicKey,
        created_at: Timestamp,
        nonce: [u8; 32],
    ) -> Self {
        Self {
            actor_kind,
            initial_key,
            created_at,
            nonce,
        }
    }

    pub fn actor_kind(&self) -> &ActorKind {
        &self.actor_kind
    }

    pub fn initial_key(&self) -> &PublicKey {
        &self.initial_key
    }

    pub fn created_at(&self) -> Timestamp {
        self.created_at
    }

    pub fn nonce(&self) -> &[u8; 32] {
        &self.nonce
    }

    pub fn actor_id(&self) -> ActorId {
        ActorId::from_bytes(ACTOR_GENESIS_DOMAIN, &self.canonical_bytes())
    }
}

impl CanonicalEncode for ActorGenesisBody {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_str(out, "ActorGenesis");
        encode_u8(out, 1);
        encode_str(out, self.actor_kind.as_str());
        self.initial_key.encode_canonical(out);
        self.created_at.encode_canonical(out);
        encode_bytes(out, &self.nonce);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorKind(String);

impl ActorKind {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn person() -> Self {
        Self("person".to_owned())
    }

    pub fn project() -> Self {
        Self("project".to_owned())
    }

    pub fn organization() -> Self {
        Self("organization".to_owned())
    }

    pub fn service() -> Self {
        Self("service".to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyId(String);

impl KeyId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn from_public_key(public_key: &PublicKey) -> Self {
        Self(BlobId::from_bytes(ACTOR_KEY_DOMAIN, &public_key.canonical_bytes()).to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for KeyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureAlgorithm {
    Ed25519,
}

impl SignatureAlgorithm {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ed25519 => "ed25519",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicKey {
    algorithm: SignatureAlgorithm,
    bytes: [u8; 32],
}

impl PublicKey {
    pub fn ed25519(bytes: [u8; 32]) -> Self {
        Self {
            algorithm: SignatureAlgorithm::Ed25519,
            bytes,
        }
    }

    pub fn algorithm(&self) -> SignatureAlgorithm {
        self.algorithm
    }

    pub fn bytes(&self) -> &[u8; 32] {
        &self.bytes
    }

    pub fn key_id(&self) -> KeyId {
        KeyId::from_public_key(self)
    }
}

impl CanonicalEncode for PublicKey {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_str(out, self.algorithm.as_str());
        encode_bytes(out, &self.bytes);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureBytes {
    algorithm: SignatureAlgorithm,
    bytes: [u8; 64],
}

impl SignatureBytes {
    pub fn ed25519(bytes: [u8; 64]) -> Self {
        Self {
            algorithm: SignatureAlgorithm::Ed25519,
            bytes,
        }
    }

    pub fn algorithm(&self) -> SignatureAlgorithm {
        self.algorithm
    }

    pub fn bytes(&self) -> &[u8; 64] {
        &self.bytes
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedSignature;

pub fn verify_signature(
    public_key: &PublicKey,
    payload: &[u8],
    signature: &SignatureBytes,
) -> Result<VerifiedSignature, SignatureVerificationError> {
    if public_key.algorithm() != signature.algorithm() {
        return Err(SignatureVerificationError::AlgorithmMismatch {
            public_key: public_key.algorithm(),
            signature: signature.algorithm(),
        });
    }

    match public_key.algorithm() {
        SignatureAlgorithm::Ed25519 => {
            verify_ed25519(public_key.bytes(), payload, signature.bytes())
        }
    }
}

pub trait ActorResolver {
    fn actor_genesis(&self, actor: &ActorId)
        -> Result<Option<ActorGenesisBody>, ActorResolveError>;

    fn initial_key(&self, actor: &ActorId) -> Result<Option<PublicKey>, ActorResolveError> {
        Ok(self
            .actor_genesis(actor)?
            .map(|genesis| genesis.initial_key().clone()))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryActorResolver {
    actors: BTreeMap<ActorId, ActorGenesisBody>,
}

impl MemoryActorResolver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, genesis: ActorGenesisBody) -> ActorId {
        let actor_id = genesis.actor_id();
        self.actors.insert(actor_id.clone(), genesis);
        actor_id
    }

    pub fn len(&self) -> usize {
        self.actors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.actors.is_empty()
    }
}

impl ActorResolver for MemoryActorResolver {
    fn actor_genesis(
        &self,
        actor: &ActorId,
    ) -> Result<Option<ActorGenesisBody>, ActorResolveError> {
        Ok(self.actors.get(actor).cloned())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActorResolveError {
    Unavailable(String),
}

impl fmt::Display for ActorResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(reason) => write!(f, "actor resolver unavailable: {reason}"),
        }
    }
}

impl Error for ActorResolveError {}

fn verify_ed25519(
    public_key: &[u8; 32],
    payload: &[u8],
    signature: &[u8; 64],
) -> Result<VerifiedSignature, SignatureVerificationError> {
    let public_key = VerifyingKey::from_bytes(public_key)
        .map_err(|_| SignatureVerificationError::InvalidPublicKey)?;
    let signature = ed25519_dalek::Signature::from_bytes(signature);

    public_key
        .verify(payload, &signature)
        .map(|()| VerifiedSignature)
        .map_err(|_| SignatureVerificationError::InvalidSignature)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureVerificationError {
    AlgorithmMismatch {
        public_key: SignatureAlgorithm,
        signature: SignatureAlgorithm,
    },
    InvalidPublicKey,
    InvalidSignature,
}

impl fmt::Display for SignatureVerificationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlgorithmMismatch {
                public_key,
                signature,
            } => write!(
                f,
                "signature algorithm {} does not match public key algorithm {}",
                signature.as_str(),
                public_key.as_str()
            ),
            Self::InvalidPublicKey => f.write_str("invalid public key"),
            Self::InvalidSignature => f.write_str("invalid signature"),
        }
    }
}

impl Error for SignatureVerificationError {}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};

    use super::*;

    fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&[7; 32])
    }

    fn public_key() -> PublicKey {
        PublicKey::ed25519(signing_key().verifying_key().to_bytes())
    }

    fn timestamp() -> Timestamp {
        Timestamp::from_seconds(1_700_000_000)
    }

    #[test]
    fn same_actor_genesis_produces_same_actor_id() {
        let first = ActorGenesisBody::new(ActorKind::person(), public_key(), timestamp(), [9; 32]);
        let second = ActorGenesisBody::new(ActorKind::person(), public_key(), timestamp(), [9; 32]);

        assert_eq!(first.actor_id(), second.actor_id());
    }

    #[test]
    fn actor_genesis_nonce_changes_actor_id() {
        let first = ActorGenesisBody::new(ActorKind::person(), public_key(), timestamp(), [9; 32]);
        let second =
            ActorGenesisBody::new(ActorKind::person(), public_key(), timestamp(), [10; 32]);

        assert_ne!(first.actor_id(), second.actor_id());
    }

    #[test]
    fn actor_genesis_key_changes_actor_id() {
        let first = ActorGenesisBody::new(ActorKind::person(), public_key(), timestamp(), [9; 32]);
        let second_key =
            PublicKey::ed25519(SigningKey::from_bytes(&[8; 32]).verifying_key().to_bytes());
        let second = ActorGenesisBody::new(ActorKind::person(), second_key, timestamp(), [9; 32]);

        assert_ne!(first.actor_id(), second.actor_id());
    }

    #[test]
    fn actor_genesis_created_at_changes_actor_id() {
        let first = ActorGenesisBody::new(ActorKind::person(), public_key(), timestamp(), [9; 32]);
        let second = ActorGenesisBody::new(
            ActorKind::person(),
            public_key(),
            Timestamp::from_seconds(timestamp().seconds() + 1),
            [9; 32],
        );

        assert_ne!(first.actor_id(), second.actor_id());
    }

    #[test]
    fn key_id_is_stable_for_same_public_key() {
        assert_eq!(public_key().key_id(), public_key().key_id());
    }

    #[test]
    fn verifies_ed25519_signature() {
        let payload = b"kairo payload";
        let signature = signing_key().sign(payload).to_bytes();

        assert_eq!(
            verify_signature(&public_key(), payload, &SignatureBytes::ed25519(signature)),
            Ok(VerifiedSignature)
        );
    }

    #[test]
    fn rejects_changed_payload() {
        let signature = signing_key().sign(b"kairo payload").to_bytes();

        assert_eq!(
            verify_signature(
                &public_key(),
                b"changed payload",
                &SignatureBytes::ed25519(signature)
            ),
            Err(SignatureVerificationError::InvalidSignature)
        );
    }

    #[test]
    fn rejects_changed_signature() {
        let payload = b"kairo payload";
        let mut signature = signing_key().sign(payload).to_bytes();
        signature[0] ^= 1;

        assert_eq!(
            verify_signature(&public_key(), payload, &SignatureBytes::ed25519(signature)),
            Err(SignatureVerificationError::InvalidSignature)
        );
    }

    #[test]
    fn memory_resolver_finds_actor_genesis_by_derived_actor_id() {
        let genesis =
            ActorGenesisBody::new(ActorKind::person(), public_key(), timestamp(), [9; 32]);
        let mut resolver = MemoryActorResolver::new();
        let actor_id = resolver.insert(genesis.clone());

        assert_eq!(resolver.actor_genesis(&actor_id), Ok(Some(genesis)));
    }

    #[test]
    fn memory_resolver_returns_none_for_missing_actor() {
        let resolver = MemoryActorResolver::new();
        let missing_actor =
            ActorGenesisBody::new(ActorKind::person(), public_key(), timestamp(), [9; 32])
                .actor_id();

        assert_eq!(resolver.actor_genesis(&missing_actor), Ok(None));
    }

    #[test]
    fn memory_resolver_resolves_initial_key() {
        let genesis =
            ActorGenesisBody::new(ActorKind::person(), public_key(), timestamp(), [9; 32]);
        let mut resolver = MemoryActorResolver::new();
        let actor_id = resolver.insert(genesis.clone());

        assert_eq!(resolver.initial_key(&actor_id), Ok(Some(public_key())));
    }
}
