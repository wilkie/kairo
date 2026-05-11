//! JSON envelope helpers for the daemon API.
//!
//! Per `specs/API.md` §7, every non-streaming response is wrapped
//! in one of two envelopes:
//!
//! ```json
//! { "ok": true,  "schema": "kairo.api.result.v1", "result": {...} }
//! { "ok": false, "schema": "kairo.api.error.v1",  "error":  {...} }
//! ```
//!
//! Handlers return `Result<T, ApiError>`; the `IntoResponse` impl
//! on `ApiResult<T>` (and on `ApiError`) wraps the body in the
//! correct envelope and sets the right HTTP status.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

const RESULT_SCHEMA: &str = "kairo.api.result.v1";
const ERROR_SCHEMA: &str = "kairo.api.error.v1";

/// Successful response payload wrapper.
///
/// Wrap the handler's typed result in `ApiResult(value)` (or via
/// `From`) and return it; the `IntoResponse` impl handles
/// serialization and the success envelope.
#[derive(Debug)]
pub struct ApiResult<T>(pub T);

impl<T> From<T> for ApiResult<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

impl<T: Serialize> IntoResponse for ApiResult<T> {
    fn into_response(self) -> Response {
        let body = SuccessEnvelope {
            ok: true,
            schema: RESULT_SCHEMA,
            result: self.0,
        };
        (StatusCode::OK, Json(body)).into_response()
    }
}

#[derive(Debug, Serialize)]
struct SuccessEnvelope<T: Serialize> {
    ok: bool,
    schema: &'static str,
    result: T,
}

/// API-level error returned by handlers.
///
/// Maps to an HTTP status + a structured error code per
/// `specs/API.md` §8. The `code` is the wire-stable identifier
/// program clients should switch on; the `message` is human-
/// readable. `details` is reserved for future structured payloads
/// and currently always empty.
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: ApiErrorCode,
    pub message: String,
}

impl ApiError {
    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: ApiErrorCode::NotFound,
            message: message.into(),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: ApiErrorCode::BadRequest,
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: ApiErrorCode::InternalError,
            message: message.into(),
        }
    }

    pub fn store(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: ApiErrorCode::StoreError,
            message: message.into(),
        }
    }

    /// Statement signature did not verify against the actor's
    /// active signing key at the statement's `created_at`. See
    /// `kairo_statement::verify::SignatureStatus` for the full
    /// set of underlying causes the daemon collapses into this
    /// single wire code.
    pub fn bad_signature(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: ApiErrorCode::BadSignature,
            message: message.into(),
        }
    }

    /// The envelope's `actor` field references an actor whose
    /// `ActorGenesis` is not present in the store. Distinct from
    /// `not_found` (404): the verification call surfaces an actor
    /// gap as a *body-level* defect, not a resource-not-found on
    /// the request URL.
    pub fn actor_not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: ApiErrorCode::ActorNotFound,
            message: message.into(),
        }
    }

    /// The envelope's `actor` field and the signature's `actor`
    /// field disagree. The signature is not evaluated in this
    /// case; the body is rejected on shape grounds.
    pub fn signature_actor_mismatch(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: ApiErrorCode::SignatureActorMismatch,
            message: message.into(),
        }
    }

    /// Phase A's write dispatcher does not accept this statement
    /// kind yet. Returned for every body type other than the ones
    /// the current slice has opened up (`ActorTrust` only in
    /// Slice 2). 415 keeps the wire code aligned with the
    /// content-negotiation family without reusing 400 / 422.
    pub fn unsupported_statement_kind(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNSUPPORTED_MEDIA_TYPE,
            code: ApiErrorCode::UnsupportedStatementKind,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status;
        let body = ErrorEnvelope {
            ok: false,
            schema: ERROR_SCHEMA,
            error: ErrorBody {
                code: self.code.as_str(),
                message: self.message,
            },
        };
        (status, Json(body)).into_response()
    }
}

#[derive(Debug, Serialize)]
struct ErrorEnvelope {
    ok: bool,
    schema: &'static str,
    error: ErrorBody,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    code: &'static str,
    message: String,
}

/// Wire-stable error codes (subset of `specs/API.md` §8). Later
/// slices add codes as the surfaces that produce them land.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum ApiErrorCode {
    BadRequest,
    NotFound,
    StoreError,
    InternalError,
    BadSignature,
    ActorNotFound,
    SignatureActorMismatch,
    UnsupportedStatementKind,
}

impl ApiErrorCode {
    fn as_str(self) -> &'static str {
        match self {
            Self::BadRequest => "bad_request",
            Self::NotFound => "not_found",
            Self::StoreError => "store_error",
            Self::InternalError => "internal_error",
            Self::BadSignature => "bad_signature",
            Self::ActorNotFound => "actor_not_found",
            Self::SignatureActorMismatch => "signature_actor_mismatch",
            Self::UnsupportedStatementKind => "unsupported_statement_kind",
        }
    }
}
