use std::fmt;
use std::str::FromStr;

use crate::{ActorId, BlobId, IdError, ObjectId, SnapshotId, StatementId};

/// A standalone Kairo reference.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum KairoRef {
    Object(ObjectId),
    ObjectSnapshot {
        object: ObjectId,
        snapshot: SnapshotId,
    },
    Statement(StatementId),
    Blob(BlobId),
    Actor(ActorId),
}

impl KairoRef {
    pub fn to_external_string(&self) -> String {
        format!("kairo:{self}")
    }
}

impl fmt::Display for KairoRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Object(object) => write!(f, "object:{object}"),
            Self::ObjectSnapshot { object, snapshot } => {
                write!(f, "object:{object}:snapshot:{snapshot}")
            }
            Self::Statement(statement) => write!(f, "statement:{statement}"),
            Self::Blob(blob) => write!(f, "blob:{blob}"),
            Self::Actor(actor) => write!(f, "actor:{actor}"),
        }
    }
}

impl FromStr for KairoRef {
    type Err = IdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.strip_prefix("kairo:").unwrap_or(value);
        let parts: Vec<&str> = value.split(':').collect();

        match parts.as_slice() {
            ["object", object] => Ok(Self::Object(ObjectId::new(*object)?)),
            ["object", object, "snapshot", snapshot] => Ok(Self::ObjectSnapshot {
                object: ObjectId::new(*object)?,
                snapshot: SnapshotId::new(*snapshot)?,
            }),
            ["statement", statement] => Ok(Self::Statement(StatementId::new(*statement)?)),
            ["blob", blob] => Ok(Self::Blob(BlobId::new(*blob)?)),
            ["actor", actor] => Ok(Self::Actor(ActorId::new(*actor)?)),
            [] | [""] => Err(IdError::Empty),
            [kind, ..] if !matches!(*kind, "object" | "statement" | "blob" | "actor") => {
                Err(IdError::UnsupportedReferenceKind)
            }
            _ => Err(IdError::InvalidReference),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_internal_object_ref() {
        assert_eq!(
            "object:zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk"
                .parse::<KairoRef>()
                .map(|reference| reference.to_string()),
            Ok("object:zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk".to_owned())
        );
    }

    #[test]
    fn parses_internal_snapshot_ref() {
        assert_eq!(
            "object:zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk:snapshot:zQmPrz1SBNXD1XAPCaWrTtpBbEZHGevtUoWY8ibihamaApZ"
                .parse::<KairoRef>()
                .map(|reference| reference.to_string()),
            Ok("object:zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk:snapshot:zQmPrz1SBNXD1XAPCaWrTtpBbEZHGevtUoWY8ibihamaApZ".to_owned())
        );
    }

    #[test]
    fn parses_external_ref_and_formats_internal() {
        assert_eq!(
            "kairo:actor:zQmTn1mdQDA1ryQZsiqYfRbqj5DGcG8TNvYcRmBrBLAuk5t"
                .parse::<KairoRef>()
                .map(|reference| (reference.to_string(), reference.to_external_string())),
            Ok((
                "actor:zQmTn1mdQDA1ryQZsiqYfRbqj5DGcG8TNvYcRmBrBLAuk5t".to_owned(),
                "kairo:actor:zQmTn1mdQDA1ryQZsiqYfRbqj5DGcG8TNvYcRmBrBLAuk5t".to_owned()
            ))
        );
    }

    #[test]
    fn rejects_unsupported_reference_spellings() {
        assert_eq!(
            "obj_zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk".parse::<KairoRef>(),
            Err(IdError::UnsupportedReferenceKind)
        );
        assert_eq!(
            "kai:object:zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk".parse::<KairoRef>(),
            Err(IdError::UnsupportedReferenceKind)
        );
    }

    #[test]
    fn rejects_malformed_snapshot_ref() {
        assert_eq!(
            "object:zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk:snapshot".parse::<KairoRef>(),
            Err(IdError::InvalidReference)
        );
    }
}
