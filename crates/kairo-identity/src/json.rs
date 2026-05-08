use std::error::Error;
use std::fmt;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use kairo_core::{Timestamp, TimestampError};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{ActorGenesisBody, ActorGenesisShapeError, ActorKind, PublicKey};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActorGenesisJsonError {
    UnexpectedType {
        expected: &'static str,
        actual: String,
    },
    UnexpectedVersion {
        expected: u8,
        actual: u8,
    },
    UnsupportedPublicKeyAlgorithm(String),
    InvalidPublicKeyBase64,
    InvalidPublicKeyLength {
        expected: usize,
        actual: usize,
    },
    InvalidNonceHex,
    InvalidCreatedAt(TimestampError),
    InvalidShape(ActorGenesisShapeError),
}

impl fmt::Display for ActorGenesisJsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedType { expected, actual } => {
                write!(
                    f,
                    "unexpected actor genesis type {actual}; expected {expected}"
                )
            }
            Self::UnexpectedVersion { expected, actual } => {
                write!(
                    f,
                    "unexpected actor genesis version {actual}; expected {expected}"
                )
            }
            Self::UnsupportedPublicKeyAlgorithm(algorithm) => {
                write!(f, "unsupported public key algorithm {algorithm}")
            }
            Self::InvalidPublicKeyBase64 => f.write_str("invalid public key base64"),
            Self::InvalidPublicKeyLength { expected, actual } => {
                write!(f, "invalid public key length {actual}; expected {expected}")
            }
            Self::InvalidNonceHex => f.write_str("invalid ActorGenesis nonce hex"),
            Self::InvalidCreatedAt(error) => write!(f, "invalid created_at: {error}"),
            Self::InvalidShape(error) => write!(f, "{error}"),
        }
    }
}

impl Error for ActorGenesisJsonError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidCreatedAt(error) => Some(error),
            Self::InvalidShape(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ActorGenesisJson {
    #[serde(rename = "type")]
    pub statement_type: String,
    pub version: u8,
    pub actor_kind: String,
    pub initial_key: PublicKeyJson,
    pub attestation_keys: Vec<PublicKeyJson>,
    pub attestation_threshold: u8,
    pub created_at: String,
    pub nonce: String,
}

impl ActorGenesisJson {
    pub fn to_body(&self) -> Result<ActorGenesisBody, ActorGenesisJsonError> {
        ensure_shape(&self.statement_type, self.version)?;

        let created_at: Timestamp = self
            .created_at
            .parse()
            .map_err(ActorGenesisJsonError::InvalidCreatedAt)?;

        let mut attestation_keys = Vec::with_capacity(self.attestation_keys.len());
        for key in &self.attestation_keys {
            attestation_keys.push(key.to_public_key()?);
        }

        ActorGenesisBody::new(
            ActorKind::new(self.actor_kind.clone()),
            self.initial_key.to_public_key()?,
            attestation_keys,
            self.attestation_threshold,
            created_at,
            decode_nonce_hex(&self.nonce)?,
        )
        .map_err(ActorGenesisJsonError::InvalidShape)
    }

    pub fn from_body(body: &ActorGenesisBody) -> Self {
        Self {
            statement_type: "ActorGenesis".to_owned(),
            version: 1,
            actor_kind: body.actor_kind().as_str().to_owned(),
            initial_key: PublicKeyJson::from_public_key(body.initial_key()),
            attestation_keys: body
                .attestation_keys()
                .iter()
                .map(PublicKeyJson::from_public_key)
                .collect(),
            attestation_threshold: body.attestation_threshold(),
            created_at: body.created_at().to_string(),
            nonce: encode_nonce_hex(body.nonce()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct PublicKeyJson {
    pub algorithm: String,
    pub bytes: String,
}

impl PublicKeyJson {
    pub fn to_public_key(&self) -> Result<PublicKey, ActorGenesisJsonError> {
        match self.algorithm.as_str() {
            "ed25519" => {
                let bytes = STANDARD
                    .decode(&self.bytes)
                    .map_err(|_| ActorGenesisJsonError::InvalidPublicKeyBase64)?;
                let bytes = <[u8; 32]>::try_from(bytes.as_slice()).map_err(|_| {
                    ActorGenesisJsonError::InvalidPublicKeyLength {
                        expected: 32,
                        actual: bytes.len(),
                    }
                })?;
                Ok(PublicKey::ed25519(bytes))
            }
            algorithm => Err(ActorGenesisJsonError::UnsupportedPublicKeyAlgorithm(
                algorithm.to_owned(),
            )),
        }
    }

    pub fn from_public_key(public_key: &PublicKey) -> Self {
        Self {
            algorithm: public_key.algorithm().as_str().to_owned(),
            bytes: STANDARD.encode(public_key.bytes()),
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

fn ensure_shape(actual_type: &str, actual_version: u8) -> Result<(), ActorGenesisJsonError> {
    if actual_type != "ActorGenesis" {
        return Err(ActorGenesisJsonError::UnexpectedType {
            expected: "ActorGenesis",
            actual: actual_type.to_owned(),
        });
    }

    if actual_version != 1 {
        return Err(ActorGenesisJsonError::UnexpectedVersion {
            expected: 1,
            actual: actual_version,
        });
    }

    Ok(())
}

fn decode_nonce_hex(value: &str) -> Result<[u8; 32], ActorGenesisJsonError> {
    if value.len() != 64 {
        return Err(ActorGenesisJsonError::InvalidNonceHex);
    }

    let mut bytes = [0_u8; 32];
    for (index, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = hex_value(chunk[0]).ok_or(ActorGenesisJsonError::InvalidNonceHex)?;
        let low = hex_value(chunk[1]).ok_or(ActorGenesisJsonError::InvalidNonceHex)?;
        bytes[index] = (high << 4) | low;
    }

    Ok(bytes)
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
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use ed25519_dalek::SigningKey;

    use super::*;

    #[test]
    fn parses_actor_genesis_json() {
        let json = actor_genesis_json();
        let dto: Result<ActorGenesisJson, serde_json::Error> = serde_json::from_str(&json);
        let body = dto.and_then(|dto| {
            dto.to_body()
                .map_err(|error| serde_json::Error::io(std::io::Error::other(error)))
        });

        assert!(matches!(body, Ok(body) if body.actor_kind().as_str() == "person"));
    }

    #[test]
    fn rejects_invalid_public_key_base64() {
        let mut dto = actor_genesis_dto();
        dto.initial_key.bytes = "not base64!".to_owned();

        assert_eq!(
            dto.to_body(),
            Err(ActorGenesisJsonError::InvalidPublicKeyBase64)
        );
    }

    #[test]
    fn rejects_invalid_nonce_hex() {
        let mut dto = actor_genesis_dto();
        dto.nonce = "not-hex".to_owned();

        assert_eq!(dto.to_body(), Err(ActorGenesisJsonError::InvalidNonceHex));
    }

    fn actor_genesis_json() -> String {
        serde_json::to_string(&actor_genesis_dto()).unwrap_or_else(|_| "{}".to_owned())
    }

    fn actor_genesis_dto() -> ActorGenesisJson {
        ActorGenesisJson {
            statement_type: "ActorGenesis".to_owned(),
            version: 1,
            actor_kind: "person".to_owned(),
            initial_key: PublicKeyJson {
                algorithm: "ed25519".to_owned(),
                bytes: STANDARD.encode(SigningKey::from_bytes(&[7; 32]).verifying_key().to_bytes()),
            },
            attestation_keys: vec![PublicKeyJson {
                algorithm: "ed25519".to_owned(),
                bytes: STANDARD
                    .encode(SigningKey::from_bytes(&[200; 32]).verifying_key().to_bytes()),
            }],
            attestation_threshold: 1,
            created_at: "2026-05-01T14:32:07Z".to_owned(),
            nonce: "0909090909090909090909090909090909090909090909090909090909090909".to_owned(),
        }
    }

    #[test]
    fn rejects_invalid_created_at() {
        let mut dto = actor_genesis_dto();
        dto.created_at = "not a timestamp".to_owned();

        assert!(matches!(
            dto.to_body(),
            Err(ActorGenesisJsonError::InvalidCreatedAt(_))
        ));
    }

    #[test]
    fn rejects_non_utc_offset_in_created_at() {
        let mut dto = actor_genesis_dto();
        dto.created_at = "2026-05-01T14:32:07+00:00".to_owned();

        assert!(matches!(
            dto.to_body(),
            Err(ActorGenesisJsonError::InvalidCreatedAt(_))
        ));
    }
}
