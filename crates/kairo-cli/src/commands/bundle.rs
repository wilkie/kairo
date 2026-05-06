//! `kairo bundle export` and `kairo bundle import` command runners.

use kairo_bundle::{import_bundle, write_bundle, ImportSummary};
use kairo_core::{ObjectId, Timestamp};

use crate::cli::BundleCommand;
use crate::error::CliError;
use crate::store_paths::{open_store, StorePaths};

pub(crate) fn run_bundle_command(
    command: BundleCommand,
    paths: &StorePaths,
) -> Result<String, CliError> {
    match command {
        BundleCommand::Export { object, output } => {
            let object_id = ObjectId::new(object.clone())
                .map_err(|source| CliError::ParseObjectId { object, source })?;
            let store = open_store(paths)?;
            let manifest = write_bundle(
                &store,
                &object_id,
                &output,
                &Timestamp::now().to_string(),
                env!("CARGO_PKG_VERSION"),
            )
            .map_err(CliError::Bundle)?;
            let mut out = String::new();
            out.push_str("export bundle\n");
            out.push_str(&format!("object = {}\n", object_id));
            out.push_str(&format!("output = {}\n", output.display()));
            out.push_str(&format!("actors = {}\n", manifest.contents.actors.len()));
            out.push_str(&format!(
                "statements = {}\n",
                manifest.contents.statements.len()
            ));
            out.push_str(&format!("blobs = {}\n", manifest.contents.blobs.len()));
            out.push_str(&format!(
                "expected_git_commits = {}\n",
                manifest.git_history.expected_commits.len()
            ));
            Ok(out)
        }
        BundleCommand::Import { input } => {
            let store = open_store(paths)?;
            let summary: ImportSummary =
                import_bundle(&input, &store).map_err(CliError::Bundle)?;
            Ok(format!(
                "import bundle\nactors = {}\nobjects = {}\nstatements = {}\nblobs = {}\n",
                summary.actors, summary.objects, summary.statements, summary.blobs,
            ))
        }
    }
}
