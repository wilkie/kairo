//! `kairo-daemon-client` — Rust client for the Kairo daemon's
//! Unix-socket HTTP+JSON API.
//!
//! Slice 1 ships a [`Client`] type bound to a socket path and
//! nothing else. Slice 3 wires the actual hyper-over-Unix-socket
//! transport, `probe`, `version`, and `status` methods. Slice 5
//! onward adds the per-endpoint methods.
//!
//! The crate is the **only** sanctioned entry point to the daemon
//! from Rust callers (`kairo-cli` in v1; a future Rust web-server
//! impl post-v1). See `specs/DECISIONS.md` §11 for the trust
//! model and `specs/PHASE_2_DAEMON.md` for the slice plan.

use std::path::{Path, PathBuf};

/// A handle bound to a daemon's listening Unix socket.
///
/// Cheap to clone; the underlying connection pool lands with the
/// hyper-based transport in slice 3.
#[derive(Debug, Clone)]
pub struct Client {
    socket_path: PathBuf,
}

impl Client {
    /// Construct a client targeting the daemon listening on
    /// `socket_path`. Does not connect — `connect(2)` happens
    /// lazily on the first request (slice 3 onward).
    pub fn new<P: AsRef<Path>>(socket_path: P) -> Self {
        Self {
            socket_path: socket_path.as_ref().to_path_buf(),
        }
    }

    /// Filesystem path of the daemon's listening socket.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_records_socket_path() {
        let client = Client::new("/tmp/kairo-test.sock");
        assert_eq!(client.socket_path(), Path::new("/tmp/kairo-test.sock"));
    }

    #[test]
    fn client_is_clone() {
        let client = Client::new("/tmp/kairo-test.sock");
        let cloned = client.clone();
        assert_eq!(client.socket_path(), cloned.socket_path());
    }
}
