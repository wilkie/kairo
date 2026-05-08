//! Deserialization helpers for the daemon's success and error
//! envelopes (see `specs/API.md` §7).
//!
//! Handlers wrap their result in
//! `{ "ok": true, "schema": "kairo.api.result.v1", "result": {...} }`
//! or
//! `{ "ok": false, "schema": "kairo.api.error.v1", "error": {...} }`.
//! This module unwraps either shape into the inner result type
//! (or the appropriate [`ClientError`]).

use serde::de::DeserializeOwned;
use serde::Deserialize;

use crate::error::ClientError;

#[derive(Debug, Deserialize)]
struct SuccessEnvelope<T> {
    #[allow(dead_code)]
    ok: bool,
    #[allow(dead_code)]
    schema: String,
    result: T,
}

#[derive(Debug, Deserialize)]
struct ErrorEnvelope {
    #[allow(dead_code)]
    ok: bool,
    #[allow(dead_code)]
    schema: String,
    error: ErrorBody,
}

#[derive(Debug, Deserialize)]
struct ErrorBody {
    code: String,
    message: String,
}

/// Decode a 2xx response body into `T`.
///
/// Returns [`ClientError::Decode`] if the bytes aren't JSON or
/// don't match the success envelope shape.
pub(crate) fn decode_success<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, ClientError> {
    let envelope: SuccessEnvelope<T> = serde_json::from_slice(bytes)
        .map_err(|error| ClientError::Decode(format!("success envelope: {error}")))?;
    Ok(envelope.result)
}

/// Decode a non-2xx response body into a [`ClientError::Http`]
/// with the daemon's structured error code and message.
///
/// If the body fails to parse, returns [`ClientError::Decode`]
/// describing the parse failure. The HTTP status is preserved in
/// either path.
pub(crate) fn decode_error(status: u16, bytes: &[u8]) -> ClientError {
    match serde_json::from_slice::<ErrorEnvelope>(bytes) {
        Ok(envelope) => ClientError::Http {
            status,
            code: envelope.error.code,
            message: envelope.error.message,
        },
        Err(error) => ClientError::Decode(format!(
            "non-2xx response (HTTP {status}) with non-envelope body: {error}"
        )),
    }
}
