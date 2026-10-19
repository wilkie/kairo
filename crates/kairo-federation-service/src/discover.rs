//! Discovery, step one (`specs/DECISIONS.md` §29): fetch inventories
//! from listed peers and bootstrap nodes, keep the newest per node as
//! hints, and answer "who has object X?" from them.

use std::path::PathBuf;
use std::time::Duration;

use kairo_config::FederationConfig;
use kairo_core::{ActorId, ObjectId, Timestamp};
use kairo_federation_http::client::{fetch_inventory, normalize_peer_url, PullError, PullOptions};
use kairo_store::{FilesystemStore, Hint, HintPut, HintStore, StoreError};

use crate::{limits_from, SyncError};

/// What one source gave a discovery pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoverOutcome {
    /// A verified inventory. `newer` when it replaced an older hint (or
    /// was the first for that node).
    Learned {
        source_url: String,
        node_actor: ActorId,
        objects: usize,
        newer: bool,
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

/// Hints signed before this are stale.
fn expired_before(config: &FederationConfig) -> Timestamp {
    let ttl = Duration::from_secs(config.discovery.hint_ttl_secs);
    let ttl = i64::try_from(ttl.as_secs()).unwrap_or(i64::MAX);
    Timestamp::from_seconds(Timestamp::now().seconds().saturating_sub(ttl))
}

/// Fetch every listed peer's and bootstrap node's own inventory, once,
/// keeping the newest per node as a hint (§29 sub-decisions 3 and 5).
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
        outcomes.push(match fetch_inventory(store, &url, &options) {
            Ok(fetched) => {
                let inventory = &fetched.inventory;
                let hint = Hint {
                    node_actor: inventory.node_actor.clone(),
                    base_url: inventory.base_url.clone(),
                    objects: inventory.objects.clone(),
                    signed_at: fetched.signed_at,
                    learned_from: fetched.peer_url.clone(),
                    learned_via: None,
                    learned_at: Timestamp::now(),
                    inventory: fetched.raw.clone(),
                };
                let put = store
                    .put_hint(&hint, expired_before(config))
                    .map_err(SyncError::Store)?;
                DiscoverOutcome::Learned {
                    source_url: url,
                    node_actor: hint.node_actor,
                    objects: hint.objects.len(),
                    newer: put == HintPut::Stored,
                }
            }
            Err(PullError::Unsupported { .. }) => DiscoverOutcome::Unsupported { source_url: url },
            Err(error) => DiscoverOutcome::Failed {
                source_url: url,
                reason: error.to_string(),
            },
        });
    }
    Ok(outcomes)
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
