//! `GET /api/v1/capabilities/{grantor}` — list of
//! `(grantee, scope)` capability heads issued by the grantor.
//! Mirrors `kairo capability list --grantor` in direct mode.

use axum::extract::{Path, State};
use kairo_core::ActorId;
use kairo_daemon_client::dto::CapabilityHeadDto;
use kairo_statement::json::CapabilityScopeJson;
use kairo_store::CapabilityResolver;

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
