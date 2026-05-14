//! Request-time capability enforcement for the daemon's write
//! surface (Phase B; `DECISIONS.md §14` sub-decision 6,
//! `PHASE_2_DAEMON_AUTH.md` Slice 3).
//!
//! Runs *after* `verify_request_auth` produces an
//! `AuthenticatedRequest`, *before* the per-kind dispatcher
//! arm in `handlers::statements::put_handler` calls into the
//! store. Two-step check per `CAPABILITIES.md §6.1`:
//!
//! 1. **Root-authority short-circuit.** If `signer` is in the
//!    object's `created_by` root-authority set, allow without
//!    walking any capability chain.
//! 2. **Capability lookup.** Otherwise, run
//!    `evaluate_capability(grantee=signer, scope=Object(...),
//!    kind, at=request_timestamp)`. Only `Held` allows the
//!    write; every other variant (`NotHeld`, `Revoked`,
//!    `Expired`, `DelegationTooDeep`, `GrantorLacksAuthority`)
//!    becomes a 403 `not_authorized`.
//!
//! Slice 3 covers `ObjectRevision`, `ObjectBranch`, and
//! `ObjectVersionTag`. Slice 4 will add the recursive
//! grant/revocation cases.

use kairo_core::{ActorId, ObjectId, Timestamp};
use kairo_statement::verify::{
    evaluate_capability, CapabilityEvaluation, CapabilityResolver, ResolutionTarget,
};
use kairo_statement::StatementKind;

use crate::api::envelope::ApiError;
use crate::api::state::AppState;

/// Permit `signer` to write a statement of kind `K` against
/// `object_id` at the request's `at` timestamp.
///
/// Returns `Ok(())` if the signer is the object's root authority
/// OR holds a covering capability at `at`. Any other outcome
/// becomes a 403 `not_authorized` carrying the underlying
/// `CapabilityEvaluation` variant in the message for diagnostics.
///
/// Resolver lookups run on `tokio::task::spawn_blocking` because
/// the underlying `FilesystemStore` reads are sync I/O — matches
/// the existing per-handler pattern in this crate.
pub async fn require_object_write_authority(
    state: &AppState,
    signer: &ActorId,
    object_id: &ObjectId,
    kind: StatementKind,
    at: Timestamp,
) -> Result<(), ApiError> {
    // Root-authority short-circuit. The check is one indexed
    // store read; it's worth doing first because same-actor
    // writes are the common path and don't need a chain walk.
    let store = state.store.clone();
    let object_for_root = object_id.clone();
    let root_authority =
        tokio::task::spawn_blocking(move || store.object_root_authority(&object_for_root))
            .await
            .map_err(|error| {
                ApiError::internal(format!("spawn_blocking join failed: {error}"))
            })?
            .map_err(|error| {
                ApiError::store(format!(
                    "object root-authority lookup failed for {object_for_root_display}: {error}",
                    object_for_root_display = object_id,
                ))
            })?;

    let Some(roots) = root_authority else {
        // No genesis recorded means we can't establish authority
        // either way — bias toward refusal. The body-level
        // dispatcher would also reject this on the put, but
        // surfacing it as 403 here keeps the failure mode
        // legible.
        return Err(ApiError::not_authorized(format!(
            "object {object_id} has no recorded root authority; \
             refuse write before reaching the dispatcher",
        )));
    };

    if roots.contains(signer) {
        return Ok(());
    }

    // Capability lookup. The evaluator handles revocation,
    // expiration, delegation depth, and key-pinning per
    // `CAPABILITIES.md §6.1` — we just translate the outcome.
    let store_for_eval = state.store.clone();
    let signer_for_eval = signer.clone();
    let target = ResolutionTarget::Object {
        id: object_id.clone(),
        kind,
    };
    let object_id_for_msg = object_id.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        evaluate_capability(&signer_for_eval, &target, at, store_for_eval.as_ref())
    })
    .await
    .map_err(|error| ApiError::internal(format!("spawn_blocking join failed: {error}")))?
    .map_err(|error| ApiError::store(format!("capability evaluator failed: {error}")))?;

    match outcome {
        CapabilityEvaluation::Held => Ok(()),
        CapabilityEvaluation::NotHeld => Err(ApiError::not_authorized(format!(
            "signer {signer} does not hold a {kind:?} capability on object {object_id_for_msg}",
        ))),
        CapabilityEvaluation::Revoked(grant_id) => Err(ApiError::not_authorized(format!(
            "signer {signer}'s {kind:?} capability on {object_id_for_msg} \
             was revoked (grant {grant_id})",
        ))),
        CapabilityEvaluation::Expired(grant_id) => Err(ApiError::not_authorized(format!(
            "signer {signer}'s {kind:?} capability on {object_id_for_msg} \
             expired (grant {grant_id})",
        ))),
        CapabilityEvaluation::DelegationTooDeep => Err(ApiError::not_authorized(format!(
            "signer {signer}'s delegation chain for {kind:?} on {object_id_for_msg} \
             exceeds an upstream MaxDelegationDepth constraint",
        ))),
        CapabilityEvaluation::GrantorLacksAuthority => Err(ApiError::not_authorized(format!(
            "no covering grant chain back to a root authority for \
             signer {signer} on {object_id_for_msg} (kind {kind:?})",
        ))),
    }
}
