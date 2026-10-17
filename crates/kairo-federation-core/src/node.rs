//! The node document served at `/.well-known/kairo-node`
//! (`specs/DECISIONS.md` §16 sub-decisions 1 and 9).
//!
//! A node is an actor, so the document names that actor and ships
//! its `ActorGenesis`; a client checks that the id derives from the
//! genesis it got. The document is unsigned in the MVP, so even a
//! valid one only *claims* the node actor: provenance records it as
//! claimed, and nothing authorizes on it.

use std::fmt;

use kairo_core::ActorId;
use kairo_identity::json::{ActorGenesisJson, ActorGenesisJsonError};
use kairo_identity::{ActorGenesisBody, ActorKind};
use serde::{Deserialize, Serialize};

use crate::schema::schema_tag;
use crate::PROTOCOL_VERSION;

schema_tag!(
    /// `"kairo.fed.node.v1"`.
    NodeDocumentSchema = "kairo.fed.node.v1"
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeDocument {
    pub schema: NodeDocumentSchema,
    #[serde(with = "crate::wire_id::one")]
    pub node_actor: ActorId,
    pub actor_genesis: ActorGenesisJson,
    /// Public base URL peers use, e.g. `https://archive.example.org`.
    /// Behind a reverse proxy this is the proxy's `https://` origin,
    /// not the process's loopback bind.
    pub base_url: String,
    pub protocol_versions: Vec<String>,
    pub supports: Vec<String>,
}

impl NodeDocument {
    /// Check what shape alone can't: the base URL is usable, the
    /// node speaks [`PROTOCOL_VERSION`], `node_actor` derives from
    /// `actor_genesis`, and that genesis declares kind `node`.
    /// Returns the parsed genesis on success.
    pub fn validate(&self) -> Result<ActorGenesisBody, NodeDocumentError> {
        validate_base_url(&self.base_url)?;
        if !self.protocol_versions.iter().any(|v| v == PROTOCOL_VERSION) {
            return Err(NodeDocumentError::UnsupportedProtocol {
                offered: self.protocol_versions.clone(),
            });
        }
        let genesis = self
            .actor_genesis
            .to_body()
            .map_err(NodeDocumentError::InvalidGenesis)?;
        let derived = genesis.actor_id();
        if derived != self.node_actor {
            return Err(NodeDocumentError::ActorMismatch {
                claimed: self.node_actor.clone(),
                derived,
            });
        }
        if genesis.actor_kind() != &ActorKind::node() {
            return Err(NodeDocumentError::NotANodeActor {
                kind: genesis.actor_kind().as_str().to_owned(),
            });
        }
        Ok(genesis)
    }

    /// Whether the node advertises `feature` in `supports`.
    pub fn supports(&self, feature: &str) -> bool {
        self.supports.iter().any(|value| value == feature)
    }
}

/// `http://` or `https://`, a non-empty host, no path, query,
/// fragment, trailing slash, or whitespace. Clients join endpoint
/// paths onto it directly.
fn validate_base_url(value: &str) -> Result<(), NodeDocumentError> {
    let rest = value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"));
    let valid = rest.is_some_and(|authority| {
        !authority.is_empty()
            && !authority
                .chars()
                .any(|c| c.is_whitespace() || matches!(c, '/' | '?' | '#' | '@'))
    });
    if valid {
        Ok(())
    } else {
        Err(NodeDocumentError::InvalidBaseUrl {
            value: value.to_owned(),
        })
    }
}

#[derive(Debug)]
pub enum NodeDocumentError {
    InvalidBaseUrl { value: String },
    UnsupportedProtocol { offered: Vec<String> },
    InvalidGenesis(ActorGenesisJsonError),
    ActorMismatch { claimed: ActorId, derived: ActorId },
    NotANodeActor { kind: String },
}

impl fmt::Display for NodeDocumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBaseUrl { value } => write!(
                f,
                "base_url {value:?} is not an http(s) origin with no path or trailing slash"
            ),
            Self::UnsupportedProtocol { offered } => write!(
                f,
                "node does not speak {PROTOCOL_VERSION} (offers {})",
                offered.join(", ")
            ),
            Self::InvalidGenesis(error) => write!(f, "invalid actor_genesis: {error}"),
            Self::ActorMismatch { claimed, derived } => write!(
                f,
                "node_actor {claimed} does not derive from actor_genesis (derives {derived})"
            ),
            Self::NotANodeActor { kind } => {
                write!(f, "node actor has actor_kind {kind:?}, expected \"node\"")
            }
        }
    }
}

impl std::error::Error for NodeDocumentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidGenesis(error) => Some(error),
            _ => None,
        }
    }
}
