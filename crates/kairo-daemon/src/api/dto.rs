//! Re-export of the wire DTOs.
//!
//! The single source of truth lives in
//! [`kairo_daemon_client::dto`]. The daemon serializes these
//! types in handler responses; the client deserializes them.
//! Adding a new endpoint shape goes in the client crate and is
//! used here.

pub use kairo_daemon_client::dto::{StatusInfo, VersionInfo};
