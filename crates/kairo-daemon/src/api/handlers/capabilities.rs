//! `GET /api/v1/capabilities/...` handlers.
//!
//! - `/capabilities/{grantor}` — list of `(grantee, scope)`
//!   capability heads issued by the grantor. Mirrors
//!   `kairo capability list --grantor` in direct mode.
//! - `/capabilities/for-object/{object}` — capability heads
//!   scoped to the given object, hydrated with each head's
//!   scope (a per-row follow-up read against the underlying
//!   `ActorCapabilityGrant` statement). N+1 by design — the
//!   per-object index intentionally stays light, and inspector
//!   workloads have small N. Add a richer materialized index
//!   if a real workload demands it.

use axum::extract::{Path, State};
use kairo_core::{ActorId, ObjectId};
use kairo_daemon_client::dto::CapabilityHeadDto;
use kairo_statement::json::CapabilityScopeJson;
use kairo_store::{CapabilityResolver, StatementStore};

use crate::api::envelope::{ApiError, ApiResult};
use crate::api::state::AppState;
use crate::api::store_errors::map_store_error;

#[utoipa::path(
    get,
    path = "/api/v1/capabilities/{grantor}",
    tag = "capabilities",
    operation_id = "listCapabilitiesFromGrantor",
    params(
        ("grantor" = String, Path, description = "Grantor actor id (kairo:actor:...)"),
    ),
    responses(
        (status = 200, description = "Capability head summaries: one per (grantee, scope) chain leaf", body = [CapabilityHeadDto]),
        (status = 400, description = "Malformed grantor id"),
    ),
)]
pub async fn list_from_handler(
    State(state): State<AppState>,
    Path(grantor): Path<String>,
) -> Result<ApiResult<Vec<CapabilityHeadDto>>, ApiError> {
    let grantor_id: ActorId = grantor.parse().map_err(|error| {
        ApiError::bad_request(format!("invalid grantor id {grantor:?}: {error}"))
    })?;

    let store = state.store.clone();
    let heads = tokio::task::spawn_blocking(move || store.list_capabilities_from(&grantor_id))
        .await
        .map_err(|error| ApiError::internal(format!("spawn_blocking join failed: {error}")))?
        .map_err(|error| map_store_error(error, "list_capabilities_from"))?;

    let dtos = heads
        .into_iter()
        .map(|head| CapabilityHeadDto {
            grantor: head.grantor.to_string(),
            grantee: head.grantee.to_string(),
            scope: CapabilityScopeJson::from_scope(&head.scope),
            statement_id: head.statement_id.to_string(),
            created_at: head.created_at.to_string(),
        })
        .collect();

    Ok(ApiResult(dtos))
}

/// `GET /api/v1/capabilities/for-object/{object_id}`
#[utoipa::path(
    get,
    path = "/api/v1/capabilities/for-object/{object}",
    tag = "capabilities",
    operation_id = "listCapabilitiesForObject",
    params(
        ("object" = String, Path, description = "Object id (kairo:object:...)"),
    ),
    responses(
        (status = 200, description = "Capability head summaries scoped to this object, hydrated with each head's scope", body = [CapabilityHeadDto]),
        (status = 400, description = "Malformed object id"),
    ),
)]
pub async fn list_for_object_handler(
    State(state): State<AppState>,
    Path(object_id): Path<String>,
) -> Result<ApiResult<Vec<CapabilityHeadDto>>, ApiError> {
    let object: ObjectId = object_id.parse().map_err(|error| {
        ApiError::bad_request(format!("invalid object id {object_id:?}: {error}"))
    })?;

    let store = state.store.clone();
    let dtos = tokio::task::spawn_blocking(move || -> Result<Vec<CapabilityHeadDto>, ApiError> {
        let heads = store
            .list_capabilities_for_object(&object)
            .map_err(|error| map_store_error(error, "list_capabilities_for_object"))?;
        heads
            .into_iter()
            .map(|head| {
                let signed = store
                    .get_actor_capability_grant(&head.statement_id)
                    .map_err(|error| map_store_error(error, "get_actor_capability_grant"))?;
                Ok(CapabilityHeadDto {
                    grantor: head.grantor.to_string(),
                    grantee: head.grantee.to_string(),
                    scope: CapabilityScopeJson::from_scope(
                        signed.unsigned().body().capability().scope(),
                    ),
                    statement_id: head.statement_id.to_string(),
                    created_at: head.created_at.to_string(),
                })
            })
            .collect()
    })
    .await
    .map_err(|error| ApiError::internal(format!("spawn_blocking join failed: {error}")))??;

    Ok(ApiResult(dtos))
}
