//! `GET /api/v1/objects/{id}/log` — an object's revision history as a
//! log, walked like `git log` (`specs/DECISIONS.md` §20).
//!
//! The walk starts from every revision this node holds for the object
//! (so one just signed shows before any branch points at it), or from
//! the revision statements named by `from`, and follows parent links
//! newest first. What a federation peer receives is narrower:
//! revisions reachable from the object's refs.
//! It is backed by the store's per-object revision index, so a page
//! costs the history walked, not the store's size. `cursor` is an
//! opaque position the previous page returned.
//!
//! Rows are the light `RevisionHeadDto`; callers who want the full
//! signed envelope follow up with `GET /api/v1/statements/{id}`.

use axum::extract::{Path, Query, State};
use kairo_core::{ObjectId, StatementId};
use kairo_daemon_client::dto::{RevisionHeadDto, RevisionLogPage};
use serde::Deserialize;

use crate::api::envelope::{ApiError, ApiResult};
use crate::api::state::AppState;
use crate::api::store_errors::map_store_error;

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 500;

#[derive(Debug, Deserialize)]
pub struct LogQuery {
    /// Comma-separated revision statement ids to start from.
    pub from: Option<String>,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
}

/// `GET /api/v1/objects/{id}/log`
#[utoipa::path(
    get,
    path = "/api/v1/objects/{id}/log",
    tag = "revisions",
    operation_id = "getRevisionLog",
    params(
        ("id" = String, Path, description = "Object id"),
        ("from" = Option<String>, Query, description = "Comma-separated revision statement ids to walk from; defaults to every revision this node holds for the object"),
        ("limit" = Option<usize>, Query, description = "Page size, 1 to 500; default 50"),
        ("cursor" = Option<String>, Query, description = "`next_cursor` from the previous page"),
    ),
    responses(
        (status = 200, description = "Revisions reachable from the starting points along parent links, newest first", body = RevisionLogPage),
        (status = 400, description = "Malformed object id, statement id, limit, or cursor"),
    ),
)]
pub async fn log_handler(
    State(state): State<AppState>,
    Path(object_id): Path<String>,
    Query(query): Query<LogQuery>,
) -> Result<ApiResult<RevisionLogPage>, ApiError> {
    let object: ObjectId = object_id.parse().map_err(|error| {
        ApiError::bad_request(format!("invalid object id {object_id:?}: {error}"))
    })?;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT);
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(ApiError::bad_request(format!(
            "limit must be from 1 to {MAX_LIMIT}"
        )));
    }
    let skip = match &query.cursor {
        None => 0,
        Some(raw) => raw
            .parse::<usize>()
            .map_err(|_| ApiError::bad_request(format!("invalid cursor {raw:?}")))?,
    };
    let from: Option<Vec<StatementId>> = query
        .from
        .as_deref()
        .map(|raw| {
            raw.split(',')
                .filter(|part| !part.is_empty())
                .map(|part| {
                    part.parse().map_err(|error| {
                        ApiError::bad_request(format!("invalid statement id {part:?}: {error}"))
                    })
                })
                .collect()
        })
        .transpose()?;

    let store = state.store.clone();
    let log = tokio::task::spawn_blocking(move || {
        let tips = match from {
            Some(tips) => tips,
            None => store.revision_statement_ids(&object)?,
        };
        store.revision_log(&object, &tips, skip, limit)
    })
    .await
    .map_err(|error| ApiError::internal(format!("spawn_blocking join failed: {error}")))?
    .map_err(|error| map_store_error(error, "revision_log"))?;

    let revisions = log
        .revisions
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
        .collect::<Vec<_>>();
    let next_cursor = log.more.then(|| (skip + revisions.len()).to_string());

    Ok(ApiResult(RevisionLogPage {
        revisions,
        next_cursor,
    }))
}
