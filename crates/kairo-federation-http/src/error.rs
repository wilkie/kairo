//! Startup and serve errors. Per-request failures are HTTP responses
//! (`handlers`), not values of this type.

use std::fmt;
use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;

use kairo_core::ActorId;
use kairo_federation_core::NodeDocumentError;
use kairo_store::StoreError;

#[derive(Debug)]
pub enum Error {
    OpenStore {
        path: PathBuf,
        source: StoreError,
    },
    /// `--node-actor` isn't in the store.
    NodeActorMissing {
        actor: ActorId,
    },
    ReadNodeActor {
        actor: ActorId,
        source: StoreError,
    },
    /// The node actor's genesis declares a kind other than `node`.
    NotANodeActor {
        actor: ActorId,
        kind: String,
    },
    /// The assembled node document doesn't validate (bad base URL).
    NodeDocument(NodeDocumentError),
    Bind {
        addr: SocketAddr,
        source: io::Error,
    },
    Serve(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OpenStore { path, source } => {
                write!(f, "failed to open store at {}: {source}", path.display())
            }
            Self::NodeActorMissing { actor } => {
                write!(f, "node actor {actor} is not in the store")
            }
            Self::ReadNodeActor { actor, source } => {
                write!(f, "failed to read node actor {actor}: {source}")
            }
            Self::NotANodeActor { actor, kind } => write!(
                f,
                "actor {actor} has actor_kind {kind:?}; a node must serve as an actor created with \
                 `kairo actor create --kind node`"
            ),
            Self::NodeDocument(error) => write!(f, "invalid node document: {error}"),
            Self::Bind { addr, source } => write!(f, "failed to bind {addr}: {source}"),
            Self::Serve(error) => write!(f, "server error: {error}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::OpenStore { source, .. } | Self::ReadNodeActor { source, .. } => Some(source),
            Self::NodeDocument(error) => Some(error),
            Self::Bind { source, .. } => Some(source),
            Self::Serve(error) => Some(error),
            Self::NodeActorMissing { .. } | Self::NotANodeActor { .. } => None,
        }
    }
}
