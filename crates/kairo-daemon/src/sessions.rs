//! In-memory session machinery for the daemon's actor-attributed
//! write path (Phase 2 §5 auth slice, `specs/PHASE_2_WEB_CLIENT_AUTH.md`).
//!
//! A session binds a [`SessionId`] (a 256-bit random handle the
//! browser carries in a cookie) to an [`ActorId`] for the duration
//! of an interactive sign-in. Sessions are *not* the daemon's
//! authoritative authorization — capability statements still are.
//! They are an authentication front end that produces the same
//! `actor_id` value as the existing `Authorization: KairoActor1`
//! request signature, allowing future write endpoints to dispatch
//! identically against either auth mode (see
//! `PHASE_2_WEB_CLIENT_AUTH.md` §2.3).
//!
//! Sessions live entirely in memory; daemon restart clears the
//! table. Persistence is an explicit deferral (the encrypted-
//! cookie approach in `PHASE_2_WEB_CLIENT_AUTH.md` §8).
//!
//! The [`Clock`] trait is intentionally tiny — tests use a
//! controllable clock to exercise the idle / absolute timeout
//! paths without sleeping.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use base64::Engine;
use kairo_core::{ActorId, Timestamp};

/// Length in bytes of a freshly-generated session id.
///
/// 32 bytes / 256 bits matches the cookie-session conventions in
/// `PHASE_2_WEB_CLIENT_AUTH.md` §2.1. Base64url-no-pad encodes 32
/// bytes as exactly 43 characters.
const SESSION_ID_BYTES: usize = 32;
const SESSION_ID_BASE64_LEN: usize = 43;

/// Default idle timeout: 30 minutes.
pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// Default absolute timeout: 24 hours.
pub const DEFAULT_ABSOLUTE_TIMEOUT: Duration = Duration::from_secs(24 * 60 * 60);

/// Tuning knobs for the session store.
///
/// `idle_timeout` is the sliding deadline — every authenticated
/// touch resets `last_used_at`, and the session is evicted if it
/// goes unused for this long. `absolute_timeout` is the hard cap
/// from `created_at`; a session is never valid past it regardless
/// of activity.
#[derive(Debug, Clone, Copy)]
pub struct SessionConfig {
    pub idle_timeout: Duration,
    pub absolute_timeout: Duration,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
            absolute_timeout: DEFAULT_ABSOLUTE_TIMEOUT,
        }
    }
}

/// Opaque cookie-borne session handle.
///
/// 43-character base64url-no-pad string of 32 random bytes.
/// `parse` rejects strings of any other length or with characters
/// outside the URL-safe base64 alphabet, so callers can use
/// `SessionId::parse` as a cheap cookie-shape gate before any
/// store lookup.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(String);

impl SessionId {
    /// Generate a fresh session id from system entropy.
    ///
    /// Returns an error only if the platform CSPRNG is
    /// unavailable, which is fatal for the daemon's session
    /// surface. Callers map this to `ApiError::internal`.
    pub fn generate() -> Result<Self, getrandom::Error> {
        let mut bytes = [0u8; SESSION_ID_BYTES];
        getrandom::getrandom(&mut bytes)?;
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
        debug_assert_eq!(encoded.len(), SESSION_ID_BASE64_LEN);
        Ok(Self(encoded))
    }

    /// Validate and wrap a cookie value into a `SessionId`.
    ///
    /// Returns `None` for any input that isn't exactly 43
    /// base64url-no-pad characters decoding to 32 bytes. Callers
    /// should treat a `None` result as "no valid session cookie
    /// present" and continue as anonymous.
    pub fn parse(s: &str) -> Option<Self> {
        if s.len() != SESSION_ID_BASE64_LEN {
            return None;
        }
        let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(s.as_bytes())
            .ok()?;
        if decoded.len() != SESSION_ID_BYTES {
            return None;
        }
        Some(Self(s.to_owned()))
    }

    /// Borrow the cookie-form string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Live session record.
///
/// Cloned out of the store on every lookup; the store owns the
/// canonical copy. The fields are designed to be ready for the
/// future AEAD-encrypted-cookie variant — every authorization
/// decision a future write handler needs is here.
#[derive(Debug, Clone)]
pub struct Session {
    pub actor_id: ActorId,
    /// When the session was opened (`POST /api/v1/sessions`).
    pub created_at: Timestamp,
    /// Last authenticated touch. Drives the sliding idle deadline.
    pub last_used_at: Timestamp,
    /// Absolute hard cap: `created_at + absolute_timeout`.
    pub expires_at: Timestamp,
}

impl Session {
    /// Sliding idle deadline derived from `last_used_at`.
    pub fn idle_expires_at(&self, idle_timeout: Duration) -> Timestamp {
        Timestamp::from_seconds(self.last_used_at.seconds() + idle_secs(idle_timeout))
    }

    /// `true` if `now` is past either the idle or the absolute
    /// deadline. Boundary values count as expired (`>=`), so a
    /// session created exactly `absolute_timeout` seconds ago is
    /// rejected.
    pub fn is_expired_at(&self, now: Timestamp, idle_timeout: Duration) -> bool {
        now.seconds() >= self.expires_at.seconds()
            || now.seconds() >= self.last_used_at.seconds() + idle_secs(idle_timeout)
    }
}

fn idle_secs(idle_timeout: Duration) -> i64 {
    i64::try_from(idle_timeout.as_secs()).unwrap_or(i64::MAX)
}

fn absolute_secs(absolute_timeout: Duration) -> i64 {
    i64::try_from(absolute_timeout.as_secs()).unwrap_or(i64::MAX)
}

/// Time source for the session store. Production uses
/// [`SystemClock`]; tests substitute a controllable clock.
pub trait Clock: Send + Sync + 'static {
    fn now(&self) -> Timestamp;
}

/// Real wall-clock time via `Timestamp::now`.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

/// In-memory session table.
///
/// Cheap to wrap in `Arc` and share across handlers. The single
/// `Mutex` is acceptable because lookups are pure-memory `HashMap`
/// operations measured in microseconds; the daemon's load is
/// dominated by blocking store calls on `spawn_blocking`.
///
/// Eviction is lazy on every lookup/touch — a session that goes
/// untouched past either deadline is removed on the next request
/// that names it. [`purge_expired`](Self::purge_expired) is
/// exposed for tests that want to drive eviction explicitly.
pub struct SessionStore {
    config: SessionConfig,
    clock: Box<dyn Clock>,
    table: Mutex<HashMap<SessionId, Session>>,
}

impl std::fmt::Debug for SessionStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionStore")
            .field("config", &self.config)
            .field("entries", &self.len())
            .finish()
    }
}

impl SessionStore {
    /// Construct a store backed by the system clock.
    pub fn new(config: SessionConfig) -> Self {
        Self::with_clock(config, Box::new(SystemClock))
    }

    /// Construct a store with a caller-supplied clock. Used by
    /// tests to drive expiry deterministically.
    pub fn with_clock(config: SessionConfig, clock: Box<dyn Clock>) -> Self {
        Self {
            config,
            clock,
            table: Mutex::new(HashMap::new()),
        }
    }

    pub fn config(&self) -> SessionConfig {
        self.config
    }

    /// Lock the table, recovering from poisoning. A `PoisonError`
    /// only fires if some prior holder panicked while mutating —
    /// the data itself is still consistent (every mutation in
    /// this module is panic-safe), so we recover into the inner
    /// guard rather than panic onward.
    fn locked(&self) -> MutexGuard<'_, HashMap<SessionId, Session>> {
        self.table
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Create a fresh session bound to `actor_id`. Returns the
    /// generated id alongside the live record so the handler can
    /// set both the cookie and the response body.
    ///
    /// Errors only if the platform CSPRNG can't supply 32 random
    /// bytes — fatal for the session surface; surface to the
    /// caller as a 500.
    pub fn create(&self, actor_id: ActorId) -> Result<(SessionId, Session), getrandom::Error> {
        let now = self.clock.now();
        let session = Session {
            actor_id,
            created_at: now,
            last_used_at: now,
            expires_at: Timestamp::from_seconds(
                now.seconds() + absolute_secs(self.config.absolute_timeout),
            ),
        };
        let mut table = self.locked();
        let id = loop {
            let candidate = SessionId::generate()?;
            if !table.contains_key(&candidate) {
                break candidate;
            }
            // 256 bits of entropy makes a collision astronomically
            // unlikely; the loop exists purely so generation is
            // total rather than panicking on the impossible.
        };
        table.insert(id.clone(), session.clone());
        Ok((id, session))
    }

    /// Look up a session and, if live, advance its `last_used_at`.
    /// Returns the updated record. Expired sessions are evicted
    /// and `None` is returned.
    pub fn touch(&self, id: &SessionId) -> Option<Session> {
        let now = self.clock.now();
        let mut table = self.locked();
        let stored = table.get(id)?.clone();
        if stored.is_expired_at(now, self.config.idle_timeout) {
            table.remove(id);
            return None;
        }
        let mut updated = stored;
        updated.last_used_at = now;
        table.insert(id.clone(), updated.clone());
        Some(updated)
    }

    /// Non-mutating lookup. Still lazy-evicts expired records.
    pub fn lookup(&self, id: &SessionId) -> Option<Session> {
        let now = self.clock.now();
        let mut table = self.locked();
        let stored = table.get(id)?.clone();
        if stored.is_expired_at(now, self.config.idle_timeout) {
            table.remove(id);
            return None;
        }
        Some(stored)
    }

    /// Remove a session. Idempotent — returns the prior record if
    /// one was present.
    pub fn delete(&self, id: &SessionId) -> Option<Session> {
        self.locked().remove(id)
    }

    /// Sweep every entry, evicting any that have expired against
    /// the current clock. Lookups already evict lazily; this is
    /// here for tests and a future periodic-sweep follow-up.
    pub fn purge_expired(&self) {
        let now = self.clock.now();
        let idle_timeout = self.config.idle_timeout;
        self.locked()
            .retain(|_, session| !session.is_expired_at(now, idle_timeout));
    }

    /// Number of entries currently in the table. Exposed for
    /// tests; production code does not need to inspect this.
    pub fn len(&self) -> usize {
        self.locked().len()
    }

    /// `true` iff the table holds no sessions.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic)]

    use super::*;
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::sync::Arc;

    struct FakeClock(Arc<AtomicI64>);

    impl FakeClock {
        fn new(seconds: i64) -> (Self, Arc<AtomicI64>) {
            let handle = Arc::new(AtomicI64::new(seconds));
            (Self(handle.clone()), handle)
        }
    }

    impl Clock for FakeClock {
        fn now(&self) -> Timestamp {
            Timestamp::from_seconds(self.0.load(Ordering::SeqCst))
        }
    }

    fn fixed_actor() -> ActorId {
        ActorId::from_sha256_digest([0x42; 32])
    }

    fn config_with(idle_secs: u64, absolute_secs: u64) -> SessionConfig {
        SessionConfig {
            idle_timeout: Duration::from_secs(idle_secs),
            absolute_timeout: Duration::from_secs(absolute_secs),
        }
    }

    #[test]
    fn session_id_round_trips_through_parse() {
        let id = SessionId::generate().expect("generate id");
        let parsed = SessionId::parse(id.as_str()).expect("parse");
        assert_eq!(parsed, id);
    }

    #[test]
    fn session_id_parse_rejects_wrong_length() {
        assert!(SessionId::parse("").is_none());
        assert!(SessionId::parse("too-short").is_none());
        assert!(SessionId::parse(&"A".repeat(42)).is_none());
        assert!(SessionId::parse(&"A".repeat(44)).is_none());
    }

    #[test]
    fn session_id_parse_rejects_non_base64url() {
        // base64url uses '-' and '_' in place of '+' and '/'; a
        // '+' is not in the alphabet.
        let bogus = format!("{}+", "A".repeat(42));
        assert_eq!(bogus.len(), 43);
        assert!(SessionId::parse(&bogus).is_none());
    }

    #[test]
    fn session_id_generate_is_unique() {
        let a = SessionId::generate().expect("generate id");
        let b = SessionId::generate().expect("generate id");
        assert_ne!(a, b);
        assert_eq!(a.as_str().len(), SESSION_ID_BASE64_LEN);
    }

    #[test]
    fn create_and_lookup_round_trip() {
        let store = SessionStore::new(SessionConfig::default());
        let (id, session) = store.create(fixed_actor()).expect("create session");
        assert_eq!(store.len(), 1);
        let looked_up = store.lookup(&id).expect("lookup");
        assert_eq!(looked_up.actor_id, session.actor_id);
        assert_eq!(looked_up.created_at, session.created_at);
        assert_eq!(looked_up.expires_at, session.expires_at);
    }

    #[test]
    fn delete_removes_session() {
        let store = SessionStore::new(SessionConfig::default());
        let (id, _) = store.create(fixed_actor()).expect("create session");
        let removed = store.delete(&id);
        assert!(removed.is_some());
        assert!(store.lookup(&id).is_none());
        // Idempotent.
        assert!(store.delete(&id).is_none());
    }

    #[test]
    fn touch_advances_last_used_at() {
        let (clock, ticker) = FakeClock::new(1_000_000);
        let store = SessionStore::with_clock(config_with(60, 3600), Box::new(clock));
        let (id, session) = store.create(fixed_actor()).expect("create session");
        assert_eq!(session.last_used_at.seconds(), 1_000_000);

        ticker.store(1_000_030, Ordering::SeqCst);
        let touched = store.touch(&id).expect("touch");
        assert_eq!(touched.last_used_at.seconds(), 1_000_030);
        assert_eq!(touched.created_at, session.created_at);
    }

    #[test]
    fn idle_timeout_evicts_session() {
        let (clock, ticker) = FakeClock::new(1_000_000);
        let store = SessionStore::with_clock(config_with(60, 3600), Box::new(clock));
        let (id, _) = store.create(fixed_actor()).expect("create session");

        // 59 seconds elapsed: still alive.
        ticker.store(1_000_059, Ordering::SeqCst);
        assert!(store.lookup(&id).is_some());

        // 60 seconds elapsed: idle deadline hit.
        ticker.store(1_000_060, Ordering::SeqCst);
        assert!(store.lookup(&id).is_none());
        assert_eq!(store.len(), 0, "expired session should be evicted");
    }

    #[test]
    fn touch_resets_the_idle_deadline() {
        let (clock, ticker) = FakeClock::new(1_000_000);
        let store = SessionStore::with_clock(config_with(60, 3600), Box::new(clock));
        let (id, _) = store.create(fixed_actor()).expect("create session");

        ticker.store(1_000_030, Ordering::SeqCst);
        assert!(store.touch(&id).is_some());

        // Without the touch, this would expire at 1_000_060; the
        // touch pushes the deadline to 1_000_090.
        ticker.store(1_000_089, Ordering::SeqCst);
        assert!(store.lookup(&id).is_some());

        ticker.store(1_000_090, Ordering::SeqCst);
        assert!(store.lookup(&id).is_none());
    }

    #[test]
    fn absolute_timeout_evicts_even_with_recent_touch() {
        let (clock, ticker) = FakeClock::new(1_000_000);
        let store = SessionStore::with_clock(config_with(60, 100), Box::new(clock));
        let (id, _) = store.create(fixed_actor()).expect("create session");

        // Touch at +50s and again at +99s — both before the
        // absolute deadline at +100s, both inside the sliding
        // idle window (last_used_at resets each touch).
        for &offset in &[50, 99] {
            ticker.store(1_000_000 + offset, Ordering::SeqCst);
            assert!(store.touch(&id).is_some(), "touch at +{offset}s");
        }

        // +100s: absolute deadline hit, even though the most
        // recent touch was 1 second ago.
        ticker.store(1_000_100, Ordering::SeqCst);
        assert!(store.lookup(&id).is_none());
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn purge_expired_clears_only_expired() {
        let (clock, ticker) = FakeClock::new(1_000_000);
        let store = SessionStore::with_clock(config_with(60, 3600), Box::new(clock));
        let (alive_id, _) = store.create(fixed_actor()).expect("create session");
        let (expired_id, _) = store.create(fixed_actor()).expect("create session");
        assert_eq!(store.len(), 2);

        // Touch only the survivor at 30s.
        ticker.store(1_000_030, Ordering::SeqCst);
        assert!(store.touch(&alive_id).is_some());

        // Jump past the original idle deadline; the expired
        // session's last_used_at is still 1_000_000, the alive
        // one's is 1_000_030.
        ticker.store(1_000_070, Ordering::SeqCst);
        store.purge_expired();

        assert_eq!(store.len(), 1);
        assert!(store.lookup(&alive_id).is_some());
        assert!(store.lookup(&expired_id).is_none());
    }

    #[test]
    fn idle_expires_at_reflects_touch() {
        let (clock, ticker) = FakeClock::new(1_000_000);
        let store = SessionStore::with_clock(config_with(60, 3600), Box::new(clock));
        let (id, session) = store.create(fixed_actor()).expect("create session");
        assert_eq!(
            session.idle_expires_at(Duration::from_secs(60)).seconds(),
            1_000_060
        );

        ticker.store(1_000_030, Ordering::SeqCst);
        let touched = store.touch(&id).expect("touch");
        assert_eq!(
            touched.idle_expires_at(Duration::from_secs(60)).seconds(),
            1_000_090
        );
    }

    #[test]
    fn concurrent_create_and_lookup_is_consistent() {
        use std::sync::Arc;
        use std::thread;

        let store = Arc::new(SessionStore::new(SessionConfig::default()));
        let mut handles = Vec::new();
        for _ in 0..16 {
            let store = store.clone();
            handles.push(thread::spawn(move || {
                let mut ids = Vec::new();
                for _ in 0..32 {
                    let (id, _) = store.create(fixed_actor()).expect("create session");
                    ids.push(id);
                }
                for id in &ids {
                    assert!(store.lookup(id).is_some());
                }
                for id in ids {
                    assert!(store.delete(&id).is_some());
                }
            }));
        }
        for handle in handles {
            handle.join().expect("join");
        }
        assert_eq!(store.len(), 0);
    }
}
