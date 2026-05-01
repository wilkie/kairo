use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use kairo_core::ActorId;
use kairo_identity::{KeyId, SecretSigningKey, SignatureAlgorithm};
use serde::{Deserialize, Serialize};

use crate::error::{CorruptReason, KeystoreError};

const SCHEMA: &str = "kairo.key.private.v1";
const ED25519: &str = "ed25519";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivateKeyJson {
    pub schema: String,
    pub algorithm: String,
    pub actor_id: String,
    pub key_id: String,
    pub secret_key: String,
}

impl PrivateKeyJson {
    pub fn from_secret(actor_id: &ActorId, secret: &SecretSigningKey) -> Self {
        let algorithm = match secret.algorithm() {
            SignatureAlgorithm::Ed25519 => ED25519,
        };
        Self {
            schema: SCHEMA.to_owned(),
            algorithm: algorithm.to_owned(),
            actor_id: actor_id.to_string(),
            key_id: secret.public_key().key_id().to_string(),
            secret_key: STANDARD.encode(secret.seed_bytes()),
        }
    }

    /// Validate cross-references and recover the [`SecretSigningKey`].
    ///
    /// The fixity contract:
    ///
    /// 1. `schema` must be `kairo.key.private.v1`.
    /// 2. `algorithm` must be a supported algorithm (currently only ed25519).
    /// 3. The file's `actor_id` field must match the requested actor.
    /// 4. The recomputed `KeyId` from the secret bytes must match the file's
    ///    `key_id` field.
    pub fn to_secret(&self, requested: &ActorId) -> Result<SecretSigningKey, KeystoreError> {
        if self.schema != SCHEMA {
            return Err(KeystoreError::Corrupt {
                id: requested.to_string(),
                reason: CorruptReason::SchemaMismatch,
            });
        }

        if self.actor_id != requested.as_str() {
            return Err(KeystoreError::Corrupt {
                id: requested.to_string(),
                reason: CorruptReason::ActorIdMismatch {
                    expected: requested.to_string(),
                    actual: self.actor_id.clone(),
                },
            });
        }

        let secret = match self.algorithm.as_str() {
            ED25519 => {
                let bytes =
                    STANDARD
                        .decode(&self.secret_key)
                        .map_err(|_| KeystoreError::Corrupt {
                            id: requested.to_string(),
                            reason: CorruptReason::InvalidSecretKey,
                        })?;
                let bytes =
                    <[u8; 32]>::try_from(bytes.as_slice()).map_err(|_| KeystoreError::Corrupt {
                        id: requested.to_string(),
                        reason: CorruptReason::InvalidSecretKey,
                    })?;
                SecretSigningKey::ed25519(bytes)
            }
            other => {
                return Err(KeystoreError::Corrupt {
                    id: requested.to_string(),
                    reason: CorruptReason::UnsupportedAlgorithm(other.to_owned()),
                });
            }
        };

        let derived: KeyId = secret.public_key().key_id();
        if derived.to_string() != self.key_id {
            return Err(KeystoreError::Corrupt {
                id: requested.to_string(),
                reason: CorruptReason::KeyIdMismatch {
                    expected: self.key_id.clone(),
                    actual: derived.to_string(),
                },
            });
        }

        Ok(secret)
    }
}
