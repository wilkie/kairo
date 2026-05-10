//! Transparent reverse proxy from the public TCP surface to the
//! daemon's Unix socket.
//!
//! v1 ships a *bytes-level* passthrough rather than typed routes
//! because the daemon's API is the single source of truth — every
//! shipped read endpoint, every future endpoint, and the
//! `/api/v1/openapi.json` doc itself, all flow through one
//! handler. Each browser request opens a fresh `connect(2)` to
//! `<daemon>.sock`, replays method / path / query / headers /
//! body, and streams the response back.
//!
//! What this proxy does *not* do:
//!
//! - Inspect or rewrite payloads. The web client consumes the
//!   daemon's wire shapes verbatim; the typed `kairo-daemon-client`
//!   crate is for Rust callers, not for kairo-web.
//! - Authenticate. v1 trust model is loopback-only (DECISIONS.md
//!   §12.3); browser-side auth lands post-v1.
//! - Pool connections. One TCP request = one Unix-socket connect.
//!   Modest overhead at v1 traffic levels; pooling is a follow-up
//!   if it becomes load-bearing.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, Response, StatusCode};
use axum::response::IntoResponse;
use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper_util::rt::TokioIo;
use tokio::net::UnixStream;

/// Shared state for the proxy handler. Cheap to clone (just an
/// `Arc<PathBuf>`).
#[derive(Debug, Clone)]
pub(crate) struct ProxyState {
    pub(crate) daemon_socket: Arc<PathBuf>,
}

impl ProxyState {
    pub(crate) fn new(daemon_socket: PathBuf) -> Self {
        Self {
            daemon_socket: Arc::new(daemon_socket),
        }
    }
}

/// Proxy handler. Mounted under `/api/v1/*path`. Forwards the
/// browser's request to the daemon's Unix socket and streams the
/// response back.
pub(crate) async fn handler(
    State(state): State<ProxyState>,
    request: Request<Body>,
) -> Result<Response<Body>, ProxyError> {
    let socket_path = Arc::clone(&state.daemon_socket);
    forward(socket_path.as_path(), request).await
}

/// Errors produced by the proxy handler. Mapped to HTTP
/// responses by the `IntoResponse` impl so axum can render them
/// without an `unwrap` panic in the call site.
#[derive(Debug)]
pub(crate) enum ProxyError {
    /// Could not `connect(2)` to the daemon socket. Most often
    /// the daemon isn't running.
    Connect {
        socket: PathBuf,
        source: std::io::Error,
    },
    /// HTTP/1 handshake on the connected socket failed.
    Handshake { source: hyper::Error },
    /// Forwarding the request to the daemon failed.
    SendRequest { source: hyper::Error },
    /// Reading the request body off the browser-side connection
    /// failed.
    BrowserBody { source: axum::Error },
    /// Building the daemon-bound request failed (e.g., invalid
    /// header name from a malicious client).
    BuildRequest { source: hyper::http::Error },
}

impl std::fmt::Display for ProxyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connect { socket, source } => {
                write!(
                    f,
                    "could not connect to daemon socket {}: {source}",
                    socket.display()
                )
            }
            Self::Handshake { source } => write!(f, "daemon HTTP handshake failed: {source}"),
            Self::SendRequest { source } => write!(f, "daemon request failed: {source}"),
            Self::BrowserBody { source } => {
                write!(f, "could not read browser request body: {source}")
            }
            Self::BuildRequest { source } => {
                write!(f, "could not build daemon-bound request: {source}")
            }
        }
    }
}

impl std::error::Error for ProxyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Connect { source, .. } => Some(source),
            Self::Handshake { source } | Self::SendRequest { source } => Some(source),
            Self::BrowserBody { source } => Some(source),
            Self::BuildRequest { source } => Some(source),
        }
    }
}

impl IntoResponse for ProxyError {
    fn into_response(self) -> Response<Body> {
        let (status, message) = match &self {
            Self::Connect { .. } | Self::Handshake { .. } | Self::SendRequest { .. } => {
                (StatusCode::BAD_GATEWAY, self.to_string())
            }
            Self::BrowserBody { .. } | Self::BuildRequest { .. } => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
        };
        // JSON envelope mirrors the daemon's error shape so
        // browser clients have one error format to handle.
        let body = format!(
            r#"{{"ok":false,"schema":"kairo.api.error.v1","error":{{"code":"web_proxy_error","message":{}}}}}"#,
            json_string(&message)
        );
        let mut resp = Response::new(Body::from(body));
        *resp.status_mut() = status;
        resp.headers_mut().insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("application/json"),
        );
        resp
    }
}

/// JSON-string-quote a value without pulling in `serde_json` for
/// a single error-path string. Always quoted; escapes `"`, `\`,
/// and control characters per RFC 8259 §7.
fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

async fn forward(socket_path: &Path, request: Request<Body>) -> Result<Response<Body>, ProxyError> {
    let (parts, body) = request.into_parts();

    // Drain the browser's body into bytes before opening the
    // daemon connection. v1 endpoints are GET-only so this is
    // typically empty; future write paths would benefit from
    // stream-through, but the current daemon doesn't accept
    // bodies anyway.
    let body_bytes = body
        .collect()
        .await
        .map_err(|source| ProxyError::BrowserBody { source })?
        .to_bytes();

    // Open a fresh Unix connection per request.
    let stream = UnixStream::connect(socket_path)
        .await
        .map_err(|source| ProxyError::Connect {
            socket: socket_path.to_path_buf(),
            source,
        })?;
    let io = TokioIo::new(stream);

    let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
        .await
        .map_err(|source| ProxyError::Handshake { source })?;

    let conn_task = tokio::spawn(async move {
        // Drives the underlying byte stream. The connection's
        // own error path is the request's error path; a tail
        // error here is informational at most.
        let _ = conn.await;
    });

    // Build the daemon-bound request. Path + query come from the
    // axum request; method and headers carry over verbatim except
    // `host`, which we set to a stub the daemon will accept (the
    // daemon doesn't honor virtual hosts).
    let path_and_query = parts
        .uri
        .path_and_query()
        .map(|pq| pq.as_str().to_owned())
        .unwrap_or_else(|| parts.uri.path().to_owned());

    let mut req_builder = Request::builder().method(parts.method).uri(path_and_query);

    if let Some(headers) = req_builder.headers_mut() {
        for (name, value) in &parts.headers {
            // Skip hop-by-hop headers per RFC 7230 §6.1.
            if is_hop_by_hop(name.as_str()) {
                continue;
            }
            headers.insert(name.clone(), value.clone());
        }
        // Force a host the daemon will accept; the original `Host:`
        // header points at the TCP front-end, which is irrelevant
        // to the daemon.
        headers.insert(
            axum::http::header::HOST,
            axum::http::HeaderValue::from_static("daemon"),
        );
    }

    let upstream_request = req_builder
        .body(http_body_util::Full::new(body_bytes))
        .map_err(|source| ProxyError::BuildRequest { source })?;

    let upstream_response: Response<Incoming> = sender
        .send_request(upstream_request)
        .await
        .map_err(|source| ProxyError::SendRequest { source })?;

    // Convert the upstream Incoming body into an axum-compatible
    // streaming Body so chunked responses (e.g., the blob endpoint)
    // pass through without buffering.
    let (resp_parts, incoming) = upstream_response.into_parts();
    let mut resp_builder = Response::builder().status(resp_parts.status);
    if let Some(headers) = resp_builder.headers_mut() {
        for (name, value) in &resp_parts.headers {
            if is_hop_by_hop(name.as_str()) {
                continue;
            }
            headers.insert(name.clone(), value.clone());
        }
    }
    let response = resp_builder
        .body(Body::new(incoming))
        .map_err(|source| ProxyError::BuildRequest { source })?;

    // Detach the connection task so the response body can drain
    // on its own timeline.
    drop(sender);
    tokio::spawn(async move {
        let _ = conn_task.await;
    });

    Ok(response)
}

/// RFC 7230 §6.1 hop-by-hop headers. These must not be forwarded
/// across a proxy hop. Compared case-insensitively because HTTP
/// header names are case-insensitive on the wire.
fn is_hop_by_hop(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailers"
            | "transfer-encoding"
            | "upgrade"
    )
}
