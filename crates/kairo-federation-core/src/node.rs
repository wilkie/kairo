//! The node document served at `/.well-known/kairo-node`
//! (`specs/DECISIONS.md` §16 sub-decision 1, §25 sub-decision 1).
//!
//! A node is an actor, so the document names that actor and ships
//! its `ActorGenesis`; a client checks that the id derives from the
//! genesis it got. The node signs the document with the node actor's
//! active key, binding "this `base_url` serves this node actor". The
//! signature covers the bytes in
//! `schemas/canonical/node-document-v1.md`.

use std::fmt;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use kairo_core::canonical::{encode_list, encode_str, CanonicalEncode};
use kairo_core::{ActorId, Timestamp};
use kairo_identity::json::{ActorGenesisJson, ActorGenesisJsonError};
use kairo_identity::{
    verify_signature, ActorGenesisBody, ActorKind, ActorResolveError, ActorResolver,
    SecretSigningKey, SignatureBytes,
};
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
    /// When the node signed the document, which it does at startup.
    pub signed_at: String,
    pub signature: NodeSignature,
}

/// The node actor's signature over the document. The signing actor
/// is `node_actor`, so it isn't repeated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeSignature {
    pub key_id: String,
    pub algorithm: String,
    /// Standard base64 of the signature bytes.
    pub bytes: String,
}

/// Domain separator that starts the signed bytes, so a document
/// signature can't stand in for any other signature.
pub const NODE_DOCUMENT_DOMAIN: &str = "kairo.fed.node-document.v1";

/// A node document's fields before signing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsignedNodeDocument {
    pub node_actor: ActorId,
    pub actor_genesis: ActorGenesisJson,
    pub base_url: String,
    pub protocol_versions: Vec<String>,
    pub supports: Vec<String>,
}

impl UnsignedNodeDocument {
    /// Sign with `key` as of `signed_at`. The key should be the node
    /// actor's active key then, or peers refuse the document.
    pub fn sign(self, key: &SecretSigningKey, signed_at: Timestamp) -> NodeDocument {
        let mut document = NodeDocument {
            schema: NodeDocumentSchema,
            node_actor: self.node_actor,
            actor_genesis: self.actor_genesis,
            base_url: self.base_url,
            protocol_versions: self.protocol_versions,
            supports: self.supports,
            signed_at: String::new(),
            signature: NodeSignature {
                key_id: String::new(),
                algorithm: String::new(),
                bytes: String::new(),
            },
        };
        document.sign(key, signed_at);
        document
    }
}

impl NodeDocument {
    /// (Re)sign the document's current fields with `key` as of
    /// `signed_at`, replacing `signed_at` and `signature`.
    pub fn sign(&mut self, key: &SecretSigningKey, signed_at: Timestamp) {
        self.signed_at = signed_at.to_string();
        self.signature = sign_node_bytes(key, &self.signing_bytes(signed_at));
    }

    /// The bytes the signature covers (`node-document-v1.md`): every
    /// field but `actor_genesis` (which `node_actor` derives from) and
    /// `signature`.
    fn signing_bytes(&self, signed_at: Timestamp) -> Vec<u8> {
        let mut out = Vec::new();
        encode_str(&mut out, NODE_DOCUMENT_DOMAIN);
        encode_str(&mut out, NodeDocumentSchema::VALUE);
        encode_str(&mut out, self.node_actor.as_str());
        encode_str(&mut out, &self.base_url);
        encode_list(&mut out, &self.protocol_versions, |out, value| {
            encode_str(out, value);
        });
        encode_list(&mut out, &self.supports, |out, value| {
            encode_str(out, value)
        });
        signed_at.encode_canonical(&mut out);
        out
    }

    /// Check the signature against the node actor's key state in
    /// `resolver`: it must be by the key active at `signed_at`, not
    /// revoked then. Call after [`NodeDocument::validate`], with the
    /// node actor's genesis and key events already in `resolver`.
    /// Returns `signed_at`.
    pub fn verify_signature<R: ActorResolver>(
        &self,
        resolver: &R,
    ) -> Result<Timestamp, NodeDocumentError> {
        let signed_at = parse_signed_at(&self.signed_at)?;
        verify_node_bytes(
            resolver,
            &self.node_actor,
            signed_at,
            &self.signature,
            &self.signing_bytes(signed_at),
        )?;
        Ok(signed_at)
    }

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

pub(crate) fn parse_signed_at(value: &str) -> Result<Timestamp, NodeDocumentError> {
    value
        .parse()
        .map_err(|_| NodeDocumentError::InvalidSignedAt {
            value: value.to_owned(),
        })
}

/// A node actor's signature over `bytes` (node documents and
/// inventories, `DECISIONS.md` §25 and §29).
pub(crate) fn sign_node_bytes(key: &SecretSigningKey, bytes: &[u8]) -> NodeSignature {
    let public = key.public_key();
    NodeSignature {
        key_id: public.key_id().to_string(),
        algorithm: public.algorithm().as_str().to_owned(),
        bytes: STANDARD.encode(key.sign(bytes).bytes()),
    }
}

/// `signature` over `bytes` is by `actor`'s key active at `signed_at`,
/// not revoked then.
pub(crate) fn verify_node_bytes<R: ActorResolver>(
    resolver: &R,
    actor: &ActorId,
    signed_at: Timestamp,
    signature: &NodeSignature,
    bytes: &[u8],
) -> Result<(), NodeDocumentError> {
    let active = resolver
        .active_key_at(actor, signed_at)
        .map_err(NodeDocumentError::Resolve)?
        .ok_or(NodeDocumentError::NoActiveKey)?;
    let active_key_id = active.key_id();
    if signature.key_id != active_key_id.as_str() {
        return Err(NodeDocumentError::KeyMismatch {
            signed_with: signature.key_id.clone(),
            active: active_key_id.to_string(),
        });
    }
    if resolver
        .is_key_revoked_at(actor, &active_key_id, signed_at)
        .map_err(NodeDocumentError::Resolve)?
    {
        return Err(NodeDocumentError::RevokedKey {
            key_id: signature.key_id.clone(),
        });
    }
    if signature.algorithm != active.algorithm().as_str() {
        return Err(NodeDocumentError::BadSignature);
    }
    let raw = STANDARD
        .decode(&signature.bytes)
        .ok()
        .and_then(|raw| <[u8; 64]>::try_from(raw.as_slice()).ok())
        .ok_or(NodeDocumentError::BadSignature)?;
    verify_signature(&active, bytes, &SignatureBytes::ed25519(raw))
        .map_err(|_| NodeDocumentError::BadSignature)?;
    Ok(())
}

/// `http://` or `https://`, a non-empty host, no path, query,
/// fragment, trailing slash, or whitespace. Clients join endpoint
/// paths onto it directly.
pub(crate) fn validate_base_url(value: &str) -> Result<(), NodeDocumentError> {
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
    InvalidBaseUrl {
        value: String,
    },
    UnsupportedProtocol {
        offered: Vec<String>,
    },
    InvalidGenesis(ActorGenesisJsonError),
    ActorMismatch {
        claimed: ActorId,
        derived: ActorId,
    },
    NotANodeActor {
        kind: String,
    },
    InvalidSignedAt {
        value: String,
    },
    /// The node actor's key state couldn't be read.
    Resolve(ActorResolveError),
    NoActiveKey,
    /// Signed by a key other than the one active at `signed_at`.
    KeyMismatch {
        signed_with: String,
        active: String,
    },
    RevokedKey {
        key_id: String,
    },
    BadSignature,
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
            Self::InvalidSignedAt { value } => write!(f, "signed_at {value:?} is not a timestamp"),
            Self::Resolve(error) => write!(f, "could not read the node actor's keys: {error}"),
            Self::NoActiveKey => write!(f, "the node actor has no active key at signed_at"),
            Self::KeyMismatch {
                signed_with,
                active,
            } => write!(
                f,
                "signed with key {signed_with}, but the node actor's key at signed_at is {active}"
            ),
            Self::RevokedKey { key_id } => {
                write!(f, "signed with key {key_id}, revoked at signed_at")
            }
            Self::BadSignature => write!(f, "the signature does not verify"),
        }
    }
}

impl std::error::Error for NodeDocumentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidGenesis(error) => Some(error),
            Self::Resolve(error) => Some(error),
            _ => None,
        }
    }
}
