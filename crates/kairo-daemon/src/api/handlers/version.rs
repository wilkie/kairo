//! `GET /api/v1/version` — returns crate-version metadata for
//! the daemon, the API URL prefix, kairo-core, and kairo-store.

use crate::api::dto::VersionInfo;
use crate::api::envelope::ApiResult;

const API_VERSION: &str = "v1";

pub async fn handler() -> ApiResult<VersionInfo> {
    ApiResult(VersionInfo {
        daemon_version: env!("CARGO_PKG_VERSION").to_owned(),
        api_version: API_VERSION.to_owned(),
        core_version: kairo_core::VERSION.to_owned(),
        store_version: kairo_store::VERSION.to_owned(),
    })
}
