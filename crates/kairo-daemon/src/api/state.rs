//! Application state shared across handlers.
//!
//! A single [`AppState`] is built at server startup, cloned into
//! every request via axum's `State` extractor. The contained
//! `Arc<FilesystemStore>` is the daemon's only handle to the
//! store; concurrent requests share it. Blocking store calls run
//! on `tokio::task::spawn_blocking`.
//!
//! The keystore handle is the daemon's only access to local
//! signing keys; today it powers `GET /api/v1/keystore/actors`
//! and the session-create check that the actor has a usable
//! key. Future write slices will use it to sign envelopes on
//! the browser's behalf (per `PHASE_2_WEB_CLIENT_AUTH.md` §2.3).

use std::path::PathBuf;
use std::sync::Arc;

use kairo_keystore::FilesystemKeystore;
use kairo_store::FilesystemStore;

use crate::federation::FederationService;
use crate::sessions::SessionStore;

/// Daemon-wide state built at startup.
#[derive(Debug, Clone)]
pub struct AppState {
    pub store: Arc<FilesystemStore>,
    pub store_path: PathBuf,
    pub keystore: Arc<FilesystemKeystore>,
    pub keys_path: PathBuf,
    pub sessions: Arc<SessionStore>,
    pub pid: u32,
    /// Background sync (`DECISIONS.md` §28); its scheduler loop runs
    /// only under [`crate::serve`].
    pub federation: Arc<FederationService>,
}
