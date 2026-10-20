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

/// The source objects `object`'s revisions pin, with the commits each
/// is pinned at (`DECISIONS.md` §37 sub-decisions 6, 9), and the file
/// sources they pin, with no commits (§38): read from each listed
/// `ObjectRevision`'s `kairo.toml` in the object's Git cache repo,
/// top-level and target dependencies alike. A revision whose
/// tree isn't held, or whose manifest doesn't parse, pins nothing here;
/// `verify object` reports it.
pub fn manifest_source_pins<'a>(
    store: &kairo_store::FilesystemStore,
    object: &kairo_core::ObjectId,
    statements: impl IntoIterator<Item = &'a kairo_core::StatementId>,
) -> std::collections::BTreeMap<
    kairo_core::ObjectId,
    std::collections::BTreeSet<kairo_statement::GitRevision>,
> {
    use kairo_object::DependencyDeclaration;
    use kairo_store::StatementStore;
    let mut pins: std::collections::BTreeMap<_, std::collections::BTreeSet<_>> = Default::default();
    let Ok(path) = kairo_git::object_repo_path(&git_root(store), object.as_str()) else {
        return pins;
    };
    let Ok(repo) = kairo_git::open(&path) else {
        return pins;
    };
    for id in statements {
        let Ok(revision) = store.get_object_revision(id) else {
            continue;
        };
        let Ok(Some(commit)) = revision.unsigned().body().revision().git() else {
            continue;
        };
        let Ok(Some(bytes)) = repo.read_blob_at_path(&commit, "kairo.toml") else {
            continue;
        };
        let Ok(manifest) =
            kairo_object::ObjectManifest::parse_toml(&String::from_utf8_lossy(&bytes))
        else {
            continue;
        };
        let targets = manifest
            .build_targets()
            .iter()
            .flat_map(|target| target.dependencies());
        for dependency in manifest.dependencies().iter().chain(targets) {
            match dependency {
                DependencyDeclaration::Source(source) => {
                    pins.entry(source.source())
                        .or_default()
                        .insert(source.revision().clone());
                }
                DependencyDeclaration::File(file) => {
                    pins.entry(file.source()).or_default();
                }
                _ => {}
            }
        }
    }
    pins
}
