//! `GET /api/v1/branches/...` handlers.
//!
//! - `/branches/{object}` — list of `(actor, name)` branch
//!   tips (light summary, one per chain leaf).
//! - `/branches/{object}/{name}/latest?actor=<id>` — the
//!   chain-leaf `ObjectBranch` statement. `actor` is optional
//!   — defaults to the object's `created_by`, mirroring the
//!   `kairo branch show` convention.

use axum::extract::{Path, Query, State};
use kairo_core::{ActorId, ObjectId};
use kairo_daemon_client::dto::BranchTipDto;
use kairo_statement::json::ObjectBranchStatementJson;
use kairo_store::{BranchResolver, ObjectStore};
use serde::Deserialize;

use crate::api::envelope::{ApiError, ApiResult};
use crate::api::state::AppState;
use crate::api::store_errors::map_store_error;

#[derive(Debug, Deserialize)]
pub struct ActorQuery {
    pub actor: Option<String>,
}

/// `GET /api/v1/branches/{object_id}`
pub async fn list_handler(
    State(state): State<AppState>,
    Path(object_id): Path<String>,
) -> Result<ApiResult<Vec<BranchTipDto>>, ApiError> {
    let object: ObjectId = object_id.parse().map_err(|error| {
        ApiError::bad_request(format!("invalid object id {object_id:?}: {error}"))
    })?;

    let store = state.store.clone();
    let tips = tokio::task::spawn_blocking(move || store.list_branches(&object))
        .await
        .map_err(|error| ApiError::internal(format!("spawn_blocking join failed: {error}")))?
        .map_err(|error| map_store_error(error, "list_branches"))?;

    let dtos = tips
        .into_iter()
        .map(|tip| BranchTipDto {
            actor: tip.actor.to_string(),
            object: tip.object.to_string(),
            name: tip.name,
            statement_id: tip.statement_id.to_string(),
            created_at: tip.created_at.to_string(),
        })
        .collect();

    Ok(ApiResult(dtos))
}

/// `GET /api/v1/branches/{object_id}/{name}/latest?actor=<id>`
pub async fn latest_handler(
    State(state): State<AppState>,
    Path((object_id, name)): Path<(String, String)>,
    Query(q): Query<ActorQuery>,
) -> Result<ApiResult<ObjectBranchStatementJson>, ApiError> {
    let object: ObjectId = object_id.parse().map_err(|error| {
        ApiError::bad_request(format!("invalid object id {object_id:?}: {error}"))
    })?;

    let actor = resolve_actor(&state, &object, q.actor).await?;

    let store = state.store.clone();
    let signed = tokio::task::spawn_blocking(move || store.latest_branch(&actor, &object, &name))
        .await
        .map_err(|error| ApiError::internal(format!("spawn_blocking join failed: {error}")))?
        .map_err(|error| map_store_error(error, "latest_branch"))?
        .ok_or_else(|| ApiError::not_found("branch head not found"))?;

    Ok(ApiResult(ObjectBranchStatementJson::from_statement(&signed)))
}

/// Resolve the effective actor for branch / version-tag lookup
/// — `?actor=<id>` if supplied, otherwise the object's
/// `created_by` from the genesis. Surfaces a 404 when the
/// genesis itself is missing (you can't ask "default actor for
/// object X" if there's no X), and a 400 when `?actor=` is
/// shape-invalid.
pub(crate) async fn resolve_actor(
    state: &AppState,
    object: &ObjectId,
    actor_param: Option<String>,
) -> Result<ActorId, ApiError> {
    if let Some(raw) = actor_param {
        return raw
            .parse()
            .map_err(|error| ApiError::bad_request(format!("invalid actor id {raw:?}: {error}")));
    }

    let store = state.store.clone();
    let object_clone = object.clone();
    let genesis = tokio::task::spawn_blocking(move || store.get_object_genesis(&object_clone))
        .await
        .map_err(|error| ApiError::internal(format!("spawn_blocking join failed: {error}")))?
        .map_err(|error| map_store_error(error, "get_object_genesis"))?;

    Ok(genesis.body().created_by().clone())
}
