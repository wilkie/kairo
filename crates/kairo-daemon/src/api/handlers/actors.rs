//! `GET /api/v1/actors/...` handlers.
//!
//! - `/actors/{id}` — actor genesis JSON.
//! - `/actors/{id}/statements` — the per-actor signed-statement
//!   audit list, backed by the store's `statements_by_actor` index
//!   (see `kairo-store::statements_by_actor`). One entry per signed
//!   envelope; `ObjectGenesis` is intentionally absent (it uses
//!   `created_by`, not the envelope `actor` field every other
//!   statement type uses).
//!
//! Path ids are parsed into `ActorId` (400 `bad_request` on shape
//! failure). Store reads run on `spawn_blocking` since
//! `FilesystemStore` is sync.

use axum::extract::{Path, State};
use kairo_core::ActorId;
use kairo_daemon_client::dto::StatementByActorDto;
use kairo_identity::json::ActorGenesisJson;
use kairo_store::{ActorStore, StatementByActorResolver};

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

/// `GET /api/v1/actors/{id}/statements`
#[utoipa::path(
    get,
    path = "/api/v1/actors/{id}/statements",
    tag = "actors",
    operation_id = "listStatementsByActor",
    params(
        ("id" = String, Path, description = "Actor id (kairo:actor:...)"),
    ),
    responses(
        (status = 200, description = "Signed statements authored by the actor, sorted by (created_at, statement_id) ascending", body = [StatementByActorDto]),
        (status = 400, description = "Malformed actor id"),
    ),
)]
pub async fn list_statements_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<ApiResult<Vec<StatementByActorDto>>, ApiError> {
    let actor_id: ActorId = id
        .parse()
        .map_err(|error| ApiError::bad_request(format!("invalid actor id {id:?}: {error}")))?;

    let store = state.store.clone();
    let summaries = tokio::task::spawn_blocking(move || store.list_statements_by_actor(&actor_id))
        .await
        .map_err(|error| ApiError::internal(format!("spawn_blocking join failed: {error}")))?
        .map_err(|error| map_store_error(error, "list_statements_by_actor"))?;

    let dtos = summaries
        .into_iter()
        .map(|s| StatementByActorDto {
            actor: s.actor.to_string(),
            statement_id: s.statement_id.to_string(),
            kind: s.kind.as_str().to_owned(),
            created_at: s.created_at.to_string(),
        })
        .collect();

    Ok(ApiResult(dtos))
}
