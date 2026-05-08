//! `kairo-daemon-client` — Rust client for the Kairo daemon's
//! Unix-socket HTTP+JSON API.
//!
//! The crate is the **only** sanctioned entry point to the daemon
//! from Rust callers (`kairo-cli` in v1; a future Rust web-server
//! impl post-v1). See `specs/DECISIONS.md` §11 for the trust
//! model and `specs/PHASE_2_DAEMON.md` for the slice plan.
//!
//! Slice 3 ships:
//!
//! - HTTP-over-Unix-socket transport in [`Client`].
//! - [`Client::probe`] — the liveness check used by the CLI's
//!   probe-and-fall-back dispatch.
//! - [`Client::version`] / [`Client::status`].
//! - DTOs in [`dto`] (single source of truth — the daemon
//!   imports them from here).
//! - [`ClientError`] for the typed failure modes.
//!
//! Per-endpoint methods for the rest of the v1 surface (actors,
//! objects, statements, branches, version-tags, trust,
//! capabilities, blobs) land in slices 5–7.

// dev-deps are visible to the lib's unit-test target; shim the
// ones used only by integration tests under `tests/`.
#[cfg(test)]
use kairo_core as _;
#[cfg(test)]
use kairo_daemon as _;
#[cfg(test)]
use kairo_store as _;
#[cfg(test)]
use kairo_test_support as _;
#[cfg(test)]
use tempfile as _;

mod blob;
mod client;
pub mod dto;
mod envelope;
mod error;

pub use blob::BlobReader;
pub use client::{Client, DEFAULT_PROBE_TIMEOUT};
pub use error::{ClientError, ClientResult};
