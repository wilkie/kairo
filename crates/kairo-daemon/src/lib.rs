//! Kairo daemon — long-running local service that fronts the store
//! over an HTTP+JSON API on a Unix domain socket.
//!
//! Phase 2 §2 ships the read-only sliver: see `specs/DAEMON.md`,
//! `specs/API.md`, and `specs/PHASE_2_DAEMON.md` for the surface.
//! This is slice 1 — crate scaffolding only. The HTTP server,
//! socket bind, lifecycle, and handlers land in subsequent slices.
//!
//! The daemon is the only async crate in the workspace alongside
//! [`kairo-daemon-client`]. Blocking [`kairo_store::FilesystemStore`]
//! calls are wrapped in `tokio::task::spawn_blocking` once handlers
//! arrive.

use std::path::PathBuf;

/// Resolved daemon configuration assembled by the binary entry
/// point and consumed by [`serve`]. Stays minimal in slice 1 —
/// later slices add socket path overrides, log filters, etc.
#[derive(Debug, Clone)]
pub struct Config {
    /// Filesystem path of the Kairo store the daemon serves.
    pub store_path: PathBuf,
}

/// Run the daemon to completion.
///
/// Slice 1: returns immediately. Slice 2 wires the actual axum
/// server, socket bind, PID file, and graceful shutdown.
pub async fn serve(config: Config) -> Result<(), Error> {
    tracing::info!(
        store = %config.store_path.display(),
        "kairo-daemon serve() invoked (slice 1 stub: returning immediately)"
    );
    tokio::task::yield_now().await;
    Ok(())
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

/// Errors produced by the daemon's top-level entry points.
///
/// Slice 1 has no failing paths yet; variants land alongside the
/// code that produces them so each is exercised by a real test
/// when introduced.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {}

impl std::fmt::Display for Error {
    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {}
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn serve_stub_returns_ok() {
        let config = Config {
            store_path: PathBuf::from("/nonexistent-slice1-stub"),
        };
        serve(config).await.expect("slice 1 stub serve");
    }
}
