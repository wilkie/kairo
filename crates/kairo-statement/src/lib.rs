//! Signed statement envelope primitives.

pub mod json;

use std::error::Error;
use std::fmt;

use kairo_core::canonical::{
    encode_bytes, encode_list, encode_option, encode_str, encode_u8, CanonicalEncode,
};
use kairo_core::{ActorId, BlobId, KairoRef, ObjectId, StatementId};
use kairo_identity::{
    verify_signature as verify_identity_signature, PublicKey, SignatureBytes,
    SignatureVerificationError, VerifiedSignature,
};

/// Canonical ObjectGenesis body v1 encoding is documented at
/// `schemas/canonical/object-genesis-v1.md`.
const OBJECT_GENESIS_DOMAIN: &[u8] = b"kairo.object.genesis.v1";
const STATEMENT_DOMAIN: &[u8] = b"kairo.statement.v1";

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

pub trait StatementBody: CanonicalEncode {
    const TYPE: &'static str;
    const VERSION: u8;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsignedStatement<B> {
    actor: ActorId,
    subject: KairoRef,
    body: B,
}

impl<B> UnsignedStatement<B> {
    pub fn new(actor: ActorId, subject: KairoRef, body: B) -> Self {
        Self {
            actor,
            subject,
            body,
        }
    }

    pub fn actor(&self) -> &ActorId {
        &self.actor
    }

    pub fn subject(&self) -> &KairoRef {
        &self.subject
    }

    pub fn body(&self) -> &B {
        &self.body
    }
}

impl<B: StatementBody> UnsignedStatement<B> {
    pub fn statement_id(&self) -> StatementId {
        StatementId::from_bytes(STATEMENT_DOMAIN, &self.canonical_bytes())
    }
}

impl<B: StatementBody> CanonicalEncode for UnsignedStatement<B> {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_str(out, B::TYPE);
        encode_u8(out, B::VERSION);
        encode_str(out, self.actor.as_str());
        encode_str(out, &self.subject.to_string());
        self.body.encode_canonical(out);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedStatement<B> {
    unsigned: UnsignedStatement<B>,
    signature: Signature,
}

impl<B> SignedStatement<B> {
    pub fn new(unsigned: UnsignedStatement<B>, signature: Signature) -> Self {
        Self {
            unsigned,
            signature,
        }
    }

    pub fn unsigned(&self) -> &UnsignedStatement<B> {
        &self.unsigned
    }

    pub fn signature(&self) -> &Signature {
        &self.signature
    }
}

impl<B: StatementBody> SignedStatement<B> {
    pub fn statement_id(&self) -> StatementId {
        self.unsigned.statement_id()
    }

    pub fn signed_bytes(&self) -> Vec<u8> {
        self.unsigned.canonical_bytes()
    }

    pub fn verify_signature(
        &self,
        public_key: &PublicKey,
    ) -> Result<VerifiedSignature, StatementSignatureError> {
        let signature = self.signature.to_signature_bytes()?;
        verify_identity_signature(public_key, &self.signed_bytes(), &signature)
            .map_err(StatementSignatureError::Verification)
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

/// Canonical ObjectRevision body v1 encoding is documented at
/// `schemas/canonical/object-revision-v1.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectRevisionBody {
    object: ObjectId,
    revision: RevisionId,
    parents: Vec<RevisionId>,
    manifest_hash: BlobId,
    attests_reachable_history: bool,
}

impl ObjectRevisionBody {
    pub fn new(
        object: ObjectId,
        revision: RevisionId,
        parents: Vec<RevisionId>,
        manifest_hash: BlobId,
        attests_reachable_history: bool,
    ) -> Self {
        Self {
            object,
            revision,
            parents,
            manifest_hash,
            attests_reachable_history,
        }
    }

    pub fn object(&self) -> &ObjectId {
        &self.object
    }

    pub fn revision(&self) -> &RevisionId {
        &self.revision
    }

    pub fn parents(&self) -> &[RevisionId] {
        &self.parents
    }

    pub fn manifest_hash(&self) -> &BlobId {
        &self.manifest_hash
    }

    pub fn attests_reachable_history(&self) -> bool {
        self.attests_reachable_history
    }
}

impl StatementBody for ObjectRevisionBody {
    const TYPE: &'static str = "ObjectRevision";
    const VERSION: u8 = 1;
}

impl CanonicalEncode for ObjectRevisionBody {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_str(out, self.object.as_str());
        encode_str(out, self.revision.as_str());
        encode_list(out, &self.parents, |out, revision| {
            encode_str(out, revision.as_str());
        });
        encode_str(out, self.manifest_hash.as_str());
        encode_u8(out, u8::from(self.attests_reachable_history));
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

    pub fn to_signature_bytes(&self) -> Result<SignatureBytes, StatementSignatureError> {
        match self.algorithm.as_str() {
            "ed25519" => {
                let bytes = <[u8; 64]>::try_from(self.bytes.as_slice()).map_err(|_| {
                    StatementSignatureError::InvalidSignatureLength {
                        expected: 64,
                        actual: self.bytes.len(),
                    }
                })?;
                Ok(SignatureBytes::ed25519(bytes))
            }
            algorithm => Err(StatementSignatureError::UnsupportedAlgorithm(
                algorithm.to_owned(),
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatementSignatureError {
    UnsupportedAlgorithm(String),
    InvalidSignatureLength { expected: usize, actual: usize },
    Verification(SignatureVerificationError),
}

impl fmt::Display for StatementSignatureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedAlgorithm(algorithm) => {
                write!(f, "unsupported signature algorithm {algorithm}")
            }
            Self::InvalidSignatureLength { expected, actual } => {
                write!(f, "invalid signature length {actual}; expected {expected}")
            }
            Self::Verification(error) => write!(f, "{error}"),
        }
    }
}

impl Error for StatementSignatureError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Verification(error) => Some(error),
            Self::UnsupportedAlgorithm(_) | Self::InvalidSignatureLength { .. } => None,
        }
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
    use ed25519_dalek::{Signer, SigningKey};
    use kairo_core::canonical::encode_str;
    use kairo_identity::PublicKey;

    const ACTOR_ID: &str = "zQmTn1mdQDA1ryQZsiqYfRbqj5DGcG8TNvYcRmBrBLAuk5t";
    const OBJECT_ID: &str = "zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk";
    const BLOB_ID: &str = "zQmfBE2w2UqKJhGZAxK4ZWb4JuCrvxnN9P4YKuvzfbPSvD5";

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

    fn object_ref() -> Result<KairoRef, kairo_core::IdError> {
        format!("object:{OBJECT_ID}").parse()
    }

    fn object_id() -> Result<ObjectId, kairo_core::IdError> {
        ObjectId::new(OBJECT_ID)
    }

    fn blob_id() -> Result<BlobId, kairo_core::IdError> {
        BlobId::new(BLOB_ID)
    }

    fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&[7; 32])
    }

    fn public_key() -> PublicKey {
        PublicKey::ed25519(signing_key().verifying_key().to_bytes())
    }

    fn other_public_key() -> PublicKey {
        PublicKey::ed25519(SigningKey::from_bytes(&[8; 32]).verifying_key().to_bytes())
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestBody {
        value: String,
    }

    impl CanonicalEncode for TestBody {
        fn encode_canonical(&self, out: &mut Vec<u8>) {
            encode_str(out, &self.value);
        }
    }

    impl StatementBody for TestBody {
        const TYPE: &'static str = "TestBody";
        const VERSION: u8 = 1;
    }

    fn unsigned_test_statement(
        value: &str,
    ) -> Result<UnsignedStatement<TestBody>, kairo_core::IdError> {
        Ok(UnsignedStatement::new(
            actor_id()?,
            object_ref()?,
            TestBody {
                value: value.to_owned(),
            },
        ))
    }

    fn object_revision_body(
        parents: Vec<RevisionId>,
        manifest_hash: BlobId,
    ) -> Result<ObjectRevisionBody, kairo_core::IdError> {
        Ok(ObjectRevisionBody::new(
            object_id()?,
            RevisionId::new("git:sha256:revision"),
            parents,
            manifest_hash,
            true,
        ))
    }

    fn unsigned_object_revision(
        body: ObjectRevisionBody,
    ) -> Result<UnsignedStatement<ObjectRevisionBody>, kairo_core::IdError> {
        Ok(UnsignedStatement::new(actor_id()?, object_ref()?, body))
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

    #[test]
    fn same_unsigned_statement_produces_same_statement_id() {
        let first = unsigned_test_statement("same").map(|statement| statement.statement_id());
        let second = unsigned_test_statement("same").map(|statement| statement.statement_id());

        assert_eq!(first, second);
    }

    #[test]
    fn different_body_produces_different_statement_id() {
        let first = unsigned_test_statement("first").map(|statement| statement.statement_id());
        let second = unsigned_test_statement("second").map(|statement| statement.statement_id());

        assert!(matches!((first, second), (Ok(first), Ok(second)) if first != second));
    }

    #[test]
    fn signature_does_not_change_statement_id() {
        let unsigned = unsigned_test_statement("same");
        let first = unsigned
            .clone()
            .and_then(|unsigned| {
                signature("key-1", vec![1, 2, 3]).map(|signature| (unsigned, signature))
            })
            .map(|(unsigned, signature)| SignedStatement::new(unsigned, signature).statement_id());
        let second = unsigned
            .and_then(|unsigned| {
                signature("key-2", vec![4, 5, 6]).map(|signature| (unsigned, signature))
            })
            .map(|(unsigned, signature)| SignedStatement::new(unsigned, signature).statement_id());

        assert_eq!(first, second);
    }

    #[test]
    fn same_object_revision_produces_same_statement_id() -> Result<(), kairo_core::IdError> {
        let first = object_revision_body(vec![RevisionId::new("git:sha256:parent")], blob_id()?)
            .and_then(unsigned_object_revision)
            .map(|statement| statement.statement_id());
        let second = object_revision_body(vec![RevisionId::new("git:sha256:parent")], blob_id()?)
            .and_then(unsigned_object_revision)
            .map(|statement| statement.statement_id());

        assert_eq!(first, second);
        Ok(())
    }

    #[test]
    fn object_revision_parent_order_affects_statement_id() -> Result<(), kairo_core::IdError> {
        let first = object_revision_body(
            vec![
                RevisionId::new("git:sha256:first-parent"),
                RevisionId::new("git:sha256:second-parent"),
            ],
            blob_id()?,
        )
        .and_then(unsigned_object_revision)
        .map(|statement| statement.statement_id());
        let second = object_revision_body(
            vec![
                RevisionId::new("git:sha256:second-parent"),
                RevisionId::new("git:sha256:first-parent"),
            ],
            blob_id()?,
        )
        .and_then(unsigned_object_revision)
        .map(|statement| statement.statement_id());

        assert!(matches!((first, second), (Ok(first), Ok(second)) if first != second));
        Ok(())
    }

    #[test]
    fn object_revision_manifest_hash_affects_statement_id() {
        let first = object_revision_body(
            vec![RevisionId::new("git:sha256:parent")],
            BlobId::from_sha256_digest([1; 32]),
        )
        .and_then(unsigned_object_revision)
        .map(|statement| statement.statement_id());
        let second = object_revision_body(
            vec![RevisionId::new("git:sha256:parent")],
            BlobId::from_sha256_digest([2; 32]),
        )
        .and_then(unsigned_object_revision)
        .map(|statement| statement.statement_id());

        assert!(matches!((first, second), (Ok(first), Ok(second)) if first != second));
    }

    #[test]
    fn object_revision_signature_does_not_change_statement_id() -> Result<(), kairo_core::IdError> {
        let unsigned = object_revision_body(vec![RevisionId::new("git:sha256:parent")], blob_id()?)
            .and_then(unsigned_object_revision);
        let first = unsigned
            .clone()
            .and_then(|unsigned| {
                signature("key-1", vec![1, 2, 3]).map(|signature| (unsigned, signature))
            })
            .map(|(unsigned, signature)| SignedStatement::new(unsigned, signature).statement_id());
        let second = unsigned
            .and_then(|unsigned| {
                signature("key-2", vec![4, 5, 6]).map(|signature| (unsigned, signature))
            })
            .map(|(unsigned, signature)| SignedStatement::new(unsigned, signature).statement_id());

        assert_eq!(first, second);
        Ok(())
    }

    #[test]
    fn verifies_object_revision_ed25519_signature() -> Result<(), Box<dyn std::error::Error>> {
        let unsigned = unsigned_object_revision(object_revision_body(
            vec![RevisionId::new("git:sha256:parent")],
            blob_id()?,
        )?)?;
        let signature = ed25519_signature(&unsigned)?;
        let signed = SignedStatement::new(unsigned, signature);

        assert_eq!(
            signed.verify_signature(&public_key()),
            Ok(VerifiedSignature)
        );
        Ok(())
    }

    #[test]
    fn rejects_object_revision_signature_after_body_change(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let signed_unsigned = unsigned_object_revision(object_revision_body(
            vec![RevisionId::new("git:sha256:parent")],
            blob_id()?,
        )?)?;
        let changed_unsigned = unsigned_object_revision(object_revision_body(
            vec![RevisionId::new("git:sha256:different-parent")],
            blob_id()?,
        )?)?;
        let signature = ed25519_signature(&signed_unsigned)?;
        let signed = SignedStatement::new(changed_unsigned, signature);

        assert!(matches!(
            signed.verify_signature(&public_key()),
            Err(StatementSignatureError::Verification(
                SignatureVerificationError::InvalidSignature
            ))
        ));
        Ok(())
    }

    #[test]
    fn rejects_changed_object_revision_signature() -> Result<(), Box<dyn std::error::Error>> {
        let unsigned = unsigned_object_revision(object_revision_body(
            vec![RevisionId::new("git:sha256:parent")],
            blob_id()?,
        )?)?;
        let mut signature = ed25519_signature(&unsigned)?;
        signature.bytes[0] ^= 1;
        let signed = SignedStatement::new(unsigned, signature);

        assert!(matches!(
            signed.verify_signature(&public_key()),
            Err(StatementSignatureError::Verification(
                SignatureVerificationError::InvalidSignature
            ))
        ));
        Ok(())
    }

    #[test]
    fn rejects_wrong_object_revision_public_key() -> Result<(), Box<dyn std::error::Error>> {
        let unsigned = unsigned_object_revision(object_revision_body(
            vec![RevisionId::new("git:sha256:parent")],
            blob_id()?,
        )?)?;
        let signature = ed25519_signature(&unsigned)?;
        let signed = SignedStatement::new(unsigned, signature);

        assert!(matches!(
            signed.verify_signature(&other_public_key()),
            Err(StatementSignatureError::Verification(
                SignatureVerificationError::InvalidSignature
            ))
        ));
        Ok(())
    }

    fn ed25519_signature<B: StatementBody>(
        unsigned: &UnsignedStatement<B>,
    ) -> Result<Signature, kairo_core::IdError> {
        let signature = signing_key().sign(&unsigned.canonical_bytes()).to_bytes();
        Ok(Signature::new(
            actor_id()?,
            public_key().key_id().to_string(),
            "ed25519",
            signature.to_vec(),
        ))
    }
}
