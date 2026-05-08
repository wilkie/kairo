//! `GET /api/v1/version-tags/{object}/{version}` — chain-leaf
//! `ObjectVersionTag` statement for `(actor, object, version)`,
//! honoring cross-actor `supersedes` flips (the resolver does
//! the capability evaluation transparently — see
//! `specs/CAPABILITIES.md` §6.2).
//!
//! `actor` is an optional `?actor=<id>` query — defaults to the
//! object's `created_by`, mirroring `kairo tag show`.

use axum::extract::{Path, Query, State};
use kairo_core::ObjectId;
use kairo_statement::json::ObjectVersionTagStatementJson;
use kairo_store::VersionTagResolver;

use crate::api::envelope::{ApiError, ApiResult};
use crate::api::handlers::branches::{resolve_actor, ActorQuery};
use crate::api::state::AppState;
use crate::api::store_errors::map_store_error;

#[utoipa::path(
    get,
    path = "/api/v1/version-tags/{object}/{version}",
    tag = "version-tags",
    operation_id = "getLatestVersionTag",
    params(
        ("object" = String, Path, description = "Object id (kairo:object:...)"),
        ("version" = String, Path, description = "Version label (e.g., \"v1.0.0\")"),
        ("actor" = Option<String>, Query, description = "Actor whose tag chain to resolve; defaults to the object's `created_by`"),
    ),
    responses(
        (status = 200, description = "Latest ObjectVersionTag statement honoring cross-actor supersedes flips", body = ObjectVersionTagStatementJson),
        (status = 400, description = "Malformed object id, version, or actor query"),
        (status = 404, description = "Version tag head not found"),
    ),
)]
pub async fn latest_handler(
    State(state): State<AppState>,
    Path((object_id, version)): Path<(String, String)>,
    Query(q): Query<ActorQuery>,
) -> Result<ApiResult<ObjectVersionTagStatementJson>, ApiError> {
    let object: ObjectId = object_id.parse().map_err(|error| {
        ApiError::bad_request(format!("invalid object id {object_id:?}: {error}"))
    })?;

    let actor = resolve_actor(&state, &object, q.actor).await?;

    let store = state.store.clone();
    let signed =
        tokio::task::spawn_blocking(move || store.latest_version_tag(&actor, &object, &version))
            .await
            .map_err(|error| ApiError::internal(format!("spawn_blocking join failed: {error}")))?
            .map_err(|error| map_store_error(error, "latest_version_tag"))?
            .ok_or_else(|| ApiError::not_found("version tag head not found"))?;

    Ok(ApiResult(ObjectVersionTagStatementJson::from_statement(
        &signed,
    )))
}
