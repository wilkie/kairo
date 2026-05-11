//! `/api/v1/statements/...` handlers — the generic statement
//! read and Phase A's single generic write endpoint.
//!
//! `GET /api/v1/statements/{id}` returns the statement's JSON
//! envelope by id, polymorphic across kinds. The handler reads
//! raw JSON bytes via `FilesystemStore::get_statement_bytes`,
//! parses to a `serde_json::Value` so axum re-serializes the
//! body verbatim — the kind discriminator lives inside, and
//! callers either match on it or feed the value back into a
//! typed `serde_json::from_value`.
//!
//! `POST /api/v1/statements` is Phase A's generic write surface
//! (`DECISIONS.md §13` sub-decision 2). The request body is a
//! `SignedStatement<*>` JSON envelope; the handler dispatches on
//! the `type` discriminator to the matching kairo-store `put_*`.
//! Slice 2 opens dispatch for `ActorTrust` only — every other
//! kind returns 415 `unsupported_statement_kind` until a later
//! slice widens the dispatcher.

use axum::extract::{Path, State};
use axum::Json;
use kairo_core::StatementId;
use kairo_daemon_client::dto::PutStatementResult;
use kairo_statement::json::ActorTrustStatementJson;
use kairo_statement::verify::{
    verify_envelope_statement, ActorResolution, SignatureStatus, VerificationReport,
};
use kairo_statement::{ActorTrustBody, SignedStatement};
use kairo_store::StatementStore;
use serde_json::Value;

use crate::api::envelope::{ApiError, ApiResult};
use crate::api::state::AppState;
use crate::api::store_errors::map_store_error;

#[utoipa::path(
    get,
    path = "/api/v1/statements/{id}",
    tag = "statements",
    operation_id = "getStatement",
    params(
        ("id" = String, Path, description = "Statement id (kairo:stmt:...)"),
    ),
    responses(
        (
            status = 200,
            description = "Polymorphic signed statement JSON; refine via the embedded `type` discriminator",
            body = Object,
        ),
        (status = 400, description = "Malformed statement id"),
        (status = 404, description = "Statement not found"),
    ),
)]
pub async fn handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<ApiResult<Value>, ApiError> {
    let statement_id: StatementId = id
        .parse()
        .map_err(|error| ApiError::bad_request(format!("invalid statement id {id:?}: {error}")))?;

    let store = state.store.clone();
    let bytes = tokio::task::spawn_blocking(move || store.get_statement_bytes(&statement_id))
        .await
        .map_err(|error| ApiError::internal(format!("spawn_blocking join failed: {error}")))?
        .map_err(|error| map_store_error(error, "get_statement_bytes"))?;

    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|error| ApiError::store(format!("statement JSON parse failed: {error}")))?;

    Ok(ApiResult(value))
}

/// `POST /api/v1/statements` — Phase A's generic write endpoint.
///
/// Accepts a `SignedStatement<*>` JSON envelope and routes on the
/// `type` discriminator. Slice 2 dispatches `ActorTrust` only;
/// every other kind returns 415 `unsupported_statement_kind`.
///
/// The handler runs the same verification the direct-mode CLI
/// runs (`verify_envelope_statement` against the daemon's
/// resolver), then persists via the store's typed `put_*`. No
/// per-request auth: Phase A's trust model is the Unix socket's
/// filesystem permissions, identical to the v1 read endpoints.
/// Signed-request auth is locked for Phase B (`DECISIONS.md §13`
/// sub-decision 7).
#[utoipa::path(
    post,
    path = "/api/v1/statements",
    tag = "statements",
    operation_id = "putStatement",
    request_body(
        description = "Signed statement envelope. Phase A accepts ActorTrust only.",
        content_type = "application/json",
        content = ActorTrustStatementJson,
    ),
    responses(
        (
            status = 200,
            description = "Persisted statement; `statement` echoes the body the daemon stored",
            body = PutStatementResult,
        ),
        (status = 400, description = "Malformed body (decode failure, missing `type` discriminator, …)"),
        (status = 415, description = "Statement kind not yet accepted by Phase A's dispatcher"),
        (status = 422, description = "Signature did not verify, or actor/signature shape rejected"),
        (status = 500, description = "Store I/O or fixity failure"),
    ),
)]
pub async fn put_handler(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<ApiResult<PutStatementResult>, ApiError> {
    let kind = body
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ApiError::bad_request(
                "statement body missing required string field `type`".to_string(),
            )
        })?
        .to_owned();

    match kind.as_str() {
        "ActorTrust" => put_actor_trust(state, body).await,
        other => Err(ApiError::unsupported_statement_kind(format!(
            "statement kind {other:?} is not accepted by Phase A; \
             Slice 2 supports `ActorTrust` only — additional kinds \
             land in later slices (see PHASE_2_DAEMON_WRITES.md)"
        ))),
    }
}

async fn put_actor_trust(
    state: AppState,
    body: Value,
) -> Result<ApiResult<PutStatementResult>, ApiError> {
    let dto: ActorTrustStatementJson = serde_json::from_value(body).map_err(|error| {
        ApiError::bad_request(format!("invalid ActorTrust body shape: {error}"))
    })?;
    let signed = dto
        .to_statement()
        .map_err(|error| ApiError::bad_request(format!("invalid ActorTrust body: {error}")))?;

    verify_signed_against_store(&state, &signed).await?;

    let store = state.store.clone();
    let signed_for_put = signed.clone();
    let statement_id = tokio::task::spawn_blocking(move || store.put_actor_trust(&signed_for_put))
        .await
        .map_err(|error| ApiError::internal(format!("spawn_blocking join failed: {error}")))?
        .map_err(|error| map_store_error(error, "put_actor_trust"))?;

    let echo = ActorTrustStatementJson::from_statement(&signed);
    let statement = serde_json::to_value(&echo).map_err(|error| {
        ApiError::internal(format!("failed to serialize stored statement: {error}"))
    })?;

    Ok(ApiResult(PutStatementResult {
        statement_id: statement_id.to_string(),
        statement,
    }))
}

/// Run `kairo_statement::verify::verify_envelope_statement`
/// against the daemon's store-backed resolver and collapse the
/// structured report into an `ApiError`.
///
/// Order of checks mirrors the report's surfacing order so the
/// daemon's response leads with the earliest invariant the body
/// violated:
///
/// 1. `signature_actor_mismatch` — envelope vs signature actor
///    disagree; signature is not evaluated.
/// 2. `actor_not_found` — envelope actor's `ActorGenesis` not in
///    the store.
/// 3. Signature outcomes other than `Valid` collapse to
///    `bad_signature` with the structured cause attached to the
///    message for diagnostics.
///
/// Trust evaluation is intentionally ignored: trust is
/// informational and never makes a cryptographically valid
/// statement invalid (`verify.rs` §"Trust evaluation").
async fn verify_signed_against_store(
    state: &AppState,
    signed: &SignedStatement<ActorTrustBody>,
) -> Result<(), ApiError> {
    let resolver = state.store.clone();
    let signed_for_verify = signed.clone();
    let report: VerificationReport = tokio::task::spawn_blocking(move || {
        verify_envelope_statement(&signed_for_verify, resolver.as_ref())
    })
    .await
    .map_err(|error| ApiError::internal(format!("spawn_blocking join failed: {error}")))?;

    match report.actor {
        ActorResolution::Resolved => {}
        ActorResolution::SignatureActorMismatch => {
            return Err(ApiError::signature_actor_mismatch(format!(
                "envelope actor {} does not match signature actor {}",
                report.envelope_actor, report.signature_actor,
            )));
        }
        ActorResolution::NotFound => {
            return Err(ApiError::actor_not_found(format!(
                "actor {} has no ActorGenesis in the store",
                report.envelope_actor,
            )));
        }
        ActorResolution::ResolverUnavailable(reason) => {
            return Err(ApiError::store(format!(
                "actor resolver unavailable for {}: {reason}",
                report.envelope_actor,
            )));
        }
    }

    match report.signature {
        SignatureStatus::Valid => Ok(()),
        SignatureStatus::Invalid => Err(ApiError::bad_signature(
            "signature did not verify against the actor's active signing key".to_string(),
        )),
        SignatureStatus::UnsupportedAlgorithm(algorithm) => Err(ApiError::bad_signature(format!(
            "unsupported signature algorithm {algorithm:?}"
        ))),
        SignatureStatus::Malformed {
            expected_len,
            actual_len,
        } => Err(ApiError::bad_signature(format!(
            "signature bytes malformed (expected {expected_len} bytes, got {actual_len})"
        ))),
        SignatureStatus::AlgorithmMismatch => Err(ApiError::bad_signature(
            "signature algorithm does not match the resolved actor key".to_string(),
        )),
        SignatureStatus::KeyMismatch {
            signature_key_id,
            active_key_id,
        } => Err(ApiError::bad_signature(format!(
            "signature key_id {signature_key_id} does not match active key_id {active_key_id} \
             at the statement's created_at"
        ))),
        SignatureStatus::KeyRevoked => Err(ApiError::bad_signature(
            "signing key is revoked at the statement's created_at".to_string(),
        )),
        SignatureStatus::NoActiveKey => Err(ApiError::bad_signature(
            "actor has no active signing key at the statement's created_at".to_string(),
        )),
        SignatureStatus::NotInAttestationSet { signature_key_id } => {
            Err(ApiError::bad_signature(format!(
                "emergency-surface signature key_id {signature_key_id} is not in the attestation set"
            )))
        }
        SignatureStatus::BelowThreshold { provided, required } => Err(ApiError::bad_signature(
            format!("multi-sig signature count {provided} is below threshold {required}"),
        )),
        SignatureStatus::NotEvaluated => Err(ApiError::internal(
            "signature outcome was NotEvaluated despite actor resolving cleanly".to_string(),
        )),
    }
}
