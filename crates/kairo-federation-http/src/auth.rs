//! Verifying `KairoNode1` request signatures (`specs/DECISIONS.md` §25
//! sub-decision 3) for endpoints that act on who is asking. Reads stay
//! anonymous (sub-decision 4): nothing on the pull surface calls this.
//!
//! The signer must be a node actor this node holds, signing with its
//! key active at the request's `timestamp`, unrevoked, within
//! ±[`NODE_REQUEST_WINDOW_SECONDS`] of now, over a signing string that
//! names this node's base URL. A `(node, nonce)` pair is accepted once
//! per window. The cache is in memory, so a replay across a restart
//! within one window is possible, which v1 accepts.

use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;

use kairo_core::{ActorId, Timestamp};
use kairo_federation_core::{
    FedErrorCode, NodeAuthHeaderError, NodeAuthorization, NodeRequest, NODE_NONCE_LEN,
    NODE_REQUEST_WINDOW_SECONDS, NODE_SCHEME_TAG,
};
use kairo_identity::{verify_signature, ActorKind, ActorResolver, KeyId, SignatureBytes};

use crate::node::Node;

/// Who signed an authenticated request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedNode {
    pub node: ActorId,
    pub key_id: KeyId,
    /// The request's nonce, unique for this node within the window.
    pub nonce: [u8; NODE_NONCE_LEN],
}

/// Why a request didn't authenticate. Everything but
/// [`NodeAuthError::Store`] is `401 fed_unauthenticated`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeAuthError {
    Missing,
    Header(NodeAuthHeaderError),
    OutOfWindow {
        timestamp: i64,
        now: i64,
    },
    /// No genesis for the signer in this node's store.
    UnknownNode(ActorId),
    NotANode {
        node: ActorId,
        kind: String,
    },
    NoActiveKey {
        node: ActorId,
    },
    KeyMismatch {
        signed_with: KeyId,
        active: KeyId,
    },
    RevokedKey {
        key_id: KeyId,
    },
    BadSignature,
    /// This `(node, nonce)` was already accepted within the window.
    Replayed,
    /// Reading the signer's key state failed.
    Store(String),
}

impl NodeAuthError {
    pub fn code(&self) -> FedErrorCode {
        match self {
            Self::Store(_) => FedErrorCode::FedStoreError,
            _ => FedErrorCode::FedUnauthenticated,
        }
    }
}

impl fmt::Display for NodeAuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => write!(f, "this endpoint requires Authorization: {NODE_SCHEME_TAG}"),
            Self::Header(error) => write!(f, "{error}"),
            Self::OutOfWindow { timestamp, now } => write!(
                f,
                "request timestamp {timestamp} is more than {NODE_REQUEST_WINDOW_SECONDS}s from \
                 this node's clock ({now})"
            ),
            Self::UnknownNode(node) => write!(f, "node actor {node} is not known to this node"),
            Self::NotANode { node, kind } => {
                write!(f, "actor {node} has actor_kind {kind:?}, not \"node\"")
            }
            Self::NoActiveKey { node } => write!(f, "node actor {node} has no active key"),
            Self::KeyMismatch {
                signed_with,
                active,
            } => write!(
                f,
                "signed with key {signed_with}, but the node actor's active key is {active}"
            ),
            Self::RevokedKey { key_id } => write!(f, "key {key_id} is revoked"),
            Self::BadSignature => write!(f, "the request signature does not verify"),
            Self::Replayed => write!(f, "this request's nonce was already used"),
            Self::Store(message) => write!(f, "could not read the signer's keys: {message}"),
        }
    }
}

impl std::error::Error for NodeAuthError {}

/// `(node, nonce)` pairs accepted within the window, each kept until
/// its timestamp leaves the window.
#[derive(Debug, Default)]
pub(crate) struct NonceCache {
    seen: Mutex<HashMap<(ActorId, [u8; NODE_NONCE_LEN]), i64>>,
}

impl NonceCache {
    /// Record the pair; `false` if it was already there.
    fn insert(
        &self,
        node: &ActorId,
        nonce: [u8; NODE_NONCE_LEN],
        timestamp: i64,
        now: i64,
    ) -> bool {
        let mut seen = self
            .seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        seen.retain(|_, at| at.saturating_add(NODE_REQUEST_WINDOW_SECONDS) >= now);
        seen.insert((node.clone(), nonce), timestamp).is_none()
    }
}

impl Node {
    /// Authenticate a request by its `Authorization: KairoNode1`
    /// header. `path_and_query` is the request target as received;
    /// `body` the raw body. Reads the store, so call it off the async
    /// runtime.
    pub fn authenticate_request(
        &self,
        method: &str,
        path_and_query: &str,
        authorization: Option<&str>,
        body: &[u8],
    ) -> Result<AuthenticatedNode, NodeAuthError> {
        let header = authorization.ok_or(NodeAuthError::Missing)?;
        let auth = NodeAuthorization::parse(header).map_err(NodeAuthError::Header)?;
        let now = Timestamp::now().seconds();
        if (now - auth.timestamp).abs() > NODE_REQUEST_WINDOW_SECONDS {
            return Err(NodeAuthError::OutOfWindow {
                timestamp: auth.timestamp,
                now,
            });
        }

        let store = &self.store;
        let node = &auth.node;
        let at = Timestamp::from_seconds(auth.timestamp);
        let read =
            |error: kairo_identity::ActorResolveError| NodeAuthError::Store(error.to_string());
        let genesis = store
            .actor_genesis(node)
            .map_err(read)?
            .ok_or_else(|| NodeAuthError::UnknownNode(node.clone()))?;
        if genesis.actor_kind() != &ActorKind::node() {
            return Err(NodeAuthError::NotANode {
                node: node.clone(),
                kind: genesis.actor_kind().as_str().to_owned(),
            });
        }
        let active = store
            .active_key_at(node, at)
            .map_err(read)?
            .ok_or_else(|| NodeAuthError::NoActiveKey { node: node.clone() })?;
        if active.key_id() != auth.key_id {
            return Err(NodeAuthError::KeyMismatch {
                signed_with: auth.key_id,
                active: active.key_id(),
            });
        }
        for when in [at, Timestamp::from_seconds(now)] {
            if store
                .is_key_revoked_at(node, &auth.key_id, when)
                .map_err(read)?
            {
                return Err(NodeAuthError::RevokedKey {
                    key_id: auth.key_id,
                });
            }
        }

        let request = NodeRequest {
            method,
            receiver_base_url: &self.document.base_url,
            path_and_query,
            body,
        };
        let signing = request.signing_string(auth.timestamp, &auth.nonce);
        verify_signature(
            &active,
            signing.as_bytes(),
            &SignatureBytes::ed25519(auth.signature),
        )
        .map_err(|_| NodeAuthError::BadSignature)?;

        // Only after the signature verifies, so an unauthenticated
        // client can't fill the cache.
        if !self.nonces.insert(node, auth.nonce, auth.timestamp, now) {
            return Err(NodeAuthError::Replayed);
        }
        Ok(AuthenticatedNode {
            node: auth.node,
            key_id: auth.key_id,
            nonce: auth.nonce,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_nonce_is_forgotten_once_its_timestamp_leaves_the_window() {
        let cache = NonceCache::default();
        let node = ActorId::from_sha256_digest([1; 32]);
        assert!(cache.insert(&node, [7; 16], 1_000, 1_000));
        assert!(!cache.insert(&node, [7; 16], 1_000, 1_000 + NODE_REQUEST_WINDOW_SECONDS));
        // Past the window, the old request can't pass the timestamp
        // check, so its entry is dropped.
        assert!(cache.insert(&node, [8; 16], 2_000, 1_001 + NODE_REQUEST_WINDOW_SECONDS));
        assert!(cache.insert(&node, [7; 16], 2_000, 1_001 + NODE_REQUEST_WINDOW_SECONDS));
    }
}
