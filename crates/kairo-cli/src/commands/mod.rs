//! Per-verb command runners. Each module owns the dispatch + helpers
//! for a single top-level CLI verb. The clap definitions live in
//! `crate::cli`; runners take a parsed subcommand and a `StorePaths`
//! and return either the formatted output or a `CliError`.

pub(crate) mod bundle;
pub(crate) mod daemon;
pub(crate) mod git;
pub(crate) mod manifest;
pub(crate) mod store;
pub(crate) mod web;
