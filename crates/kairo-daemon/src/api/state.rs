//! Application state shared across handlers.
//!
//! A single [`AppState`] is built at server startup, cloned into
//! every request via axum's `State` extractor. The contained
//! `Arc<FilesystemStore>` is the daemon's only handle to the
//! store; concurrent requests share it. Blocking store calls run
//! on `tokio::task::spawn_blocking`.

use std::path::PathBuf;
use std::sync::Arc;

use kairo_store::FilesystemStore;

/// Daemon-wide state built at startup.
#[derive(Debug, Clone)]
pub struct AppState {
    pub store: Arc<FilesystemStore>,
    pub store_path: PathBuf,
    pub pid: u32,
}
