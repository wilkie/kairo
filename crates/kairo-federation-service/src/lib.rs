//! `kairo-federation-service` — the sync engine behind the daemon's
//! `FederationService` (`specs/DECISIONS.md` §28, `specs/DAEMON.md`
//! §5.3).
//!
//! It has no scheduler. [`sync_follows`] syncs what the operator
//! follows, once, from every listed peer; the daemon calls it on a
//! timer and `kairo federation sync` calls it directly. Every record
//! goes through the same pull verifier as `kairo fetch`.

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use kairo_config::{FederationConfig, LimitsConfig};
use kairo_core::Timestamp;
use kairo_federation_core::FedErrorCode;
use kairo_federation_http::client::{
    fetch_trust, normalize_peer_url, pull_object, Limits, PullError, PullOptions,
};
use kairo_store::{
    FilesystemStore, Follow, FollowStore, PeerOutcome, PullLogStore, PullRecord, StoreError,
    SyncAttempt,
};

/// The pull client's limits with `config`'s overrides.
pub fn limits_from(config: &LimitsConfig) -> Limits {
    let defaults = Limits::default();
    let secs = |value: Option<u64>, default: Duration| value.map_or(default, Duration::from_secs);
    Limits {
        max_json_bytes: config.max_json_bytes.unwrap_or(defaults.max_json_bytes),
        max_batch_bytes: config.max_batch_bytes.unwrap_or(defaults.max_batch_bytes),
        max_blob_bytes: config.max_blob_bytes.unwrap_or(defaults.max_blob_bytes),
        max_git_bytes: config.max_git_bytes.unwrap_or(defaults.max_git_bytes),
        max_records: config.max_records.map_or(defaults.max_records, |n| {
            usize::try_from(n).unwrap_or(usize::MAX)
        }),
        request_timeout: secs(config.request_timeout_secs, defaults.request_timeout),
        git_timeout: secs(config.git_timeout_secs, defaults.git_timeout),
    }
}

#[derive(Debug)]
pub enum SyncError {
    /// `[federation] enabled = false` (`DAEMON.md` §10.1).
    Disabled {
        config: PathBuf,
    },
    Store(StoreError),
}

impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disabled { config } => write!(
                f,
                "federation is disabled ([federation] enabled = false in {})",
                config.display()
            ),
            Self::Store(error) => write!(f, "store error: {error}"),
        }
    }
}

impl std::error::Error for SyncError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Store(error) => Some(error),
            Self::Disabled { .. } => None,
        }
    }
}

/// Sync every follow (or just `only`), once, from every listed peer
/// (`DECISIONS.md` §28 sub-decisions 2 and 4), recording each result.
/// `config_path` names the config in a [`SyncError::Disabled`].
pub fn sync_follows(
    store: &FilesystemStore,
    config: &FederationConfig,
    config_path: PathBuf,
    only: Option<&Follow>,
) -> Result<Vec<(Follow, SyncAttempt)>, SyncError> {
    if !config.enabled {
        return Err(SyncError::Disabled {
            config: config_path,
        });
    }
    let follows = store.list_follows().map_err(SyncError::Store)?;
    let mut results = Vec::new();
    for record in follows {
        if only.is_some_and(|only| only != &record.follow) {
            continue;
        }
        let attempt = sync_follow(store, config, &record.follow);
        store
            .record_sync(&record.follow, &attempt)
            .map_err(SyncError::Store)?;
        results.push((record.follow, attempt));
    }
    Ok(results)
}

/// One follow from each listed peer. A peer that doesn't publish it
/// (`404 fed_not_found`) is [`PeerOutcome::NotPublished`], not a
/// failure.
pub fn sync_follow(
    store: &FilesystemStore,
    config: &FederationConfig,
    follow: &Follow,
) -> SyncAttempt {
    let limits = limits_from(&config.limits);
    let mut peers = Vec::new();
    for peer in &config.peers {
        let url = match normalize_peer_url(&peer.url) {
            Ok(url) => url,
            Err(error) => {
                peers.push(PeerOutcome::Failed {
                    peer_url: peer.url.clone(),
                    reason: error.to_string(),
                });
                continue;
            }
        };
        let options = PullOptions {
            limits: limits.clone(),
            expected_node_actor: peer.node_actor.clone(),
            require_complete: false,
        };
        peers.push(sync_from(store, &url, follow, &options));
    }
    SyncAttempt {
        at: Timestamp::now(),
        peers,
    }
}

fn sync_from(
    store: &FilesystemStore,
    url: &str,
    follow: &Follow,
    options: &PullOptions,
) -> PeerOutcome {
    let outcome = match follow {
        Follow::Object(object) => pull_object(store, url, object, options).map(|report| {
            // As `kairo fetch` does; a failure to log isn't a failed sync.
            let _ = store.record_pull(&PullRecord {
                peer_url: report.peer_url.clone(),
                object: object.clone(),
                peer_actor: report.peer_actor.clone(),
                at: Timestamp::now(),
                fetched: report.fetched as u64,
                already_held: report.already_held as u64,
            });
            PeerOutcome::Fetched {
                peer_url: url.to_owned(),
                fetched: report.fetched as u64,
                already_held: report.already_held as u64,
                incomplete_chains: report.incomplete.len() as u64,
                flagged: report.flagged.len() as u64,
            }
        }),
        Follow::Trust { by, scope } => {
            fetch_trust(store, url, by, scope, options).map(|report| PeerOutcome::Fetched {
                peer_url: url.to_owned(),
                fetched: report.fetched as u64,
                already_held: report.already_held as u64,
                incomplete_chains: 0,
                flagged: report.flagged.len() as u64,
            })
        }
    };
    match outcome {
        Ok(outcome) => outcome,
        Err(PullError::Status {
            code: Some(FedErrorCode::FedNotFound),
            ..
        }) => PeerOutcome::NotPublished {
            peer_url: url.to_owned(),
        },
        Err(error) => PeerOutcome::Failed {
            peer_url: url.to_owned(),
            reason: error.to_string(),
        },
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use kairo_core::ObjectId;

    fn store() -> (tempfile::TempDir, FilesystemStore) {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let store = FilesystemStore::open(dir.path()).expect("store");
        (dir, store)
    }

    #[test]
    fn local_only_mode_syncs_nothing() {
        let (_dir, store) = store();
        let config = FederationConfig {
            enabled: false,
            ..FederationConfig::default()
        };
        assert!(matches!(
            sync_follows(&store, &config, PathBuf::from("config.toml"), None),
            Err(SyncError::Disabled { .. })
        ));
    }

    #[test]
    fn with_no_peers_a_follow_is_tried_and_not_current() {
        let (_dir, store) = store();
        let follow = Follow::Object(ObjectId::from_sha256_digest([1; 32]));
        store
            .add_follow(&follow, Timestamp::from_seconds(1))
            .expect("follow");
        let results = sync_follows(
            &store,
            &FederationConfig::default(),
            PathBuf::from("config.toml"),
            None,
        )
        .expect("sync");
        assert_eq!(results.len(), 1);
        assert!(!results[0].1.succeeded());
        let record = store.list_follows().expect("list").remove(0);
        assert!(record.last_attempt.is_some());
        assert_eq!(record.last_success, None);
    }
}
