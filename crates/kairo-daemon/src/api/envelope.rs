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

/// Wire-stable error codes (subset of `specs/API.md` §8). Slice
/// 2 needs only `not_found`, `store_error`, and `internal_error`;
/// later slices add the rest as the surfaces that produce them
/// land.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum ApiErrorCode {
    NotFound,
    StoreError,
    InternalError,
}

impl ApiErrorCode {
    fn as_str(self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::StoreError => "store_error",
            Self::InternalError => "internal_error",
        }
    }
}
