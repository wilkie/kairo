//! Serde adapters that carry `kairo-core` id newtypes as bare-id
//! strings.
//!
//! `kairo-core` stays serde-free, so the wire types opt in field by
//! field: `#[serde(with = "crate::wire_id::one")]` for a single id,
//! `crate::wire_id::many` for a list. Deserializing runs the id's
//! own validating constructor, so a malformed id fails the parse
//! rather than surviving as a string.

use std::fmt::Display;
use std::str::FromStr;

use kairo_core::{ActorId, BlobId, ObjectId, StatementId};

/// An id newtype the wire carries as its bare payload.
pub(crate) trait WireId: FromStr<Err: Display> {
    fn as_wire_str(&self) -> &str;
}

macro_rules! wire_id {
    ($($name:ty),* $(,)?) => {
        $(impl WireId for $name {
            fn as_wire_str(&self) -> &str {
                self.as_str()
            }
        })*
    };
}

wire_id!(ActorId, BlobId, ObjectId, StatementId);

pub(crate) mod one {
    use serde::{Deserialize, Deserializer, Serializer};

    use super::WireId;

    pub(crate) fn serialize<S: Serializer, T: WireId>(id: &T, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(id.as_wire_str())
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>, T: WireId>(d: D) -> Result<T, D::Error> {
        let raw = String::deserialize(d)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

pub(crate) mod many {
    use serde::ser::SerializeSeq;
    use serde::{Deserialize, Deserializer, Serializer};

    use super::WireId;

    pub(crate) fn serialize<S: Serializer, T: WireId>(ids: &[T], s: S) -> Result<S::Ok, S::Error> {
        let mut seq = s.serialize_seq(Some(ids.len()))?;
        for id in ids {
            seq.serialize_element(id.as_wire_str())?;
        }
        seq.end()
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>, T: WireId>(
        d: D,
    ) -> Result<Vec<T>, D::Error> {
        let raw = Vec::<String>::deserialize(d)?;
        raw.iter()
            .map(|value| value.parse().map_err(serde::de::Error::custom))
            .collect()
    }
}
