//! Core Kairo primitives shared across crates.

/// Crate version string, drawn from `Cargo.toml`. Surfaced in
/// daemon `/api/v1/version` responses and similar diagnostics.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod canonical;
pub mod error;
pub mod ids;
pub mod refs;
pub mod timestamp;

pub use error::IdError;
pub use ids::{ActorId, BlobId, ObjectId, SnapshotId, StatementId};
pub use refs::KairoRef;
pub use timestamp::{Timestamp, TimestampError};
