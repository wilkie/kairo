//! Request-auth pipeline for the daemon's write surface
//! (`POST /api/v1/statements`). Implements Phase B
//! authentication per `DECISIONS.md §14` and Slice 2 of
//! `PHASE_2_DAEMON_AUTH.md`.
//!
//! The pipeline runs *before* the existing write dispatcher.
//! On success it produces a typed [`AuthenticatedRequest`] that
//! the handler can pass downstream (Slices 3 / 4 will use the
//! resolved `ActorId` to gate authorization). On failure it
//! returns a typed [`ApiError`] mapping onto the 401 codes
//! documented in `PHASE_2_DAEMON_AUTH.md §2.3`.
//!
//! **Authentication only** in this slice. Capability-based
//! authorization (`evaluate_capability`) lands in Slice 3 for
//! object writes and Slice 4 for grants / revocations. A
//! successfully-authenticated request still falls through to
//! the existing handler unchanged.

use std::sync::Arc;

use kairo_core::{ActorId, Timestamp};
use kairo_daemon_client::auth::{
    build_signing_string, AuthEnvelope, AuthHeaderError, SCHEME_TAG, TIMESTAMP_WINDOW_SECONDS,
};
use kairo_identity::{
    verify_signature, ActorResolveError, ActorResolver, KeyId, SignatureAlgorithm, SignatureBytes,
};
use kairo_store::FilesystemStore;

use crate::api::envelope::ApiError;
use crate::api::state::AppState;

/// The output of a successful authn pipeline. Carries the
/// resolved actor so Slices 3 / 4 can run authz against it
/// without re-parsing the header. The `timestamp` and `nonce`
/// fields are preserved for diagnostic logging and for a
/// future stateful-nonce-cache implementation
/// (`DECISIONS.md §14` sub-decision 5).
#[derive(Debug, Clone)]
pub struct AuthenticatedRequest {
    pub actor: ActorId,
    pub key_id: KeyId,
    pub timestamp: i64,
    pub nonce: [u8; 16],
}

/// Verify a write request's `Authorization` envelope against
/// the daemon's actor resolver. Returns the resolved actor on
/// success, a typed 401 error on any failure.
///
/// Pipeline (matches `PHASE_2_DAEMON_AUTH.md` Slice 2):
///
/// 1. Extract the `Authorization` header — 401
///    `missing_authorization` if absent.
/// 2. Parse the `KairoActor1` envelope — 401
///    `malformed_authorization` (or `unsupported_auth_scheme`
///    when the scheme tag doesn't match).
/// 3. Check the timestamp window — 401
///    `timestamp_out_of_window` if outside ±60s.
/// 4. Resolve the envelope actor through the store-backed
///    resolver — 401 `actor_not_found_for_request` if missing.
/// 5. Confirm the envelope `key_id` matches the actor's active
///    signing key at `timestamp` — 401 `request_key_id_unknown`
///    if not.
/// 6. Rebuild the signing string from the request bytes and
///    verify the ed25519 signature — 401 `bad_request_signature`
///    on any failure.
///
/// The function is `async` because step (4)/(5) call into the
/// store via `spawn_blocking`. The rest is synchronous.
pub async fn verify_request_auth(
    state: &AppState,
    authorization_header: Option<&str>,
    method: &str,
    path_and_query: &str,
    body_bytes: &[u8],
) -> Result<AuthenticatedRequest, ApiError> {
    // Step 1: header presence.
    let header_value = authorization_header.ok_or_else(|| {
        ApiError::missing_authorization(
            "POST /api/v1/statements requires Authorization: KairoActor1 …".to_string(),
        )
    })?;

    // Step 2: parse the envelope.
    let envelope = AuthEnvelope::parse(header_value).map_err(map_auth_parse_error)?;

    // Step 3: timestamp window.
    let now = Timestamp::now().seconds();
    let skew = (now - envelope.timestamp).abs();
    if skew > TIMESTAMP_WINDOW_SECONDS {
        return Err(ApiError::timestamp_out_of_window(format!(
            "request timestamp {ts} is {skew}s away from now ({now}); window is \u{00b1}{window}s",
            ts = envelope.timestamp,
            window = TIMESTAMP_WINDOW_SECONDS,
        )));
    }

    // Step 4 + 5: resolve actor + active key at request.timestamp.
    let resolver = state.store.clone();
    let actor_for_lookup = envelope.actor.clone();
    let lookup_at = Timestamp::from_seconds(envelope.timestamp);
    let resolved = tokio::task::spawn_blocking(move || -> Result<KeyLookup, ActorResolveError> {
        let resolver_ref: &FilesystemStore = resolver.as_ref();
        let genesis = resolver_ref.actor_genesis(&actor_for_lookup)?;
        if genesis.is_none() {
            return Ok(KeyLookup::ActorAbsent);
        }
        match resolver_ref.active_key_at(&actor_for_lookup, lookup_at)? {
            Some(key) => Ok(KeyLookup::Found(key)),
            None => Ok(KeyLookup::NoActiveKey),
        }
    })
    .await
    .map_err(|error| ApiError::internal(format!("spawn_blocking join failed: {error}")))?
    .map_err(|error| {
        ApiError::store(format!(
            "actor resolver unavailable for {}: {error}",
            envelope.actor,
        ))
    })?;

    let active_key = match resolved {
        KeyLookup::ActorAbsent => {
            return Err(ApiError::actor_not_found_for_request(format!(
                "request actor {} has no ActorGenesis in the store",
                envelope.actor,
            )));
        }
        KeyLookup::NoActiveKey => {
            return Err(ApiError::request_key_id_unknown(format!(
                "actor {} has no active signing key at timestamp {}",
                envelope.actor, envelope.timestamp,
            )));
        }
        KeyLookup::Found(key) => key,
    };

    if active_key.key_id() != envelope.key_id {
        return Err(ApiError::request_key_id_unknown(format!(
            "request key_id {} does not match active key_id {} for actor {} at timestamp {}",
            envelope.key_id,
            active_key.key_id(),
            envelope.actor,
            envelope.timestamp,
        )));
    }

    // Step 6: rebuild the signing string and verify.
    let signing = build_signing_string(
        method,
        path_and_query,
        body_bytes,
        envelope.timestamp,
        &envelope.nonce,
    );
    let signature_bytes = match active_key.algorithm() {
        SignatureAlgorithm::Ed25519 => SignatureBytes::ed25519(envelope.signature),
    };
    verify_signature(&active_key, signing.as_bytes(), &signature_bytes).map_err(|error| {
        ApiError::bad_request_signature(format!(
            "request signature did not verify against active key: {error}"
        ))
    })?;

    Ok(AuthenticatedRequest {
        actor: envelope.actor,
        key_id: envelope.key_id,
        timestamp: envelope.timestamp,
        nonce: envelope.nonce,
    })
}

fn map_auth_parse_error(error: AuthHeaderError) -> ApiError {
    match error {
        AuthHeaderError::UnsupportedScheme(scheme) => ApiError::unsupported_auth_scheme(format!(
            "unsupported Authorization scheme {scheme:?}; expected {SCHEME_TAG}",
        )),
        AuthHeaderError::Malformed(reason) => {
            ApiError::malformed_authorization(format!("Authorization header malformed: {reason}"))
        }
    }
}

/// Helper enum for the spawn_blocking lookup. Carries the
/// active key when both lookups succeeded; otherwise marks
/// which failure mode the caller should surface.
#[derive(Debug)]
enum KeyLookup {
    ActorAbsent,
    NoActiveKey,
    Found(kairo_identity::PublicKey),
}

/// Build the `path_and_query` string from an axum `Uri`. The
/// signing-string contract is "the request's `:path` exactly as
/// it appears on the wire" — so this is just `Uri::path_and_query()`
/// stringified, falling back to `path()` when there is no
/// query.
pub fn path_and_query_for_signing(uri: &axum::http::Uri) -> String {
    match uri.path_and_query() {
        Some(pq) => pq.as_str().to_owned(),
        None => uri.path().to_owned(),
    }
}

/// Look up the `Authorization` header value from a headers map.
/// Returns `None` if absent or if the header bytes aren't valid
/// UTF-8 (in which case the request is treated as
/// authorization-missing, which is the correct safety
/// behaviour).
pub fn authorization_header_value(headers: &axum::http::HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
}

// `state` is unused by the helpers above but the verifier
// keeps the `&AppState` parameter for symmetry with the
// existing handler conventions and to keep the resolver
// reachable when Slices 3 / 4 grow the authz check on the
// same call site.
#[allow(dead_code)]
fn _state_marker(_state: &AppState) {}

// Re-export the AppState type so `Arc<FilesystemStore>` plumbing
// in the verifier stays referenced even if the helper is the
// only consumer of the type alias.
#[allow(dead_code)]
type _StateArc = Arc<FilesystemStore>;
