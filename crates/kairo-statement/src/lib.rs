//! Signed statement envelope primitives.

use kairo_core::canonical::{encode_bytes, encode_option, encode_str, encode_u8, CanonicalEncode};
use kairo_core::{ActorId, KairoRef, ObjectId, StatementId};

/// Canonical ObjectGenesis body v1 encoding is documented at
/// `schemas/canonical/object-genesis-v1.md`.
const OBJECT_GENESIS_DOMAIN: &[u8] = b"kairo.object.genesis.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatementEnvelope {
    id: StatementId,
    actor: ActorId,
    subject: KairoRef,
}

impl StatementEnvelope {
    pub fn new(id: StatementId, actor: ActorId, subject: KairoRef) -> Self {
        Self { id, actor, subject }
    }

    pub fn id(&self) -> &StatementId {
        &self.id
    }

    pub fn actor(&self) -> &ActorId {
        &self.actor
    }

    pub fn subject(&self) -> &KairoRef {
        &self.subject
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectGenesisBody {
    object_kind: ObjectKind,
    created_by: ActorId,
    nonce: [u8; 32],
    initial_revision: Option<RevisionId>,
}

impl ObjectGenesisBody {
    pub fn new(
        object_kind: ObjectKind,
        created_by: ActorId,
        nonce: [u8; 32],
        initial_revision: Option<RevisionId>,
    ) -> Self {
        Self {
            object_kind,
            created_by,
            nonce,
            initial_revision,
        }
    }

    pub fn object_kind(&self) -> &ObjectKind {
        &self.object_kind
    }

    pub fn created_by(&self) -> &ActorId {
        &self.created_by
    }

    pub fn nonce(&self) -> &[u8; 32] {
        &self.nonce
    }

    pub fn initial_revision(&self) -> Option<&RevisionId> {
        self.initial_revision.as_ref()
    }

    pub fn object_id(&self) -> ObjectId {
        ObjectId::from_bytes(OBJECT_GENESIS_DOMAIN, &self.canonical_bytes())
    }
}

impl CanonicalEncode for ObjectGenesisBody {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_bytes(out, b"ObjectGenesis");
        encode_u8(out, 1);
        encode_str(out, self.object_kind.as_str());
        encode_str(out, self.created_by.as_str());
        encode_bytes(out, &self.nonce);
        encode_option(out, self.initial_revision.as_ref(), |out, revision| {
            encode_str(out, revision.as_str());
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectGenesisStatement {
    body: ObjectGenesisBody,
    signature: Signature,
}

impl ObjectGenesisStatement {
    pub fn new(body: ObjectGenesisBody, signature: Signature) -> Self {
        Self { body, signature }
    }

    pub fn body(&self) -> &ObjectGenesisBody {
        &self.body
    }

    pub fn signature(&self) -> &Signature {
        &self.signature
    }

    pub fn object_id(&self) -> ObjectId {
        self.body.object_id()
    }

    pub fn signed_bytes(&self) -> Vec<u8> {
        self.body.canonical_bytes()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    actor: ActorId,
    key_id: String,
    algorithm: String,
    bytes: Vec<u8>,
}

impl Signature {
    pub fn new(
        actor: ActorId,
        key_id: impl Into<String>,
        algorithm: impl Into<String>,
        bytes: Vec<u8>,
    ) -> Self {
        Self {
            actor,
            key_id: key_id.into(),
            algorithm: algorithm.into(),
            bytes,
        }
    }

    pub fn actor(&self) -> &ActorId {
        &self.actor
    }

    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    pub fn algorithm(&self) -> &str {
        &self.algorithm
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectKind(String);

impl ObjectKind {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn software() -> Self {
        Self("software".to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionId(String);

impl RevisionId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACTOR_ID: &str = "zQmTn1mdQDA1ryQZsiqYfRbqj5DGcG8TNvYcRmBrBLAuk5t";

    fn actor_id() -> Result<ActorId, kairo_core::IdError> {
        ActorId::new(ACTOR_ID)
    }

    fn genesis_with_nonce(nonce: [u8; 32]) -> Result<ObjectGenesisBody, kairo_core::IdError> {
        Ok(ObjectGenesisBody::new(
            ObjectKind::software(),
            actor_id()?,
            nonce,
            None,
        ))
    }

    fn signature(key_id: &str, bytes: Vec<u8>) -> Result<Signature, kairo_core::IdError> {
        Ok(Signature::new(actor_id()?, key_id, "test", bytes))
    }

    #[test]
    fn same_genesis_produces_same_object_id() {
        let first = genesis_with_nonce([7; 32]);
        let second = genesis_with_nonce([7; 32]);

        assert_eq!(
            first.map(|genesis| (genesis.canonical_bytes(), genesis.object_id())),
            second.map(|genesis| (genesis.canonical_bytes(), genesis.object_id()))
        );
    }

    #[test]
    fn different_nonce_produces_different_object_id() {
        let first = genesis_with_nonce([7; 32]).map(|genesis| genesis.object_id());
        let second = genesis_with_nonce([8; 32]).map(|genesis| genesis.object_id());

        assert!(matches!((first, second), (Ok(first), Ok(second)) if first != second));
    }

    #[test]
    fn initial_revision_changes_object_id() {
        let without_revision = genesis_with_nonce([7; 32]).map(|genesis| genesis.object_id());
        let with_revision = actor_id().map(|actor_id| {
            ObjectGenesisBody::new(
                ObjectKind::software(),
                actor_id,
                [7; 32],
                Some(RevisionId::new("git:sha256:abc123")),
            )
            .object_id()
        });

        assert!(
            matches!((without_revision, with_revision), (Ok(without_revision), Ok(with_revision)) if without_revision != with_revision)
        );
    }

    #[test]
    fn generated_object_id_is_valid() {
        let object_id = genesis_with_nonce([7; 32]).map(|genesis| genesis.object_id());

        assert!(
            matches!(object_id, Ok(object_id) if ObjectId::new(object_id.to_string()) == Ok(object_id.clone()))
        );
    }

    #[test]
    fn signature_does_not_change_object_id() {
        let body = genesis_with_nonce([7; 32]);
        let first = body
            .clone()
            .and_then(|body| signature("key-1", vec![1, 2, 3]).map(|signature| (body, signature)))
            .map(|(body, signature)| ObjectGenesisStatement::new(body, signature).object_id());
        let second = body
            .and_then(|body| signature("key-2", vec![4, 5, 6]).map(|signature| (body, signature)))
            .map(|(body, signature)| ObjectGenesisStatement::new(body, signature).object_id());

        assert_eq!(first, second);
    }
}
