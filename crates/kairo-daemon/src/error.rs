//! Top-level daemon errors. Returned from [`crate::serve`] and
//! [`crate::serve_with_shutdown`] when startup or shutdown fails.
//!
//! Variants land alongside the code that produces them; each is
//! exercised by at least one test.

use std::error::Error as StdError;
use std::fmt;
use std::path::PathBuf;

#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// `FilesystemStore::open(path)` failed. Underlying cause is
    /// the `kairo_store::StoreError`.
    OpenStore {
        path: PathBuf,
        source: Box<dyn StdError + Send + Sync>,
    },
    /// Another daemon already holds the listening socket — the
    /// double-start probe found a live peer (or a probe timeout,
    /// which is treated conservatively as live).
    AlreadyRunning { socket: PathBuf },
    /// I/O failure under the socket path: bind, chmod, unlink of
    /// stale file, or metadata read.
    SocketIo {
        path: PathBuf,
        source: std::io::Error,
    },
    /// I/O failure writing or removing the PID file (atomic
    /// rename, write, or removal).
    PidIo {
        path: PathBuf,
        source: std::io::Error,
    },
    /// `axum::serve` returned an error during the request loop.
    Serve {
        source: Box<dyn StdError + Send + Sync>,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OpenStore { path, source } => {
                write!(f, "failed to open store at {}: {source}", path.display())
            }
            Self::AlreadyRunning { socket } => write!(
                f,
                "another daemon is listening at {}; refusing to start a second one",
                socket.display()
            ),
            Self::SocketIo { path, source } => {
                write!(f, "socket I/O at {} failed: {source}", path.display())
            }
            Self::PidIo { path, source } => {
                write!(f, "PID file I/O at {} failed: {source}", path.display())
            }
            Self::Serve { source } => write!(f, "server loop failed: {source}"),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::OpenStore { source, .. } | Self::Serve { source, .. } => Some(source.as_ref()),
            Self::SocketIo { source, .. } | Self::PidIo { source, .. } => Some(source),
            Self::AlreadyRunning { .. } => None,
        }
    }
}
