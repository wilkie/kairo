//! Drift test for the checked-in `openapi/kairo-daemon.json`.
//!
//! The schema generated from `utoipa` annotations on handlers /
//! DTOs is the source of truth. The on-disk file is the consumer-
//! facing artifact (used by `openapi-typescript` codegen for the
//! web client). They must agree.
//!
//! When this test fails after a handler / DTO change, regenerate
//! the file:
//!
//! ```text
//! cargo run -p kairo-daemon -- dump-openapi --out openapi/kairo-daemon.json
//! ```
//!
//! and commit the result alongside the schema-changing edit.

#![allow(clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::PathBuf;

use kairo_daemon::ApiDoc;

// dev-deps not used by this test; silence unused-crate warnings.
use axum as _;
use http_body_util as _;
use hyper as _;
use hyper_util as _;
use kairo_core as _;
use kairo_daemon_client as _;
use kairo_identity as _;
use kairo_object as _;
use kairo_statement as _;
use kairo_store as _;
use kairo_test_support as _;
use serde as _;
use serde_json as _;
use tempfile as _;
use tokio as _;
use tokio_util as _;
use tower as _;
use tower_http as _;
use tracing as _;
use tracing_subscriber as _;
use utoipa as _;

fn schema_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("openapi")
        .join("kairo-daemon.json")
}

#[test]
fn checked_in_schema_matches_live() {
    let live = ApiDoc::pretty_json().expect("serialize live OpenAPI schema");
    let path = schema_path();
    let on_disk = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));

    assert!(
        live == on_disk,
        "openapi/kairo-daemon.json is out of date.\n\
         Regenerate it with:\n\
         \n    \
         cargo run -p kairo-daemon -- dump-openapi --out openapi/kairo-daemon.json\n\
         \n\
         and commit the result alongside the schema-changing edit."
    );
}
