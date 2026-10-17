//! `kairo-federation-http` — HTTP transport for Kairo federation.
//!
//! Slice 2 of `specs/PHASE_2_FEDERATION.md` ships the serving side:
//! a separate, read-only process that serves the closures of this
//! node's published objects (`specs/DECISIONS.md` §16 sub-decisions
//! 3–6). It opens the store directly and never proxies the daemon
//! API, whose reads assume socket permissions and would expose the
//! whole store.
//!
//! - [`Node::open`] checks the configuration (node actor, base URL)
//!   and builds the node document.
//! - [`Node::router`] builds the axum router, exposed for tests that
//!   drive it without a socket.
//! - [`serve`] / [`serve_with_shutdown`] bind and run it.
//!
//! The process serves plain HTTP and binds loopback by default; TLS
//! is a reverse proxy's job. The client side (pull) lands in slice 3.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use kairo_core::ActorId;

// dev-deps used only by integration tests under `tests/`.
#[cfg(test)]
use http_body_util as _;
#[cfg(test)]
use kairo_test_support as _;
#[cfg(test)]
use tempfile as _;
#[cfg(test)]
use tower as _;

mod error;
mod handlers;
mod node;
mod server;
mod view;

pub use error::Error;
pub use node::Node;
pub use server::{serve, serve_with_shutdown};

/// Default port: one above `kairo-web`'s 7878 family, easy to tell
/// apart in `ss -ltn`.
pub const DEFAULT_PORT: u16 = 7880;

/// Default bind: loopback, because TLS terminates at a reverse proxy
/// in front of this process (§16 sub-decision 6).
pub const DEFAULT_BIND: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), DEFAULT_PORT);

/// How long a computed view of the published closures is reused
/// before recomputing. Bounds both the cost of a pull's many
/// per-record requests and how long an `unpublish` takes to stop
/// serving.
pub const DEFAULT_VIEW_TTL: Duration = Duration::from_secs(5);

/// Resolved server configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// Store root to serve from. Opened read-only in practice: the
    /// server calls no `put_*`.
    pub store: PathBuf,
    pub bind_addr: SocketAddr,
    /// Public base URL peers reach this node at, e.g.
    /// `https://archive.example.org`. Goes into the node document.
    pub base_url: String,
    /// The node's own actor. Must be in the store with
    /// `actor_kind = "node"`.
    pub node_actor: ActorId,
    /// See [`DEFAULT_VIEW_TTL`]. Zero recomputes on every request.
    pub view_ttl: Duration,
}

/// Install the structured-text tracing subscriber on stderr, matching
/// `kairo-daemon` and `kairo-web`.
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
