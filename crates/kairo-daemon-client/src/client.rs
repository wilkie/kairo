//! HTTP-over-Unix-socket client for the Kairo daemon.
//!
//! Each request opens a fresh `connect(2)` and runs an HTTP/1
//! handshake; there is no connection pool yet. That's a fine
//! default for the read-only daemon under expected load — a CLI
//! invocation makes one or two requests per dispatch — and keeps
//! the lifetime story trivially simple. If pooling becomes
//! load-bearing (e.g., a long-lived web server doing many
//! requests), it lands as a follow-up; the public API on
//! [`Client`] is shaped so pooling can be added without changing
//! call sites.

use std::path::{Path, PathBuf};
use std::time::Duration;

use http_body_util::{BodyExt, Empty};
use hyper::body::Bytes;
use hyper::Request;
use hyper_util::rt::TokioIo;
use serde::de::DeserializeOwned;
use tokio::net::UnixStream;
use tokio::time::timeout;

use crate::dto::{StatusInfo, VersionInfo};
use crate::envelope::{decode_error, decode_success};
use crate::error::{ClientError, ClientResult};

/// Default timeout used by [`Client::probe`] when the caller
/// passes `None`.
pub const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_millis(500);

/// Connect-phase timeout used by request methods other than
/// `probe`. Bounded so a hung accept queue can't wedge the CLI.
const REQUEST_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// Whole-request timeout (connect + handshake + response). Long
/// enough for modest blob streaming; future streaming methods
/// will use a different deadline (or none).
const REQUEST_DEADLINE: Duration = Duration::from_secs(30);

/// A handle bound to a daemon's listening Unix socket.
///
/// Cheap to clone (just a `PathBuf`). Connections are lazy — the
/// client does not contact the daemon until a method is called.
#[derive(Debug, Clone)]
pub struct Client {
    socket_path: PathBuf,
}

impl Client {
    /// Construct a client targeting the daemon listening on
    /// `socket_path` (typically `<store>/daemon.sock`). Does not
    /// connect.
    pub fn new<P: AsRef<Path>>(socket_path: P) -> Self {
        Self {
            socket_path: socket_path.as_ref().to_path_buf(),
        }
    }

    /// Filesystem path of the daemon's listening socket.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Quick liveness probe: try `GET /api/v1/status`, returning
    /// `true` iff the daemon answers with a 2xx within
    /// `timeout_dur` (or `DEFAULT_PROBE_TIMEOUT` when `None`).
    ///
    /// Used by the CLI's probe-and-fall-back dispatch (see
    /// `specs/CLI.md` §3.3): missing socket → fall back to
    /// direct mode silently. Any error — connect refused, hang,
    /// non-2xx, malformed body — counts as "no daemon".
    pub async fn probe(&self, timeout_dur: Option<Duration>) -> bool {
        let deadline = timeout_dur.unwrap_or(DEFAULT_PROBE_TIMEOUT);
        matches!(timeout(deadline, self.do_probe()).await, Ok(Ok(())))
    }

    async fn do_probe(&self) -> ClientResult<()> {
        // Discard the response body — probe only cares about 2xx.
        let _: StatusInfo = self.get_json("/api/v1/status").await?;
        Ok(())
    }

    /// `GET /api/v1/version`.
    pub async fn version(&self) -> ClientResult<VersionInfo> {
        self.get_json("/api/v1/version").await
    }

    /// `GET /api/v1/status`.
    pub async fn status(&self) -> ClientResult<StatusInfo> {
        self.get_json("/api/v1/status").await
    }

    async fn get_json<T: DeserializeOwned>(&self, path: &str) -> ClientResult<T> {
        let path = path.to_owned();
        let work = async move {
            let (status, bytes) = self.send_get(&path).await?;
            if (200..300).contains(&status) {
                decode_success(&bytes)
            } else {
                Err(decode_error(status, &bytes))
            }
        };

        match timeout(REQUEST_DEADLINE, work).await {
            Ok(result) => result,
            Err(_) => Err(ClientError::Timeout(REQUEST_DEADLINE)),
        }
    }

    async fn send_get(&self, path: &str) -> ClientResult<(u16, Bytes)> {
        let stream = match timeout(
            REQUEST_CONNECT_TIMEOUT,
            UnixStream::connect(&self.socket_path),
        )
        .await
        {
            Ok(Ok(stream)) => stream,
            Ok(Err(error)) => return Err(ClientError::Connect(error)),
            Err(_) => return Err(ClientError::Timeout(REQUEST_CONNECT_TIMEOUT)),
        };
        let io = TokioIo::new(stream);

        let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
            .await
            .map_err(|error| ClientError::Transport(Box::new(error)))?;

        let conn_task = tokio::spawn(async move {
            // Connection task drains the underlying stream while
            // the request is in flight. Its return value is the
            // hyper connection error (if any); we surface those
            // through the request path, not here.
            let _ = conn.await;
        });

        let req = Request::builder()
            .method("GET")
            .uri(path)
            .header("host", "daemon")
            .body(Empty::<Bytes>::new())
            .map_err(|error| ClientError::Transport(Box::new(error)))?;

        let response = sender
            .send_request(req)
            .await
            .map_err(|error| ClientError::Transport(Box::new(error)))?;
        let status = response.status().as_u16();
        let body = response
            .into_body()
            .collect()
            .await
            .map_err(|error| ClientError::Transport(Box::new(error)))?
            .to_bytes();

        // Drop sender so the connection task notices the half-close
        // and finishes; await it briefly so any tail error on the
        // wire is observed before the function returns.
        drop(sender);
        let _ = conn_task.await;

        Ok((status, body))
    }
}
