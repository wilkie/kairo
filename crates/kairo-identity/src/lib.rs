//! Actor identity and signature primitives.

pub use kairo_core::ActorId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyId(String);

impl KeyId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}
