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

use kairo_git::{GitCache, GitCli};

use crate::cli::{GitCacheCommand, GitCommand};
use crate::error::CliError;
use crate::store_paths::StorePaths;

pub(crate) fn run_git_command(
    command: GitCommand,
    paths: &StorePaths,
) -> Result<String, CliError> {
    match command {
        GitCommand::Fetch {
            object,
            remote,
            branch,
        } => run_fetch(paths, &object, &remote, &branch),
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
) -> Result<String, CliError> {
    let branch = strip_refs_heads_prefix(branch);
    let cache = GitCache::open(paths.git_root()).map_err(|source| CliError::GitOperation { source })?;
    let fetched = cache
        .fetch(object, remote, branch, &GitCli::new())
        .map_err(|source| CliError::GitOperation { source })?;
    let mut out = String::new();
    out.push_str("fetched\n");
    out.push_str(&format!("object = {object}\n"));
    out.push_str(&format!("remote = {remote}\n"));
    out.push_str(&format!("ref = {}\n", fetched.ref_name));
    out.push_str(&format!("oid = {}\n", fetched.oid));
    Ok(out)
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

    let pool_objects = git_root.join("pool").join("objects");
    let pool_state = if pool_objects.is_dir() {
        "initialized"
    } else {
        "not initialized"
    };
    out.push_str(&format!("pool: {pool_state}\n"));

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
        if entry.file_name() == "pool" {
            continue;
        }
        for level2 in
            std::fs::read_dir(entry.path()).map_err(|source| cache_io_error(&entry.path(), source))?
        {
            let level2 = level2.map_err(|source| cache_io_error(&entry.path(), source))?;
            if !level2
                .file_type()
                .map_err(|source| cache_io_error(&level2.path(), source))?
                .is_dir()
            {
                continue;
            }
            for level3 in
                std::fs::read_dir(level2.path()).map_err(|source| cache_io_error(&level2.path(), source))?
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
        .args([
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            "refs/",
        ])
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
                command: format!(
                    "git -C {} for-each-ref refs/heads/",
                    repo_path.display()
                ),
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
