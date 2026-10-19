//! `GET /api/v1/federation/status` and `POST /api/v1/federation/sync`
//! (`specs/DECISIONS.md` §28): follows with their last results, the
//! sync schedule, and starting syncs now.
//!
//! Only on the daemon's Unix socket: `kairo-web` forwards no write but
//! `POST /api/v1/statements`.

use axum::body::Bytes;
use axum::extract::State;
use kairo_core::{ActorId, ObjectId};
use kairo_daemon_client::dto::{
    FederationRunDto, FederationStatusDto, FederationSyncRequestDto, FederationSyncStartedDto,
    FollowStatusDto, PeerOutcomeDto,
};
use kairo_statement::TrustScope;
use kairo_store::{Follow, FollowStore, InboxStore, PeerOutcome};

use crate::api::envelope::{ApiError, ApiResult};
use crate::api::state::AppState;
use crate::api::store_errors::map_store_error;
use crate::federation::{RunKind, TriggerError};

fn peer_dto(outcome: &PeerOutcome) -> PeerOutcomeDto {
    let mut dto = PeerOutcomeDto {
        peer_url: outcome.peer_url().to_owned(),
        outcome: String::new(),
        fetched: None,
        already_held: None,
        incomplete_chains: None,
        flagged: None,
        reason: None,
    };
    match outcome {
        PeerOutcome::Fetched {
            fetched,
            already_held,
            incomplete_chains,
            flagged,
            ..
        } => {
            dto.outcome = "fetched".to_owned();
            dto.fetched = Some(*fetched);
            dto.already_held = Some(*already_held);
            dto.incomplete_chains = Some(*incomplete_chains);
            dto.flagged = Some(*flagged);
        }
        PeerOutcome::NotPublished { .. } => dto.outcome = "not_published".to_owned(),
        PeerOutcome::Failed { reason, .. } => {
            dto.outcome = "failed".to_owned();
            dto.reason = Some(reason.clone());
        }
    }
    dto
}

#[utoipa::path(
    get,
    path = "/api/v1/federation/status",
    tag = "federation",
    operation_id = "getFederationStatus",
    responses(
        (status = 200, description = "Follows with their last sync, the schedule, and recent runs", body = FederationStatusDto),
    ),
)]
pub async fn status_handler(
    State(state): State<AppState>,
) -> Result<ApiResult<FederationStatusDto>, ApiError> {
    let federation = state.federation.clone();
    let (config, _) = federation
        .config()
        .map_err(|error| ApiError::internal(format!("node configuration: {error}")))?;
    let store = state.store.clone();
    let (follows, inbox_entries) = tokio::task::spawn_blocking(move || {
        let follows = store.list_follows()?;
        let inbox = store.list_inbox()?.len();
        Ok::<_, kairo_store::StoreError>((follows, inbox))
    })
    .await
    .map_err(|error| ApiError::internal(format!("spawn_blocking join failed: {error}")))?
    .map_err(|error| map_store_error(error, "federation status"))?;
    let running = federation.running();
    Ok(ApiResult(FederationStatusDto {
        enabled: config.enabled,
        interval_secs: config.sync.interval_secs,
        inbox_secs: config.sync.inbox_secs,
        max_concurrent: config.sync.max_concurrent,
        follows: follows
            .into_iter()
            .map(|record| FollowStatusDto {
                follow: record.follow.to_string(),
                running: running.contains(&record.follow),
                added_at: record.added_at.to_string(),
                last_attempt: record.last_attempt.as_ref().map(|a| a.at.to_string()),
                last_success: record.last_success.map(|at| at.to_string()),
                peers: record
                    .last_attempt
                    .map(|attempt| attempt.peers.iter().map(peer_dto).collect())
                    .unwrap_or_default(),
            })
            .collect(),
        inbox_entries: inbox_entries as u64,
        runs: federation
            .runs()
            .into_iter()
            .map(|run| {
                let (kind, follow) = match run.kind {
                    RunKind::Sync(follow) => ("sync", Some(follow.to_string())),
                    RunKind::Inbox => ("inbox", None),
                    RunKind::Discover => ("discover", None),
                };
                FederationRunDto {
                    id: run.id,
                    kind: kind.to_owned(),
                    follow,
                    started_at: run.started_at.to_string(),
                    finished_at: run.finished_at.map(|at| at.to_string()),
                    result: run.result,
                }
            })
            .collect(),
    }))
}

fn requested_follow(request: &FederationSyncRequestDto) -> Result<Option<Follow>, ApiError> {
    match (&request.object, &request.trust_by, &request.scope) {
        (None, None, None) => Ok(None),
        (Some(object), None, None) => ObjectId::new(object.clone())
            .map(|object| Some(Follow::Object(object)))
            .map_err(|error| ApiError::bad_request(format!("invalid object {object:?}: {error}"))),
        (None, Some(by), Some(scope)) => {
            let by = ActorId::new(by.clone())
                .map_err(|error| ApiError::bad_request(format!("invalid actor {by:?}: {error}")))?;
            let scope = TrustScope::parse(scope)
                .map_err(|error| ApiError::bad_request(error.to_string()))?;
            Ok(Some(Follow::Trust { by, scope }))
        }
        _ => Err(ApiError::bad_request(
            "name an object, or trust_by with scope, or nothing for every follow",
        )),
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/federation/sync",
    tag = "federation",
    operation_id = "startFederationSync",
    request_body(content = FederationSyncRequestDto, description = "One follow, or empty for every follow", content_type = "application/json"),
    responses(
        (status = 200, description = "Syncs started; results arrive in the status", body = FederationSyncStartedDto),
        (status = 400, description = "Malformed request"),
        (status = 404, description = "The named follow isn't followed"),
        (status = 409, description = "Federation is disabled"),
    ),
)]
pub async fn sync_handler(
    State(state): State<AppState>,
    body: Bytes,
) -> Result<ApiResult<FederationSyncStartedDto>, ApiError> {
    let request: FederationSyncRequestDto = if body.is_empty() {
        FederationSyncRequestDto::default()
    } else {
        serde_json::from_slice(&body)
            .map_err(|error| ApiError::bad_request(format!("malformed sync request: {error}")))?
    };
    let only = requested_follow(&request)?;
    let federation = state.federation.clone();
    let triggered = tokio::task::spawn_blocking(move || federation.trigger(only.as_ref()))
        .await
        .map_err(|error| ApiError::internal(format!("spawn_blocking join failed: {error}")))?
        .map_err(|error| match error {
            TriggerError::Disabled(message) => ApiError::federation_disabled(message),
            TriggerError::NotFollowed(follow) => {
                ApiError::not_found(format!("{follow} is not followed"))
            }
            TriggerError::Unavailable(message) => ApiError::internal(message),
        })?;
    let names = |follows: Vec<Follow>| follows.iter().map(ToString::to_string).collect();
    Ok(ApiResult(FederationSyncStartedDto {
        started: names(triggered.started),
        already_running: names(triggered.already_running),
        deferred: names(triggered.deferred),
    }))
}
