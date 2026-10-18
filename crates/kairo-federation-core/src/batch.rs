//! Statement batch fetch (`specs/DECISIONS.md` §24).
//!
//! A pull names the statements it lacks, up to [`MAX_BATCH_IDS`] at a
//! time, and the node answers with those it serves. Each statement is
//! the store's exact JSON, embedded verbatim as a raw value, so the
//! client verifies the same bytes a per-record `GET` would return.

use kairo_core::StatementId;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

use crate::schema::schema_tag;

/// Most ids one batch request may name.
pub const MAX_BATCH_IDS: usize = 256;

/// Path of the batch endpoint, under [`crate::API_PREFIX`].
pub const STATEMENT_BATCH_PATH: &str = "/statement-batch";

schema_tag!(
    /// `"kairo.fed.statement-batch-request.v1"`.
    StatementBatchRequestSchema = "kairo.fed.statement-batch-request.v1"
);

schema_tag!(
    /// `"kairo.fed.statement-batch.v1"`.
    StatementBatchSchema = "kairo.fed.statement-batch.v1"
);

/// `POST /federation/v1/statement-batch` body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatementBatchRequest {
    pub schema: StatementBatchRequestSchema,
    #[serde(with = "crate::wire_id::many")]
    pub ids: Vec<StatementId>,
}

/// Why a batch request was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatementBatchRequestError {
    Empty,
    TooMany { count: usize },
}

impl std::fmt::Display for StatementBatchRequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => f.write_str("a batch names at least one statement id"),
            Self::TooMany { count } => write!(
                f,
                "a batch names at most {MAX_BATCH_IDS} statement ids, not {count}"
            ),
        }
    }
}

impl std::error::Error for StatementBatchRequestError {}

impl StatementBatchRequest {
    pub fn new(ids: Vec<StatementId>) -> Self {
        Self {
            schema: StatementBatchRequestSchema,
            ids,
        }
    }

    /// The id count is within 1 to [`MAX_BATCH_IDS`].
    pub fn validate(&self) -> Result<(), StatementBatchRequestError> {
        match self.ids.len() {
            0 => Err(StatementBatchRequestError::Empty),
            count if count > MAX_BATCH_IDS => Err(StatementBatchRequestError::TooMany { count }),
            _ => Ok(()),
        }
    }
}

/// One served statement.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchedStatement {
    #[serde(with = "crate::wire_id::one")]
    pub id: StatementId,
    /// The stored JSON, exactly. Verify it as the statement `id`.
    pub statement: Box<RawValue>,
}

/// `POST /federation/v1/statement-batch` response: the requested
/// statements the node serves, in request order, each at most once.
/// An id left out is missing or unpublished, indistinguishably.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatementBatch {
    pub schema: StatementBatchSchema,
    pub statements: Vec<BatchedStatement>,
}
