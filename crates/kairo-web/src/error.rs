//! Top-level errors for the `kairo-web` server.
//!
//! Each variant lands alongside the code that produces it; bind /
//! socket / proxy paths surface their own diagnostics. The
//! `Display` and `source` impls follow the daemon's pattern so
//! call sites read the same way regardless of which process they
//! came from.

use std::error::Error as StdError;
use std::fmt;
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// `--bind` resolved to a non-loopback address. v1 binds
    /// 127.0.0.0/8 or ::1 only; remote exposure lands with the
    /// post-v1 bearer-token / TLS work (see DECISIONS.md §12.3).
    NonLoopbackBind { addr: SocketAddr },
    /// `--spa-dir` does not exist or is not a directory.
    SpaDirInvalid { path: PathBuf, reason: String },
    /// `--daemon-socket` does not exist when the server starts.
    /// The daemon-client connects lazily, so a missing socket only
    /// fails per-request — but we surface it once at startup so
    /// the operator sees the misconfiguration immediately.
    DaemonSocketMissing { path: PathBuf },
    /// `bind(2)` on the TCP listener failed.
    Bind {
        addr: SocketAddr,
        source: std::io::Error,
    },
    /// I/O failure writing or removing the PID file (atomic
    /// rename, write, or removal).
    PidIo {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The accept loop or graceful shutdown returned an error.
    Serve {
        source: Box<dyn StdError + Send + Sync>,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonLoopbackBind { addr } => write!(
                f,
                "kairo-web v1 binds loopback addresses only; refusing to bind {addr}",
            ),
            Self::SpaDirInvalid { path, reason } => {
                write!(f, "--spa-dir {} is not usable: {reason}", path.display())
            }
            Self::DaemonSocketMissing { path } => write!(
                f,
                "daemon socket {} does not exist; start `kairo daemon` first",
                path.display()
            ),
            Self::Bind { addr, source } => write!(f, "bind {addr} failed: {source}"),
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
            Self::Bind { source, .. } | Self::PidIo { source, .. } => Some(source),
            Self::Serve { source } => Some(source.as_ref()),
            Self::NonLoopbackBind { .. }
            | Self::SpaDirInvalid { .. }
            | Self::DaemonSocketMissing { .. } => None,
        }
    }
}
