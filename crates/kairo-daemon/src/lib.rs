//! Kairo daemon — long-running local service that fronts the store
//! over an HTTP+JSON API on a Unix domain socket.
//!
//! Phase 2 §2 ships the read-only sliver: see `specs/DAEMON.md`,
//! `specs/API.md`, and `specs/PHASE_2_DAEMON.md` for the surface.
//!
//! The crate is the only async crate in the workspace alongside
//! [`kairo-daemon-client`]. Blocking [`kairo_store::FilesystemStore`]
//! calls run on `tokio::task::spawn_blocking` (per slice 5+).
//!
//! Public entry points:
//!
//! - [`serve`] runs the daemon until `SIGTERM` / `SIGINT` (production).
//! - [`serve_with_shutdown`] runs the daemon until a caller-supplied
//!   future resolves (tests).
//! - [`install_tracing`] installs the structured-text subscriber on
//!   stderr; the binary calls it once on startup.
//! - [`api::router`] builds the axum router, exposed for handler
//!   tests that don't need a real socket.

use std::path::PathBuf;

// dev-deps are visible to the lib's unit-test target; shim the
// ones used only by integration tests under `tests/`.
#[cfg(test)]
use http_body_util as _;
#[cfg(test)]
use kairo_test_support as _;
#[cfg(test)]
use tempfile as _;

pub mod api;
mod error;
mod server;

pub use error::Error;
pub use server::{serve, serve_with_shutdown};

/// Resolved daemon configuration assembled by the binary entry
/// point and consumed by [`serve`].
#[derive(Debug, Clone)]
pub struct Config {
    /// Filesystem path of the Kairo store the daemon serves.
    /// Both the listening socket (`<store>/daemon.sock`) and the
    /// PID file (`<store>/daemon.pid`) live under this directory.
    pub store_path: PathBuf,
}

/// Install the daemon's tracing subscriber on stderr with the
/// structured-text formatter.
///
/// `RUST_LOG` is honored; the default level is `info`. Safe to
/// call from tests (uses `try_init`, so a competing subscriber
/// installed by the test harness is left alone).
pub fn install_tracing() {
    use tracing_subscriber::fmt;
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_writer(std::io::stderr)
        .try_init();
}
