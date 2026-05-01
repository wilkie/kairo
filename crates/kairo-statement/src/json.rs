use std::error::Error;
use std::fmt;

use kairo_core::{ActorId, BlobId, KairoRef, ObjectId};
use serde::{Deserialize, Serialize};

use crate::{
    ObjectGenesisBody, ObjectKind, ObjectRevisionBody, RevisionId, Signature, SignedStatement,
    UnsignedStatement,
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
    InvalidNonceHex,
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
            Self::InvalidNonceHex => f.write_str("invalid ObjectGenesis nonce hex"),
        }
    }
}

impl Error for StatementJsonError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidActor(error)
            | Self::InvalidObject(error)
            | Self::InvalidSubject(error)
            | Self::InvalidBlob(error) => Some(error),
            Self::UnexpectedType { .. }
            | Self::UnexpectedVersion { .. }
            | Self::InvalidNonceHex => None,
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
            self.bytes.as_bytes().to_vec(),
        ))
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectGenesisBodyJson {
    pub object_kind: String,
    pub created_by: String,
    pub nonce: String,
    pub initial_revision: Option<String>,
}

impl ObjectGenesisBodyJson {
    pub fn to_body(&self) -> Result<ObjectGenesisBody, StatementJsonError> {
        Ok(ObjectGenesisBody::new(
            ObjectKind::new(self.object_kind.clone()),
            ActorId::new(self.created_by.clone()).map_err(StatementJsonError::InvalidActor)?,
            decode_nonce_hex(&self.nonce)?,
            self.initial_revision.clone().map(RevisionId::new),
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectRevisionStatementJson {
    #[serde(rename = "type")]
    pub statement_type: String,
    pub version: u8,
    pub actor: String,
    pub subject: String,
    pub body: ObjectRevisionBodyJson,
    pub signature: SignatureJson,
}

impl ObjectRevisionStatementJson {
    pub fn to_statement(&self) -> Result<SignedStatement<ObjectRevisionBody>, StatementJsonError> {
        ensure_statement_shape(&self.statement_type, self.version, "ObjectRevision", 1)?;

        let unsigned = UnsignedStatement::new(
            ActorId::new(self.actor.clone()).map_err(StatementJsonError::InvalidActor)?,
            self.subject
                .parse::<KairoRef>()
                .map_err(StatementJsonError::InvalidSubject)?,
            self.body.to_body()?,
        );

        Ok(SignedStatement::new(
            unsigned,
            self.signature.to_signature()?,
        ))
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

    #[test]
    fn parses_object_revision_json_to_canonical_statement() {
        let json = format!(
            r#"{{
              "type": "ObjectRevision",
              "version": 1,
              "actor": "{ACTOR_ID}",
              "subject": "object:{OBJECT_ID}",
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
                "bytes": "signature"
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
                "bytes": "signature-one"
              }}
            }}"#
        );
        let second = format!(
            r#"{{
              "signature": {{
                "bytes": "signature-two",
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
                "nonce": "0707070707070707070707070707070707070707070707070707070707070707",
                "initial_revision": "git:sha256:revision"
              }},
              "signature": {{
                "actor": "{ACTOR_ID}",
                "key_id": "primary",
                "algorithm": "example",
                "bytes": "signature"
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
        let first = genesis_json("signature-one");
        let second = genesis_json("signature-two");

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
            nonce: "not-hex".to_owned(),
            initial_revision: None,
        };

        assert_eq!(body.to_body(), Err(StatementJsonError::InvalidNonceHex));
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
        let json = revision_json("signature");
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
                bytes: "signature".to_owned(),
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

        Ok(UnsignedStatement::new(
            ActorId::new(ACTOR_ID)?,
            format!("object:{OBJECT_ID}").parse()?,
            body,
        ))
    }
}
