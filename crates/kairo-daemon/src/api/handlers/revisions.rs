//! `GET /api/v1/revisions/{object}` — chronological list of
//! `ObjectRevision` heads observable in this store whose body
//! targets `object`. Backed by
//! [`kairo_store::FilesystemStore::list_object_revisions`],
//! which does a full statements-dir scan — no per-object
//! revision index exists in the v1 store layout. Acceptable
//! for inspector workloads on MVP-sized stores; replace with a
//! `RevisionResolver` trait + materialized index if a real
//! workload demands faster listing.
//!
//! The response is a light per-row shape (`RevisionHeadDto`) —
//! id, parents, manifest hash, timestamp. Callers who want the
//! full signed envelope follow up with
//! `GET /api/v1/statements/{statement_id}`.

use axum::extract::{Path, State};
use kairo_core::ObjectId;
use kairo_daemon_client::dto::RevisionHeadDto;

use crate::api::envelope::{ApiError, ApiResult};
use crate::api::state::AppState;
use crate::api::store_errors::map_store_error;

/// `GET /api/v1/revisions/{object_id}`
#[utoipa::path(
    get,
    path = "/api/v1/revisions/{object}",
    tag = "revisions",
    operation_id = "listRevisions",
    params(
        ("object" = String, Path, description = "Object id (kairo:object:...)"),
    ),
    responses(
        (status = 200, description = "Revision heads for the object, sorted by created_at ascending (ties by statement_id)", body = [RevisionHeadDto]),
        (status = 400, description = "Malformed object id"),
    ),
)]
pub async fn list_handler(
    State(state): State<AppState>,
    Path(object_id): Path<String>,
) -> Result<ApiResult<Vec<RevisionHeadDto>>, ApiError> {
    let object: ObjectId = object_id.parse().map_err(|error| {
        ApiError::bad_request(format!("invalid object id {object_id:?}: {error}"))
    })?;

    let store = state.store.clone();
    let revisions = tokio::task::spawn_blocking(move || store.list_object_revisions(&object))
        .await
        .map_err(|error| ApiError::internal(format!("spawn_blocking join failed: {error}")))?
        .map_err(|error| map_store_error(error, "list_object_revisions"))?;

    let dtos = revisions
        .iter()
        .map(|signed| {
            let unsigned = signed.unsigned();
            let body = unsigned.body();
            RevisionHeadDto {
                actor: unsigned.actor().to_string(),
                object: body.object().to_string(),
                revision_id: body.revision().as_str().to_owned(),
                statement_id: unsigned.statement_id().to_string(),
                parents: body
                    .parents()
                    .iter()
                    .map(|p| p.as_str().to_owned())
                    .collect(),
                manifest_hash: body.manifest_hash().to_string(),
                created_at: unsigned.created_at().to_string(),
            }
        })
        .collect();

    Ok(ApiResult(dtos))
}
