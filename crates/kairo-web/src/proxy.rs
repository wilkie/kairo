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
//! - Authenticate. Session cookies and `Authorization` headers
//!   pass through to the daemon, which is the only auth
//!   authority. The proxy's own guards (loopback `Host`, write
//!   allowlist, same-origin writes) are request-shape checks;
//!   see [`handler`] and `DECISIONS.md §15`.
//! - Pool connections. One TCP request = one Unix-socket connect.
//!   Modest overhead at v1 traffic levels; pooling is a follow-up
//!   if it becomes load-bearing.

use std::net::IpAddr;
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
/// Three request guards run before any daemon connection is
/// opened (`DECISIONS.md §15`):
///
/// 1. **Loopback `Host`.** Every request must name a loopback
///    host (`localhost`, `127.0.0.0/8`, `[::1]`); anything else is
///    a `403 web_forbidden_host`. This is the DNS-rebind defense:
///    a rebound page carries its own hostname in `Host`.
/// 2. **Method/path allowlist.** `GET` and `HEAD` pass on every
///    path. Writes pass only on the exact routes in
///    [`WRITE_ROUTES`]; any other method is a `405`.
/// 3. **Same-origin writes.** An allowlisted write must carry an
///    `Origin` equal to `http://<Host>`; otherwise `403
///    web_cross_origin`. `SameSite=Strict` alone doesn't cover
///    this — every `localhost` port is the same *site*.
///
/// The proxy does not authenticate: cookies and `Authorization`
/// headers pass through untouched and the daemon decides.
pub(crate) async fn handler(
    State(state): State<ProxyState>,
    request: Request<Body>,
) -> Result<Response<Body>, ProxyError> {
    let Some(host) = request_host(&request) else {
        return Ok(json_error_response(
            StatusCode::FORBIDDEN,
            "web_forbidden_host",
            "request carries no Host header; kairo-web serves loopback hosts only",
        ));
    };
    if !is_loopback_host(&host) {
        return Ok(json_error_response(
            StatusCode::FORBIDDEN,
            "web_forbidden_host",
            &format!("Host {host:?} is not a loopback host; kairo-web serves loopback hosts only"),
        ));
    }

    let method = request.method();
    if method != Method::GET && method != Method::HEAD {
        if !is_write_route(method, request.uri().path()) {
            return Ok(method_not_allowed_response(request.uri().path()));
        }
        if !is_same_origin(request.headers(), &host) {
            return Ok(json_error_response(
                StatusCode::FORBIDDEN,
                "web_cross_origin",
                &format!("write requests must carry Origin: http://{host}"),
            ));
        }
    }

    let socket_path = Arc::clone(&state.daemon_socket);
    forward(socket_path.as_path(), request).await
}

/// The browser write surface: exact `(method, path)` pairs the
/// proxy forwards beyond `GET`/`HEAD`. Pinned by `DECISIONS.md
/// §15` sub-decision 2; widening this list widens what a browser
/// can ask the daemon to do.
const WRITE_ROUTES: &[(Method, &str)] = &[
    (Method::POST, "/api/v1/sessions"),
    (Method::DELETE, "/api/v1/sessions/current"),
    (Method::POST, "/api/v1/statements"),
];

fn is_write_route(method: &Method, path: &str) -> bool {
    WRITE_ROUTES
        .iter()
        .any(|(route_method, route_path)| route_method == method && *route_path == path)
}

/// The request's authority: the `Host` header on HTTP/1.1, the
/// URI authority (`:authority`) on HTTP/2.
fn request_host(request: &Request<Body>) -> Option<String> {
    request
        .headers()
        .get(axum::http::header::HOST)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .or_else(|| request.uri().authority().map(|a| a.as_str().to_owned()))
}

/// Whether an authority (`host[:port]`) names a loopback host.
/// Any port is accepted: the Vite dev server forwards its own
/// `Host` (`localhost:5173`) through to this proxy.
fn is_loopback_host(authority: &str) -> bool {
    let host = if let Some(rest) = authority.strip_prefix('[') {
        match rest.split_once(']') {
            Some((v6, port)) if port.is_empty() || port.starts_with(':') => v6,
            _ => return false,
        }
    } else {
        authority
            .rsplit_once(':')
            .map_or(authority, |(host, _port)| host)
    };
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// Whether the request's `Origin` is exactly `http://<host>`.
/// Missing, `null`, and `https://` origins all fail: kairo-web is
/// loopback HTTP only, so a legitimate same-origin page can only
/// be served from `http://<host>`.
fn is_same_origin(headers: &axum::http::HeaderMap, host: &str) -> bool {
    headers
        .get(axum::http::header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .and_then(|origin| origin.strip_prefix("http://"))
        .is_some_and(|authority| authority.eq_ignore_ascii_case(host))
}

/// Build the `405 Method Not Allowed` response for a method the
/// proxy doesn't forward on `path`. `Allow` lists `GET, HEAD`
/// plus whichever write methods [`WRITE_ROUTES`] opens on it.
fn method_not_allowed_response(path: &str) -> Response<Body> {
    let mut allow = String::from("GET, HEAD");
    for (method, _) in WRITE_ROUTES.iter().filter(|(_, p)| *p == path) {
        allow.push_str(", ");
        allow.push_str(method.as_str());
    }
    let mut resp = json_error_response(
        StatusCode::METHOD_NOT_ALLOWED,
        "web_method_not_allowed",
        &format!(
            "kairo-web forwards GET and HEAD on every path, and writes only on \
             POST /api/v1/sessions, DELETE /api/v1/sessions/current, and \
             POST /api/v1/statements; this path allows {allow}"
        ),
    );
    if let Ok(value) = HeaderValue::from_str(&allow) {
        resp.headers_mut().insert(axum::http::header::ALLOW, value);
    }
    resp
}

/// Build a response carrying the daemon's error-envelope shape so
/// browser clients have one error format to handle.
fn json_error_response(status: StatusCode, code: &str, message: &str) -> Response<Body> {
    let body = format!(
        r#"{{"ok":false,"schema":"kairo.api.error.v1","error":{{"code":{},"message":{}}}}}"#,
        json_string(code),
        json_string(message)
    );
    let mut resp = Response::new(Body::from(body));
    *resp.status_mut() = status;
    resp.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
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
        json_error_response(status, "web_proxy_error", &message)
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
    // daemon connection. Write bodies are small statement JSON;
    // the daemon enforces its own write-body cap.
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
    //! Request-guard invariants (`DECISIONS.md §15`).
    //!
    //! These tests drive `handler` directly with a deliberately
    //! invalid daemon-socket path. A guard short-circuit means the
    //! handler never tries to `connect(2)`; a request that passes
    //! every guard surfaces as a `Connect` error instead. The
    //! write allowlist is pinned exactly so any widening must
    //! update a test.
    use super::*;
    use axum::extract::State;
    use http_body_util::BodyExt;
    use std::path::PathBuf;

    const HOST: &str = "127.0.0.1:7878";
    const ORIGIN: &str = "http://127.0.0.1:7878";

    fn state() -> State<ProxyState> {
        State(ProxyState::new(PathBuf::from(
            "/nonexistent/kairo-web-guard.sock",
        )))
    }

    fn req(method: Method, path: &str, host: Option<&str>, origin: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder().method(method).uri(path);
        if let Some(host) = host {
            builder = builder.header(axum::http::header::HOST, host);
        }
        if let Some(origin) = origin {
            builder = builder.header(axum::http::header::ORIGIN, origin);
        }
        builder.body(Body::empty()).expect("build req")
    }

    /// The request passed every guard and reached `forward`.
    async fn assert_forwarded(request: Request<Body>) {
        let label = format!("{} {}", request.method(), request.uri());
        let err = handler(state(), request)
            .await
            .expect_err("expected Connect error past the guards");
        assert!(
            matches!(err, ProxyError::Connect { .. }),
            "{label}: got {err:?}"
        );
    }

    /// The request was short-circuited with `status` and `code`.
    async fn assert_rejected(
        request: Request<Body>,
        status: StatusCode,
        code: &str,
    ) -> Response<Body> {
        let label = format!("{} {}", request.method(), request.uri());
        let response = handler(state(), request)
            .await
            .unwrap_or_else(|err| panic!("{label} unexpectedly produced ProxyError: {err:?}"));
        assert_eq!(response.status(), status, "{label}");
        let (parts, body) = response.into_parts();
        let body_bytes = body.collect().await.expect("collect body").to_bytes();
        let body_text = std::str::from_utf8(&body_bytes).expect("utf-8 body");
        assert!(
            body_text.contains(&format!(r#""code":"{code}""#)),
            "{label} body missing code {code}: {body_text}",
        );
        Response::from_parts(parts, Body::empty())
    }

    #[tokio::test]
    async fn forwards_reads_without_origin() {
        assert_forwarded(req(Method::GET, "/api/v1/version", Some(HOST), None)).await;
        assert_forwarded(req(Method::HEAD, "/api/v1/version", Some(HOST), None)).await;
    }

    #[tokio::test]
    async fn forwards_allowlisted_writes_with_same_origin() {
        for (method, path) in WRITE_ROUTES {
            assert_forwarded(req(method.clone(), path, Some(HOST), Some(ORIGIN))).await;
        }
    }

    #[test]
    fn write_allowlist_is_pinned() {
        let routes: Vec<(&str, &str)> = WRITE_ROUTES
            .iter()
            .map(|(method, path)| (method.as_str(), *path))
            .collect();
        assert_eq!(
            routes,
            [
                ("POST", "/api/v1/sessions"),
                ("DELETE", "/api/v1/sessions/current"),
                ("POST", "/api/v1/statements"),
            ]
        );
    }

    #[tokio::test]
    async fn accepts_every_loopback_host_spelling() {
        for host in [
            "localhost:5173",
            "LOCALHOST",
            "127.0.0.1",
            "127.9.9.9:80",
            "[::1]:7878",
            "[::1]",
        ] {
            let origin = format!("http://{host}");
            assert_forwarded(req(Method::GET, "/api/v1/version", Some(host), None)).await;
            assert_forwarded(req(
                Method::POST,
                "/api/v1/statements",
                Some(host),
                Some(&origin),
            ))
            .await;
        }
    }

    #[tokio::test]
    async fn rejects_non_loopback_host() {
        for host in [
            "evil.example:7878",
            "evil.example",
            "localhost.evil.example",
            "192.168.1.10:7878",
            "[::2]:7878",
            "[::1]x",
            "",
        ] {
            assert_rejected(
                req(Method::GET, "/api/v1/version", Some(host), None),
                StatusCode::FORBIDDEN,
                "web_forbidden_host",
            )
            .await;
        }
    }

    #[tokio::test]
    async fn rejects_missing_host() {
        assert_rejected(
            req(Method::GET, "/api/v1/version", None, None),
            StatusCode::FORBIDDEN,
            "web_forbidden_host",
        )
        .await;
    }

    #[tokio::test]
    async fn rejects_writes_without_matching_origin() {
        for origin in [
            None,
            Some("null"),
            Some("http://localhost:3000"),
            Some("http://127.0.0.1:7879"),
            Some("https://127.0.0.1:7878"),
            Some("http://evil.example"),
        ] {
            assert_rejected(
                req(Method::POST, "/api/v1/statements", Some(HOST), origin),
                StatusCode::FORBIDDEN,
                "web_cross_origin",
            )
            .await;
        }
    }

    #[tokio::test]
    async fn rejects_methods_off_the_allowlist() {
        let cases = [
            (Method::PUT, "/api/v1/statements", "GET, HEAD, POST"),
            (Method::DELETE, "/api/v1/statements", "GET, HEAD, POST"),
            (
                Method::POST,
                "/api/v1/sessions/current",
                "GET, HEAD, DELETE",
            ),
            (Method::DELETE, "/api/v1/sessions", "GET, HEAD, POST"),
            (Method::POST, "/api/v1/statements/", "GET, HEAD"),
            (Method::POST, "/api/v1/anything", "GET, HEAD"),
            (Method::PATCH, "/api/v1/anything", "GET, HEAD"),
            (Method::OPTIONS, "/api/v1/statements", "GET, HEAD, POST"),
        ];
        for (method, path, allow) in cases {
            // Same-origin, so the 405 is the method guard, not the
            // origin guard.
            let response = assert_rejected(
                req(method.clone(), path, Some(HOST), Some(ORIGIN)),
                StatusCode::METHOD_NOT_ALLOWED,
                "web_method_not_allowed",
            )
            .await;
            assert_eq!(
                response
                    .headers()
                    .get(axum::http::header::ALLOW)
                    .and_then(|v| v.to_str().ok()),
                Some(allow),
                "{method} {path}",
            );
        }
    }

    #[tokio::test]
    async fn host_guard_runs_before_method_guard() {
        assert_rejected(
            req(
                Method::POST,
                "/api/v1/sessions",
                Some("evil.example"),
                Some("http://evil.example"),
            ),
            StatusCode::FORBIDDEN,
            "web_forbidden_host",
        )
        .await;
    }
}
