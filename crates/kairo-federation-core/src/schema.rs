//! Exact-match `schema` tags.
//!
//! Each top-level document carries a `schema` string naming its
//! shape and version. Modeling the tag as a zero-sized type that
//! serializes to one constant and refuses every other string means
//! a document of the wrong kind or version fails to parse, instead
//! of parsing and relying on a later check.

macro_rules! schema_tag {
    ($(#[$meta:meta])* $name:ident = $value:literal) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
        pub struct $name;

        impl $name {
            pub const VALUE: &'static str = $value;
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(Self::VALUE)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let raw = <String as serde::Deserialize>::deserialize(d)?;
                if raw == Self::VALUE {
                    Ok(Self)
                } else {
                    Err(serde::de::Error::custom(format!(
                        "expected schema {:?}, found {raw:?}",
                        Self::VALUE
                    )))
                }
            }
        }
    };
}

pub(crate) use schema_tag;
