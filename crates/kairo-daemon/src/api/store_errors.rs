//! Mapping from [`kairo_store::StoreError`] to [`ApiError`].
//!
//! Centralizing the mapping here keeps every handler's error
//! handling identical without each one re-deriving the
//! `Missing → 404`, `Corrupt → 500` rules. The intent map is:
//!
//! - `Missing`: 404 `not_found`. The record does not exist; the
//!   caller can act on this without operator help.
//! - `Corrupt`: 500 `store_error`. Surface the fixity-failure
//!   reason verbatim so the operator sees what's wrong on disk.
//! - `Unavailable`: 500 `store_error`. Underlying I/O issue;
//!   transient or permanent depending on the OS error.
//! - `Rejected`: 500 `store_error`. Spec-invariant violation
//!   should not happen on the read path, but if it does we
//!   surface the reason.
//! - `LockTimeout`: 500 `store_error`. Contention; callers may
//!   retry. We don't yet have a richer code; if this becomes
//!   load-bearing we add `lock_timeout` and surface the path.

use kairo_store::StoreError;

use crate::api::envelope::ApiError;

pub(crate) fn map_store_error(error: StoreError, context: &'static str) -> ApiError {
    match error {
        StoreError::Missing => ApiError::not_found(format!("{context}: not found")),
        other => ApiError::store(format!("{context}: {other}")),
    }
}
