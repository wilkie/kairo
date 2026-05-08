//! `GET /api/v1/actors/{actor_id}` — returns the actor's
//! genesis JSON.
//!
//! Path id is parsed into `ActorId` (400 `bad_request` on
//! shape failure). The store read runs on `spawn_blocking`
//! since `FilesystemStore` is sync.

use axum::extract::{Path, State};
use kairo_core::ActorId;
use kairo_identity::json::ActorGenesisJson;
use kairo_store::ActorStore;

use crate::api::envelope::{ApiError, ApiResult};
use crate::api::state::AppState;
use crate::api::store_errors::map_store_error;

#[utoipa::path(
    get,
    path = "/api/v1/actors/{id}",
    tag = "actors",
    operation_id = "getActor",
    params(
        ("id" = String, Path, description = "Actor id (kairo:actor:...)"),
    ),
    responses(
        (status = 200, description = "Actor genesis JSON", body = ActorGenesisJson),
        (status = 400, description = "Malformed actor id"),
        (status = 404, description = "Actor not found"),
    ),
)]
pub async fn handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<ApiResult<ActorGenesisJson>, ApiError> {
    let actor_id: ActorId = id
        .parse()
        .map_err(|error| ApiError::bad_request(format!("invalid actor id {id:?}: {error}")))?;

    let store = state.store.clone();
    let body = tokio::task::spawn_blocking(move || store.get_actor(&actor_id))
        .await
        .map_err(|error| ApiError::internal(format!("spawn_blocking join failed: {error}")))?
        .map_err(|error| map_store_error(error, "get_actor"))?;

    Ok(ApiResult(ActorGenesisJson::from_body(&body)))
}
