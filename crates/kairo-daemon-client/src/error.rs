//! Errors returned by [`crate::Client`] methods.
//!
//! The variants distinguish failure modes that callers (the CLI
//! today, a future Rust web-server later) need to act on
//! differently:
//!
//! - [`ClientError::Connect`] — the daemon's socket couldn't be
//!   reached. The CLI's probe-and-fall-back dispatch treats this
//!   as "no daemon" and falls back to direct mode.
//! - [`ClientError::Timeout`] — the request did not complete
//!   within the deadline. The CLI treats this as a transport
//!   problem rather than a daemon-up signal.
//! - [`ClientError::Http`] — the daemon answered with a non-2xx
//!   status carrying a structured error envelope. `code` is the
//!   wire-stable string from `specs/API.md` §8.
//! - [`ClientError::Decode`] — the response body wasn't JSON or
//!   didn't match the expected envelope shape.
//! - [`ClientError::Transport`] — hyper-level error (broken pipe,
//!   protocol failure mid-response, etc.).

use std::error::Error as StdError;
use std::fmt;

/// Result alias for [`Client`](crate::Client) methods.
pub type ClientResult<T> = Result<T, ClientError>;

#[derive(Debug)]
#[non_exhaustive]
pub enum ClientError {
    /// `connect(2)` to the daemon's Unix socket failed (no socket
    /// file, peer not listening, permission denied, etc.).
    Connect(std::io::Error),
    /// The request did not complete within the caller-supplied
    /// deadline. The `Duration` is the deadline that elapsed.
    Timeout(std::time::Duration),
    /// The daemon answered with an error envelope.
    Http {
        status: u16,
        code: String,
        message: String,
    },
    /// JSON or envelope shape decoding failed.
    Decode(String),
    /// Hyper or HTTP/1 transport error after the connection was
    /// established.
    Transport(Box<dyn StdError + Send + Sync>),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connect(error) => write!(f, "connect to daemon socket failed: {error}"),
            Self::Timeout(duration) => {
                write!(f, "request to daemon timed out after {duration:?}")
            }
            Self::Http {
                status,
                code,
                message,
            } => write!(f, "daemon returned HTTP {status} ({code}): {message}"),
            Self::Decode(message) => write!(f, "decoding daemon response failed: {message}"),
            Self::Transport(error) => write!(f, "transport error talking to daemon: {error}"),
        }
    }
}

impl StdError for ClientError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Connect(error) => Some(error),
            Self::Transport(error) => Some(error.as_ref()),
            Self::Timeout(_) | Self::Http { .. } | Self::Decode(_) => None,
        }
    }
}
