//! `kairo-web` — browser-facing TCP front-end that translates
//! browser HTTP requests into daemon-client calls over the Unix
//! socket.
//!
//! Phase 2 §5 ships a deliberately narrow slice of `WEB_CLIENT.md`:
//! a loopback-only static-asset server with a transparent
//! `/api/v1/*` reverse proxy to the daemon. See `DECISIONS.md`
//! §12 for the full scope decision; `PHASE_2_WEB_CLIENT.md` for
//! the slice plan.
//!
//! Public entry points:
//!
//! - [`serve`] runs the server until `SIGTERM` / `SIGINT`.
//! - [`serve_with_shutdown`] runs until a caller-supplied future
//!   resolves (tests).
//! - [`install_tracing`] installs the structured-text subscriber
//!   on stderr; the binary calls it once on startup.
//! - [`router`] builds the axum router, exposed for handler
//!   tests that don't need a real socket.

use std::net::SocketAddr;
use std::path::PathBuf;

// dev-deps used by integration tests under `tests/` but not by
// the lib itself.
#[cfg(test)]
use http_body_util as _;
#[cfg(test)]
use kairo_core as _;
#[cfg(test)]
use kairo_daemon as _;
#[cfg(test)]
use kairo_daemon_client as _;
#[cfg(test)]
use kairo_store as _;
#[cfg(test)]
use kairo_test_support as _;
#[cfg(test)]
use serde_json as _;
#[cfg(test)]
use tempfile as _;

mod error;
mod proxy;
mod server;

pub use error::Error;
pub use server::{router, serve, serve_with_shutdown};

/// Default loopback port. Picked to match the daemon's port-style
/// constants (4-digit, easy to remember). v1 has no per-host
/// service-discovery story; an operator who wants a different
/// port passes `--bind 127.0.0.1:<port>`.
pub const DEFAULT_PORT: u16 = 7878;

/// Resolved web-server configuration assembled by the binary
/// entry point and consumed by [`serve`].
#[derive(Debug, Clone)]
pub struct Config {
    /// TCP address to listen on. Must be a loopback address;
    /// non-loopback values are rejected with [`Error::NonLoopbackBind`].
    pub bind_addr: SocketAddr,
    /// Filesystem path of the built SPA bundle. Served at `/` via
    /// `tower_http::services::ServeDir` with an `index.html`
    /// fallback for client-side routes.
    pub spa_dir: PathBuf,
    /// Filesystem path of the daemon's listening Unix socket.
    /// Each `/api/v1/*` request opens a fresh `connect(2)` to
    /// this path.
    pub daemon_socket: PathBuf,
    /// Optional PID file. When `Some`, the server writes
    /// `<pid>\n` atomically at startup and removes the file on
    /// shutdown. Used by `kairo web stop` to locate the running
    /// process. Mirrors `kairo-daemon`'s `<store>/daemon.pid`
    /// pattern.
    pub pid_file: Option<PathBuf>,
}

/// Install the web-server's tracing subscriber on stderr with the
/// structured-text formatter. Mirrors `kairo_daemon::install_tracing`
/// so logs from the two processes look the same.
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
