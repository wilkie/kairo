//! Session-lifecycle handlers:
//!
//! - `POST   /api/v1/sessions`           open
//! - `GET    /api/v1/sessions/current`   inspect
//! - `DELETE /api/v1/sessions/current`   close
//!
//! See `specs/PHASE_2_WEB_CLIENT_AUTH.md` §3.2 for the contract.
//!
//! The cookie name is `kairo_session`; attributes are `HttpOnly;
//! SameSite=Strict; Path=/`. `Max-Age` matches the idle timeout
//! so a browser-side cache evicts an idle session at roughly the
//! same moment the server-side store does. The absolute hard
//! cap is enforced server-side regardless.

use std::str::FromStr;
use std::time::Duration;

use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use kairo_core::{ActorId, Timestamp};
use kairo_identity::ActorResolver;
use kairo_keystore::Keystore;

use crate::api::dto::{OpenSessionRequest, SessionInfo};
use crate::api::envelope::{ApiError, ApiResult};
use crate::api::state::AppState;
use crate::sessions::{Session, SessionId, SessionStore};

const COOKIE_NAME: &str = "kairo_session";

/// Open a fresh session.
///
/// Verifies the actor id is well-formed, the keystore holds a
/// usable signing key for it, the active key is not revoked, and
/// then creates a session bound to that actor. The response sets
/// `kairo_session=<id>; HttpOnly; SameSite=Strict; Path=/` and
/// returns the session metadata.
#[utoipa::path(
    post,
    path = "/api/v1/sessions",
    tag = "sessions",
    operation_id = "openSession",
    request_body = OpenSessionRequest,
    responses(
        (status = 201, description = "Session opened; cookie set", body = SessionInfo),
        (status = 400, description = "Malformed actor id"),
        (status = 404, description = "No private key for that actor in the local keystore"),
        (status = 409, description = "Actor's active signing key is revoked"),
    ),
)]
pub async fn open_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<OpenSessionRequest>,
) -> Result<Response, ApiError> {
    let actor_id = ActorId::from_str(&body.actor_id).map_err(|error| {
        ApiError::bad_request(format!(
            "invalid actor id {:?}: {error}",
            body.actor_id
        ))
    })?;

    let keystore = state.keystore.clone();
    let actor_for_keystore = actor_id.clone();
    let has_key = tokio::task::spawn_blocking(move || {
        keystore.has_signing_key(&actor_for_keystore)
    })
    .await
    .map_err(|join| ApiError::internal(format!("keystore probe task: {join}")))?
    .map_err(|error| ApiError::internal(format!("keystore probe: {error}")))?;

    if !has_key {
        return Err(ApiError::actor_not_in_keystore(format!(
            "no local signing key for actor {}",
            actor_id
        )));
    }

    // Resolve the active key at "now" so a revoked key can't open
    // a session it'd immediately fail to sign with. Matches the
    // CLI's behaviour: signing always resolves through the same
    // §5.5 chain.
    let now = Timestamp::now();
    let store = state.store.clone();
    let actor_for_resolver = actor_id.clone();
    let active_key = tokio::task::spawn_blocking(move || {
        store.active_key_at(&actor_for_resolver, now)
    })
    .await
    .map_err(|join| ApiError::internal(format!("active-key task: {join}")))?
    .map_err(|error| ApiError::internal(format!("active-key lookup: {error}")))?
    .ok_or_else(|| {
        ApiError::actor_not_found(format!(
            "actor {} has no genesis in the store",
            actor_id
        ))
    })?;

    let key_id = active_key.key_id();
    let store_for_revocation = state.store.clone();
    let actor_for_revocation = actor_id.clone();
    let key_id_for_revocation = key_id.clone();
    let revoked = tokio::task::spawn_blocking(move || {
        store_for_revocation.is_key_revoked_at(
            &actor_for_revocation,
            &key_id_for_revocation,
            now,
        )
    })
    .await
    .map_err(|join| ApiError::internal(format!("revocation task: {join}")))?
    .map_err(|error| ApiError::internal(format!("revocation lookup: {error}")))?;

    if revoked {
        return Err(ApiError::key_revoked(format!(
            "active signing key for actor {} is revoked",
            actor_id
        )));
    }

    // Best-effort: if the caller already has a session cookie,
    // delete the prior session so we hold one record per cookie.
    if let Some(prior) = parse_session_cookie(&headers) {
        state.sessions.delete(&prior);
    }

    let (new_id, session) = state
        .sessions
        .create(actor_id)
        .map_err(|error| ApiError::internal(format!("session id generation: {error}")))?;
    let idle_timeout = state.sessions.config().idle_timeout;

    let body = SessionInfo {
        actor_id: session.actor_id.to_string(),
        created_at: session.created_at.to_string(),
        expires_at: session.expires_at.to_string(),
        idle_expires_at: session.idle_expires_at(idle_timeout).to_string(),
    };

    let mut response = ApiResult(body).into_response();
    *response.status_mut() = StatusCode::CREATED;
    response.headers_mut().insert(
        header::SET_COOKIE,
        set_cookie_for(&new_id, idle_timeout),
    );
    Ok(response)
}

/// Read the current session's metadata.
///
/// The `kairo_session` cookie's value is the only input. Misses
/// (no cookie, malformed cookie, expired session) all map to 401
/// `no_session`. A successful lookup also touches the session,
/// resetting the sliding idle deadline.
#[utoipa::path(
    get,
    path = "/api/v1/sessions/current",
    tag = "sessions",
    operation_id = "getCurrentSession",
    responses(
        (status = 200, description = "Session metadata", body = SessionInfo),
        (status = 401, description = "No live session for this cookie"),
    ),
)]
pub async fn current_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let cookie = parse_session_cookie(&headers)
        .ok_or_else(|| ApiError::no_session("no kairo_session cookie present"))?;
    let session = state
        .sessions
        .touch(&cookie)
        .ok_or_else(|| no_session_with_clear(&state.sessions))?;

    let body = session_info(&session, state.sessions.config().idle_timeout);
    Ok(ApiResult(body).into_response())
}

/// Terminate the current session.
///
/// Idempotent: returns 204 even when no cookie / no live session
/// is present, and always emits a clearing `Set-Cookie` so the
/// browser drops its handle even if the server already evicted
/// the record.
#[utoipa::path(
    delete,
    path = "/api/v1/sessions/current",
    tag = "sessions",
    operation_id = "deleteCurrentSession",
    responses(
        (status = 204, description = "Session terminated (or already gone)"),
    ),
)]
pub async fn delete_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if let Some(cookie) = parse_session_cookie(&headers) {
        state.sessions.delete(&cookie);
    }

    let mut response = Response::default();
    *response.status_mut() = StatusCode::NO_CONTENT;
    response
        .headers_mut()
        .insert(header::SET_COOKIE, clear_cookie());
    response
}

/// Read the `kairo_session` cookie out of a request's headers.
/// Returns `None` for any input that doesn't match the expected
/// shape — including unparseable utf-8, missing cookie, or
/// malformed session id. Exported so the write-statement
/// handler can detect cookie-mode auth on
/// `POST /api/v1/statements` (per `PHASE_2_WEB_CLIENT_AUTH.md`
/// §2.3 "two payload shapes on one endpoint").
pub fn parse_session_cookie(headers: &HeaderMap) -> Option<SessionId> {
    for value in &headers.get_all(header::COOKIE) {
        let value = value.to_str().ok()?;
        for pair in value.split(';') {
            let pair = pair.trim();
            if let Some(rest) = pair.strip_prefix(&format!("{COOKIE_NAME}=")) {
                if let Some(id) = SessionId::parse(rest) {
                    return Some(id);
                }
            }
        }
    }
    None
}

fn no_session_with_clear(_sessions: &SessionStore) -> ApiError {
    // Future enhancement: we could clear the cookie on the 401
    // response as well. The 401 path currently relies on the
    // browser inferring expiry from `idle_expires_at`. Keeping
    // this hook so it's easy to extend later.
    ApiError::no_session("no live session for that cookie")
}

fn session_info(session: &Session, idle_timeout: Duration) -> SessionInfo {
    SessionInfo {
        actor_id: session.actor_id.to_string(),
        created_at: session.created_at.to_string(),
        expires_at: session.expires_at.to_string(),
        idle_expires_at: session.idle_expires_at(idle_timeout).to_string(),
    }
}

// The cookie value is built from compile-time constants plus a
// base64url-no-pad session id — every byte is in
// `HeaderValue`'s visible-ASCII charset, so `from_str` cannot
// fail at runtime. The `expect` is the standard infallible-cast
// shape; allow the lint locally with this justification.
#[allow(clippy::expect_used)]
fn set_cookie_for(id: &SessionId, idle_timeout: Duration) -> HeaderValue {
    let max_age = idle_timeout.as_secs();
    let value = format!(
        "{COOKIE_NAME}={id}; HttpOnly; SameSite=Strict; Path=/; Max-Age={max_age}",
        id = id.as_str()
    );
    HeaderValue::from_str(&value).expect("set-cookie header bytes are visible ASCII")
}

fn clear_cookie() -> HeaderValue {
    HeaderValue::from_static(
        "kairo_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0",
    )
}
