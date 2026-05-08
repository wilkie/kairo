//! `GET /api/v1/trust/{by_actor}/{of_actor}` — chain-leaf
//! `ActorTrust` statement (grant, block, or withdrawal).
//! Returns 404 when `by_actor` has never expressed an opinion
//! about `of_actor` (no opinion is its own state — `unknown` —
//! and is *not* an error in CLI tooling, but the daemon API
//! reflects absence as 404 so callers can branch cleanly).

use axum::extract::{Path, State};
use kairo_core::ActorId;
use kairo_statement::json::ActorTrustStatementJson;
use kairo_store::TrustResolver;

use crate::api::envelope::{ApiError, ApiResult};
use crate::api::state::AppState;
use crate::api::store_errors::map_store_error;

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
