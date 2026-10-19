//! The daemon's `FederationService` (`specs/DAEMON.md` §5.3,
//! `specs/DECISIONS.md` §28): it runs the sync engine in
//! `kairo-federation-service` on timers.
//!
//! One scheduler loop ticks every `min(inbox_secs, interval_secs)`.
//! Each tick re-reads `<store>/config.toml`, so edits apply without a
//! restart, and then:
//!
//! - processes the inbox if `inbox_secs` have passed since the last
//!   pass (one pass at a time);
//! - runs discovery (`DECISIONS.md` §29) if `interval_secs` have passed
//!   since the last pass (one at a time);
//! - starts every follow whose last attempt is `interval_secs` old (or
//!   that has never been tried), as long as fewer than
//!   `max_concurrent` are running, and never the same follow twice.
//!
//! Work runs on the blocking pool; a tick never waits for it.
//! `POST /api/v1/federation/sync` starts follows now, subject to the
//! same single-flight and cap. Runs are kept in memory (the last
//! [`MAX_RUNS`]) for `GET /api/v1/federation/status`.

use std::collections::{BTreeSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use kairo_config::FederationConfig;
use kairo_core::Timestamp;
use kairo_federation_service::{
    discover, receive_inbox, sync_one, DiscoverOutcome, ReceiveOutcome,
};
use kairo_store::{FilesystemStore, Follow, FollowStore};

/// Runs kept for status.
pub const MAX_RUNS: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunKind {
    Sync(Follow),
    Inbox,
    Discover,
}

/// One engine run, for status (`DAEMON.md` §14: operational, not
/// validation, state).
#[derive(Debug, Clone)]
pub struct Run {
    pub id: u64,
    pub kind: RunKind,
    pub started_at: Timestamp,
    pub finished_at: Option<Timestamp>,
    /// `current` / `not_current` for a sync, `received=<n> not_pulled=<n>`
    /// for the inbox, or the error.
    pub result: Option<String>,
}

#[derive(Debug)]
pub enum TriggerError {
    Disabled(String),
    NotFollowed(Follow),
    Unavailable(String),
}

/// What a trigger started, and what was already running.
#[derive(Debug, Default)]
pub struct Triggered {
    pub started: Vec<Follow>,
    pub already_running: Vec<Follow>,
    /// Over `max_concurrent`; the scheduler starts them when slots free.
    pub deferred: Vec<Follow>,
}

#[derive(Debug)]
pub struct FederationService {
    store: Arc<FilesystemStore>,
    store_path: PathBuf,
    running: Mutex<BTreeSet<Follow>>,
    inbox_running: AtomicBool,
    last_inbox: AtomicI64,
    discover_running: AtomicBool,
    last_discover: AtomicI64,
    runs: Mutex<VecDeque<Run>>,
    next_run: AtomicU64,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl FederationService {
    pub fn new(store: Arc<FilesystemStore>, store_path: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            store,
            store_path,
            running: Mutex::new(BTreeSet::new()),
            inbox_running: AtomicBool::new(false),
            last_inbox: AtomicI64::new(i64::MIN),
            discover_running: AtomicBool::new(false),
            last_discover: AtomicI64::new(i64::MIN),
            runs: Mutex::new(VecDeque::new()),
            next_run: AtomicU64::new(1),
        })
    }

    /// The `[federation]` section now, and the file it came from.
    pub fn config(&self) -> Result<(FederationConfig, PathBuf), String> {
        kairo_config::load(&self.store_path)
            .map(|(config, source)| (config.federation, source.path().to_path_buf()))
            .map_err(|error| error.to_string())
    }

    pub fn running(&self) -> BTreeSet<Follow> {
        lock(&self.running).clone()
    }

    /// Newest first.
    pub fn runs(&self) -> Vec<Run> {
        lock(&self.runs).iter().rev().cloned().collect()
    }

    fn begin(&self, kind: RunKind) -> u64 {
        let id = self.next_run.fetch_add(1, Ordering::Relaxed);
        let mut runs = lock(&self.runs);
        runs.push_back(Run {
            id,
            kind,
            started_at: Timestamp::now(),
            finished_at: None,
            result: None,
        });
        while runs.len() > MAX_RUNS {
            runs.pop_front();
        }
        id
    }

    fn finish(&self, id: u64, result: String) {
        if let Some(run) = lock(&self.runs).iter_mut().find(|run| run.id == id) {
            run.finished_at = Some(Timestamp::now());
            run.result = Some(result);
        }
    }

    /// Start `follow` on the blocking pool unless it's running or the
    /// cap is reached. `Some(true)` started, `Some(false)` already
    /// running, `None` at the cap.
    fn start(self: &Arc<Self>, follow: &Follow, config: &FederationConfig) -> Option<bool> {
        {
            let mut running = lock(&self.running);
            if running.contains(follow) {
                return Some(false);
            }
            let cap = usize::try_from(config.sync.max_concurrent).unwrap_or(usize::MAX);
            if running.len() >= cap {
                return None;
            }
            running.insert(follow.clone());
        }
        let id = self.begin(RunKind::Sync(follow.clone()));
        let service = Arc::clone(self);
        let follow = follow.clone();
        let config = config.clone();
        tokio::task::spawn_blocking(move || {
            let result = match sync_one(&service.store, &config, &follow) {
                Ok(attempt) if attempt.succeeded() => "current".to_owned(),
                Ok(_) => "not_current".to_owned(),
                Err(error) => format!("error: {error}"),
            };
            if result.starts_with("error") {
                tracing::warn!(%follow, %result, "follow sync failed");
            }
            lock(&service.running).remove(&follow);
            service.finish(id, result);
        });
        Some(true)
    }

    fn start_inbox(self: &Arc<Self>, config: &FederationConfig, config_path: PathBuf) {
        if self.inbox_running.swap(true, Ordering::AcqRel) {
            return;
        }
        self.last_inbox
            .store(Timestamp::now().seconds(), Ordering::Release);
        let service = Arc::clone(self);
        let config = config.clone();
        tokio::task::spawn_blocking(move || {
            // An empty inbox isn't worth a run record.
            let pending = service.store_inbox_len();
            let id = (pending > 0).then(|| service.begin(RunKind::Inbox));
            let result = match receive_inbox(&service.store, &config, config_path) {
                Ok(received) => {
                    let failed = received
                        .iter()
                        .filter(|item| !matches!(item.outcome, ReceiveOutcome::Received { .. }))
                        .count();
                    format!("received={} not_pulled={failed}", received.len())
                }
                Err(error) => format!("error: {error}"),
            };
            if let Some(id) = id {
                service.finish(id, result);
            }
            service.inbox_running.store(false, Ordering::Release);
        });
    }

    fn start_discover(self: &Arc<Self>, config: &FederationConfig, config_path: PathBuf) {
        if self.discover_running.swap(true, Ordering::AcqRel) {
            return;
        }
        self.last_discover
            .store(Timestamp::now().seconds(), Ordering::Release);
        let service = Arc::clone(self);
        let config = config.clone();
        tokio::task::spawn_blocking(move || {
            let id = service.begin(RunKind::Discover);
            let result = match discover(&service.store, &config, config_path) {
                Ok(outcomes) => {
                    let learned = outcomes
                        .iter()
                        .filter(|outcome| {
                            matches!(
                                outcome,
                                DiscoverOutcome::Learned { .. } | DiscoverOutcome::Relayed { .. }
                            )
                        })
                        .count();
                    format!("outcomes={} learned={learned}", outcomes.len())
                }
                Err(error) => format!("error: {error}"),
            };
            service.finish(id, result);
            service.discover_running.store(false, Ordering::Release);
        });
    }

    fn store_inbox_len(&self) -> usize {
        use kairo_store::InboxStore;
        self.store
            .list_inbox()
            .map(|entries| entries.len())
            .unwrap_or(0)
    }

    /// One scheduler step. Returns how long to wait before the next.
    pub fn tick(self: &Arc<Self>) -> Duration {
        let (config, config_path) = match self.config() {
            Ok(loaded) => loaded,
            Err(error) => {
                tracing::warn!(%error, "federation sync skipped: the config doesn't load");
                return Duration::from_secs(60);
            }
        };
        let every = Duration::from_secs(config.sync.inbox_secs.min(config.sync.interval_secs));
        if !config.enabled {
            return every;
        }
        let now = Timestamp::now().seconds();
        let inbox_due = self
            .last_inbox
            .load(Ordering::Acquire)
            .saturating_add(i64::try_from(config.sync.inbox_secs).unwrap_or(i64::MAX))
            <= now;
        if inbox_due {
            self.start_inbox(&config, config_path.clone());
        }
        let has_sources = !config.peers.is_empty() || !config.bootstrap.is_empty();
        let discover_due = self
            .last_discover
            .load(Ordering::Acquire)
            .saturating_add(i64::try_from(config.sync.interval_secs).unwrap_or(i64::MAX))
            <= now;
        if has_sources && discover_due {
            self.start_discover(&config, config_path);
        }
        let follows = match self.store.list_follows() {
            Ok(follows) => follows,
            Err(error) => {
                tracing::warn!(%error, "could not read follows");
                return every;
            }
        };
        let interval = i64::try_from(config.sync.interval_secs).unwrap_or(i64::MAX);
        for record in follows {
            let due = record
                .last_attempt
                .as_ref()
                .is_none_or(|attempt| attempt.at.seconds().saturating_add(interval) <= now);
            if due && self.start(&record.follow, &config).is_none() {
                break;
            }
        }
        every
    }

    /// Run until `stop` turns true.
    pub async fn run(self: Arc<Self>, mut stop: tokio::sync::watch::Receiver<bool>) {
        loop {
            let wait = self.tick();
            tokio::select! {
                () = tokio::time::sleep(wait) => {}
                changed = stop.changed() => {
                    if changed.is_err() || *stop.borrow() {
                        return;
                    }
                }
            }
        }
    }

    /// `POST /api/v1/federation/sync`: start every follow, or `only`.
    pub fn trigger(self: &Arc<Self>, only: Option<&Follow>) -> Result<Triggered, TriggerError> {
        let (config, config_path) = self.config().map_err(TriggerError::Unavailable)?;
        if !config.enabled {
            return Err(TriggerError::Disabled(format!(
                "federation is disabled ([federation] enabled = false in {})",
                config_path.display()
            )));
        }
        let follows = self
            .store
            .list_follows()
            .map_err(|error| TriggerError::Unavailable(error.to_string()))?;
        let mut targets: Vec<Follow> = follows.into_iter().map(|record| record.follow).collect();
        if let Some(only) = only {
            if !targets.contains(only) {
                return Err(TriggerError::NotFollowed(only.clone()));
            }
            targets.retain(|follow| follow == only);
        }
        let mut triggered = Triggered::default();
        for follow in targets {
            match self.start(&follow, &config) {
                Some(true) => triggered.started.push(follow),
                Some(false) => triggered.already_running.push(follow),
                None => triggered.deferred.push(follow),
            }
        }
        Ok(triggered)
    }
}
