//! Core Kairo primitives shared across crates.

pub mod error;
pub mod ids;
pub mod refs;

pub use error::IdError;
pub use ids::{ActorId, BlobId, ObjectId, SnapshotId, StatementId};
pub use refs::KairoRef;
