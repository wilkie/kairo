//! `GET /api/v1/capabilities/...` handlers.
//!
//! - `/capabilities/{grantor}` — list of `(grantee, scope)`
//!   capability heads issued by the grantor. Mirrors
//!   `kairo capability list --grantor` in direct mode.
//! - `/capabilities/for-object/{object}` — capability heads
//!   scoped to the given object.
//!
//! Both hydrate each head from its `ActorCapabilityGrant`
//! (scope, statement kinds, delegable, constraints) and attach
//! the grantor's revocation of it, if any — two per-row
//! follow-up reads. N+1 by design: the indices intentionally
//! stay light, and inspector workloads have small N. Add a
//! richer materialized index if a real workload demands it.

use axum::extract::{Path, State};
use kairo_core::{ActorId, ObjectId, StatementId, Timestamp};
use kairo_daemon_client::dto::{CapabilityHeadDto, CapabilityRevocationDto};
use kairo_statement::json::ActorCapabilityGrantBodyJson;
use kairo_store::{CapabilityResolver, FilesystemStore, StatementStore};

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
    let dtos = tokio::task::spawn_blocking(move || -> Result<Vec<CapabilityHeadDto>, ApiError> {
        let heads = store
            .list_capabilities_from(&grantor_id)
            .map_err(|error| map_store_error(error, "list_capabilities_from"))?;
        heads
            .into_iter()
            .map(|head| {
                hydrate_head(
                    &store,
                    &head.grantor,
                    &head.grantee,
                    &head.statement_id,
                    head.created_at,
                )
            })
            .collect()
    })
    .await
    .map_err(|error| ApiError::internal(format!("spawn_blocking join failed: {error}")))??;

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
                hydrate_head(
                    &store,
                    &head.grantor,
                    &head.grantee,
                    &head.statement_id,
                    head.created_at,
                )
            })
            .collect()
    })
    .await
    .map_err(|error| ApiError::internal(format!("spawn_blocking join failed: {error}")))??;

    Ok(ApiResult(dtos))
}

/// Build a [`CapabilityHeadDto`] for one chain head: read its
/// grant for the capability fields, and the grantor's
/// most-restrictive revocation of it, if any.
fn hydrate_head(
    store: &FilesystemStore,
    grantor: &ActorId,
    grantee: &ActorId,
    statement_id: &StatementId,
    created_at: Timestamp,
) -> Result<CapabilityHeadDto, ApiError> {
    let grant = store
        .get_actor_capability_grant(statement_id)
        .map_err(|error| map_store_error(error, "get_actor_capability_grant"))?;
    let capability = ActorCapabilityGrantBodyJson::from_body(grant.unsigned().body()).capability;
    let revocation = store
        .latest_capability_revocation(grantor, statement_id)
        .map_err(|error| map_store_error(error, "latest_capability_revocation"))?
        .map(|signed| CapabilityRevocationDto {
            statement_id: signed.statement_id().to_string(),
            retroactive: signed.unsigned().body().retroactive(),
            created_at: signed.unsigned().created_at().to_string(),
        });
    Ok(CapabilityHeadDto {
        grantor: grantor.to_string(),
        grantee: grantee.to_string(),
        scope: capability.scope,
        statement_id: statement_id.to_string(),
        created_at: created_at.to_string(),
        statement_kinds: capability.statement_kinds,
        delegable: capability.delegable,
        constraints: capability.constraints,
        revocation,
    })
}
