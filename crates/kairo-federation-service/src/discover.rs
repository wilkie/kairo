//! Discovery, step one (`specs/DECISIONS.md` §29): fetch inventories
//! from listed peers and bootstrap nodes, along with what they relay,
//! keep the newest per node as hints, and answer "who has object X?"
//! from them.

use std::path::PathBuf;

use kairo_config::FederationConfig;
use kairo_core::{ActorId, ObjectId, Timestamp};
use kairo_federation_http::client::{
    fetch_inventory, fetch_relayed, normalize_peer_url, FetchedInventory, PullError, PullOptions,
    RelayedOutcome,
};
use kairo_store::{FilesystemStore, Hint, HintPut, HintStore, StoreError};

use crate::{limits_from, SyncError};

/// What one source gave a discovery pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoverOutcome {
    /// The source's own verified inventory. `newer` when it replaced an
    /// older hint (or was the first for that node).
    Learned {
        source_url: String,
        node_actor: ActorId,
        objects: usize,
        newer: bool,
    },
    /// A verified inventory the source relayed for another node (§29
    /// sub-decision 4).
    Relayed {
        source_url: String,
        via: ActorId,
        node_actor: ActorId,
        objects: usize,
        newer: bool,
    },
    /// A relayed entry that didn't verify; the others still count.
    RelayRefused {
        source_url: String,
        node_actor: Option<ActorId>,
        reason: String,
    },
    /// The source doesn't serve inventories.
    Unsupported {
        source_url: String,
    },
    Failed {
        source_url: String,
        reason: String,
    },
}

impl DiscoverOutcome {
    pub fn source_url(&self) -> &str {
        match self {
            Self::Learned { source_url, .. }
            | Self::Relayed { source_url, .. }
            | Self::RelayRefused { source_url, .. }
            | Self::Unsupported { source_url }
            | Self::Failed { source_url, .. } => source_url,
        }
    }
}

/// Hints signed before this are stale.
pub(crate) fn expired_before(config: &FederationConfig) -> Timestamp {
    let ttl = i64::try_from(config.discovery.hint_ttl_secs).unwrap_or(i64::MAX);
    Timestamp::from_seconds(Timestamp::now().seconds().saturating_sub(ttl))
}

/// Fetch every listed peer's and bootstrap node's own inventory and the
/// inventories it relays, once, keeping the newest per node as a hint
/// (§29 sub-decisions 3–5).
pub fn discover(
    store: &FilesystemStore,
    config: &FederationConfig,
    config_path: PathBuf,
) -> Result<Vec<DiscoverOutcome>, SyncError> {
    if !config.enabled {
        return Err(SyncError::Disabled {
            config: config_path,
        });
    }
    let limits = limits_from(&config.limits);
    let mut seen = Vec::new();
    let mut outcomes = Vec::new();
    for source in config.peers.iter().chain(&config.bootstrap) {
        let url = match normalize_peer_url(&source.url) {
            Ok(url) => url,
            Err(error) => {
                outcomes.push(DiscoverOutcome::Failed {
                    source_url: source.url.clone(),
                    reason: error.to_string(),
                });
                continue;
            }
        };
        if seen.contains(&url) {
            continue;
        }
        seen.push(url.clone());
        let options = PullOptions {
            limits: limits.clone(),
            expected_node_actor: source.node_actor.clone(),
            require_complete: false,
        };
        match fetch_inventory(store, &url, &options) {
            Ok(fetched) => {
                let newer = keep(store, config, &fetched, &url, None)?;
                outcomes.push(DiscoverOutcome::Learned {
                    source_url: url.clone(),
                    node_actor: fetched.inventory.node_actor.clone(),
                    objects: fetched.inventory.objects.len(),
                    newer,
                });
            }
            Err(PullError::Unsupported { .. }) => {
                outcomes.push(DiscoverOutcome::Unsupported { source_url: url });
                continue;
            }
            Err(error) => {
                outcomes.push(DiscoverOutcome::Failed {
                    source_url: url,
                    reason: error.to_string(),
                });
                continue;
            }
        }
        match fetch_relayed(store, &url, &options) {
            Ok((via, relayed)) => {
                for outcome in relayed {
                    outcomes.push(match outcome {
                        RelayedOutcome::Verified(fetched) => {
                            let newer = keep(store, config, &fetched, &url, Some(&via))?;
                            DiscoverOutcome::Relayed {
                                source_url: url.clone(),
                                via: via.clone(),
                                node_actor: fetched.inventory.node_actor.clone(),
                                objects: fetched.inventory.objects.len(),
                                newer,
                            }
                        }
                        RelayedOutcome::Refused { node, reason } => DiscoverOutcome::RelayRefused {
                            source_url: url.clone(),
                            node_actor: node,
                            reason,
                        },
                    });
                }
            }
            Err(error) => outcomes.push(DiscoverOutcome::Failed {
                source_url: url,
                reason: format!("relayed inventories: {error}"),
            }),
        }
    }
    Ok(outcomes)
}

/// Keep `fetched` as a hint learned from `source_url` (via the relaying
/// node, if any); whether it was newer.
fn keep(
    store: &FilesystemStore,
    config: &FederationConfig,
    fetched: &FetchedInventory,
    source_url: &str,
    via: Option<&ActorId>,
) -> Result<bool, SyncError> {
    let inventory = &fetched.inventory;
    let hint = Hint {
        node_actor: inventory.node_actor.clone(),
        base_url: inventory.base_url.clone(),
        objects: inventory.objects.clone(),
        signed_at: fetched.signed_at,
        learned_from: source_url.to_owned(),
        learned_via: via.cloned(),
        learned_at: Timestamp::now(),
        inventory: fetched.raw.clone(),
    };
    let put = store
        .put_hint(&hint, expired_before(config))
        .map_err(SyncError::Store)?;
    Ok(put == HintPut::Stored)
}

/// Fresh hints whose inventory lists `object`.
pub fn holders(
    store: &FilesystemStore,
    config: &FederationConfig,
    object: &ObjectId,
) -> Result<Vec<Hint>, StoreError> {
    Ok(store
        .list_hints(expired_before(config))?
        .into_iter()
        .filter(|hint| hint.objects.contains(object))
        .collect())
}
