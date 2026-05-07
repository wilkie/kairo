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
            let git_object_ids: Vec<ObjectId> = if include_git {
                vec![object_id.clone()]
            } else {
                Vec::new()
            };
            let manifest = write_bundle(
                &store,
                &object_id,
                &output,
                &Timestamp::now().to_string(),
                env!("CARGO_PKG_VERSION"),
                &git_object_ids,
            )
            .map_err(CliError::Bundle)?;
            // After `write_bundle` reserved `<output>/git/`, stream
            // each declared pack into it without holding the bytes
            // in memory. Open the cache lazily so the export path
            // stays git-binary-free when `--include-git` is off.
            if include_git {
                let cache = GitCache::open(paths.git_root())
                    .map_err(|source| CliError::GitOperation { source })?;
                let pack_path = output.join("git").join(format!("{object_id}.pack"));
                let mut file = std::fs::File::create(&pack_path).map_err(|source| {
                    CliError::GitOperation {
                        source: kairo_git::GitError::CacheIo {
                            path: pack_path.clone(),
                            source,
                        },
                    }
                })?;
                cache
                    .pack_for_object_to(object_id.as_str(), &mut file)
                    .map_err(|source| CliError::GitOperation { source })?;
            }
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
