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
use axum::http::{HeaderValue, Method, Request, Response, StatusCode};
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
///
/// During the daemon write-path Phase A (DECISIONS.md §13), this
/// proxy refuses every method other than `GET` and `HEAD` with a
/// `405 Method Not Allowed`. The short-circuit happens before any
/// daemon connection is opened, so an XSS or DNS-rebind that
/// reaches the loopback port cannot tunnel a write through to the
/// daemon's unauthenticated Phase A write surface.
pub(crate) async fn handler(
    State(state): State<ProxyState>,
    request: Request<Body>,
) -> Result<Response<Body>, ProxyError> {
    if !is_phase_a_allowed_method(request.method()) {
        return Ok(method_not_allowed_response());
    }
    let socket_path = Arc::clone(&state.daemon_socket);
    forward(socket_path.as_path(), request).await
}

/// Phase A method allowlist. Exactly `GET` and `HEAD`. Pinned to
/// these two by `DECISIONS.md §13` sub-decision 5; widening this
/// set lifts the POST-block invariant.
fn is_phase_a_allowed_method(method: &Method) -> bool {
    method == Method::GET || method == Method::HEAD
}

/// Build the `405 Method Not Allowed` response returned for any
/// non-`GET`/`HEAD` method during Phase A.
fn method_not_allowed_response() -> Response<Body> {
    let body = concat!(
        "kairo-web rejects this method during the daemon write-path Phase A.\n",
        "\n",
        "The browser proxy forwards GET and HEAD only. The daemon's write\n",
        "surface is reachable over the local Unix socket; signed-request\n",
        "auth (Phase B) is the prerequisite for any browser write path.\n",
        "\n",
        "For local writes, use the CLI: e.g., `kairo trust grant`, `kairo\n",
        "revision create`, `kairo capability grant`.\n",
    );
    let mut resp = Response::new(Body::from(body));
    *resp.status_mut() = StatusCode::METHOD_NOT_ALLOWED;
    resp.headers_mut().insert(
        axum::http::header::ALLOW,
        HeaderValue::from_static("GET, HEAD"),
    );
    resp.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    resp
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
    // daemon connection. The Phase A allowlist (GET, HEAD) means
    // this is empty in practice; the buffer lives here so the
    // forwarder stays method-agnostic when Phase B widens the
    // allowlist.
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

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    //! Phase A POST-block invariant.
    //!
    //! These tests drive `handler` directly with a deliberately
    //! invalid daemon-socket path. A 405 short-circuit means the
    //! handler never tries to `connect(2)`; if the filter
    //! regressed, the bogus path would surface as a `Connect`
    //! error and the assertion below would fail loudly.
    //!
    //! When Phase B lifts the POST-block, the allowlist will
    //! widen — this test fixture pins `[GET, HEAD]` exactly so any
    //! widening must update the test.
    use super::*;
    use axum::extract::State;
    use http_body_util::BodyExt;
    use std::path::PathBuf;

    fn state() -> State<ProxyState> {
        State(ProxyState::new(PathBuf::from(
            "/nonexistent/phase-a-post-block.sock",
        )))
    }

    fn req(method: Method, path: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(path)
            .body(Body::empty())
            .expect("build req")
    }

    #[tokio::test]
    async fn allows_get() {
        // GET passes the filter and proceeds to `forward`, which
        // fails to connect to the bogus socket. That's the
        // happy-path signal: the filter did *not* short-circuit.
        let response = handler(state(), req(Method::GET, "/api/v1/version")).await;
        let err = response.expect_err("expected Connect error past the filter");
        assert!(matches!(err, ProxyError::Connect { .. }), "got {err:?}");
    }

    #[tokio::test]
    async fn allows_head() {
        let response = handler(state(), req(Method::HEAD, "/api/v1/version")).await;
        let err = response.expect_err("expected Connect error past the filter");
        assert!(matches!(err, ProxyError::Connect { .. }), "got {err:?}");
    }

    async fn assert_method_not_allowed(method: Method) {
        let response = handler(state(), req(method.clone(), "/api/v1/anything"))
            .await
            .unwrap_or_else(|err| panic!("{method} unexpectedly produced ProxyError: {err:?}"));

        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED, "{method}");
        assert_eq!(
            response.headers().get(axum::http::header::ALLOW).map(|v| v.to_str().unwrap_or("")),
            Some("GET, HEAD"),
            "{method}",
        );
        assert_eq!(
            response
                .headers()
                .get(axum::http::header::CONTENT_TYPE)
                .map(|v| v.to_str().unwrap_or("")),
            Some("text/plain; charset=utf-8"),
            "{method}",
        );

        let body_bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect body")
            .to_bytes();
        let body_text = std::str::from_utf8(&body_bytes).expect("utf-8 body");
        assert!(
            body_text.contains("Phase A"),
            "{method} body missing Phase A explanation: {body_text}",
        );
    }

    #[tokio::test]
    async fn blocks_post() {
        assert_method_not_allowed(Method::POST).await;
    }

    #[tokio::test]
    async fn blocks_put() {
        assert_method_not_allowed(Method::PUT).await;
    }

    #[tokio::test]
    async fn blocks_patch() {
        assert_method_not_allowed(Method::PATCH).await;
    }

    #[tokio::test]
    async fn blocks_delete() {
        assert_method_not_allowed(Method::DELETE).await;
    }

    #[tokio::test]
    async fn blocks_options() {
        assert_method_not_allowed(Method::OPTIONS).await;
    }
}
