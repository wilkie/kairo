//! Shared test fixtures for the kairo workspace.
//!
//! Two modules:
//!
//! - [`git`]: helpers that spawn a real `git` binary to build source
//!   repos, pack files, etc. Used by tests that exercise the
//!   `kairo-git` cache, bundle git-data round-trips, or verify-object's
//!   git lookups. These were previously duplicated across
//!   `kairo-git/src/test_support.rs` and `kairo-cli/src/tests.rs`.
//! - [`store`]: builder fixtures for actor → object → revision →
//!   branch chains, driven through `kairo-*` library APIs (no CLI
//!   dependency). Useful for any test that needs a populated
//!   `FilesystemStore` + `FilesystemKeystore` without re-implementing
//!   sign-and-persist sequences.
//!
//! This crate has no test gating beyond being a workspace crate;
//! consumers add it as `dev-dependencies` and call into it from
//! `#[cfg(test)] mod tests`.

#![allow(clippy::expect_used)]

pub mod git;
pub mod store;
