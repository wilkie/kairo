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
use kairo_identity::{verify_signature, ActorResolveError, ActorResolver};
use kairo_statement::json::{
    ActorCapabilityGrantStatementJson, ActorCapabilityRevocationStatementJson,
    ActorTrustStatementJson, ObjectBranchStatementJson, ObjectGenesisStatementJson,
    ObjectRevisionStatementJson, ObjectVersionTagStatementJson,
};
use kairo_statement::verify::{
    verify_envelope_statement, ActorResolution, SignatureStatus, VerificationReport,
};
use kairo_statement::{ObjectGenesisStatement, SignedStatement, StatementBody};
use kairo_store::{FilesystemStore, ObjectStore, StatementStore, StoreError};
use serde::Serialize;
use serde_json::Value;

use crate::api::envelope::{ApiError, ApiResult};
use crate::api::state::AppState;
use crate::api::store_errors::map_store_error;

/// Statement kinds the `POST /api/v1/statements` dispatcher
/// currently accepts (Phase A; see `PHASE_2_DAEMON_WRITES.md`).
/// Single-source-of-truth for the kind list — `GET /status`
/// reads this directly, and adding a kind to the dispatcher
/// match below means adding it here too.
pub const SUPPORTED_WRITE_KINDS: &[&str] = &[
    "ActorTrust",
    "ObjectRevision",
    "ObjectBranch",
    "ObjectVersionTag",
    "ObjectGenesis",
    "ActorCapabilityGrant",
    "ActorCapabilityRevocation",
];

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
/// `type` discriminator. The current allowlist lives in
/// `SUPPORTED_WRITE_KINDS`; everything else returns 415
/// `unsupported_statement_kind`. `GET /api/v1/status` reads from
/// the same const so the inspector / operator can see the live
/// surface without parsing the dispatcher source.
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
        description = "Signed statement envelope. Phase A accepts the kinds listed in \
            `GET /api/v1/status`'s `write_surface` field (ActorTrust, ObjectRevision, \
            ObjectBranch, ObjectVersionTag, ObjectGenesis, ActorCapabilityGrant, \
            ActorCapabilityRevocation). The schema annotation shows one example \
            (ActorTrust); the daemon dispatches on the `type` discriminator inside \
            the body. Polymorphic-body schema is post-Phase A.",
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
        "ActorTrust" => {
            let signed = decode_signed_body(body, &kind, ActorTrustStatementJson::to_statement)?;
            persist_signed(
                state,
                signed,
                FilesystemStore::put_actor_trust,
                "put_actor_trust",
                ActorTrustStatementJson::from_statement,
            )
            .await
        }
        "ObjectRevision" => {
            let signed =
                decode_signed_body(body, &kind, ObjectRevisionStatementJson::to_statement)?;
            persist_signed(
                state,
                signed,
                FilesystemStore::put_object_revision,
                "put_object_revision",
                ObjectRevisionStatementJson::from_statement,
            )
            .await
        }
        "ObjectBranch" => {
            let signed = decode_signed_body(body, &kind, ObjectBranchStatementJson::to_statement)?;
            persist_signed(
                state,
                signed,
                FilesystemStore::put_object_branch,
                "put_object_branch",
                ObjectBranchStatementJson::from_statement,
            )
            .await
        }
        "ObjectVersionTag" => {
            let signed =
                decode_signed_body(body, &kind, ObjectVersionTagStatementJson::to_statement)?;
            persist_signed(
                state,
                signed,
                FilesystemStore::put_object_version_tag,
                "put_object_version_tag",
                ObjectVersionTagStatementJson::from_statement,
            )
            .await
        }
        "ObjectGenesis" => put_object_genesis(state, body).await,
        "ActorCapabilityGrant" => {
            let signed =
                decode_signed_body(body, &kind, ActorCapabilityGrantStatementJson::to_statement)?;
            persist_signed(
                state,
                signed,
                FilesystemStore::put_actor_capability_grant,
                "put_actor_capability_grant",
                ActorCapabilityGrantStatementJson::from_statement,
            )
            .await
        }
        "ActorCapabilityRevocation" => {
            let signed = decode_signed_body(
                body,
                &kind,
                ActorCapabilityRevocationStatementJson::to_statement,
            )?;
            persist_signed(
                state,
                signed,
                FilesystemStore::put_actor_capability_revocation,
                "put_actor_capability_revocation",
                ActorCapabilityRevocationStatementJson::from_statement,
            )
            .await
        }
        other => Err(ApiError::unsupported_statement_kind(format!(
            "statement kind {other:?} is not accepted by Phase A's current dispatcher; \
             see PHASE_2_DAEMON_WRITES.md for the slice that opens up the remaining kinds",
        ))),
    }
}

/// `ObjectGenesis` dispatcher arm. `ObjectGenesisStatement` is a
/// distinct envelope shape (no envelope-level `actor`/`subject`/
/// `created_at`; the signature covers `body.canonical_bytes()`
/// directly — see `project_genesis_signature_asymmetry`), so it
/// can't ride the generic `persist_signed` helper the rest of
/// Phase A uses.
///
/// Verification is the same shape as `verify_envelope_statement`
/// adapted to the genesis envelope:
///
/// 1. `signature.actor() == body.created_by()` — otherwise
///    422 `signature_actor_mismatch`.
/// 2. Resolve the creating actor through the store-backed
///    `ActorResolver` — 422 `actor_not_found` if missing.
/// 3. Resolve the active signing key at `body.created_at()` and
///    verify the signature via `statement.verify_signature` —
///    422 `bad_signature` on any failure.
///
/// Successful writes echo the persisted `object_id` in the
/// `statement_id` field of `PutStatementResult` so the wire
/// shape stays uniform across kinds. The genesis envelope has
/// no separate statement_id concept — the object_id is the
/// identity.
async fn put_object_genesis(
    state: AppState,
    body: Value,
) -> Result<ApiResult<PutStatementResult>, ApiError> {
    let dto: ObjectGenesisStatementJson = serde_json::from_value(body)
        .map_err(|error| ApiError::bad_request(format!("invalid ObjectGenesis body shape: {error}")))?;
    let statement = dto
        .to_statement()
        .map_err(|error| ApiError::bad_request(format!("invalid ObjectGenesis body: {error}")))?;

    verify_object_genesis_against_store(&state, &statement).await?;

    let store = state.store.clone();
    let statement_for_put = statement.clone();
    let object_id = tokio::task::spawn_blocking(move || store.put_object_genesis(&statement_for_put))
        .await
        .map_err(|error| ApiError::internal(format!("spawn_blocking join failed: {error}")))?
        .map_err(|error| map_store_error(error, "put_object_genesis"))?;

    let echo = ObjectGenesisStatementJson::from_statement(&statement);
    let statement_value = serde_json::to_value(&echo).map_err(|error| {
        ApiError::internal(format!("failed to serialize stored statement: {error}"))
    })?;

    Ok(ApiResult(PutStatementResult {
        statement_id: object_id.to_string(),
        statement: statement_value,
    }))
}

/// Verifier for `ObjectGenesisStatement`. Mirrors
/// `verify_signed_against_store` for the envelope kind but works
/// directly against the genesis body — the signature covers
/// `body.canonical_bytes()` (no envelope wrapping), and the
/// "actor" of the statement is `body.created_by()` rather than
/// an envelope-level field.
async fn verify_object_genesis_against_store(
    state: &AppState,
    statement: &ObjectGenesisStatement,
) -> Result<(), ApiError> {
    let body_actor = statement.body().created_by().clone();
    let signature_actor = statement.signature().actor().clone();
    if signature_actor != body_actor {
        return Err(ApiError::signature_actor_mismatch(format!(
            "genesis body created_by {body_actor} does not match signature actor {signature_actor}",
        )));
    }

    let resolver = state.store.clone();
    let body_actor_for_lookup = body_actor.clone();
    let created_at = statement.body().created_at();
    let resolved = tokio::task::spawn_blocking(move || -> Result<_, ActorResolveError> {
        let resolver_ref: &FilesystemStore = resolver.as_ref();
        let genesis = resolver_ref.actor_genesis(&body_actor_for_lookup)?;
        if genesis.is_none() {
            return Ok(None);
        }
        let active_key = resolver_ref.active_key_at(&body_actor_for_lookup, created_at)?;
        Ok(active_key)
    })
    .await
    .map_err(|error| ApiError::internal(format!("spawn_blocking join failed: {error}")))?
    .map_err(|error| {
        ApiError::store(format!(
            "actor resolver unavailable for {body_actor}: {error}",
        ))
    })?;

    let active_key = match resolved {
        Some(key) => key,
        None => {
            return Err(ApiError::actor_not_found(format!(
                "actor {body_actor} has no ActorGenesis in the store",
            )));
        }
    };

    let signature_bytes = statement
        .signature()
        .to_signature_bytes()
        .map_err(|error| ApiError::bad_signature(format!("genesis signature malformed: {error}")))?;
    verify_signature(&active_key, &statement.signed_bytes(), &signature_bytes)
        .map_err(|error| ApiError::bad_signature(format!("genesis signature invalid: {error}")))?;

    Ok(())
}

/// Deserialize a JSON request body into a typed `*StatementJson`
/// DTO, then convert it into the matching `SignedStatement<B>`.
/// Both failure paths produce 400 `bad_request` — JSON-shape
/// rejection and StatementJson-domain rejection are user errors,
/// not server errors.
fn decode_signed_body<TDto, B, Err>(
    body: Value,
    kind: &str,
    to_statement: fn(&TDto) -> Result<SignedStatement<B>, Err>,
) -> Result<SignedStatement<B>, ApiError>
where
    TDto: serde::de::DeserializeOwned,
    B: StatementBody,
    Err: std::fmt::Display,
{
    let dto: TDto = serde_json::from_value(body).map_err(|error| {
        ApiError::bad_request(format!("invalid {kind} body shape: {error}"))
    })?;
    to_statement(&dto)
        .map_err(|error| ApiError::bad_request(format!("invalid {kind} body: {error}")))
}

/// Run the verify + persist + echo pipeline that every Phase A
/// write kind shares. The caller supplies the store's typed
/// `put_*` and the matching JSON `from_statement` — everything
/// else (spawn_blocking, error mapping, response shaping) is
/// uniform across kinds.
async fn persist_signed<B, TDto, FromFn, PutFn>(
    state: AppState,
    signed: SignedStatement<B>,
    put_fn: PutFn,
    put_context: &'static str,
    from_statement: FromFn,
) -> Result<ApiResult<PutStatementResult>, ApiError>
where
    B: StatementBody + Send + 'static,
    SignedStatement<B>: Clone + Send,
    TDto: Serialize,
    FromFn: FnOnce(&SignedStatement<B>) -> TDto,
    PutFn: Send + 'static + FnOnce(&FilesystemStore, &SignedStatement<B>) -> Result<StatementId, StoreError>,
{
    verify_signed_against_store(&state, &signed).await?;

    let store = state.store.clone();
    let signed_for_put = signed.clone();
    let statement_id = tokio::task::spawn_blocking(move || put_fn(store.as_ref(), &signed_for_put))
        .await
        .map_err(|error| ApiError::internal(format!("spawn_blocking join failed: {error}")))?
        .map_err(|error| map_store_error(error, put_context))?;

    let echo = from_statement(&signed);
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
async fn verify_signed_against_store<B>(
    state: &AppState,
    signed: &SignedStatement<B>,
) -> Result<(), ApiError>
where
    B: StatementBody + Send + 'static,
    SignedStatement<B>: Clone + Send,
{
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
