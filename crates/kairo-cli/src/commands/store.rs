//! `kairo store ...` runners. Maintenance operations on the
//! local store directory itself.

use std::fmt::Write as _;

use kairo_store::{RebuildReport, VerifyFailure};

use crate::cli::StoreCommand;
use crate::error::CliError;
use crate::store_paths::{open_store, StorePaths};

pub(crate) fn run_store_command(
    command: StoreCommand,
    paths: &StorePaths,
) -> Result<String, CliError> {
    match command {
        StoreCommand::RebuildIndexes => run_rebuild_indexes(paths),
        StoreCommand::Verify => run_verify(paths),
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

/// `kairo store verify` (DECISIONS.md §47): every record and blob, and
/// the build's execution records, against their ids.
fn run_verify(paths: &StorePaths) -> Result<String, CliError> {
    let store = open_store(paths)?;
    let mut report = store
        .verify(kairo_build::STORED_BLOB_DOMAINS)
        .map_err(|source| CliError::StoreVerify {
            message: source.to_string(),
        })?;
    let (executions, failures) =
        kairo_build::verify_records(&store).map_err(|source| CliError::StoreVerify {
            message: source.to_string(),
        })?;
    report.records += executions;
    report.failures.extend(failures);
    let mut out = String::new();
    let _ = writeln!(out, "verified store");
    let _ = writeln!(out, "store = {}", paths.store.display());
    let _ = writeln!(out, "records = {}", report.records);
    let _ = writeln!(out, "blobs = {}", report.blobs);
    let _ = writeln!(out, "blob_bytes = {}", report.bytes);
    let _ = writeln!(out, "corrupt = {}", report.failures.len());
    for VerifyFailure { path, reason } in &report.failures {
        let _ = writeln!(out, "corrupt: {}: {reason}", path.display());
    }
    if report.failures.is_empty() {
        Ok(out)
    } else {
        Err(CliError::StoreCorrupt {
            summary: out,
            corrupt: report.failures.len(),
        })
    }
}
