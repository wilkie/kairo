//! `kairo git fetch` and `kairo git cache status` runners.
//!
//! `fetch` uses `GitCache::fetch` with the shell-out `GitCli`
//! transport per `specs/DECISIONS.md` §8. `cache status` walks the
//! sharded cache layout and reports per-object refs by shelling out
//! to `git for-each-ref` — consistent with the cache's overall
//! "shell out for cache mutations and refs, gix for object reads"
//! split.

use std::path::{Path, PathBuf};
use std::process::Command;

use std::collections::BTreeSet;

use kairo_core::ObjectId;
use kairo_git::{GitCache, GitCli, GitObjectFormat};

use crate::cli::{GitCacheCommand, GitCommand};
use crate::error::CliError;
use crate::store_paths::{open_store, StorePaths};

pub(crate) fn run_git_command(command: GitCommand, paths: &StorePaths) -> Result<String, CliError> {
    match command {
        GitCommand::Fetch {
            object,
            remote,
            branch,
            object_format,
        } => run_fetch(paths, &object, &remote, &branch, object_format.as_deref()),
        GitCommand::Cache { command } => match command {
            GitCacheCommand::Status => run_cache_status(paths),
        },
    }
}

fn run_fetch(
    paths: &StorePaths,
    object: &str,
    remote: &str,
    branch: &str,
    object_format: Option<&str>,
) -> Result<String, CliError> {
    let branch = strip_refs_heads_prefix(branch);
    let cache =
        GitCache::open(paths.git_root()).map_err(|source| CliError::GitOperation { source })?;
    let format = resolve_object_format(paths, &cache, object, object_format)?;
    let fetched = cache
        .fetch(object, remote, branch, format, &GitCli::new())
        .map_err(|source| CliError::GitOperation { source })?;
    let mut out = String::new();
    out.push_str("fetched\n");
    out.push_str(&format!("object = {object}\n"));
    out.push_str(&format!("remote = {remote}\n"));
    out.push_str(&format!("object_format = {format}\n"));
    out.push_str(&format!("ref = {}\n", fetched.ref_name));
    out.push_str(&format!("oid = {}\n", fetched.oid));
    Ok(out)
}

/// The object format to fetch `object` in. An object's history has
/// one format (`specs/DECISIONS.md` §19), so `--object-format`, the
/// existing cache repo, and the object's stored `git:` revisions must
/// all agree; the first one present decides, and any disagreement is
/// an error rather than a silent pick.
fn resolve_object_format(
    paths: &StorePaths,
    cache: &GitCache,
    object: &str,
    flag: Option<&str>,
) -> Result<GitObjectFormat, CliError> {
    let conflict = |message: String| CliError::GitFormatConflict {
        object: object.to_owned(),
        message,
    };
    let flag = flag
        .map(|raw| {
            raw.parse::<GitObjectFormat>()
                .map_err(|source| CliError::InvalidGitRevision {
                    revision: format!("--object-format {raw}"),
                    source,
                })
        })
        .transpose()?;
    let repo = cache
        .repo_format(object)
        .map_err(|source| CliError::GitOperation { source })?;

    let mut revisions: BTreeSet<GitObjectFormat> = BTreeSet::new();
    if let Ok(object_id) = ObjectId::new(object) {
        let store = open_store(paths)?;
        let stored =
            store
                .revision_ids(&object_id)
                .map_err(|source| CliError::ReadObjectGenesis {
                    object: object_id.clone(),
                    source,
                })?;
        for revision in stored {
            if let Ok(Some(git)) = revision.git() {
                revisions.insert(git.format());
            }
        }
    }
    if revisions.len() > 1 {
        return Err(conflict("its revisions mix Git object formats".to_owned()));
    }
    let from_revisions = revisions.into_iter().next();

    let candidates = [
        ("--object-format", flag),
        ("the cache repo", repo),
        ("its revisions", from_revisions),
    ];
    let mut chosen: Option<(&str, GitObjectFormat)> = None;
    for (source, format) in candidates {
        let Some(format) = format else { continue };
        match chosen {
            None => chosen = Some((source, format)),
            Some((first, existing)) if existing != format => {
                return Err(conflict(format!(
                    "{first} says {existing} but {source} says {format}"
                )))
            }
            Some(_) => {}
        }
    }
    chosen
        .map(|(_, format)| format)
        .ok_or_else(|| CliError::GitFormatUnknown {
            object: object.to_owned(),
        })
}

/// Trim a leading `refs/heads/` from a branch name. The CLI accepts
/// either form so users can copy a fully-qualified ref out of `git
/// branch -a` and have it work; the underlying cache API takes the
/// short branch name.
fn strip_refs_heads_prefix(branch: &str) -> &str {
    branch.strip_prefix("refs/heads/").unwrap_or(branch)
}

fn run_cache_status(paths: &StorePaths) -> Result<String, CliError> {
    let git_root = paths.git_root();
    let mut out = String::new();
    out.push_str(&format!("git cache: {}\n", git_root.display()));

    for format in GitObjectFormat::ALL {
        let pool_objects = git_root.join(format!("pool-{format}")).join("objects");
        let pool_state = if pool_objects.is_dir() {
            "initialized"
        } else {
            "not initialized"
        };
        out.push_str(&format!("pool {format}: {pool_state}\n"));
    }

    let mut repos = walk_per_object_repos(&git_root)?;
    repos.sort_by(|a, b| a.0.cmp(&b.0));
    out.push_str(&format!("objects: {}\n", repos.len()));

    for (object_id, repo_path) in &repos {
        out.push_str(&format!("{object_id}\n"));
        let refs = read_object_refs(repo_path)?;
        if refs.is_empty() {
            out.push_str("  (no refs)\n");
        } else {
            for (ref_name, oid) in &refs {
                out.push_str(&format!("  {ref_name} = {oid}\n"));
            }
        }
    }
    Ok(out)
}

/// Walk `<git_root>/<XX>/<YY>/<object-id>/` two levels deep and
/// return every per-object cache repo. Skips the `pool/` directory
/// (it's the shared alternates pool, not a per-object repo) and
/// any non-directory entries (`.lock` sidecars, future top-level
/// metadata files). Identifies a per-object repo by the presence
/// of a `HEAD` file rather than by directory-name shape, so future
/// shard-layout tweaks degrade gracefully.
fn walk_per_object_repos(git_root: &Path) -> Result<Vec<(String, PathBuf)>, CliError> {
    let mut found = Vec::new();
    let level1 = match std::fs::read_dir(git_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(found),
        Err(source) => return Err(cache_io_error(git_root, source)),
    };
    for entry in level1 {
        let entry = entry.map_err(|source| cache_io_error(git_root, source))?;
        if !entry
            .file_type()
            .map_err(|source| cache_io_error(&entry.path(), source))?
            .is_dir()
        {
            continue;
        }
        if entry.file_name().to_string_lossy().starts_with("pool-") {
            continue;
        }
        for level2 in std::fs::read_dir(entry.path())
            .map_err(|source| cache_io_error(&entry.path(), source))?
        {
            let level2 = level2.map_err(|source| cache_io_error(&entry.path(), source))?;
            if !level2
                .file_type()
                .map_err(|source| cache_io_error(&level2.path(), source))?
                .is_dir()
            {
                continue;
            }
            for level3 in std::fs::read_dir(level2.path())
                .map_err(|source| cache_io_error(&level2.path(), source))?
            {
                let level3 = level3.map_err(|source| cache_io_error(&level2.path(), source))?;
                let path = level3.path();
                if !path.join("HEAD").is_file() {
                    continue;
                }
                let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                    continue;
                };
                found.push((name.to_owned(), path));
            }
        }
    }
    Ok(found)
}

/// Read all refs from a per-object cache repo via
/// `git for-each-ref`. Returns the refs as a sorted list of
/// (full-ref-name, oid) pairs. Walks the full `refs/` namespace
/// (not just `refs/heads/`) so OIDs pinned by bundle import under
/// `refs/kairo/imported/<oid>` appear in cache status alongside
/// any branches a `kairo git fetch` left under `refs/heads/`.
fn read_object_refs(repo_path: &Path) -> Result<Vec<(String, String)>, CliError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["for-each-ref", "--format=%(refname) %(objectname)", "refs/"])
        .output()
        .map_err(|source| match source.kind() {
            std::io::ErrorKind::NotFound => CliError::GitOperation {
                source: kairo_git::GitError::CacheGitMissing { source },
            },
            _ => cache_io_error(repo_path, source),
        })?;
    if !output.status.success() {
        return Err(CliError::GitOperation {
            source: kairo_git::GitError::CacheGitInvocation {
                command: format!("git -C {} for-each-ref refs/heads/", repo_path.display()),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                exit_code: output.status.code(),
            },
        });
    }
    let mut refs = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some((ref_name, oid)) = line.split_once(' ') {
            refs.push((ref_name.to_owned(), oid.to_owned()));
        }
    }
    refs.sort();
    Ok(refs)
}

fn cache_io_error(path: &Path, source: std::io::Error) -> CliError {
    CliError::GitOperation {
        source: kairo_git::GitError::CacheIo {
            path: path.to_path_buf(),
            source,
        },
    }
}
