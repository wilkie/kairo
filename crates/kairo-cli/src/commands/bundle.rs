//! `kairo bundle export` and `kairo bundle import` command runners.

use kairo_bundle::{import_bundle, write_bundle, ImportSummary};
use kairo_core::{ObjectId, Timestamp};
use kairo_git::GitCache;

use crate::cli::BundleCommand;
use crate::error::CliError;
use crate::store_paths::{open_store, StorePaths};

pub(crate) fn run_bundle_command(
    command: BundleCommand,
    paths: &StorePaths,
) -> Result<String, CliError> {
    match command {
        BundleCommand::Export {
            object,
            output,
            include_git,
        } => {
            let object_id = ObjectId::new(object.clone())
                .map_err(|source| CliError::ParseObjectId { object, source })?;
            let store = open_store(paths)?;
            // Build the pack from the managed cache only when the
            // user opts in. Skipping this entirely when --include-git
            // is off means bundle export stays git-binary-free for
            // the common case.
            let git_packs: Vec<(ObjectId, Vec<u8>)> = if include_git {
                let cache = GitCache::open(paths.git_root())
                    .map_err(|source| CliError::GitOperation { source })?;
                let pack = cache
                    .pack_for_object(object_id.as_str())
                    .map_err(|source| CliError::GitOperation { source })?;
                vec![(object_id.clone(), pack)]
            } else {
                Vec::new()
            };
            let manifest = write_bundle(
                &store,
                &object_id,
                &output,
                &Timestamp::now().to_string(),
                env!("CARGO_PKG_VERSION"),
                &git_packs,
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
            out.push_str(&format!(
                "git_history_included = {}\n",
                manifest.git_history.included
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
