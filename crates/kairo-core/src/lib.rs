//! Core Kairo primitives shared across crates.

pub mod canonical;
pub mod error;
pub mod ids;
pub mod refs;
pub mod timestamp;

pub use error::IdError;
pub use ids::{ActorId, BlobId, ObjectId, SnapshotId, StatementId};
pub use refs::KairoRef;
pub use timestamp::{Timestamp, TimestampError};
