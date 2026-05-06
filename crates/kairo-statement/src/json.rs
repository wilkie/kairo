use std::error::Error;
use std::fmt;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use kairo_core::{ActorId, BlobId, KairoRef, ObjectId, StatementId, Timestamp, TimestampError};
use kairo_identity::json::{ActorGenesisJsonError, PublicKeyJson};
use kairo_identity::KeyId;
use serde::{Deserialize, Serialize};

use crate::{
    ActorAttestationKeyAddBody, ActorAttestationKeyRevocationBody,
    ActorAttestationThresholdChangeBody, ActorAttestationThresholdChangeShapeError,
    ActorCapabilityGrantBody, ActorCapabilityRevocationBody, ActorEmergencyKeyRevocationBody,
    ActorEmergencyKeyRotationBody, ActorKeyRevocationBody, ActorKeyRotationBody, ActorTrustBody,
    ActorTrustShapeError, Capability, CapabilityConstraint, CapabilityScope, CapabilityShapeError,
    MultiSignedStatement, MultiSignedStatementError, ObjectBranchBody, ObjectGenesisBody,
    ObjectKind, ObjectRevisionBody, ObjectVersionTagBody, ObjectVersionTagShapeError, RevisionId,
    SemverParseError, SemverVersion, Signature, SignedStatement, StatementKind,
    StatementKindParseError, TrustDecision, TrustDecisionParseError, UnsignedStatement,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatementJsonError {
    UnexpectedType {
        expected: &'static str,
        actual: String,
    },
    UnexpectedVersion {
        expected: u8,
        actual: u8,
    },
    InvalidActor(kairo_core::IdError),
    InvalidObject(kairo_core::IdError),
    InvalidSubject(kairo_core::IdError),
    InvalidBlob(kairo_core::IdError),
    InvalidStatement(kairo_core::IdError),
    InvalidNonceHex,
    InvalidSignatureBase64,
    InvalidCreatedAt(TimestampError),
    InvalidVersion(SemverParseError),
    InvalidTagShape(ObjectVersionTagShapeError),
    InvalidTrustDecision(TrustDecisionParseError),
    InvalidTrustShape(ActorTrustShapeError),
    InvalidStatementKind(StatementKindParseError),
    InvalidCapabilityShape(CapabilityShapeError),
    InvalidPublicKey(ActorGenesisJsonError),
    InvalidMultiSignature(MultiSignedStatementError),
    InvalidThresholdShape(ActorAttestationThresholdChangeShapeError),
}

impl fmt::Display for StatementJsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedType { expected, actual } => {
                write!(f, "unexpected statement type {actual}; expected {expected}")
            }
            Self::UnexpectedVersion { expected, actual } => {
                write!(
                    f,
                    "unexpected statement version {actual}; expected {expected}"
                )
            }
            Self::InvalidActor(error) => write!(f, "invalid actor id: {error}"),
            Self::InvalidObject(error) => write!(f, "invalid object id: {error}"),
            Self::InvalidSubject(error) => write!(f, "invalid subject reference: {error}"),
            Self::InvalidBlob(error) => write!(f, "invalid blob id: {error}"),
            Self::InvalidStatement(error) => write!(f, "invalid statement id: {error}"),
            Self::InvalidNonceHex => f.write_str("invalid ObjectGenesis nonce hex"),
            Self::InvalidSignatureBase64 => f.write_str("invalid signature base64"),
            Self::InvalidCreatedAt(error) => write!(f, "invalid created_at: {error}"),
            Self::InvalidVersion(error) => write!(f, "{error}"),
            Self::InvalidTagShape(error) => write!(f, "{error}"),
            Self::InvalidTrustDecision(error) => write!(f, "{error}"),
            Self::InvalidTrustShape(error) => write!(f, "{error}"),
            Self::InvalidStatementKind(error) => write!(f, "{error}"),
            Self::InvalidCapabilityShape(error) => write!(f, "{error}"),
            Self::InvalidPublicKey(error) => write!(f, "invalid public key: {error}"),
            Self::InvalidMultiSignature(error) => write!(f, "{error}"),
            Self::InvalidThresholdShape(error) => write!(f, "{error}"),
        }
    }
}

impl Error for StatementJsonError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidActor(error)
            | Self::InvalidObject(error)
            | Self::InvalidSubject(error)
            | Self::InvalidBlob(error)
            | Self::InvalidStatement(error) => Some(error),
            Self::InvalidCreatedAt(error) => Some(error),
            Self::InvalidVersion(error) => Some(error),
            Self::InvalidTagShape(error) => Some(error),
            Self::InvalidTrustDecision(error) => Some(error),
            Self::InvalidTrustShape(error) => Some(error),
            Self::InvalidStatementKind(error) => Some(error),
            Self::InvalidCapabilityShape(error) => Some(error),
            Self::InvalidPublicKey(error) => Some(error),
            Self::InvalidMultiSignature(error) => Some(error),
            Self::InvalidThresholdShape(error) => Some(error),
            Self::UnexpectedType { .. }
            | Self::UnexpectedVersion { .. }
            | Self::InvalidNonceHex
            | Self::InvalidSignatureBase64 => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignatureJson {
    pub actor: String,
    pub key_id: String,
    pub algorithm: String,
    pub bytes: String,
}

impl SignatureJson {
    pub fn to_signature(&self) -> Result<Signature, StatementJsonError> {
        Ok(Signature::new(
            ActorId::new(self.actor.clone()).map_err(StatementJsonError::InvalidActor)?,
            self.key_id.clone(),
            self.algorithm.clone(),
            STANDARD
                .decode(&self.bytes)
                .map_err(|_| StatementJsonError::InvalidSignatureBase64)?,
        ))
    }

    pub fn from_signature(signature: &Signature) -> Self {
        Self {
            actor: signature.actor().to_string(),
            key_id: signature.key_id().to_owned(),
            algorithm: signature.algorithm().to_owned(),
            bytes: STANDARD.encode(signature.bytes()),
        }
    }
}

fn signatures_to_envelope(
    signatures: &[SignatureJson],
) -> Result<Vec<Signature>, StatementJsonError> {
    signatures
        .iter()
        .map(SignatureJson::to_signature)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectGenesisStatementJson {
    #[serde(rename = "type")]
    pub statement_type: String,
    pub version: u8,
    pub body: ObjectGenesisBodyJson,
    pub signature: SignatureJson,
}

impl ObjectGenesisStatementJson {
    pub fn to_statement(&self) -> Result<crate::ObjectGenesisStatement, StatementJsonError> {
        ensure_statement_shape(&self.statement_type, self.version, "ObjectGenesis", 1)?;

        Ok(crate::ObjectGenesisStatement::new(
            self.body.to_body()?,
            self.signature.to_signature()?,
        ))
    }

    pub fn from_statement(statement: &crate::ObjectGenesisStatement) -> Self {
        Self {
            statement_type: "ObjectGenesis".to_owned(),
            version: 1,
            body: ObjectGenesisBodyJson::from_body(statement.body()),
            signature: SignatureJson::from_signature(statement.signature()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectGenesisBodyJson {
    pub object_kind: String,
    pub created_by: String,
    pub created_at: String,
    pub nonce: String,
    pub initial_revision: Option<String>,
}

impl ObjectGenesisBodyJson {
    pub fn to_body(&self) -> Result<ObjectGenesisBody, StatementJsonError> {
        let created_at: Timestamp = self
            .created_at
            .parse()
            .map_err(StatementJsonError::InvalidCreatedAt)?;

        Ok(ObjectGenesisBody::new(
            ObjectKind::new(self.object_kind.clone()),
            ActorId::new(self.created_by.clone()).map_err(StatementJsonError::InvalidActor)?,
            created_at,
            decode_nonce_hex(&self.nonce)?,
            self.initial_revision.clone().map(RevisionId::new),
        ))
    }

    pub fn from_body(body: &ObjectGenesisBody) -> Self {
        Self {
            object_kind: body.object_kind().as_str().to_owned(),
            created_by: body.created_by().to_string(),
            created_at: body.created_at().to_string(),
            nonce: encode_nonce_hex(body.nonce()),
            initial_revision: body
                .initial_revision()
                .map(|revision| revision.as_str().to_owned()),
        }
    }
}

fn encode_nonce_hex(nonce: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for byte in nonce {
        out.push(hex_char(byte >> 4));
        out.push(hex_char(byte & 0x0f));
    }
    out
}

fn hex_char(value: u8) -> char {
    match value {
        0..=9 => char::from(b'0' + value),
        10..=15 => char::from(b'a' + (value - 10)),
        _ => '?',
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectRevisionStatementJson {
    #[serde(rename = "type")]
    pub statement_type: String,
    pub version: u8,
    pub actor: String,
    pub subject: String,
    pub created_at: String,
    pub body: ObjectRevisionBodyJson,
    pub signature: SignatureJson,
}

impl ObjectRevisionStatementJson {
    pub fn to_statement(&self) -> Result<SignedStatement<ObjectRevisionBody>, StatementJsonError> {
        ensure_statement_shape(&self.statement_type, self.version, "ObjectRevision", 1)?;

        let created_at: Timestamp = self
            .created_at
            .parse()
            .map_err(StatementJsonError::InvalidCreatedAt)?;

        let unsigned = UnsignedStatement::new(
            ActorId::new(self.actor.clone()).map_err(StatementJsonError::InvalidActor)?,
            self.subject
                .parse::<KairoRef>()
                .map_err(StatementJsonError::InvalidSubject)?,
            created_at,
            self.body.to_body()?,
        );

        Ok(SignedStatement::new(
            unsigned,
            self.signature.to_signature()?,
        ))
    }

    pub fn from_statement(statement: &SignedStatement<ObjectRevisionBody>) -> Self {
        let unsigned = statement.unsigned();
        Self {
            statement_type: "ObjectRevision".to_owned(),
            version: 1,
            actor: unsigned.actor().to_string(),
            subject: unsigned.subject().to_string(),
            created_at: unsigned.created_at().to_string(),
            body: ObjectRevisionBodyJson::from_body(unsigned.body()),
            signature: SignatureJson::from_signature(statement.signature()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectRevisionBodyJson {
    pub object: String,
    pub revision: String,
    pub parents: Vec<String>,
    pub manifest_hash: String,
    pub attests_reachable_history: bool,
}

impl ObjectRevisionBodyJson {
    pub fn to_body(&self) -> Result<ObjectRevisionBody, StatementJsonError> {
        Ok(ObjectRevisionBody::new(
            ObjectId::new(self.object.clone()).map_err(StatementJsonError::InvalidObject)?,
            RevisionId::new(self.revision.clone()),
            self.parents.iter().cloned().map(RevisionId::new).collect(),
            BlobId::new(self.manifest_hash.clone()).map_err(StatementJsonError::InvalidBlob)?,
            self.attests_reachable_history,
        ))
    }

    pub fn from_body(body: &ObjectRevisionBody) -> Self {
        Self {
            object: body.object().to_string(),
            revision: body.revision().as_str().to_owned(),
            parents: body
                .parents()
                .iter()
                .map(|revision| revision.as_str().to_owned())
                .collect(),
            manifest_hash: body.manifest_hash().to_string(),
            attests_reachable_history: body.attests_reachable_history(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectBranchStatementJson {
    #[serde(rename = "type")]
    pub statement_type: String,
    pub version: u8,
    pub actor: String,
    pub subject: String,
    pub created_at: String,
    pub body: ObjectBranchBodyJson,
    pub signature: SignatureJson,
}

impl ObjectBranchStatementJson {
    pub fn to_statement(&self) -> Result<SignedStatement<ObjectBranchBody>, StatementJsonError> {
        ensure_statement_shape(&self.statement_type, self.version, "ObjectBranch", 1)?;

        let created_at: Timestamp = self
            .created_at
            .parse()
            .map_err(StatementJsonError::InvalidCreatedAt)?;

        let unsigned = UnsignedStatement::new(
            ActorId::new(self.actor.clone()).map_err(StatementJsonError::InvalidActor)?,
            self.subject
                .parse::<KairoRef>()
                .map_err(StatementJsonError::InvalidSubject)?,
            created_at,
            self.body.to_body()?,
        );

        Ok(SignedStatement::new(
            unsigned,
            self.signature.to_signature()?,
        ))
    }

    pub fn from_statement(statement: &SignedStatement<ObjectBranchBody>) -> Self {
        let unsigned = statement.unsigned();
        Self {
            statement_type: "ObjectBranch".to_owned(),
            version: 1,
            actor: unsigned.actor().to_string(),
            subject: unsigned.subject().to_string(),
            created_at: unsigned.created_at().to_string(),
            body: ObjectBranchBodyJson::from_body(unsigned.body()),
            signature: SignatureJson::from_signature(statement.signature()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectBranchBodyJson {
    pub object: String,
    pub name: String,
    pub revision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<String>,
}

impl ObjectBranchBodyJson {
    pub fn to_body(&self) -> Result<ObjectBranchBody, StatementJsonError> {
        let supersedes = match &self.supersedes {
            Some(value) => Some(
                StatementId::new(value.clone())
                    .map_err(StatementJsonError::InvalidStatement)?,
            ),
            None => None,
        };
        Ok(ObjectBranchBody::new(
            ObjectId::new(self.object.clone()).map_err(StatementJsonError::InvalidObject)?,
            self.name.clone(),
            StatementId::new(self.revision.clone())
                .map_err(StatementJsonError::InvalidStatement)?,
            supersedes,
        ))
    }

    pub fn from_body(body: &ObjectBranchBody) -> Self {
        Self {
            object: body.object().to_string(),
            name: body.name().to_owned(),
            revision: body.revision().to_string(),
            supersedes: body.supersedes().map(|id| id.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectVersionTagStatementJson {
    #[serde(rename = "type")]
    pub statement_type: String,
    pub version: u8,
    pub actor: String,
    pub subject: String,
    pub created_at: String,
    pub body: ObjectVersionTagBodyJson,
    pub signature: SignatureJson,
}

impl ObjectVersionTagStatementJson {
    pub fn to_statement(
        &self,
    ) -> Result<SignedStatement<ObjectVersionTagBody>, StatementJsonError> {
        ensure_statement_shape(&self.statement_type, self.version, "ObjectVersionTag", 1)?;

        let created_at: Timestamp = self
            .created_at
            .parse()
            .map_err(StatementJsonError::InvalidCreatedAt)?;

        let unsigned = UnsignedStatement::new(
            ActorId::new(self.actor.clone()).map_err(StatementJsonError::InvalidActor)?,
            self.subject
                .parse::<KairoRef>()
                .map_err(StatementJsonError::InvalidSubject)?,
            created_at,
            self.body.to_body()?,
        );

        Ok(SignedStatement::new(
            unsigned,
            self.signature.to_signature()?,
        ))
    }

    pub fn from_statement(statement: &SignedStatement<ObjectVersionTagBody>) -> Self {
        let unsigned = statement.unsigned();
        Self {
            statement_type: "ObjectVersionTag".to_owned(),
            version: 1,
            actor: unsigned.actor().to_string(),
            subject: unsigned.subject().to_string(),
            created_at: unsigned.created_at().to_string(),
            body: ObjectVersionTagBodyJson::from_body(unsigned.body()),
            signature: SignatureJson::from_signature(statement.signature()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectVersionTagBodyJson {
    pub object: String,
    pub version: String,
    pub target: Option<String>,
    pub supersedes: Option<String>,
}

impl ObjectVersionTagBodyJson {
    pub fn to_body(&self) -> Result<ObjectVersionTagBody, StatementJsonError> {
        let object =
            ObjectId::new(self.object.clone()).map_err(StatementJsonError::InvalidObject)?;
        let semver_version =
            SemverVersion::parse(&self.version).map_err(StatementJsonError::InvalidVersion)?;
        let target = match &self.target {
            Some(value) => Some(
                StatementId::new(value.clone()).map_err(StatementJsonError::InvalidStatement)?,
            ),
            None => None,
        };
        let supersedes = match &self.supersedes {
            Some(value) => Some(
                StatementId::new(value.clone()).map_err(StatementJsonError::InvalidStatement)?,
            ),
            None => None,
        };
        ObjectVersionTagBody::new(object, semver_version, target, supersedes)
            .map_err(StatementJsonError::InvalidTagShape)
    }

    pub fn from_body(body: &ObjectVersionTagBody) -> Self {
        Self {
            object: body.object().to_string(),
            version: body.version().as_str().to_owned(),
            target: body.target().map(|id| id.to_string()),
            supersedes: body.supersedes().map(|id| id.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorTrustStatementJson {
    #[serde(rename = "type")]
    pub statement_type: String,
    pub version: u8,
    pub actor: String,
    pub subject: String,
    pub created_at: String,
    pub body: ActorTrustBodyJson,
    pub signature: SignatureJson,
}

impl ActorTrustStatementJson {
    pub fn to_statement(&self) -> Result<SignedStatement<ActorTrustBody>, StatementJsonError> {
        ensure_statement_shape(&self.statement_type, self.version, "ActorTrust", 1)?;

        let created_at: Timestamp = self
            .created_at
            .parse()
            .map_err(StatementJsonError::InvalidCreatedAt)?;

        let unsigned = UnsignedStatement::new(
            ActorId::new(self.actor.clone()).map_err(StatementJsonError::InvalidActor)?,
            self.subject
                .parse::<KairoRef>()
                .map_err(StatementJsonError::InvalidSubject)?,
            created_at,
            self.body.to_body()?,
        );

        Ok(SignedStatement::new(
            unsigned,
            self.signature.to_signature()?,
        ))
    }

    pub fn from_statement(statement: &SignedStatement<ActorTrustBody>) -> Self {
        let unsigned = statement.unsigned();
        Self {
            statement_type: "ActorTrust".to_owned(),
            version: 1,
            actor: unsigned.actor().to_string(),
            subject: unsigned.subject().to_string(),
            created_at: unsigned.created_at().to_string(),
            body: ActorTrustBodyJson::from_body(unsigned.body()),
            signature: SignatureJson::from_signature(statement.signature()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorTrustBodyJson {
    pub trusted_actor: String,
    pub decision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub supersedes: Option<String>,
}

impl ActorTrustBodyJson {
    pub fn to_body(&self) -> Result<ActorTrustBody, StatementJsonError> {
        let trusted_actor =
            ActorId::new(self.trusted_actor.clone()).map_err(StatementJsonError::InvalidActor)?;
        let decision = match &self.decision {
            Some(value) => Some(
                TrustDecision::parse(value).map_err(StatementJsonError::InvalidTrustDecision)?,
            ),
            None => None,
        };
        let supersedes = match &self.supersedes {
            Some(value) => Some(
                StatementId::new(value.clone()).map_err(StatementJsonError::InvalidStatement)?,
            ),
            None => None,
        };
        ActorTrustBody::new(trusted_actor, decision, self.reason.clone(), supersedes)
            .map_err(StatementJsonError::InvalidTrustShape)
    }

    pub fn from_body(body: &ActorTrustBody) -> Self {
        Self {
            trusted_actor: body.trusted_actor().to_string(),
            decision: body.decision().map(|d| d.as_str().to_owned()),
            reason: body.reason().map(|r| r.to_owned()),
            supersedes: body.supersedes().map(|id| id.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityScopeJson {
    Object(String),
    Actor(String),
}

impl CapabilityScopeJson {
    fn to_scope(&self) -> Result<CapabilityScope, StatementJsonError> {
        match self {
            Self::Object(id) => Ok(CapabilityScope::Object(
                ObjectId::new(id.clone()).map_err(StatementJsonError::InvalidObject)?,
            )),
            Self::Actor(id) => Ok(CapabilityScope::Actor(
                ActorId::new(id.clone()).map_err(StatementJsonError::InvalidActor)?,
            )),
        }
    }

    fn from_scope(scope: &CapabilityScope) -> Self {
        match scope {
            CapabilityScope::Object(id) => Self::Object(id.to_string()),
            CapabilityScope::Actor(id) => Self::Actor(id.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityConstraintJson {
    /// RFC 3339 UTC seconds, matching the envelope `created_at` shape.
    ExpiresAt(String),
    MaxDelegationDepth(u8),
    KeyPinned(String),
}

impl CapabilityConstraintJson {
    fn to_constraint(&self) -> Result<CapabilityConstraint, StatementJsonError> {
        match self {
            Self::ExpiresAt(value) => {
                let ts: Timestamp = value.parse().map_err(StatementJsonError::InvalidCreatedAt)?;
                Ok(CapabilityConstraint::ExpiresAt(ts))
            }
            Self::MaxDelegationDepth(depth) => {
                Ok(CapabilityConstraint::MaxDelegationDepth(*depth))
            }
            Self::KeyPinned(id) => Ok(CapabilityConstraint::KeyPinned(KeyId::new(id.clone()))),
        }
    }

    fn from_constraint(constraint: &CapabilityConstraint) -> Self {
        match constraint {
            CapabilityConstraint::ExpiresAt(ts) => Self::ExpiresAt(ts.to_string()),
            CapabilityConstraint::MaxDelegationDepth(depth) => Self::MaxDelegationDepth(*depth),
            CapabilityConstraint::KeyPinned(id) => Self::KeyPinned(id.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityJson {
    pub scope: CapabilityScopeJson,
    pub statement_kinds: Vec<String>,
    pub delegable: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<CapabilityConstraintJson>,
}

impl CapabilityJson {
    fn to_capability(&self) -> Result<Capability, StatementJsonError> {
        let scope = self.scope.to_scope()?;
        let mut kinds = Vec::with_capacity(self.statement_kinds.len());
        for kind_str in &self.statement_kinds {
            kinds.push(
                StatementKind::parse(kind_str)
                    .map_err(StatementJsonError::InvalidStatementKind)?,
            );
        }
        let mut constraints = Vec::with_capacity(self.constraints.len());
        for constraint in &self.constraints {
            constraints.push(constraint.to_constraint()?);
        }
        Capability::new(scope, kinds, self.delegable, constraints)
            .map_err(StatementJsonError::InvalidCapabilityShape)
    }

    fn from_capability(cap: &Capability) -> Self {
        Self {
            scope: CapabilityScopeJson::from_scope(cap.scope()),
            statement_kinds: cap
                .statement_kinds()
                .iter()
                .map(|k| k.as_str().to_owned())
                .collect(),
            delegable: cap.delegable(),
            constraints: cap
                .constraints()
                .iter()
                .map(CapabilityConstraintJson::from_constraint)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorCapabilityGrantStatementJson {
    #[serde(rename = "type")]
    pub statement_type: String,
    pub version: u8,
    pub actor: String,
    pub subject: String,
    pub created_at: String,
    pub body: ActorCapabilityGrantBodyJson,
    pub signature: SignatureJson,
}

impl ActorCapabilityGrantStatementJson {
    pub fn to_statement(
        &self,
    ) -> Result<SignedStatement<ActorCapabilityGrantBody>, StatementJsonError> {
        ensure_statement_shape(&self.statement_type, self.version, "ActorCapabilityGrant", 1)?;

        let created_at: Timestamp = self
            .created_at
            .parse()
            .map_err(StatementJsonError::InvalidCreatedAt)?;

        let unsigned = UnsignedStatement::new(
            ActorId::new(self.actor.clone()).map_err(StatementJsonError::InvalidActor)?,
            self.subject
                .parse::<KairoRef>()
                .map_err(StatementJsonError::InvalidSubject)?,
            created_at,
            self.body.to_body()?,
        );

        Ok(SignedStatement::new(
            unsigned,
            self.signature.to_signature()?,
        ))
    }

    pub fn from_statement(statement: &SignedStatement<ActorCapabilityGrantBody>) -> Self {
        let unsigned = statement.unsigned();
        Self {
            statement_type: "ActorCapabilityGrant".to_owned(),
            version: 1,
            actor: unsigned.actor().to_string(),
            subject: unsigned.subject().to_string(),
            created_at: unsigned.created_at().to_string(),
            body: ActorCapabilityGrantBodyJson::from_body(unsigned.body()),
            signature: SignatureJson::from_signature(statement.signature()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorCapabilityGrantBodyJson {
    pub grantee: String,
    pub capability: CapabilityJson,
    pub supersedes: Option<String>,
}

impl ActorCapabilityGrantBodyJson {
    pub fn to_body(&self) -> Result<ActorCapabilityGrantBody, StatementJsonError> {
        let grantee =
            ActorId::new(self.grantee.clone()).map_err(StatementJsonError::InvalidActor)?;
        let capability = self.capability.to_capability()?;
        let supersedes = match &self.supersedes {
            Some(value) => Some(
                StatementId::new(value.clone()).map_err(StatementJsonError::InvalidStatement)?,
            ),
            None => None,
        };
        Ok(ActorCapabilityGrantBody::new(grantee, capability, supersedes))
    }

    pub fn from_body(body: &ActorCapabilityGrantBody) -> Self {
        Self {
            grantee: body.grantee().to_string(),
            capability: CapabilityJson::from_capability(body.capability()),
            supersedes: body.supersedes().map(|id| id.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorCapabilityRevocationStatementJson {
    #[serde(rename = "type")]
    pub statement_type: String,
    pub version: u8,
    pub actor: String,
    pub subject: String,
    pub created_at: String,
    pub body: ActorCapabilityRevocationBodyJson,
    pub signature: SignatureJson,
}

impl ActorCapabilityRevocationStatementJson {
    pub fn to_statement(
        &self,
    ) -> Result<SignedStatement<ActorCapabilityRevocationBody>, StatementJsonError> {
        ensure_statement_shape(
            &self.statement_type,
            self.version,
            "ActorCapabilityRevocation",
            1,
        )?;

        let created_at: Timestamp = self
            .created_at
            .parse()
            .map_err(StatementJsonError::InvalidCreatedAt)?;

        let unsigned = UnsignedStatement::new(
            ActorId::new(self.actor.clone()).map_err(StatementJsonError::InvalidActor)?,
            self.subject
                .parse::<KairoRef>()
                .map_err(StatementJsonError::InvalidSubject)?,
            created_at,
            self.body.to_body()?,
        );

        Ok(SignedStatement::new(
            unsigned,
            self.signature.to_signature()?,
        ))
    }

    pub fn from_statement(statement: &SignedStatement<ActorCapabilityRevocationBody>) -> Self {
        let unsigned = statement.unsigned();
        Self {
            statement_type: "ActorCapabilityRevocation".to_owned(),
            version: 1,
            actor: unsigned.actor().to_string(),
            subject: unsigned.subject().to_string(),
            created_at: unsigned.created_at().to_string(),
            body: ActorCapabilityRevocationBodyJson::from_body(unsigned.body()),
            signature: SignatureJson::from_signature(statement.signature()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorCapabilityRevocationBodyJson {
    pub revoked_grant: String,
    pub retroactive: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl ActorCapabilityRevocationBodyJson {
    pub fn to_body(&self) -> Result<ActorCapabilityRevocationBody, StatementJsonError> {
        let revoked_grant = StatementId::new(self.revoked_grant.clone())
            .map_err(StatementJsonError::InvalidStatement)?;
        Ok(ActorCapabilityRevocationBody::new(
            revoked_grant,
            self.retroactive,
            self.reason.clone(),
        ))
    }

    pub fn from_body(body: &ActorCapabilityRevocationBody) -> Self {
        Self {
            revoked_grant: body.revoked_grant().to_string(),
            retroactive: body.retroactive(),
            reason: body.reason().map(|r| r.to_owned()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorKeyRotationStatementJson {
    #[serde(rename = "type")]
    pub statement_type: String,
    pub version: u8,
    pub actor: String,
    pub subject: String,
    pub created_at: String,
    pub body: ActorKeyRotationBodyJson,
    pub signature: SignatureJson,
}

impl ActorKeyRotationStatementJson {
    pub fn to_statement(
        &self,
    ) -> Result<SignedStatement<ActorKeyRotationBody>, StatementJsonError> {
        ensure_statement_shape(&self.statement_type, self.version, "ActorKeyRotation", 1)?;

        let created_at: Timestamp = self
            .created_at
            .parse()
            .map_err(StatementJsonError::InvalidCreatedAt)?;

        let unsigned = UnsignedStatement::new(
            ActorId::new(self.actor.clone()).map_err(StatementJsonError::InvalidActor)?,
            self.subject
                .parse::<KairoRef>()
                .map_err(StatementJsonError::InvalidSubject)?,
            created_at,
            self.body.to_body()?,
        );

        Ok(SignedStatement::new(
            unsigned,
            self.signature.to_signature()?,
        ))
    }

    pub fn from_statement(statement: &SignedStatement<ActorKeyRotationBody>) -> Self {
        let unsigned = statement.unsigned();
        Self {
            statement_type: "ActorKeyRotation".to_owned(),
            version: 1,
            actor: unsigned.actor().to_string(),
            subject: unsigned.subject().to_string(),
            created_at: unsigned.created_at().to_string(),
            body: ActorKeyRotationBodyJson::from_body(unsigned.body()),
            signature: SignatureJson::from_signature(statement.signature()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorKeyRotationBodyJson {
    pub next_key: PublicKeyJson,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<String>,
}

impl ActorKeyRotationBodyJson {
    pub fn to_body(&self) -> Result<ActorKeyRotationBody, StatementJsonError> {
        let next_key = self
            .next_key
            .to_public_key()
            .map_err(StatementJsonError::InvalidPublicKey)?;
        let supersedes = match &self.supersedes {
            Some(value) => Some(
                StatementId::new(value.clone())
                    .map_err(StatementJsonError::InvalidStatement)?,
            ),
            None => None,
        };
        Ok(ActorKeyRotationBody::new(next_key, supersedes))
    }

    pub fn from_body(body: &ActorKeyRotationBody) -> Self {
        Self {
            next_key: PublicKeyJson::from_public_key(body.next_key()),
            supersedes: body.supersedes().map(|id| id.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorKeyRevocationStatementJson {
    #[serde(rename = "type")]
    pub statement_type: String,
    pub version: u8,
    pub actor: String,
    pub subject: String,
    pub created_at: String,
    pub body: ActorKeyRevocationBodyJson,
    pub signature: SignatureJson,
}

impl ActorKeyRevocationStatementJson {
    pub fn to_statement(
        &self,
    ) -> Result<SignedStatement<ActorKeyRevocationBody>, StatementJsonError> {
        ensure_statement_shape(&self.statement_type, self.version, "ActorKeyRevocation", 1)?;

        let created_at: Timestamp = self
            .created_at
            .parse()
            .map_err(StatementJsonError::InvalidCreatedAt)?;

        let unsigned = UnsignedStatement::new(
            ActorId::new(self.actor.clone()).map_err(StatementJsonError::InvalidActor)?,
            self.subject
                .parse::<KairoRef>()
                .map_err(StatementJsonError::InvalidSubject)?,
            created_at,
            self.body.to_body()?,
        );

        Ok(SignedStatement::new(
            unsigned,
            self.signature.to_signature()?,
        ))
    }

    pub fn from_statement(statement: &SignedStatement<ActorKeyRevocationBody>) -> Self {
        let unsigned = statement.unsigned();
        Self {
            statement_type: "ActorKeyRevocation".to_owned(),
            version: 1,
            actor: unsigned.actor().to_string(),
            subject: unsigned.subject().to_string(),
            created_at: unsigned.created_at().to_string(),
            body: ActorKeyRevocationBodyJson::from_body(unsigned.body()),
            signature: SignatureJson::from_signature(statement.signature()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorKeyRevocationBodyJson {
    pub revoked_key: String,
    pub retroactive: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl ActorKeyRevocationBodyJson {
    pub fn to_body(&self) -> Result<ActorKeyRevocationBody, StatementJsonError> {
        Ok(ActorKeyRevocationBody::new(
            KeyId::new(self.revoked_key.clone()),
            self.retroactive,
            self.reason.clone(),
        ))
    }

    pub fn from_body(body: &ActorKeyRevocationBody) -> Self {
        Self {
            revoked_key: body.revoked_key().to_string(),
            retroactive: body.retroactive(),
            reason: body.reason().map(|r| r.to_owned()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorEmergencyKeyRotationStatementJson {
    #[serde(rename = "type")]
    pub statement_type: String,
    pub version: u8,
    pub actor: String,
    pub subject: String,
    pub created_at: String,
    pub body: ActorEmergencyKeyRotationBodyJson,
    pub signatures: Vec<SignatureJson>,
}

impl ActorEmergencyKeyRotationStatementJson {
    pub fn to_statement(
        &self,
    ) -> Result<MultiSignedStatement<ActorEmergencyKeyRotationBody>, StatementJsonError> {
        ensure_statement_shape(
            &self.statement_type,
            self.version,
            "ActorEmergencyKeyRotation",
            1,
        )?;

        let created_at: Timestamp = self
            .created_at
            .parse()
            .map_err(StatementJsonError::InvalidCreatedAt)?;

        let unsigned = UnsignedStatement::new(
            ActorId::new(self.actor.clone()).map_err(StatementJsonError::InvalidActor)?,
            self.subject
                .parse::<KairoRef>()
                .map_err(StatementJsonError::InvalidSubject)?,
            created_at,
            self.body.to_body()?,
        );

        let signatures = signatures_to_envelope(&self.signatures)?;
        MultiSignedStatement::new(unsigned, signatures)
            .map_err(StatementJsonError::InvalidMultiSignature)
    }

    pub fn from_statement(
        statement: &MultiSignedStatement<ActorEmergencyKeyRotationBody>,
    ) -> Self {
        let unsigned = statement.unsigned();
        Self {
            statement_type: "ActorEmergencyKeyRotation".to_owned(),
            version: 1,
            actor: unsigned.actor().to_string(),
            subject: unsigned.subject().to_string(),
            created_at: unsigned.created_at().to_string(),
            body: ActorEmergencyKeyRotationBodyJson::from_body(unsigned.body()),
            signatures: statement
                .signatures()
                .iter()
                .map(SignatureJson::from_signature)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorEmergencyKeyRotationBodyJson {
    pub next_key: PublicKeyJson,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<String>,
}

impl ActorEmergencyKeyRotationBodyJson {
    pub fn to_body(&self) -> Result<ActorEmergencyKeyRotationBody, StatementJsonError> {
        let next_key = self
            .next_key
            .to_public_key()
            .map_err(StatementJsonError::InvalidPublicKey)?;
        let supersedes = match &self.supersedes {
            Some(value) => Some(
                StatementId::new(value.clone())
                    .map_err(StatementJsonError::InvalidStatement)?,
            ),
            None => None,
        };
        Ok(ActorEmergencyKeyRotationBody::new(next_key, supersedes))
    }

    pub fn from_body(body: &ActorEmergencyKeyRotationBody) -> Self {
        Self {
            next_key: PublicKeyJson::from_public_key(body.next_key()),
            supersedes: body.supersedes().map(|id| id.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorEmergencyKeyRevocationStatementJson {
    #[serde(rename = "type")]
    pub statement_type: String,
    pub version: u8,
    pub actor: String,
    pub subject: String,
    pub created_at: String,
    pub body: ActorEmergencyKeyRevocationBodyJson,
    pub signatures: Vec<SignatureJson>,
}

impl ActorEmergencyKeyRevocationStatementJson {
    pub fn to_statement(
        &self,
    ) -> Result<MultiSignedStatement<ActorEmergencyKeyRevocationBody>, StatementJsonError> {
        ensure_statement_shape(
            &self.statement_type,
            self.version,
            "ActorEmergencyKeyRevocation",
            1,
        )?;

        let created_at: Timestamp = self
            .created_at
            .parse()
            .map_err(StatementJsonError::InvalidCreatedAt)?;

        let unsigned = UnsignedStatement::new(
            ActorId::new(self.actor.clone()).map_err(StatementJsonError::InvalidActor)?,
            self.subject
                .parse::<KairoRef>()
                .map_err(StatementJsonError::InvalidSubject)?,
            created_at,
            self.body.to_body()?,
        );

        let signatures = signatures_to_envelope(&self.signatures)?;
        MultiSignedStatement::new(unsigned, signatures)
            .map_err(StatementJsonError::InvalidMultiSignature)
    }

    pub fn from_statement(
        statement: &MultiSignedStatement<ActorEmergencyKeyRevocationBody>,
    ) -> Self {
        let unsigned = statement.unsigned();
        Self {
            statement_type: "ActorEmergencyKeyRevocation".to_owned(),
            version: 1,
            actor: unsigned.actor().to_string(),
            subject: unsigned.subject().to_string(),
            created_at: unsigned.created_at().to_string(),
            body: ActorEmergencyKeyRevocationBodyJson::from_body(unsigned.body()),
            signatures: statement
                .signatures()
                .iter()
                .map(SignatureJson::from_signature)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorEmergencyKeyRevocationBodyJson {
    pub revoked_key: String,
    pub retroactive: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl ActorEmergencyKeyRevocationBodyJson {
    pub fn to_body(&self) -> Result<ActorEmergencyKeyRevocationBody, StatementJsonError> {
        Ok(ActorEmergencyKeyRevocationBody::new(
            KeyId::new(self.revoked_key.clone()),
            self.retroactive,
            self.reason.clone(),
        ))
    }

    pub fn from_body(body: &ActorEmergencyKeyRevocationBody) -> Self {
        Self {
            revoked_key: body.revoked_key().to_string(),
            retroactive: body.retroactive(),
            reason: body.reason().map(|r| r.to_owned()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorAttestationKeyAddStatementJson {
    #[serde(rename = "type")]
    pub statement_type: String,
    pub version: u8,
    pub actor: String,
    pub subject: String,
    pub created_at: String,
    pub body: ActorAttestationKeyAddBodyJson,
    pub signatures: Vec<SignatureJson>,
}

impl ActorAttestationKeyAddStatementJson {
    pub fn to_statement(
        &self,
    ) -> Result<MultiSignedStatement<ActorAttestationKeyAddBody>, StatementJsonError> {
        ensure_statement_shape(
            &self.statement_type,
            self.version,
            "ActorAttestationKeyAdd",
            1,
        )?;

        let created_at: Timestamp = self
            .created_at
            .parse()
            .map_err(StatementJsonError::InvalidCreatedAt)?;

        let unsigned = UnsignedStatement::new(
            ActorId::new(self.actor.clone()).map_err(StatementJsonError::InvalidActor)?,
            self.subject
                .parse::<KairoRef>()
                .map_err(StatementJsonError::InvalidSubject)?,
            created_at,
            self.body.to_body()?,
        );

        let signatures = signatures_to_envelope(&self.signatures)?;
        MultiSignedStatement::new(unsigned, signatures)
            .map_err(StatementJsonError::InvalidMultiSignature)
    }

    pub fn from_statement(statement: &MultiSignedStatement<ActorAttestationKeyAddBody>) -> Self {
        let unsigned = statement.unsigned();
        Self {
            statement_type: "ActorAttestationKeyAdd".to_owned(),
            version: 1,
            actor: unsigned.actor().to_string(),
            subject: unsigned.subject().to_string(),
            created_at: unsigned.created_at().to_string(),
            body: ActorAttestationKeyAddBodyJson::from_body(unsigned.body()),
            signatures: statement
                .signatures()
                .iter()
                .map(SignatureJson::from_signature)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorAttestationKeyAddBodyJson {
    pub new_key: PublicKeyJson,
}

impl ActorAttestationKeyAddBodyJson {
    pub fn to_body(&self) -> Result<ActorAttestationKeyAddBody, StatementJsonError> {
        let new_key = self
            .new_key
            .to_public_key()
            .map_err(StatementJsonError::InvalidPublicKey)?;
        Ok(ActorAttestationKeyAddBody::new(new_key))
    }

    pub fn from_body(body: &ActorAttestationKeyAddBody) -> Self {
        Self {
            new_key: PublicKeyJson::from_public_key(body.new_key()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorAttestationKeyRevocationStatementJson {
    #[serde(rename = "type")]
    pub statement_type: String,
    pub version: u8,
    pub actor: String,
    pub subject: String,
    pub created_at: String,
    pub body: ActorAttestationKeyRevocationBodyJson,
    pub signatures: Vec<SignatureJson>,
}

impl ActorAttestationKeyRevocationStatementJson {
    pub fn to_statement(
        &self,
    ) -> Result<MultiSignedStatement<ActorAttestationKeyRevocationBody>, StatementJsonError> {
        ensure_statement_shape(
            &self.statement_type,
            self.version,
            "ActorAttestationKeyRevocation",
            1,
        )?;

        let created_at: Timestamp = self
            .created_at
            .parse()
            .map_err(StatementJsonError::InvalidCreatedAt)?;

        let unsigned = UnsignedStatement::new(
            ActorId::new(self.actor.clone()).map_err(StatementJsonError::InvalidActor)?,
            self.subject
                .parse::<KairoRef>()
                .map_err(StatementJsonError::InvalidSubject)?,
            created_at,
            self.body.to_body()?,
        );

        let signatures = signatures_to_envelope(&self.signatures)?;
        MultiSignedStatement::new(unsigned, signatures)
            .map_err(StatementJsonError::InvalidMultiSignature)
    }

    pub fn from_statement(
        statement: &MultiSignedStatement<ActorAttestationKeyRevocationBody>,
    ) -> Self {
        let unsigned = statement.unsigned();
        Self {
            statement_type: "ActorAttestationKeyRevocation".to_owned(),
            version: 1,
            actor: unsigned.actor().to_string(),
            subject: unsigned.subject().to_string(),
            created_at: unsigned.created_at().to_string(),
            body: ActorAttestationKeyRevocationBodyJson::from_body(unsigned.body()),
            signatures: statement
                .signatures()
                .iter()
                .map(SignatureJson::from_signature)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorAttestationKeyRevocationBodyJson {
    pub revoked_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl ActorAttestationKeyRevocationBodyJson {
    pub fn to_body(&self) -> Result<ActorAttestationKeyRevocationBody, StatementJsonError> {
        Ok(ActorAttestationKeyRevocationBody::new(
            KeyId::new(self.revoked_key.clone()),
            self.reason.clone(),
        ))
    }

    pub fn from_body(body: &ActorAttestationKeyRevocationBody) -> Self {
        Self {
            revoked_key: body.revoked_key().to_string(),
            reason: body.reason().map(|r| r.to_owned()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorAttestationThresholdChangeStatementJson {
    #[serde(rename = "type")]
    pub statement_type: String,
    pub version: u8,
    pub actor: String,
    pub subject: String,
    pub created_at: String,
    pub body: ActorAttestationThresholdChangeBodyJson,
    pub signatures: Vec<SignatureJson>,
}

impl ActorAttestationThresholdChangeStatementJson {
    pub fn to_statement(
        &self,
    ) -> Result<MultiSignedStatement<ActorAttestationThresholdChangeBody>, StatementJsonError> {
        ensure_statement_shape(
            &self.statement_type,
            self.version,
            "ActorAttestationThresholdChange",
            1,
        )?;

        let created_at: Timestamp = self
            .created_at
            .parse()
            .map_err(StatementJsonError::InvalidCreatedAt)?;

        let unsigned = UnsignedStatement::new(
            ActorId::new(self.actor.clone()).map_err(StatementJsonError::InvalidActor)?,
            self.subject
                .parse::<KairoRef>()
                .map_err(StatementJsonError::InvalidSubject)?,
            created_at,
            self.body.to_body()?,
        );

        let signatures = signatures_to_envelope(&self.signatures)?;
        MultiSignedStatement::new(unsigned, signatures)
            .map_err(StatementJsonError::InvalidMultiSignature)
    }

    pub fn from_statement(
        statement: &MultiSignedStatement<ActorAttestationThresholdChangeBody>,
    ) -> Self {
        let unsigned = statement.unsigned();
        Self {
            statement_type: "ActorAttestationThresholdChange".to_owned(),
            version: 1,
            actor: unsigned.actor().to_string(),
            subject: unsigned.subject().to_string(),
            created_at: unsigned.created_at().to_string(),
            body: ActorAttestationThresholdChangeBodyJson::from_body(unsigned.body()),
            signatures: statement
                .signatures()
                .iter()
                .map(SignatureJson::from_signature)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorAttestationThresholdChangeBodyJson {
    pub new_threshold: u8,
}

impl ActorAttestationThresholdChangeBodyJson {
    pub fn to_body(&self) -> Result<ActorAttestationThresholdChangeBody, StatementJsonError> {
        ActorAttestationThresholdChangeBody::new(self.new_threshold)
            .map_err(StatementJsonError::InvalidThresholdShape)
    }

    pub fn from_body(body: &ActorAttestationThresholdChangeBody) -> Self {
        Self {
            new_threshold: body.new_threshold(),
        }
    }
}

fn decode_nonce_hex(value: &str) -> Result<[u8; 32], StatementJsonError> {
    if value.len() != 64 {
        return Err(StatementJsonError::InvalidNonceHex);
    }

    let mut bytes = [0_u8; 32];
    for (index, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = hex_value(chunk[0]).ok_or(StatementJsonError::InvalidNonceHex)?;
        let low = hex_value(chunk[1]).ok_or(StatementJsonError::InvalidNonceHex)?;
        bytes[index] = (high << 4) | low;
    }

    Ok(bytes)
}

fn ensure_statement_shape(
    actual_type: &str,
    actual_version: u8,
    expected_type: &'static str,
    expected_version: u8,
) -> Result<(), StatementJsonError> {
    if actual_type != expected_type {
        return Err(StatementJsonError::UnexpectedType {
            expected: expected_type,
            actual: actual_type.to_owned(),
        });
    }

    if actual_version != expected_version {
        return Err(StatementJsonError::UnexpectedVersion {
            expected: expected_version,
            actual: actual_version,
        });
    }

    Ok(())
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use kairo_core::canonical::CanonicalEncode;

    use super::*;
    use crate::StatementBody;

    const ACTOR_ID: &str = "zQmTn1mdQDA1ryQZsiqYfRbqj5DGcG8TNvYcRmBrBLAuk5t";
    const OBJECT_ID: &str = "zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk";
    const BLOB_ID: &str = "zQmfBE2w2UqKJhGZAxK4ZWb4JuCrvxnN9P4YKuvzfbPSvD5";

    const CREATED_AT: &str = "2026-05-01T14:32:07Z";

    #[test]
    fn parses_object_revision_json_to_canonical_statement() {
        let json = format!(
            r#"{{
              "type": "ObjectRevision",
              "version": 1,
              "actor": "{ACTOR_ID}",
              "subject": "object:{OBJECT_ID}",
              "created_at": "{CREATED_AT}",
              "body": {{
                "object": "{OBJECT_ID}",
                "revision": "git:sha256:revision",
                "parents": ["git:sha256:parent"],
                "manifest_hash": "{BLOB_ID}",
                "attests_reachable_history": true
              }},
              "signature": {{
                "actor": "{ACTOR_ID}",
                "key_id": "primary",
                "algorithm": "example",
                "bytes": "c2lnbmF0dXJl"
              }}
            }}"#
        );

        let dto: Result<ObjectRevisionStatementJson, serde_json::Error> =
            serde_json::from_str(&json);
        let statement = dto.and_then(|dto| {
            dto.to_statement()
                .map_err(|error| serde_json::Error::io(std::io::Error::other(error)))
        });

        assert!(
            matches!(statement, Ok(statement) if statement.unsigned().body().revision().as_str() == "git:sha256:revision")
        );
    }

    #[test]
    fn json_key_order_does_not_affect_object_revision_statement_id() {
        let first = format!(
            r#"{{
              "type": "ObjectRevision",
              "version": 1,
              "actor": "{ACTOR_ID}",
              "subject": "object:{OBJECT_ID}",
              "created_at": "{CREATED_AT}",
              "body": {{
                "object": "{OBJECT_ID}",
                "revision": "git:sha256:revision",
                "parents": ["git:sha256:parent"],
                "manifest_hash": "{BLOB_ID}",
                "attests_reachable_history": true
              }},
              "signature": {{
                "actor": "{ACTOR_ID}",
                "key_id": "primary",
                "algorithm": "example",
                "bytes": "c2lnbmF0dXJlLW9uZQ=="
              }}
            }}"#
        );
        let second = format!(
            r#"{{
              "signature": {{
                "bytes": "c2lnbmF0dXJlLXR3bw==",
                "algorithm": "example",
                "key_id": "secondary",
                "actor": "{ACTOR_ID}"
              }},
              "body": {{
                "attests_reachable_history": true,
                "manifest_hash": "{BLOB_ID}",
                "parents": ["git:sha256:parent"],
                "revision": "git:sha256:revision",
                "object": "{OBJECT_ID}"
              }},
              "created_at": "{CREATED_AT}",
              "subject": "object:{OBJECT_ID}",
              "actor": "{ACTOR_ID}",
              "version": 1,
              "type": "ObjectRevision"
            }}"#
        );

        let first_id = parse_object_revision_json(&first).map(|statement| statement.statement_id());
        let second_id =
            parse_object_revision_json(&second).map(|statement| statement.statement_id());

        assert!(
            matches!((first_id, second_id), (Ok(first_id), Ok(second_id)) if first_id == second_id)
        );
    }

    #[test]
    fn parses_object_genesis_json_to_object_id_material() {
        let json = format!(
            r#"{{
              "type": "ObjectGenesis",
              "version": 1,
              "body": {{
                "object_kind": "software",
                "created_by": "{ACTOR_ID}",
                "created_at": "{CREATED_AT}",
                "nonce": "0707070707070707070707070707070707070707070707070707070707070707",
                "initial_revision": "git:sha256:revision"
              }},
              "signature": {{
                "actor": "{ACTOR_ID}",
                "key_id": "primary",
                "algorithm": "example",
                "bytes": "c2lnbmF0dXJl"
              }}
            }}"#
        );

        let statement = parse_object_genesis_json(&json);

        assert!(
            matches!(statement, Ok(statement) if statement.body().initial_revision().is_some())
        );
    }

    #[test]
    fn object_genesis_json_signature_does_not_affect_object_id() {
        let first = genesis_json("c2lnbmF0dXJlLW9uZQ==");
        let second = genesis_json("c2lnbmF0dXJlLXR3bw==");

        let first_id = parse_object_genesis_json(&first).map(|statement| statement.object_id());
        let second_id = parse_object_genesis_json(&second).map(|statement| statement.object_id());

        assert!(
            matches!((first_id, second_id), (Ok(first_id), Ok(second_id)) if first_id == second_id)
        );
    }

    #[test]
    fn rejects_invalid_nonce_hex() {
        let body = ObjectGenesisBodyJson {
            object_kind: "software".to_owned(),
            created_by: ACTOR_ID.to_owned(),
            created_at: CREATED_AT.to_owned(),
            nonce: "not-hex".to_owned(),
            initial_revision: None,
        };

        assert_eq!(body.to_body(), Err(StatementJsonError::InvalidNonceHex));
    }

    #[test]
    fn rejects_invalid_created_at_in_revision() {
        let mut dto = revision_dto();
        dto.created_at = "not a timestamp".to_owned();

        assert!(matches!(
            dto.to_statement(),
            Err(StatementJsonError::InvalidCreatedAt(_))
        ));
    }

    #[test]
    fn rejects_invalid_signature_base64() {
        let mut dto = revision_dto();
        dto.signature.bytes = "not base64!".to_owned();

        assert_eq!(
            dto.to_statement(),
            Err(StatementJsonError::InvalidSignatureBase64)
        );
    }

    #[test]
    fn rejects_unexpected_statement_type() {
        let mut dto = revision_dto();
        dto.statement_type = "Other".to_owned();

        assert_eq!(
            dto.to_statement(),
            Err(StatementJsonError::UnexpectedType {
                expected: "ObjectRevision",
                actual: "Other".to_owned()
            })
        );
    }

    #[test]
    fn rejects_unexpected_statement_version() {
        let mut dto = revision_dto();
        dto.version = 2;

        assert_eq!(
            dto.to_statement(),
            Err(StatementJsonError::UnexpectedVersion {
                expected: 1,
                actual: 2
            })
        );
    }

    #[test]
    fn object_revision_json_type_matches_statement_body_type() {
        let dto_type = ObjectRevisionBody::TYPE;

        assert_eq!(dto_type, "ObjectRevision");
    }

    #[test]
    fn parsed_object_revision_canonical_bytes_match_direct_statement() {
        let json = revision_json("c2lnbmF0dXJl");
        let parsed = parse_object_revision_json(&json).map(|statement| statement.signed_bytes());
        let direct = direct_revision_statement().map(|statement| statement.canonical_bytes());

        assert!(matches!((parsed, direct), (Ok(parsed), Ok(direct)) if parsed == direct));
    }

    fn parse_object_revision_json(
        json: &str,
    ) -> Result<SignedStatement<ObjectRevisionBody>, serde_json::Error> {
        let dto: ObjectRevisionStatementJson = serde_json::from_str(json)?;
        dto.to_statement()
            .map_err(|error| serde_json::Error::io(std::io::Error::other(error)))
    }

    fn parse_object_genesis_json(
        json: &str,
    ) -> Result<crate::ObjectGenesisStatement, serde_json::Error> {
        let dto: ObjectGenesisStatementJson = serde_json::from_str(json)?;
        dto.to_statement()
            .map_err(|error| serde_json::Error::io(std::io::Error::other(error)))
    }

    fn genesis_json(signature: &str) -> String {
        format!(
            r#"{{
              "type": "ObjectGenesis",
              "version": 1,
              "body": {{
                "object_kind": "software",
                "created_by": "{ACTOR_ID}",
                "created_at": "{CREATED_AT}",
                "nonce": "0707070707070707070707070707070707070707070707070707070707070707",
                "initial_revision": null
              }},
              "signature": {{
                "actor": "{ACTOR_ID}",
                "key_id": "primary",
                "algorithm": "example",
                "bytes": "{signature}"
              }}
            }}"#
        )
    }

    fn revision_json(signature: &str) -> String {
        format!(
            r#"{{
              "type": "ObjectRevision",
              "version": 1,
              "actor": "{ACTOR_ID}",
              "subject": "object:{OBJECT_ID}",
              "created_at": "{CREATED_AT}",
              "body": {{
                "object": "{OBJECT_ID}",
                "revision": "git:sha256:revision",
                "parents": ["git:sha256:parent"],
                "manifest_hash": "{BLOB_ID}",
                "attests_reachable_history": true
              }},
              "signature": {{
                "actor": "{ACTOR_ID}",
                "key_id": "primary",
                "algorithm": "example",
                "bytes": "{signature}"
              }}
            }}"#
        )
    }

    fn revision_dto() -> ObjectRevisionStatementJson {
        ObjectRevisionStatementJson {
            statement_type: "ObjectRevision".to_owned(),
            version: 1,
            actor: ACTOR_ID.to_owned(),
            subject: format!("object:{OBJECT_ID}"),
            created_at: CREATED_AT.to_owned(),
            body: ObjectRevisionBodyJson {
                object: OBJECT_ID.to_owned(),
                revision: "git:sha256:revision".to_owned(),
                parents: vec!["git:sha256:parent".to_owned()],
                manifest_hash: BLOB_ID.to_owned(),
                attests_reachable_history: true,
            },
            signature: SignatureJson {
                actor: ACTOR_ID.to_owned(),
                key_id: "primary".to_owned(),
                algorithm: "example".to_owned(),
                bytes: "c2lnbmF0dXJl".to_owned(),
            },
        }
    }

    fn direct_revision_statement(
    ) -> Result<UnsignedStatement<ObjectRevisionBody>, kairo_core::IdError> {
        let body = ObjectRevisionBody::new(
            ObjectId::new(OBJECT_ID)?,
            RevisionId::new("git:sha256:revision"),
            vec![RevisionId::new("git:sha256:parent")],
            BlobId::new(BLOB_ID)?,
            true,
        );

        let created_at = CREATED_AT
            .parse::<Timestamp>()
            .map_err(|_| kairo_core::IdError::InvalidEncoding)?;

        Ok(UnsignedStatement::new(
            ActorId::new(ACTOR_ID)?,
            format!("object:{OBJECT_ID}").parse()?,
            created_at,
            body,
        ))
    }

    fn revision_statement_id() -> StatementId {
        StatementId::from_sha256_digest([0x11; 32])
    }

    fn branch_json(name: &str, signature: &str) -> String {
        let revision = revision_statement_id();
        format!(
            r#"{{
              "type": "ObjectBranch",
              "version": 1,
              "actor": "{ACTOR_ID}",
              "subject": "object:{OBJECT_ID}",
              "created_at": "{CREATED_AT}",
              "body": {{
                "object": "{OBJECT_ID}",
                "name": "{name}",
                "revision": "{revision}"
              }},
              "signature": {{
                "actor": "{ACTOR_ID}",
                "key_id": "primary",
                "algorithm": "example",
                "bytes": "{signature}"
              }}
            }}"#
        )
    }

    fn parse_branch_json(
        json: &str,
    ) -> Result<SignedStatement<ObjectBranchBody>, serde_json::Error> {
        let dto: ObjectBranchStatementJson = serde_json::from_str(json)?;
        dto.to_statement()
            .map_err(|error| serde_json::Error::io(std::io::Error::other(error)))
    }

    #[test]
    fn parses_branch_json_to_canonical_statement() {
        let statement = parse_branch_json(&branch_json("head", "c2lnbmF0dXJl"));
        let expected_revision = revision_statement_id();

        assert!(matches!(
            statement,
            Ok(statement) if statement.unsigned().body().name() == "head"
                && statement.unsigned().body().revision() == &expected_revision
        ));
    }

    #[test]
    fn json_key_order_does_not_affect_branch_statement_id() {
        let first = branch_json("head", "c2lnbmF0dXJlLW9uZQ==");
        let revision = revision_statement_id();
        let second = format!(
            r#"{{
              "signature": {{
                "bytes": "c2lnbmF0dXJlLXR3bw==",
                "algorithm": "example",
                "key_id": "secondary",
                "actor": "{ACTOR_ID}"
              }},
              "body": {{
                "revision": "{revision}",
                "name": "head",
                "object": "{OBJECT_ID}"
              }},
              "created_at": "{CREATED_AT}",
              "subject": "object:{OBJECT_ID}",
              "actor": "{ACTOR_ID}",
              "version": 1,
              "type": "ObjectBranch"
            }}"#
        );

        let first_id = parse_branch_json(&first).map(|statement| statement.statement_id());
        let second_id = parse_branch_json(&second).map(|statement| statement.statement_id());

        assert!(
            matches!((first_id, second_id), (Ok(first_id), Ok(second_id)) if first_id == second_id)
        );
    }

    #[test]
    fn rejects_invalid_branch_revision_statement_id() {
        let body = ObjectBranchBodyJson {
            object: OBJECT_ID.to_owned(),
            name: "head".to_owned(),
            revision: "not-a-statement-id".to_owned(),
            supersedes: None,
        };

        assert!(matches!(
            body.to_body(),
            Err(StatementJsonError::InvalidStatement(_))
        ));
    }

    #[test]
    fn branch_round_trips_through_from_statement() -> Result<(), Box<dyn std::error::Error>> {
        let original = parse_branch_json(&branch_json("release", "c2lnbmF0dXJl"))?;
        let dto = ObjectBranchStatementJson::from_statement(&original);
        let round_tripped = dto.to_statement()?;

        assert_eq!(original.statement_id(), round_tripped.statement_id());
        Ok(())
    }

    fn version_tag_bind_json(version: &str, signature: &str) -> String {
        let target = revision_statement_id();
        format!(
            r#"{{
              "type": "ObjectVersionTag",
              "version": 1,
              "actor": "{ACTOR_ID}",
              "subject": "object:{OBJECT_ID}",
              "created_at": "{CREATED_AT}",
              "body": {{
                "object": "{OBJECT_ID}",
                "version": "{version}",
                "target": "{target}",
                "supersedes": null
              }},
              "signature": {{
                "actor": "{ACTOR_ID}",
                "key_id": "primary",
                "algorithm": "example",
                "bytes": "{signature}"
              }}
            }}"#
        )
    }

    fn version_tag_revoke_json(version: &str, signature: &str) -> String {
        let supersedes = revision_statement_id();
        format!(
            r#"{{
              "type": "ObjectVersionTag",
              "version": 1,
              "actor": "{ACTOR_ID}",
              "subject": "object:{OBJECT_ID}",
              "created_at": "{CREATED_AT}",
              "body": {{
                "object": "{OBJECT_ID}",
                "version": "{version}",
                "target": null,
                "supersedes": "{supersedes}"
              }},
              "signature": {{
                "actor": "{ACTOR_ID}",
                "key_id": "primary",
                "algorithm": "example",
                "bytes": "{signature}"
              }}
            }}"#
        )
    }

    fn parse_version_tag_json(
        json: &str,
    ) -> Result<SignedStatement<ObjectVersionTagBody>, serde_json::Error> {
        let dto: ObjectVersionTagStatementJson = serde_json::from_str(json)?;
        dto.to_statement()
            .map_err(|error| serde_json::Error::io(std::io::Error::other(error)))
    }

    #[test]
    fn parses_version_tag_bind_json_to_canonical_statement() {
        let statement = parse_version_tag_json(&version_tag_bind_json("1.2.3", "c2lnbmF0dXJl"));
        let expected_target = revision_statement_id();
        assert!(matches!(
            statement,
            Ok(statement) if statement.unsigned().body().version().as_str() == "1.2.3"
                && statement.unsigned().body().target() == Some(&expected_target)
                && statement.unsigned().body().supersedes().is_none()
        ));
    }

    #[test]
    fn parses_version_tag_revoke_json_to_canonical_statement() {
        let statement = parse_version_tag_json(&version_tag_revoke_json("1.2.3", "c2lnbmF0dXJl"));
        let expected_supersedes = revision_statement_id();
        assert!(matches!(
            statement,
            Ok(statement) if statement.unsigned().body().is_revocation()
                && statement.unsigned().body().supersedes() == Some(&expected_supersedes)
        ));
    }

    #[test]
    fn rejects_version_tag_with_invalid_semver() {
        let body = ObjectVersionTagBodyJson {
            object: OBJECT_ID.to_owned(),
            version: "not.semver".to_owned(),
            target: Some(revision_statement_id().to_string()),
            supersedes: None,
        };
        assert!(matches!(
            body.to_body(),
            Err(StatementJsonError::InvalidVersion(_))
        ));
    }

    #[test]
    fn rejects_version_tag_with_revoke_and_no_supersedes() {
        let body = ObjectVersionTagBodyJson {
            object: OBJECT_ID.to_owned(),
            version: "1.2.3".to_owned(),
            target: None,
            supersedes: None,
        };
        assert!(matches!(
            body.to_body(),
            Err(StatementJsonError::InvalidTagShape(
                ObjectVersionTagShapeError::RevokeWithoutSupersedes
            ))
        ));
    }

    #[test]
    fn rejects_version_tag_with_invalid_target_statement_id() {
        let body = ObjectVersionTagBodyJson {
            object: OBJECT_ID.to_owned(),
            version: "1.2.3".to_owned(),
            target: Some("not-a-statement-id".to_owned()),
            supersedes: None,
        };
        assert!(matches!(
            body.to_body(),
            Err(StatementJsonError::InvalidStatement(_))
        ));
    }

    #[test]
    fn json_key_order_does_not_affect_version_tag_statement_id() {
        let first = version_tag_bind_json("1.2.3", "c2lnbmF0dXJlLW9uZQ==");
        let target = revision_statement_id();
        let second = format!(
            r#"{{
              "signature": {{
                "bytes": "c2lnbmF0dXJlLXR3bw==",
                "algorithm": "example",
                "key_id": "secondary",
                "actor": "{ACTOR_ID}"
              }},
              "body": {{
                "supersedes": null,
                "target": "{target}",
                "version": "1.2.3",
                "object": "{OBJECT_ID}"
              }},
              "created_at": "{CREATED_AT}",
              "subject": "object:{OBJECT_ID}",
              "actor": "{ACTOR_ID}",
              "version": 1,
              "type": "ObjectVersionTag"
            }}"#
        );
        let first_id = parse_version_tag_json(&first).map(|s| s.statement_id());
        let second_id = parse_version_tag_json(&second).map(|s| s.statement_id());
        assert!(
            matches!((first_id, second_id), (Ok(first_id), Ok(second_id)) if first_id == second_id)
        );
    }

    #[test]
    fn version_tag_round_trips_through_from_statement() -> Result<(), Box<dyn std::error::Error>> {
        let bind = parse_version_tag_json(&version_tag_bind_json("1.2.3", "c2lnbmF0dXJl"))?;
        let dto = ObjectVersionTagStatementJson::from_statement(&bind);
        let round_tripped = dto.to_statement()?;
        assert_eq!(bind.statement_id(), round_tripped.statement_id());

        let revoke = parse_version_tag_json(&version_tag_revoke_json("1.2.3", "c2lnbmF0dXJl"))?;
        let dto = ObjectVersionTagStatementJson::from_statement(&revoke);
        let round_tripped = dto.to_statement()?;
        assert_eq!(revoke.statement_id(), round_tripped.statement_id());
        Ok(())
    }

    const TRUSTED_ACTOR_ID: &str = "zQmTbHEDi1jqyu1WKzmUaT9eJ48nWjMv55GrW88JArfCZUu";

    fn actor_trust_grant_json(signature: &str) -> String {
        format!(
            r#"{{
              "type": "ActorTrust",
              "version": 1,
              "actor": "{ACTOR_ID}",
              "subject": "actor:{TRUSTED_ACTOR_ID}",
              "created_at": "{CREATED_AT}",
              "body": {{
                "trusted_actor": "{TRUSTED_ACTOR_ID}",
                "decision": "trusted",
                "supersedes": null
              }},
              "signature": {{
                "actor": "{ACTOR_ID}",
                "key_id": "primary",
                "algorithm": "example",
                "bytes": "{signature}"
              }}
            }}"#
        )
    }

    fn actor_trust_withdraw_json(signature: &str) -> String {
        let prior = revision_statement_id();
        format!(
            r#"{{
              "type": "ActorTrust",
              "version": 1,
              "actor": "{ACTOR_ID}",
              "subject": "actor:{TRUSTED_ACTOR_ID}",
              "created_at": "{CREATED_AT}",
              "body": {{
                "trusted_actor": "{TRUSTED_ACTOR_ID}",
                "decision": null,
                "reason": "key was leaked",
                "supersedes": "{prior}"
              }},
              "signature": {{
                "actor": "{ACTOR_ID}",
                "key_id": "primary",
                "algorithm": "example",
                "bytes": "{signature}"
              }}
            }}"#
        )
    }

    fn parse_actor_trust_json(
        json: &str,
    ) -> Result<SignedStatement<ActorTrustBody>, serde_json::Error> {
        let dto: ActorTrustStatementJson = serde_json::from_str(json)?;
        dto.to_statement()
            .map_err(|error| serde_json::Error::io(std::io::Error::other(error)))
    }

    #[test]
    fn parses_actor_trust_grant_json() {
        let parsed = parse_actor_trust_json(&actor_trust_grant_json("c2lnbmF0dXJl"));
        assert!(matches!(
            parsed,
            Ok(signed)
                if signed.unsigned().body().decision() == Some(TrustDecision::Trusted)
                && signed.unsigned().body().supersedes().is_none()
        ));
    }

    #[test]
    fn parses_actor_trust_withdraw_json() {
        let parsed = parse_actor_trust_json(&actor_trust_withdraw_json("c2lnbmF0dXJl"));
        let expected_prior = revision_statement_id();
        assert!(matches!(
            parsed,
            Ok(signed)
                if signed.unsigned().body().is_withdrawal()
                && signed.unsigned().body().supersedes() == Some(&expected_prior)
                && signed.unsigned().body().reason() == Some("key was leaked")
        ));
    }

    #[test]
    fn rejects_actor_trust_with_unknown_decision_string() {
        let body = ActorTrustBodyJson {
            trusted_actor: TRUSTED_ACTOR_ID.to_owned(),
            decision: Some("maybe".to_owned()),
            reason: None,
            supersedes: None,
        };
        assert!(matches!(
            body.to_body(),
            Err(StatementJsonError::InvalidTrustDecision(_))
        ));
    }

    #[test]
    fn rejects_actor_trust_withdraw_without_supersedes() {
        let body = ActorTrustBodyJson {
            trusted_actor: TRUSTED_ACTOR_ID.to_owned(),
            decision: None,
            reason: None,
            supersedes: None,
        };
        assert!(matches!(
            body.to_body(),
            Err(StatementJsonError::InvalidTrustShape(
                ActorTrustShapeError::WithdrawWithoutSupersedes
            ))
        ));
    }

    #[test]
    fn json_key_order_does_not_affect_actor_trust_statement_id() {
        let first = actor_trust_grant_json("c2lnbmF0dXJlLW9uZQ==");
        let second = format!(
            r#"{{
              "signature": {{
                "bytes": "c2lnbmF0dXJlLXR3bw==",
                "algorithm": "example",
                "key_id": "secondary",
                "actor": "{ACTOR_ID}"
              }},
              "body": {{
                "supersedes": null,
                "decision": "trusted",
                "trusted_actor": "{TRUSTED_ACTOR_ID}"
              }},
              "created_at": "{CREATED_AT}",
              "subject": "actor:{TRUSTED_ACTOR_ID}",
              "actor": "{ACTOR_ID}",
              "version": 1,
              "type": "ActorTrust"
            }}"#
        );
        let first_id = parse_actor_trust_json(&first).map(|s| s.statement_id());
        let second_id = parse_actor_trust_json(&second).map(|s| s.statement_id());
        assert!(
            matches!((first_id, second_id), (Ok(first_id), Ok(second_id)) if first_id == second_id)
        );
    }

    #[test]
    fn actor_trust_round_trips_through_from_statement(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let grant = parse_actor_trust_json(&actor_trust_grant_json("c2lnbmF0dXJl"))?;
        let dto = ActorTrustStatementJson::from_statement(&grant);
        let round_tripped = dto.to_statement()?;
        assert_eq!(grant.statement_id(), round_tripped.statement_id());

        let withdraw = parse_actor_trust_json(&actor_trust_withdraw_json("c2lnbmF0dXJl"))?;
        let dto = ActorTrustStatementJson::from_statement(&withdraw);
        let round_tripped = dto.to_statement()?;
        assert_eq!(withdraw.statement_id(), round_tripped.statement_id());
        Ok(())
    }

    const GRANTEE_ACTOR_ID: &str = "zQmZsHt8fzNFmDDYE3RZ7mTpCrz9rYpzXmFFvPbV5Q3KcAa";

    fn capability_grant_object_scoped_json(signature: &str) -> String {
        format!(
            r#"{{
              "type": "ActorCapabilityGrant",
              "version": 1,
              "actor": "{ACTOR_ID}",
              "subject": "actor:{GRANTEE_ACTOR_ID}",
              "created_at": "{CREATED_AT}",
              "body": {{
                "grantee": "{GRANTEE_ACTOR_ID}",
                "capability": {{
                  "scope": {{ "object": "{OBJECT_ID}" }},
                  "statement_kinds": ["ObjectBranch", "ObjectVersionTag"],
                  "delegable": true,
                  "constraints": [
                    {{ "max_delegation_depth": 1 }}
                  ]
                }},
                "supersedes": null
              }},
              "signature": {{
                "actor": "{ACTOR_ID}",
                "key_id": "primary",
                "algorithm": "example",
                "bytes": "{signature}"
              }}
            }}"#
        )
    }

    fn capability_grant_successor_json(signature: &str) -> String {
        let prior = revision_statement_id();
        format!(
            r#"{{
              "type": "ActorCapabilityGrant",
              "version": 1,
              "actor": "{ACTOR_ID}",
              "subject": "actor:{GRANTEE_ACTOR_ID}",
              "created_at": "{CREATED_AT}",
              "body": {{
                "grantee": "{GRANTEE_ACTOR_ID}",
                "capability": {{
                  "scope": {{ "object": "{OBJECT_ID}" }},
                  "statement_kinds": ["ObjectVersionTag"],
                  "delegable": true,
                  "constraints": []
                }},
                "supersedes": "{prior}"
              }},
              "signature": {{
                "actor": "{ACTOR_ID}",
                "key_id": "primary",
                "algorithm": "example",
                "bytes": "{signature}"
              }}
            }}"#
        )
    }

    fn parse_capability_grant_json(
        json: &str,
    ) -> Result<SignedStatement<ActorCapabilityGrantBody>, serde_json::Error> {
        let dto: ActorCapabilityGrantStatementJson = serde_json::from_str(json)?;
        dto.to_statement()
            .map_err(|error| serde_json::Error::io(std::io::Error::other(error)))
    }

    #[test]
    fn parses_capability_grant_object_scoped_json() {
        let parsed = parse_capability_grant_json(&capability_grant_object_scoped_json("c2ln"));
        assert!(matches!(
            parsed,
            Ok(signed)
                if signed.unsigned().body().is_genesis()
                && signed.unsigned().body().capability().delegable()
                && signed.unsigned().body().capability().statement_kinds().len() == 2
        ));
    }

    #[test]
    fn parses_capability_grant_successor_json() {
        let parsed = parse_capability_grant_json(&capability_grant_successor_json("c2ln"));
        let expected_prior = revision_statement_id();
        assert!(matches!(
            parsed,
            Ok(signed)
                if !signed.unsigned().body().is_genesis()
                && signed.unsigned().body().supersedes() == Some(&expected_prior)
        ));
    }

    #[test]
    fn rejects_capability_grant_with_unknown_statement_kind() {
        let body = ActorCapabilityGrantBodyJson {
            grantee: GRANTEE_ACTOR_ID.to_owned(),
            capability: CapabilityJson {
                scope: CapabilityScopeJson::Object(OBJECT_ID.to_owned()),
                statement_kinds: vec!["WidgetCreated".to_owned()],
                delegable: false,
                constraints: vec![],
            },
            supersedes: None,
        };
        assert!(matches!(
            body.to_body(),
            Err(StatementJsonError::InvalidStatementKind(_))
        ));
    }

    #[test]
    fn rejects_capability_grant_with_empty_statement_kinds() {
        let body = ActorCapabilityGrantBodyJson {
            grantee: GRANTEE_ACTOR_ID.to_owned(),
            capability: CapabilityJson {
                scope: CapabilityScopeJson::Object(OBJECT_ID.to_owned()),
                statement_kinds: vec![],
                delegable: false,
                constraints: vec![],
            },
            supersedes: None,
        };
        assert!(matches!(
            body.to_body(),
            Err(StatementJsonError::InvalidCapabilityShape(
                CapabilityShapeError::EmptyStatementKinds
            ))
        ));
    }

    #[test]
    fn rejects_capability_grant_with_kind_invalid_for_actor_scope() {
        let body = ActorCapabilityGrantBodyJson {
            grantee: GRANTEE_ACTOR_ID.to_owned(),
            capability: CapabilityJson {
                scope: CapabilityScopeJson::Actor(ACTOR_ID.to_owned()),
                statement_kinds: vec!["ObjectVersionTag".to_owned()],
                delegable: false,
                constraints: vec![],
            },
            supersedes: None,
        };
        assert!(matches!(
            body.to_body(),
            Err(StatementJsonError::InvalidCapabilityShape(
                CapabilityShapeError::KindInvalidForScope { .. }
            ))
        ));
    }

    #[test]
    fn json_key_order_does_not_affect_capability_grant_statement_id() {
        let first = capability_grant_object_scoped_json("c2lnLW9uZQ==");
        let second = format!(
            r#"{{
              "signature": {{
                "bytes": "c2lnLXR3bw==",
                "algorithm": "example",
                "key_id": "secondary",
                "actor": "{ACTOR_ID}"
              }},
              "body": {{
                "supersedes": null,
                "capability": {{
                  "constraints": [
                    {{ "max_delegation_depth": 1 }}
                  ],
                  "delegable": true,
                  "statement_kinds": ["ObjectBranch", "ObjectVersionTag"],
                  "scope": {{ "object": "{OBJECT_ID}" }}
                }},
                "grantee": "{GRANTEE_ACTOR_ID}"
              }},
              "created_at": "{CREATED_AT}",
              "subject": "actor:{GRANTEE_ACTOR_ID}",
              "actor": "{ACTOR_ID}",
              "version": 1,
              "type": "ActorCapabilityGrant"
            }}"#
        );
        let first_id = parse_capability_grant_json(&first).map(|s| s.statement_id());
        let second_id = parse_capability_grant_json(&second).map(|s| s.statement_id());
        assert!(matches!(
            (first_id, second_id),
            (Ok(first_id), Ok(second_id)) if first_id == second_id
        ));
    }

    #[test]
    fn capability_grant_round_trips_through_from_statement(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let grant = parse_capability_grant_json(&capability_grant_object_scoped_json("c2ln"))?;
        let dto = ActorCapabilityGrantStatementJson::from_statement(&grant);
        let round_tripped = dto.to_statement()?;
        assert_eq!(grant.statement_id(), round_tripped.statement_id());

        let successor = parse_capability_grant_json(&capability_grant_successor_json("c2ln"))?;
        let dto = ActorCapabilityGrantStatementJson::from_statement(&successor);
        let round_tripped = dto.to_statement()?;
        assert_eq!(successor.statement_id(), round_tripped.statement_id());
        Ok(())
    }

    #[test]
    fn capability_grant_input_kind_order_does_not_change_statement_id(
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Demonstrates that the canonical encoder normalizes the kind list:
        // JSON inputs with kinds in different orders produce the same statement_id.
        let unordered = format!(
            r#"{{
              "type": "ActorCapabilityGrant",
              "version": 1,
              "actor": "{ACTOR_ID}",
              "subject": "actor:{GRANTEE_ACTOR_ID}",
              "created_at": "{CREATED_AT}",
              "body": {{
                "grantee": "{GRANTEE_ACTOR_ID}",
                "capability": {{
                  "scope": {{ "object": "{OBJECT_ID}" }},
                  "statement_kinds": ["ObjectVersionTag", "ObjectBranch"],
                  "delegable": true,
                  "constraints": [
                    {{ "max_delegation_depth": 1 }}
                  ]
                }},
                "supersedes": null
              }},
              "signature": {{
                "actor": "{ACTOR_ID}",
                "key_id": "primary",
                "algorithm": "example",
                "bytes": "c2ln"
              }}
            }}"#
        );
        let ordered_id = parse_capability_grant_json(&capability_grant_object_scoped_json("c2ln"))?
            .statement_id();
        let unordered_id = parse_capability_grant_json(&unordered)?.statement_id();
        assert_eq!(ordered_id, unordered_id);
        Ok(())
    }

    fn capability_revocation_default_json(signature: &str) -> String {
        let revoked = revision_statement_id();
        format!(
            r#"{{
              "type": "ActorCapabilityRevocation",
              "version": 1,
              "actor": "{ACTOR_ID}",
              "subject": "statement:{revoked}",
              "created_at": "{CREATED_AT}",
              "body": {{
                "revoked_grant": "{revoked}",
                "retroactive": false,
                "reason": "delegate stepped down"
              }},
              "signature": {{
                "actor": "{ACTOR_ID}",
                "key_id": "primary",
                "algorithm": "example",
                "bytes": "{signature}"
              }}
            }}"#
        )
    }

    fn capability_revocation_retroactive_json(signature: &str) -> String {
        let revoked = revision_statement_id();
        format!(
            r#"{{
              "type": "ActorCapabilityRevocation",
              "version": 1,
              "actor": "{ACTOR_ID}",
              "subject": "statement:{revoked}",
              "created_at": "{CREATED_AT}",
              "body": {{
                "revoked_grant": "{revoked}",
                "retroactive": true
              }},
              "signature": {{
                "actor": "{ACTOR_ID}",
                "key_id": "primary",
                "algorithm": "example",
                "bytes": "{signature}"
              }}
            }}"#
        )
    }

    fn parse_capability_revocation_json(
        json: &str,
    ) -> Result<SignedStatement<ActorCapabilityRevocationBody>, serde_json::Error> {
        let dto: ActorCapabilityRevocationStatementJson = serde_json::from_str(json)?;
        dto.to_statement()
            .map_err(|error| serde_json::Error::io(std::io::Error::other(error)))
    }

    #[test]
    fn parses_capability_revocation_default_json() {
        let parsed =
            parse_capability_revocation_json(&capability_revocation_default_json("c2ln"));
        assert!(matches!(
            parsed,
            Ok(signed)
                if !signed.unsigned().body().retroactive()
                && signed.unsigned().body().reason() == Some("delegate stepped down")
        ));
    }

    #[test]
    fn parses_capability_revocation_retroactive_json() {
        let parsed =
            parse_capability_revocation_json(&capability_revocation_retroactive_json("c2ln"));
        assert!(matches!(
            parsed,
            Ok(signed)
                if signed.unsigned().body().retroactive()
                && signed.unsigned().body().reason().is_none()
        ));
    }

    #[test]
    fn rejects_capability_revocation_with_invalid_grant_id() {
        let body = ActorCapabilityRevocationBodyJson {
            revoked_grant: "not-a-valid-statement-id".to_owned(),
            retroactive: false,
            reason: None,
        };
        assert!(matches!(
            body.to_body(),
            Err(StatementJsonError::InvalidStatement(_))
        ));
    }

    #[test]
    fn json_key_order_does_not_affect_capability_revocation_statement_id() {
        let first = capability_revocation_default_json("c2ln");
        let revoked = revision_statement_id();
        let second = format!(
            r#"{{
              "signature": {{
                "bytes": "c2ln",
                "algorithm": "example",
                "key_id": "primary",
                "actor": "{ACTOR_ID}"
              }},
              "body": {{
                "reason": "delegate stepped down",
                "retroactive": false,
                "revoked_grant": "{revoked}"
              }},
              "created_at": "{CREATED_AT}",
              "subject": "statement:{revoked}",
              "actor": "{ACTOR_ID}",
              "version": 1,
              "type": "ActorCapabilityRevocation"
            }}"#
        );
        let first_id = parse_capability_revocation_json(&first).map(|s| s.statement_id());
        let second_id = parse_capability_revocation_json(&second).map(|s| s.statement_id());
        assert!(matches!(
            (first_id, second_id),
            (Ok(first_id), Ok(second_id)) if first_id == second_id
        ));
    }

    #[test]
    fn capability_revocation_round_trips_through_from_statement(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let default =
            parse_capability_revocation_json(&capability_revocation_default_json("c2ln"))?;
        let dto = ActorCapabilityRevocationStatementJson::from_statement(&default);
        let round_tripped = dto.to_statement()?;
        assert_eq!(default.statement_id(), round_tripped.statement_id());

        let retro =
            parse_capability_revocation_json(&capability_revocation_retroactive_json("c2ln"))?;
        let dto = ActorCapabilityRevocationStatementJson::from_statement(&retro);
        let round_tripped = dto.to_statement()?;
        assert_eq!(retro.statement_id(), round_tripped.statement_id());
        Ok(())
    }

    // ---- ActorKeyRotation ----

    fn key_rotation_genesis_json(signature: &str) -> String {
        format!(
            r#"{{
              "type": "ActorKeyRotation",
              "version": 1,
              "actor": "{ACTOR_ID}",
              "subject": "actor:{ACTOR_ID}",
              "created_at": "{CREATED_AT}",
              "body": {{
                "next_key": {{
                  "algorithm": "ed25519",
                  "bytes": "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8="
                }}
              }},
              "signature": {{
                "actor": "{ACTOR_ID}",
                "key_id": "prior-active",
                "algorithm": "ed25519",
                "bytes": "{signature}"
              }}
            }}"#
        )
    }

    fn key_rotation_successor_json(signature: &str, supersedes: &str) -> String {
        format!(
            r#"{{
              "type": "ActorKeyRotation",
              "version": 1,
              "actor": "{ACTOR_ID}",
              "subject": "actor:{ACTOR_ID}",
              "created_at": "{CREATED_AT}",
              "body": {{
                "next_key": {{
                  "algorithm": "ed25519",
                  "bytes": "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8="
                }},
                "supersedes": "{supersedes}"
              }},
              "signature": {{
                "actor": "{ACTOR_ID}",
                "key_id": "prior-active",
                "algorithm": "ed25519",
                "bytes": "{signature}"
              }}
            }}"#
        )
    }

    fn parse_key_rotation_json(
        json: &str,
    ) -> Result<SignedStatement<ActorKeyRotationBody>, serde_json::Error> {
        let dto: ActorKeyRotationStatementJson = serde_json::from_str(json)?;
        dto.to_statement()
            .map_err(|error| serde_json::Error::io(std::io::Error::other(error)))
    }

    #[test]
    fn parses_key_rotation_genesis_json() {
        let parsed = parse_key_rotation_json(&key_rotation_genesis_json("c2ln"));
        assert!(matches!(
            parsed,
            Ok(signed) if signed.unsigned().body().is_genesis()
        ));
    }

    #[test]
    fn parses_key_rotation_successor_json() {
        let parsed = parse_key_rotation_json(&key_rotation_successor_json(
            "c2ln",
            revision_statement_id().as_str(),
        ));
        assert!(matches!(
            parsed,
            Ok(signed) if signed.unsigned().body().supersedes().is_some()
        ));
    }

    #[test]
    fn rejects_key_rotation_with_invalid_public_key() {
        let body = ActorKeyRotationBodyJson {
            next_key: PublicKeyJson {
                algorithm: "ed25519".to_owned(),
                bytes: "not-base64!".to_owned(),
            },
            supersedes: None,
        };
        assert!(matches!(
            body.to_body(),
            Err(StatementJsonError::InvalidPublicKey(_))
        ));
    }

    #[test]
    fn json_key_order_does_not_affect_key_rotation_statement_id() {
        let first = key_rotation_successor_json("c2ln", revision_statement_id().as_str());
        let revoked = revision_statement_id();
        let second = format!(
            r#"{{
              "signature": {{
                "bytes": "c2ln",
                "algorithm": "ed25519",
                "key_id": "prior-active",
                "actor": "{ACTOR_ID}"
              }},
              "body": {{
                "supersedes": "{revoked}",
                "next_key": {{
                  "bytes": "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=",
                  "algorithm": "ed25519"
                }}
              }},
              "created_at": "{CREATED_AT}",
              "subject": "actor:{ACTOR_ID}",
              "actor": "{ACTOR_ID}",
              "version": 1,
              "type": "ActorKeyRotation"
            }}"#
        );
        let first_id = parse_key_rotation_json(&first).map(|s| s.statement_id());
        let second_id = parse_key_rotation_json(&second).map(|s| s.statement_id());
        assert!(matches!(
            (first_id, second_id),
            (Ok(first_id), Ok(second_id)) if first_id == second_id
        ));
    }

    #[test]
    fn key_rotation_round_trips_through_from_statement(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let genesis = parse_key_rotation_json(&key_rotation_genesis_json("c2ln"))?;
        let dto = ActorKeyRotationStatementJson::from_statement(&genesis);
        assert_eq!(genesis.statement_id(), dto.to_statement()?.statement_id());

        let successor = parse_key_rotation_json(&key_rotation_successor_json(
            "c2ln",
            revision_statement_id().as_str(),
        ))?;
        let dto = ActorKeyRotationStatementJson::from_statement(&successor);
        assert_eq!(
            successor.statement_id(),
            dto.to_statement()?.statement_id()
        );
        Ok(())
    }

    // ---- ActorKeyRevocation ----

    fn key_revocation_default_json(signature: &str, revoked_key: &str) -> String {
        format!(
            r#"{{
              "type": "ActorKeyRevocation",
              "version": 1,
              "actor": "{ACTOR_ID}",
              "subject": "actor:{ACTOR_ID}",
              "created_at": "{CREATED_AT}",
              "body": {{
                "revoked_key": "{revoked_key}",
                "retroactive": false,
                "reason": "rotated out"
              }},
              "signature": {{
                "actor": "{ACTOR_ID}",
                "key_id": "current-active",
                "algorithm": "ed25519",
                "bytes": "{signature}"
              }}
            }}"#
        )
    }

    fn key_revocation_retroactive_json(signature: &str, revoked_key: &str) -> String {
        format!(
            r#"{{
              "type": "ActorKeyRevocation",
              "version": 1,
              "actor": "{ACTOR_ID}",
              "subject": "actor:{ACTOR_ID}",
              "created_at": "{CREATED_AT}",
              "body": {{
                "revoked_key": "{revoked_key}",
                "retroactive": true
              }},
              "signature": {{
                "actor": "{ACTOR_ID}",
                "key_id": "current-active",
                "algorithm": "ed25519",
                "bytes": "{signature}"
              }}
            }}"#
        )
    }

    fn parse_key_revocation_json(
        json: &str,
    ) -> Result<SignedStatement<ActorKeyRevocationBody>, serde_json::Error> {
        let dto: ActorKeyRevocationStatementJson = serde_json::from_str(json)?;
        dto.to_statement()
            .map_err(|error| serde_json::Error::io(std::io::Error::other(error)))
    }

    #[test]
    fn parses_key_revocation_default_json() {
        let parsed = parse_key_revocation_json(&key_revocation_default_json(
            "c2ln",
            revision_statement_id().as_str(),
        ));
        assert!(matches!(
            parsed,
            Ok(signed)
                if !signed.unsigned().body().retroactive()
                && signed.unsigned().body().reason() == Some("rotated out")
        ));
    }

    #[test]
    fn parses_key_revocation_retroactive_json() {
        let parsed = parse_key_revocation_json(&key_revocation_retroactive_json(
            "c2ln",
            revision_statement_id().as_str(),
        ));
        assert!(matches!(
            parsed,
            Ok(signed)
                if signed.unsigned().body().retroactive()
                && signed.unsigned().body().reason().is_none()
        ));
    }

    #[test]
    fn json_key_order_does_not_affect_key_revocation_statement_id() {
        let revoked = revision_statement_id();
        let first = key_revocation_default_json("c2ln", revoked.as_str());
        let second = format!(
            r#"{{
              "signature": {{
                "bytes": "c2ln",
                "algorithm": "ed25519",
                "key_id": "current-active",
                "actor": "{ACTOR_ID}"
              }},
              "body": {{
                "reason": "rotated out",
                "retroactive": false,
                "revoked_key": "{revoked}"
              }},
              "created_at": "{CREATED_AT}",
              "subject": "actor:{ACTOR_ID}",
              "actor": "{ACTOR_ID}",
              "version": 1,
              "type": "ActorKeyRevocation"
            }}"#
        );
        let first_id = parse_key_revocation_json(&first).map(|s| s.statement_id());
        let second_id = parse_key_revocation_json(&second).map(|s| s.statement_id());
        assert!(matches!(
            (first_id, second_id),
            (Ok(first_id), Ok(second_id)) if first_id == second_id
        ));
    }

    #[test]
    fn key_revocation_round_trips_through_from_statement(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let default = parse_key_revocation_json(&key_revocation_default_json(
            "c2ln",
            revision_statement_id().as_str(),
        ))?;
        let dto = ActorKeyRevocationStatementJson::from_statement(&default);
        assert_eq!(default.statement_id(), dto.to_statement()?.statement_id());

        let retro = parse_key_revocation_json(&key_revocation_retroactive_json(
            "c2ln",
            revision_statement_id().as_str(),
        ))?;
        let dto = ActorKeyRevocationStatementJson::from_statement(&retro);
        assert_eq!(retro.statement_id(), dto.to_statement()?.statement_id());
        Ok(())
    }
}
