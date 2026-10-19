//! `kairo-federation-service` — the sync engine behind the daemon's
//! `FederationService` (`specs/DECISIONS.md` §28, `specs/DAEMON.md`
//! §5.3).
//!
//! It has no scheduler. [`sync_follows`] syncs what the operator
//! follows, once, from every listed peer, and [`receive_inbox`]
//! processes the inbox once; the daemon calls them on timers and
//! `kairo federation sync` / `receive` call them directly. Every
//! record goes through the same pull verifier as `kairo fetch`.

mod discover;
mod receive;

pub use discover::{discover, holders, DiscoverOutcome};
pub use receive::{receive_inbox, ReceiveOutcome, Received, Via};

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use kairo_config::{FederationConfig, LimitsConfig};
use kairo_core::Timestamp;
use kairo_federation_core::FedErrorCode;
use kairo_federation_http::client::{
    fetch_trust, normalize_peer_url, pull_object, Limits, PullError, PullOptions,
};
use kairo_statement::{TrustDecision, TrustScope};
use kairo_store::{
    FilesystemStore, Follow, FollowStore, PeerOutcome, ProvenanceStore, PullLogStore, PullRecord,
    StoreError, SyncAttempt, TrustResolver,
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
        let attempt = sync_one(store, config, &record.follow).map_err(SyncError::Store)?;
        results.push((record.follow, attempt));
    }
    Ok(results)
}

/// Sync one follow and record the result: what the daemon runs per
/// due follow. Doesn't check `enabled`; the caller does.
pub fn sync_one(
    store: &FilesystemStore,
    config: &FederationConfig,
    follow: &Follow,
) -> Result<SyncAttempt, StoreError> {
    let attempt = sync_follow(store, config, follow);
    store.record_sync(follow, &attempt)?;
    Ok(attempt)
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
    if let Follow::Object(object) = follow {
        for (url, holder) in vouched_holders(store, config, object, &peers) {
            let options = PullOptions {
                limits: limits.clone(),
                expected_node_actor: Some(holder),
                require_complete: false,
            };
            peers.push(sync_from(store, &url, follow, &options));
        }
    }
    SyncAttempt {
        at: Timestamp::now(),
        peers,
    }
}

/// Acquire `object` from wherever it is held (`DECISIONS.md` §30
/// sub-decisions 1 and 2): the sync sources, those its provenance names
/// first (most recent first), stopping at the first that serves it.
/// Nothing is recorded as a follow.
pub fn acquire(
    store: &FilesystemStore,
    config: &FederationConfig,
    config_path: PathBuf,
    object: &kairo_core::ObjectId,
) -> Result<SyncAttempt, SyncError> {
    if !config.enabled {
        return Err(SyncError::Disabled {
            config: config_path,
        });
    }
    let limits = limits_from(&config.limits);
    let follow = Follow::Object(object.clone());
    let mut peers = Vec::new();
    for (url, pin) in acquisition_sources(store, config, object) {
        let options = PullOptions {
            limits: limits.clone(),
            expected_node_actor: pin,
            require_complete: false,
        };
        let outcome = sync_from(store, &url, &follow, &options);
        let served = outcome.succeeded();
        peers.push(outcome);
        if served {
            break;
        }
    }
    Ok(SyncAttempt {
        at: Timestamp::now(),
        peers,
    })
}

/// The sync sources for `object` with pins, reordered so the peers its
/// provenance names come first, most recently seen first. Provenance
/// only reorders: it never adds a source (§30 sub-decision 1).
fn acquisition_sources(
    store: &FilesystemStore,
    config: &FederationConfig,
    object: &kairo_core::ObjectId,
) -> Vec<(String, Option<kairo_core::ActorId>)> {
    let mut sources: Vec<(String, Option<kairo_core::ActorId>)> = config
        .peers
        .iter()
        .filter_map(|peer| {
            normalize_peer_url(&peer.url)
                .ok()
                .map(|url| (url, peer.node_actor.clone()))
        })
        .collect();
    for (url, holder) in vouched_holders(store, config, object, &[]) {
        if !sources.iter().any(|(listed, _)| listed == &url) {
            sources.push((url, Some(holder)));
        }
    }
    let mut seen: Vec<(String, Timestamp)> = store
        .get_provenance(&kairo_core::KairoRef::Object(object.clone()))
        .unwrap_or_default()
        .into_iter()
        // `push:` and `file:` entries aren't URLs.
        .filter_map(|entry| {
            normalize_peer_url(&entry.peer_url)
                .ok()
                .map(|url| (url, entry.fetched_at))
        })
        .collect();
    seen.sort_by_key(|entry| std::cmp::Reverse(entry.1));
    let rank = |url: &str| {
        seen.iter()
            .position(|(seen, _)| seen == url)
            .unwrap_or(usize::MAX)
    };
    // Stable: unseen sources keep config order after the seen ones.
    sources.sort_by_key(|(url, _)| rank(url));
    sources
}

/// With `use_hints` (`DECISIONS.md` §29 sub-decision 5): fresh hinted
/// holders of `object` not already tried, that `vouched_by` trusts
/// with `federation:index`, or whose hint was relayed by a node it so
/// trusts. Each comes with its node actor, which the pull pins.
fn vouched_holders(
    store: &FilesystemStore,
    config: &FederationConfig,
    object: &kairo_core::ObjectId,
    tried: &[PeerOutcome],
) -> Vec<(String, kairo_core::ActorId)> {
    let (true, Some(voucher)) = (config.discovery.use_hints, &config.discovery.vouched_by) else {
        return Vec::new();
    };
    let Ok(hints) = holders(store, config, object) else {
        return Vec::new();
    };
    let trusts = |node: &kairo_core::ActorId| {
        store
            .latest_trust(voucher, node, &TrustScope::federation_index())
            .ok()
            .flatten()
            .is_some_and(|signed| {
                signed.unsigned().body().decision() == Some(TrustDecision::Trusted)
            })
    };
    let mut chosen: Vec<(String, kairo_core::ActorId)> = Vec::new();
    for hint in hints {
        let Ok(url) = normalize_peer_url(&hint.base_url) else {
            continue;
        };
        let already = tried.iter().any(|outcome| outcome.peer_url() == url)
            || chosen.iter().any(|(chosen, _)| chosen == &url);
        let vouched = trusts(&hint.node_actor) || hint.learned_via.as_ref().is_some_and(trusts);
        if !already && vouched {
            chosen.push((url, hint.node_actor));
        }
    }
    chosen
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
    fn the_inbox_pass_refuses_in_local_only_mode_and_empties_the_inbox() {
        use kairo_store::{InboxAnnouncement, InboxStore};
        let (_dir, store) = store();
        let disabled = FederationConfig {
            enabled: false,
            ..FederationConfig::default()
        };
        assert!(matches!(
            receive_inbox(&store, &disabled, PathBuf::from("config.toml")),
            Err(SyncError::Disabled { .. })
        ));
        // An announcement nobody trusts is refused, and still removed.
        let from = kairo_core::ActorId::from_sha256_digest([2; 32]);
        store
            .put_inbox_announcement(
                &InboxAnnouncement {
                    from: from.clone(),
                    to: kairo_core::ActorId::from_sha256_digest([3; 32]),
                    base_url: "https://laptop.example.org".to_owned(),
                    objects: vec![ObjectId::from_sha256_digest([1; 32])],
                    received_at: Timestamp::from_seconds(1),
                },
                "0a",
                8,
            )
            .expect("put");
        let received = receive_inbox(
            &store,
            &FederationConfig::default(),
            PathBuf::from("config.toml"),
        )
        .expect("receive");
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].from, from);
        assert_eq!(received[0].via, Via::Announcement);
        assert!(matches!(
            received[0].outcome,
            ReceiveOutcome::Refused { .. }
        ));
        assert!(store.list_inbox().expect("inbox").is_empty());
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
