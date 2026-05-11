//! `kairo store ...` runners. Maintenance operations on the
//! local store directory itself.

use kairo_store::RebuildReport;

use crate::cli::StoreCommand;
use crate::error::CliError;
use crate::store_paths::{open_store, StorePaths};

pub(crate) fn run_store_command(
    command: StoreCommand,
    paths: &StorePaths,
) -> Result<String, CliError> {
    match command {
        StoreCommand::RebuildIndexes => run_rebuild_indexes(paths),
    }
}

fn run_rebuild_indexes(paths: &StorePaths) -> Result<String, CliError> {
    let store = open_store(paths)?;
    let report = store
        .rebuild_indexes()
        .map_err(|source| CliError::RebuildIndexes { source })?;
    Ok(format_report(paths, &report))
}

fn format_report(paths: &StorePaths, report: &RebuildReport) -> String {
    let mut out = String::new();
    out.push_str("rebuilt indexes\n");
    out.push_str(&format!("store = {}\n", paths.store.display()));
    out.push_str(&format!(
        "statements_scanned = {}\n",
        report.statements_scanned
    ));
    out.push_str(&format!("objects_scanned = {}\n", report.objects_scanned));
    if report.by_kind.is_empty() {
        out.push_str("by_kind = (none)\n");
    } else {
        out.push_str("by_kind =\n");
        for (kind, count) in &report.by_kind {
            out.push_str(&format!("  {} = {count}\n", kind.as_str()));
        }
    }
    out
}
