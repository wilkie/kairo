use std::error::Error;
use std::fmt;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::{ActorGenesisBody, ActorKind, PublicKey};

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
        }
    }
}

impl Error for ActorGenesisJsonError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorGenesisJson {
    #[serde(rename = "type")]
    pub statement_type: String,
    pub version: u8,
    pub actor_kind: String,
    pub initial_key: PublicKeyJson,
    pub nonce: String,
}

impl ActorGenesisJson {
    pub fn to_body(&self) -> Result<ActorGenesisBody, ActorGenesisJsonError> {
        ensure_shape(&self.statement_type, self.version)?;

        Ok(ActorGenesisBody::new(
            ActorKind::new(self.actor_kind.clone()),
            self.initial_key.to_public_key()?,
            decode_nonce_hex(&self.nonce)?,
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
            nonce: "0909090909090909090909090909090909090909090909090909090909090909".to_owned(),
        }
    }
}
