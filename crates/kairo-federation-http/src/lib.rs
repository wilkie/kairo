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
//! is a reverse proxy's job.
//!
//! Slice 3 adds the pulling side, [`client::pull_object`] (feature
//! `client`): a blocking HTTPS client that fetches a published
//! object's closure from a peer, verifies every record, stores it,
//! and records provenance. The server items above are behind the
//! `server` feature; both are on by default.

// dev-deps used only by integration tests under `tests/`.
#[cfg(test)]
use http_body_util as _;
#[cfg(test)]
use kairo_test_support as _;
#[cfg(test)]
use tempfile as _;
#[cfg(test)]
use tower as _;

mod actor_export;
#[cfg(feature = "server")]
mod auth;
#[cfg(feature = "client")]
pub mod client;
#[cfg(feature = "server")]
mod error;
#[cfg(feature = "server")]
mod git_protocol;
#[cfg(feature = "server")]
mod handlers;
#[cfg(feature = "server")]
mod node;
#[cfg(feature = "server")]
mod server;
#[cfg(feature = "server")]
mod upload;
#[cfg(feature = "server")]
mod view;

#[cfg(feature = "server")]
pub use auth::{AuthenticatedNode, NodeAuthError, PendingAuth};
#[cfg(feature = "server")]
pub use error::Error;
#[cfg(feature = "server")]
pub use node::Node;
#[cfg(feature = "server")]
pub use server::{
    install_tracing, serve, serve_on, serve_with_shutdown, Config, DEFAULT_BIND, DEFAULT_PORT,
    DEFAULT_VIEW_TTL,
};

/// The managed Git cache root for a store: `<store>/git`, the same
/// layout the `kairo` CLI uses (`StorePaths::git_root`).
pub fn git_root(store: &kairo_store::FilesystemStore) -> std::path::PathBuf {
    store.root().join("git")
}
