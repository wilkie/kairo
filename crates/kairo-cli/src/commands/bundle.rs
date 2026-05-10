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
                let mut file =
                    std::fs::File::create(&pack_path).map_err(|source| CliError::GitOperation {
                        source: kairo_git::GitError::CacheIo {
                            path: pack_path.clone(),
                            source,
                        },
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
            let summary: ImportSummary = import_bundle(&input, &store).map_err(CliError::Bundle)?;

            // If the bundle ships Git packs (`git_history.included`),
            // stream each `<input>/git/<object-id>.pack` into the
            // managed cache and pin every `expected_commits` OID as
            // a `refs/kairo/imported/<oid>` ref so the OID survives
            // future GC and shows up in `kairo git cache status`.
            // Cache is opened only when there's actually git data to
            // ingest, so the common path stays git-binary-free.
            let (packs_ingested, refs_pinned) = if summary.manifest.git_history.included {
                ingest_bundle_git_data(paths, &input, &summary.manifest)?
            } else {
                (0, 0)
            };

            let mut out = String::new();
            out.push_str("import bundle\n");
            out.push_str(&format!("actors = {}\n", summary.actors));
            out.push_str(&format!("objects = {}\n", summary.objects));
            out.push_str(&format!("statements = {}\n", summary.statements));
            out.push_str(&format!("blobs = {}\n", summary.blobs));
            out.push_str(&format!("git_packs = {packs_ingested}\n"));
            out.push_str(&format!("git_refs_pinned = {refs_pinned}\n"));
            Ok(out)
        }
    }
}

/// Walk `<input>/git/*.pack` and stream each into the managed
/// cache; then pin every `expected_commits` OID under each pack's
/// per-object cache repo so future GC won't collect them. Returns
/// `(packs_ingested, refs_pinned)` for diagnostic reporting.
///
/// Pack files are streamed via `File`, never read into memory.
/// Each pack's `<object-id>` filename component identifies which
/// per-object repo to pin refs under. Multi-pack bundles are
/// supported (each pack pins the same expected_commits in its own
/// repo); v1 bundles ship one pack but the loop is general.
fn ingest_bundle_git_data(
    paths: &StorePaths,
    input: &std::path::Path,
    manifest: &kairo_bundle::BundleManifest,
) -> Result<(usize, usize), CliError> {
    let cache =
        GitCache::open(paths.git_root()).map_err(|source| CliError::GitOperation { source })?;
    let git_dir = input.join("git");

    let mut pack_object_ids: Vec<String> = Vec::new();
    let entries = std::fs::read_dir(&git_dir).map_err(|source| CliError::GitOperation {
        source: kairo_git::GitError::CacheIo {
            path: git_dir.clone(),
            source,
        },
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| CliError::GitOperation {
            source: kairo_git::GitError::CacheIo {
                path: git_dir.clone(),
                source,
            },
        })?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("pack") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let file = std::fs::File::open(&path).map_err(|source| CliError::GitOperation {
            source: kairo_git::GitError::CacheIo {
                path: path.clone(),
                source,
            },
        })?;
        cache
            .ingest_pack_from(file)
            .map_err(|source| CliError::GitOperation { source })?;
        pack_object_ids.push(stem.to_owned());
    }

    let mut refs_pinned = 0usize;
    for object_id in &pack_object_ids {
        for oid in &manifest.git_history.expected_commits {
            cache
                .set_ref(object_id, &format!("refs/kairo/imported/{oid}"), oid)
                .map_err(|source| CliError::GitOperation { source })?;
            refs_pinned += 1;
        }
    }
    Ok((pack_object_ids.len(), refs_pinned))
}
