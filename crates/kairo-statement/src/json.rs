use std::error::Error;
use std::fmt;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use kairo_core::{ActorId, BlobId, KairoRef, ObjectId, StatementId, Timestamp, TimestampError};
use serde::{Deserialize, Serialize};

use crate::{
    ObjectBranchBody, ObjectGenesisBody, ObjectKind, ObjectRevisionBody, ObjectVersionTagBody,
    ObjectVersionTagShapeError, RevisionId, SemverParseError, SemverVersion, Signature,
    SignedStatement, UnsignedStatement,
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
    fn to_signature(&self) -> Result<Signature, StatementJsonError> {
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
}

impl ObjectBranchBodyJson {
    pub fn to_body(&self) -> Result<ObjectBranchBody, StatementJsonError> {
        Ok(ObjectBranchBody::new(
            ObjectId::new(self.object.clone()).map_err(StatementJsonError::InvalidObject)?,
            self.name.clone(),
            StatementId::new(self.revision.clone())
                .map_err(StatementJsonError::InvalidStatement)?,
        ))
    }

    pub fn from_body(body: &ObjectBranchBody) -> Self {
        Self {
            object: body.object().to_string(),
            name: body.name().to_owned(),
            revision: body.revision().to_string(),
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
}
