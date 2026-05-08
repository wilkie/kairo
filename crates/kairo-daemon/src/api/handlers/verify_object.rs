//! `GET /api/v1/verify-object/{id}` — verify an object's
//! statement-layer state and return a structured
//! [`ValidationResult`](kairo_daemon_client::dto::ValidationResult).
//!
//! The check composes pieces that already exist:
//!
//! - `FilesystemStore::get_object_genesis` — load the genesis
//!   envelope (404 when missing).
//! - `FilesystemStore::latest_branch` — pick the head branch
//!   tip; default actor is the genesis's `created_by`, default
//!   name is `head`. Genesis-only state (no branch tip) is
//!   reported as `Valid` with an info note.
//! - `kairo_statement::verify::verify_envelope_statement` —
//!   verify the revision envelope's signature against the
//!   actor's rotation chain.
//! - `kairo_object::validate_object_revision` — statement-layer
//!   consistency between the revision and the genesis.
//!
//! Manifest and Git content-layer checks are explicitly *not*
//! run here. The daemon serves a store, not a working tree —
//! those checks belong in `kairo verify object` on the CLI side
//! (see `specs/PHASE_2_WEB_CLIENT.md` slice 2 deliberate gaps).

use axum::extract::{Path, State};
use kairo_core::{ActorId, ObjectId};
use kairo_daemon_client::dto::{
    ValidationIssue, ValidationIssueSeverity, ValidationResult, ValidationStatus,
};
use kairo_object::{
    validate_object_revision, ContentLayerCheck, ManifestBindingCheck, ObjectConsistencyCheck,
    ObjectRevisionValidationReport,
};
use kairo_statement::verify::{
    verify_envelope_statement, ActorResolution, SignatureStatus, VerificationReport,
};
use kairo_store::{BranchResolver, FilesystemStore, ObjectStore, StatementStore, StoreError};

use crate::api::envelope::{ApiError, ApiResult};
use crate::api::state::AppState;
use crate::api::store_errors::map_store_error;

const DEFAULT_BRANCH_NAME: &str = "head";

#[utoipa::path(
    get,
    path = "/api/v1/verify-object/{id}",
    tag = "verify",
    operation_id = "verifyObject",
    params(
        ("id" = String, Path, description = "Object id (kairo:object:...)"),
    ),
    responses(
        (
            status = 200,
            description = "Verification result. Status is the worst-of-fold across all checks; issues list explains every contributing finding.",
            body = ValidationResult,
        ),
        (status = 400, description = "Malformed object id"),
        (status = 404, description = "Object genesis not found"),
    ),
)]
pub async fn handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<ApiResult<ValidationResult>, ApiError> {
    let object_id: ObjectId = id
        .parse()
        .map_err(|error| ApiError::bad_request(format!("invalid object id {id:?}: {error}")))?;

    let store = state.store.clone();
    let result = tokio::task::spawn_blocking(move || verify_object(&store, &object_id))
        .await
        .map_err(|error| ApiError::internal(format!("spawn_blocking join failed: {error}")))??;

    Ok(ApiResult(result))
}

/// Run the verify pipeline against a store snapshot. Pure sync;
/// extracted from the handler so unit tests can drive it without
/// an HTTP boundary.
fn verify_object(
    store: &FilesystemStore,
    object_id: &ObjectId,
) -> Result<ValidationResult, ApiError> {
    let genesis = store
        .get_object_genesis(object_id)
        .map_err(|error| map_store_error(error, "get_object_genesis"))?;
    let default_actor: ActorId = genesis.body().created_by().clone();

    let mut acc = IssueAccumulator::new();

    let branch_lookup = store.latest_branch(&default_actor, object_id, DEFAULT_BRANCH_NAME);
    let branch_tip = match branch_lookup {
        Ok(tip) => tip,
        Err(StoreError::Missing) => None,
        Err(error) => return Err(map_store_error(error, "latest_branch")),
    };

    let (revision_statement_id, branch_name) = match branch_tip {
        None => {
            acc.push(
                ValidationIssue {
                    kind: "branch_head_missing".to_owned(),
                    severity: ValidationIssueSeverity::Info,
                    message: format!(
                        "no \"{DEFAULT_BRANCH_NAME}\" branch tip for actor {default_actor}; \
                         object is genesis-only"
                    ),
                    statement_id: None,
                    actor_id: Some(default_actor.to_string()),
                    details: serde_json::Value::Null,
                },
                ValidationStatus::Valid,
            );
            (None, None)
        }
        Some(tip) => {
            let revision_id = tip.unsigned().body().revision().clone();
            let revision = store
                .get_object_revision(&revision_id)
                .map_err(|error| map_store_error(error, "get_object_revision"))?;
            let revision_statement_id = revision.statement_id().to_string();

            let report = verify_envelope_statement(&revision, store);
            record_signature_issues(&mut acc, &report);

            let validation = validate_object_revision(&revision, Some(&genesis), None, None);
            record_validation_issues(&mut acc, &validation);

            (
                Some(revision_statement_id),
                Some(DEFAULT_BRANCH_NAME.to_owned()),
            )
        }
    };

    Ok(ValidationResult {
        object_id: object_id.to_string(),
        status: acc.status,
        issues: acc.issues,
        revision_statement_id,
        branch_name,
    })
}

fn record_signature_issues(acc: &mut IssueAccumulator, report: &VerificationReport) {
    let stmt = report.statement_id.to_string();
    let actor = report.envelope_actor.to_string();

    match &report.actor {
        ActorResolution::Resolved => {}
        ActorResolution::NotFound => acc.push(
            ValidationIssue {
                kind: "actor_not_found".to_owned(),
                severity: ValidationIssueSeverity::Error,
                message: format!("actor {actor} has no genesis in this store"),
                statement_id: Some(stmt.clone()),
                actor_id: Some(actor.clone()),
                details: serde_json::Value::Null,
            },
            ValidationStatus::Invalid,
        ),
        ActorResolution::SignatureActorMismatch => acc.push(
            ValidationIssue {
                kind: "signature_actor_mismatch".to_owned(),
                severity: ValidationIssueSeverity::Error,
                message: "envelope actor does not match signature actor".to_owned(),
                statement_id: Some(stmt.clone()),
                actor_id: Some(actor.clone()),
                details: serde_json::Value::Null,
            },
            ValidationStatus::Invalid,
        ),
        ActorResolution::ResolverUnavailable(reason) => acc.push(
            ValidationIssue {
                kind: "actor_resolver_unavailable".to_owned(),
                severity: ValidationIssueSeverity::Warning,
                message: format!("actor resolver unavailable: {reason}"),
                statement_id: Some(stmt.clone()),
                actor_id: Some(actor.clone()),
                details: serde_json::Value::Null,
            },
            ValidationStatus::Indeterminate,
        ),
    }

    match &report.signature {
        SignatureStatus::Valid | SignatureStatus::NotEvaluated => {}
        SignatureStatus::Invalid => acc.push(
            ValidationIssue {
                kind: "signature_invalid".to_owned(),
                severity: ValidationIssueSeverity::Error,
                message: "signature did not verify against the resolved active key".to_owned(),
                statement_id: Some(stmt.clone()),
                actor_id: Some(actor.clone()),
                details: serde_json::Value::Null,
            },
            ValidationStatus::Invalid,
        ),
        SignatureStatus::UnsupportedAlgorithm(alg) => acc.push(
            ValidationIssue {
                kind: "signature_unsupported_algorithm".to_owned(),
                severity: ValidationIssueSeverity::Error,
                message: format!("unsupported signature algorithm {alg:?}"),
                statement_id: Some(stmt.clone()),
                actor_id: Some(actor.clone()),
                details: serde_json::Value::Null,
            },
            ValidationStatus::Invalid,
        ),
        SignatureStatus::Malformed {
            expected_len,
            actual_len,
        } => acc.push(
            ValidationIssue {
                kind: "signature_malformed".to_owned(),
                severity: ValidationIssueSeverity::Error,
                message: format!(
                    "signature is the wrong length (expected {expected_len}, got {actual_len})"
                ),
                statement_id: Some(stmt.clone()),
                actor_id: Some(actor.clone()),
                details: serde_json::Value::Null,
            },
            ValidationStatus::Invalid,
        ),
        SignatureStatus::AlgorithmMismatch => acc.push(
            ValidationIssue {
                kind: "signature_algorithm_mismatch".to_owned(),
                severity: ValidationIssueSeverity::Error,
                message: "signature algorithm does not match the resolved key".to_owned(),
                statement_id: Some(stmt.clone()),
                actor_id: Some(actor.clone()),
                details: serde_json::Value::Null,
            },
            ValidationStatus::Invalid,
        ),
        SignatureStatus::KeyMismatch {
            signature_key_id,
            active_key_id,
        } => acc.push(
            ValidationIssue {
                kind: "signature_key_mismatch".to_owned(),
                severity: ValidationIssueSeverity::Error,
                message: format!(
                    "signature key {signature_key_id:?} is not the active key \
                     ({active_key_id:?}) at this statement's created_at"
                ),
                statement_id: Some(stmt.clone()),
                actor_id: Some(actor.clone()),
                details: serde_json::Value::Null,
            },
            ValidationStatus::Invalid,
        ),
        SignatureStatus::KeyRevoked => acc.push(
            ValidationIssue {
                kind: "signature_key_revoked".to_owned(),
                severity: ValidationIssueSeverity::Error,
                message: "signing key was revoked at this statement's created_at".to_owned(),
                statement_id: Some(stmt.clone()),
                actor_id: Some(actor.clone()),
                details: serde_json::Value::Null,
            },
            ValidationStatus::Invalid,
        ),
        SignatureStatus::NoActiveKey => acc.push(
            ValidationIssue {
                kind: "signature_no_active_key".to_owned(),
                severity: ValidationIssueSeverity::Error,
                message: "actor has no active key at this statement's created_at".to_owned(),
                statement_id: Some(stmt.clone()),
                actor_id: Some(actor.clone()),
                details: serde_json::Value::Null,
            },
            ValidationStatus::Invalid,
        ),
        SignatureStatus::NotInAttestationSet { signature_key_id } => acc.push(
            ValidationIssue {
                kind: "signature_not_in_attestation_set".to_owned(),
                severity: ValidationIssueSeverity::Error,
                message: format!(
                    "emergency signature key {signature_key_id:?} is not in the actor's \
                     attestation set"
                ),
                statement_id: Some(stmt.clone()),
                actor_id: Some(actor.clone()),
                details: serde_json::Value::Null,
            },
            ValidationStatus::Invalid,
        ),
        SignatureStatus::BelowThreshold { provided, required } => acc.push(
            ValidationIssue {
                kind: "signature_below_threshold".to_owned(),
                severity: ValidationIssueSeverity::Error,
                message: format!(
                    "multi-signature envelope provided {provided} signatures; \
                     required {required}"
                ),
                statement_id: Some(stmt.clone()),
                actor_id: Some(actor.clone()),
                details: serde_json::Value::Null,
            },
            ValidationStatus::Invalid,
        ),
    }
}

fn record_validation_issues(acc: &mut IssueAccumulator, report: &ObjectRevisionValidationReport) {
    let stmt = report.statement_id.to_string();

    match &report.object_consistency {
        ObjectConsistencyCheck::Consistent => {}
        ObjectConsistencyCheck::Mismatch { expected, actual } => acc.push(
            ValidationIssue {
                kind: "object_consistency_mismatch".to_owned(),
                severity: ValidationIssueSeverity::Error,
                message: format!(
                    "revision binds to object {actual} but genesis derives {expected}"
                ),
                statement_id: Some(stmt.clone()),
                actor_id: None,
                details: serde_json::Value::Null,
            },
            ValidationStatus::Invalid,
        ),
        ObjectConsistencyCheck::GenesisNotProvided => acc.push(
            ValidationIssue {
                kind: "object_consistency_indeterminate".to_owned(),
                severity: ValidationIssueSeverity::Warning,
                message: "genesis was not provided to the validator".to_owned(),
                statement_id: Some(stmt.clone()),
                actor_id: None,
                details: serde_json::Value::Null,
            },
            ValidationStatus::Indeterminate,
        ),
    }

    match &report.manifest_binding {
        ManifestBindingCheck::Bound => {}
        ManifestBindingCheck::HashMismatch { expected, actual } => acc.push(
            ValidationIssue {
                kind: "manifest_hash_mismatch".to_owned(),
                severity: ValidationIssueSeverity::Error,
                message: format!(
                    "revision declares manifest hash {expected} but the resolved manifest \
                     hashes to {actual}"
                ),
                statement_id: Some(stmt.clone()),
                actor_id: None,
                details: serde_json::Value::Null,
            },
            ValidationStatus::Invalid,
        ),
        ManifestBindingCheck::DeclaredObjectMismatch { expected, actual } => acc.push(
            ValidationIssue {
                kind: "manifest_declared_object_mismatch".to_owned(),
                severity: ValidationIssueSeverity::Error,
                message: format!(
                    "manifest declares object {actual} but the revision binds to {expected}"
                ),
                statement_id: Some(stmt.clone()),
                actor_id: None,
                details: serde_json::Value::Null,
            },
            ValidationStatus::Invalid,
        ),
        ManifestBindingCheck::ManifestNotProvided => acc.push(
            ValidationIssue {
                kind: "manifest_not_provided".to_owned(),
                severity: ValidationIssueSeverity::Info,
                message: "no manifest available; the daemon does not resolve manifests \
                          from a working tree"
                    .to_owned(),
                statement_id: Some(stmt.clone()),
                actor_id: None,
                details: serde_json::Value::Null,
            },
            ValidationStatus::Indeterminate,
        ),
    }

    match &report.content {
        ContentLayerCheck::Verified => {}
        ContentLayerCheck::ParentMismatch { expected, actual } => acc.push(
            ValidationIssue {
                kind: "content_layer_parent_mismatch".to_owned(),
                severity: ValidationIssueSeverity::Error,
                message: format!(
                    "Git commit parents disagree with the revision (expected {expected:?}, \
                     got {actual:?})"
                ),
                statement_id: Some(stmt.clone()),
                actor_id: None,
                details: serde_json::Value::Null,
            },
            ValidationStatus::Invalid,
        ),
        ContentLayerCheck::CommitNotFound => acc.push(
            ValidationIssue {
                kind: "content_layer_commit_not_found".to_owned(),
                severity: ValidationIssueSeverity::Error,
                message: "the revision's storage commit was not found in the supplied repository"
                    .to_owned(),
                statement_id: Some(stmt.clone()),
                actor_id: None,
                details: serde_json::Value::Null,
            },
            ValidationStatus::Invalid,
        ),
        ContentLayerCheck::Indeterminate => acc.push(
            ValidationIssue {
                kind: "content_layer_indeterminate".to_owned(),
                severity: ValidationIssueSeverity::Info,
                message: "no Git repository was supplied; the content layer cannot be verified \
                          server-side"
                    .to_owned(),
                statement_id: Some(stmt.clone()),
                actor_id: None,
                details: serde_json::Value::Null,
            },
            ValidationStatus::Indeterminate,
        ),
    }
}

/// Push-and-fold accumulator for issues. Each push contributes a
/// `ValidationStatus` to a worst-of fold so the final status
/// reflects every check; the issues list keeps them all visible
/// to the UI.
struct IssueAccumulator {
    issues: Vec<ValidationIssue>,
    status: ValidationStatus,
}

impl IssueAccumulator {
    fn new() -> Self {
        Self {
            issues: Vec::new(),
            status: ValidationStatus::Valid,
        }
    }

    fn push(&mut self, issue: ValidationIssue, contributes: ValidationStatus) {
        self.issues.push(issue);
        self.status = worse_of(self.status.clone(), contributes);
    }
}

fn worse_of(a: ValidationStatus, b: ValidationStatus) -> ValidationStatus {
    use ValidationStatus::{Conflicted, Indeterminate, Invalid, Unverified, Valid};
    match (a, b) {
        (Invalid, _) | (_, Invalid) => Invalid,
        (Conflicted, _) | (_, Conflicted) => Conflicted,
        (Indeterminate, _) | (_, Indeterminate) => Indeterminate,
        (Unverified, _) | (_, Unverified) => Unverified,
        (Valid, Valid) => Valid,
    }
}
