//! Federation error responses (`specs/PHASE_2_FEDERATION.md` §2.3).
//!
//! The envelope is the daemon API's `kairo.api.error.v1`
//! (`specs/API.md` §7.2), so one parser handles both; codes carry a
//! `fed_` prefix.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Envelope schema shared with the daemon API.
pub const ERROR_SCHEMA: &str = "kairo.api.error.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FedErrorCode {
    /// Missing *or* unpublished. Deliberately indistinguishable so a
    /// peer can't probe for private records.
    FedNotFound,
    /// A path segment isn't a valid id of the expected type.
    FedBadId,
    /// `cursor` didn't parse or doesn't name a position.
    FedBadCursor,
    /// `limit` is not an integer from 1 to `MAX_PAGE_LIMIT`.
    FedBadLimit,
    /// Anything but `GET` / `HEAD`.
    FedMethodNotAllowed,
    /// A store read failed. The message stays generic; details are
    /// logged server-side.
    FedStoreError,
    /// A code this client doesn't know, from a newer node.
    #[serde(other)]
    Unknown,
}

impl FedErrorCode {
    /// HTTP status the code is served with. `Unknown` never
    /// originates locally; it maps to 500 for completeness.
    pub fn status(self) -> u16 {
        match self {
            Self::FedNotFound => 404,
            Self::FedBadId | Self::FedBadCursor | Self::FedBadLimit => 400,
            Self::FedMethodNotAllowed => 405,
            Self::FedStoreError | Self::Unknown => 500,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::FedNotFound => "fed_not_found",
            Self::FedBadId => "fed_bad_id",
            Self::FedBadCursor => "fed_bad_cursor",
            Self::FedBadLimit => "fed_bad_limit",
            Self::FedMethodNotAllowed => "fed_method_not_allowed",
            Self::FedStoreError => "fed_store_error",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for FedErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `{ "ok": false, "schema": "kairo.api.error.v1", "error": { … } }`.
///
/// Not `deny_unknown_fields`: it is shared with the daemon API, whose
/// envelope may grow, and an error is still an error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FedErrorEnvelope {
    pub ok: bool,
    pub schema: String,
    pub error: FedErrorBody,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FedErrorBody {
    pub code: FedErrorCode,
    pub message: String,
    #[serde(default = "empty_details")]
    pub details: serde_json::Value,
}

fn empty_details() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

impl FedErrorEnvelope {
    pub fn new(code: FedErrorCode, message: impl Into<String>) -> Self {
        Self {
            ok: false,
            schema: ERROR_SCHEMA.to_owned(),
            error: FedErrorBody {
                code,
                message: message.into(),
                details: empty_details(),
            },
        }
    }

    /// The standard `fed_not_found` response, identical for missing
    /// and unpublished records.
    pub fn not_found() -> Self {
        Self::new(FedErrorCode::FedNotFound, "not found")
    }
}
