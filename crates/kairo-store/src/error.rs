use std::fmt;
use std::io;

/// Errors returned by the store layer.
///
/// The three variants encode genuinely distinct failure modes so callers can
/// react differently:
///
/// - [`StoreError::Missing`] is **semantic**: the record is not present.
///   Callers may treat this as "not found" without paging an operator.
/// - [`StoreError::Corrupt`] is **fixity failure**: the record is on disk but
///   does not match its claimed identity. This is never silently repaired;
///   the caller must surface it.
/// - [`StoreError::Unavailable`] is **operational**: an underlying I/O error
///   happened. Callers may retry or report differently from `Missing`.
#[derive(Debug)]
pub enum StoreError {
    Missing,
    Corrupt { id: String, reason: CorruptReason },
    Unavailable(io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorruptReason {
    HashMismatch { expected: String, actual: String },
    Parse(String),
    SchemaMismatch,
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => f.write_str("record not found in store"),
            Self::Corrupt { id, reason } => write!(f, "corrupt record {id}: {reason}"),
            Self::Unavailable(error) => write!(f, "store unavailable: {error}"),
        }
    }
}

impl fmt::Display for CorruptReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HashMismatch { expected, actual } => {
                write!(f, "hash mismatch (expected {expected}, got {actual})")
            }
            Self::Parse(message) => write!(f, "parse error: {message}"),
            Self::SchemaMismatch => f.write_str("schema mismatch"),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unavailable(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for StoreError {
    fn from(error: io::Error) -> Self {
        Self::Unavailable(error)
    }
}
