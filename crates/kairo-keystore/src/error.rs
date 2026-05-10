use std::fmt;
use std::io;

/// Errors returned by the keystore layer.
///
/// Mirrors `kairo_store::StoreError` so callers can react uniformly.
#[derive(Debug)]
pub enum KeystoreError {
    Missing,
    Corrupt {
        id: String,
        reason: CorruptReason,
    },
    Unavailable(io::Error),
    /// Another writer holds the per-actor advisory lock and didn't
    /// release it within the bounded retry window. Mirrors
    /// `StoreError::LockTimeout`.
    LockTimeout {
        path: std::path::PathBuf,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorruptReason {
    /// On-disk JSON did not parse.
    Parse(String),
    /// `schema` field is absent or unrecognized.
    SchemaMismatch,
    /// Stored algorithm string is not supported.
    UnsupportedAlgorithm(String),
    /// Secret bytes are malformed (wrong length, bad base64).
    InvalidSecretKey,
    /// File's `actor_id` field disagrees with the actor we asked for.
    ActorIdMismatch { expected: String, actual: String },
    /// File's `key_id` field disagrees with the KeyId we re-derived from the
    /// secret material.
    KeyIdMismatch { expected: String, actual: String },
    /// `put_signing_key` was called for an actor whose key file already
    /// exists. Kept under `Corrupt` so callers receive structured detail.
    AlreadyExists,
}

impl fmt::Display for KeystoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => f.write_str("signing key not found"),
            Self::Corrupt { id, reason } => write!(f, "corrupt key file {id}: {reason}"),
            Self::Unavailable(error) => write!(f, "keystore unavailable: {error}"),
            Self::LockTimeout { path } => {
                write!(f, "timed out acquiring advisory lock on {}", path.display())
            }
        }
    }
}

impl fmt::Display for CorruptReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(message) => write!(f, "parse error: {message}"),
            Self::SchemaMismatch => f.write_str("unrecognized schema"),
            Self::UnsupportedAlgorithm(algorithm) => {
                write!(f, "unsupported algorithm {algorithm}")
            }
            Self::InvalidSecretKey => f.write_str("invalid secret key bytes"),
            Self::ActorIdMismatch { expected, actual } => write!(
                f,
                "actor_id field {actual} does not match requested actor {expected}"
            ),
            Self::KeyIdMismatch { expected, actual } => write!(
                f,
                "key_id field {expected} does not match key derived from secret {actual}"
            ),
            Self::AlreadyExists => f.write_str("a key already exists for this actor"),
        }
    }
}

impl std::error::Error for KeystoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unavailable(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for KeystoreError {
    fn from(error: io::Error) -> Self {
        Self::Unavailable(error)
    }
}
