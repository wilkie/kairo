//! `GET /api/v1/trust/...` handlers.
//!
//! - `/trust/{by}/{of}` — chain-leaf `ActorTrust` statement
//!   (grant, block, or withdrawal). Returns 404 when `by` has
//!   never expressed an opinion about `of` (no opinion is its
//!   own state — `unknown` — and is *not* an error in CLI
//!   tooling, but the daemon API reflects absence as 404 so
//!   callers can branch cleanly).
//! - `/trust/about/{of}` — list of chain-leaf trust heads
//!   expressed *about* `of` (one per `by_actor`). Inspector
//!   pages aggregate these per-object by intersecting the list
//!   with the object's involved actors.

use axum::extract::{Path, State};
use kairo_core::ActorId;
use kairo_daemon_client::dto::TrustHeadDto;
use kairo_statement::json::ActorTrustStatementJson;
use kairo_store::TrustResolver;

use crate::api::envelope::{ApiError, ApiResult};
use crate::api::state::AppState;
use crate::api::store_errors::map_store_error;

/// `GET /api/v1/trust/about/{of}`
#[utoipa::path(
    get,
    path = "/api/v1/trust/about/{of}",
    tag = "trust",
    operation_id = "listTrustAbout",
    params(
        ("of" = String, Path, description = "Actor whose incoming opinions to list (kairo:actor:...)"),
    ),
    responses(
        (status = 200, description = "Chain-leaf trust heads expressed about the actor (one per by_actor)", body = [TrustHeadDto]),
        (status = 400, description = "Malformed actor id"),
    ),
)]
pub async fn list_about_handler(
    State(state): State<AppState>,
    Path(of): Path<String>,
) -> Result<ApiResult<Vec<TrustHeadDto>>, ApiError> {
    let trusted_actor: ActorId = of
        .parse()
        .map_err(|error| ApiError::bad_request(format!("invalid actor id {of:?}: {error}")))?;

    let store = state.store.clone();
    let heads = tokio::task::spawn_blocking(move || store.list_opinions_about(&trusted_actor))
        .await
        .map_err(|error| ApiError::internal(format!("spawn_blocking join failed: {error}")))?
        .map_err(|error| map_store_error(error, "list_opinions_about"))?;

    let dtos = heads
        .into_iter()
        .map(|head| TrustHeadDto {
            by_actor: head.by_actor.to_string(),
            trusted_actor: head.trusted_actor.to_string(),
            statement_id: head.statement_id.to_string(),
            created_at: head.created_at.to_string(),
            decision: head.decision,
        })
        .collect();

    Ok(ApiResult(dtos))
}

#[utoipa::path(
    get,
    path = "/api/v1/trust/{by}/{of}",
    tag = "trust",
    operation_id = "getTrust",
    params(
        ("by" = String, Path, description = "Actor whose opinion to read (kairo:actor:...)"),
        ("of" = String, Path, description = "Actor whose trust state to look up"),
    ),
    responses(
        (status = 200, description = "Latest ActorTrust statement (grant, block, or withdrawal)", body = ActorTrustStatementJson),
        (status = 400, description = "Malformed actor id"),
        (status = 404, description = "No trust opinion recorded; treat as `unknown`"),
    ),
)]
pub async fn handler(
    State(state): State<AppState>,
    Path((by, of)): Path<(String, String)>,
) -> Result<ApiResult<ActorTrustStatementJson>, ApiError> {
    let by_actor: ActorId = by
        .parse()
        .map_err(|error| ApiError::bad_request(format!("invalid by-actor id {by:?}: {error}")))?;
    let trusted_actor: ActorId = of
        .parse()
        .map_err(|error| ApiError::bad_request(format!("invalid of-actor id {of:?}: {error}")))?;

    let store = state.store.clone();
    let signed =
        tokio::task::spawn_blocking(move || store.latest_trust(&by_actor, &trusted_actor))
            .await
            .map_err(|error| ApiError::internal(format!("spawn_blocking join failed: {error}")))?
            .map_err(|error| map_store_error(error, "latest_trust"))?
            .ok_or_else(|| ApiError::not_found("trust opinion not found"))?;

    Ok(ApiResult(ActorTrustStatementJson::from_statement(&signed)))
}
