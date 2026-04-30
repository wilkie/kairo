//! Signed statement envelope primitives.

use kairo_core::{ActorId, KairoRef, ObjectId, StatementId};

/// Canonical ObjectGenesis v1 encoding is documented at
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
pub struct ObjectGenesis {
    object_kind: ObjectKind,
    created_by: ActorId,
    nonce: [u8; 32],
    initial_revision: Option<RevisionId>,
}

impl ObjectGenesis {
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

    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        encode_bytes(&mut bytes, b"ObjectGenesis");
        encode_u8(&mut bytes, 1);
        encode_str(&mut bytes, self.object_kind.as_str());
        encode_str(&mut bytes, self.created_by.as_str());
        encode_bytes(&mut bytes, &self.nonce);

        match &self.initial_revision {
            Some(revision) => {
                encode_u8(&mut bytes, 1);
                encode_str(&mut bytes, revision.as_str());
            }
            None => encode_u8(&mut bytes, 0),
        }

        bytes
    }

    pub fn object_id(&self) -> ObjectId {
        ObjectId::from_bytes(OBJECT_GENESIS_DOMAIN, &self.canonical_bytes())
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

fn encode_u8(bytes: &mut Vec<u8>, value: u8) {
    bytes.push(value);
}

fn encode_str(bytes: &mut Vec<u8>, value: &str) {
    encode_bytes(bytes, value.as_bytes());
}

#[allow(clippy::cast_possible_truncation)]
fn encode_bytes(bytes: &mut Vec<u8>, value: &[u8]) {
    debug_assert!(value.len() <= u32::MAX as usize);
    bytes.extend((value.len() as u32).to_be_bytes());
    bytes.extend(value);
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACTOR_ID: &str = "zQmTn1mdQDA1ryQZsiqYfRbqj5DGcG8TNvYcRmBrBLAuk5t";

    fn actor_id() -> Result<ActorId, kairo_core::IdError> {
        ActorId::new(ACTOR_ID)
    }

    fn genesis_with_nonce(nonce: [u8; 32]) -> Result<ObjectGenesis, kairo_core::IdError> {
        Ok(ObjectGenesis::new(
            ObjectKind::software(),
            actor_id()?,
            nonce,
            None,
        ))
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
            ObjectGenesis::new(
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
}
