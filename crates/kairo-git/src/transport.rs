//! Pluggable transport for [`GitCache`](crate::GitCache) fetches.
//!
//! The transport is narrowly scoped: open a connection to a remote
//! URL, request a single explicit `<src>:<dst>` refspec, stream pack
//! data into the target bare repository, and return the resolved
//! ref. Layout, locking, and ref-mirroring are the cache's
//! concerns.
//!
//! V1 ships one impl: [`GitCli`], which shells out to the host's
//! `git` binary per `specs/DECISIONS.md` §8. A future
//! `gix-protocol`-based impl is a localized swap behind the
//! [`GitCacheTransport`] trait.

use std::path::Path;
use std::process::Command;

use crate::GitError;

/// One ref that landed in the target repo as a result of a fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedRef {
    /// Local ref name — the destination side of the refspec.
    pub ref_name: String,
    /// Hex-encoded commit OID at the ref tip.
    pub oid: String,
}

/// Transport that fetches Git refs from a remote URL into a local
/// bare repository.
///
/// Implementations must:
///
/// - Run a `git fetch`-equivalent operation that lands `refspec`'s
///   source at its destination inside `target_repo`.
/// - Return the destination ref's resolved OID after the fetch
///   completes.
/// - Treat `refspec` as a single explicit `<src>:<dst>` pair — no
///   glob expansion in v1.
pub trait GitCacheTransport {
    fn fetch(&self, target_repo: &Path, url: &str, refspec: &str) -> Result<FetchedRef, GitError>;
}

/// Shell-out transport that invokes the host's `git` binary.
///
/// Canonical invocation per `specs/DECISIONS.md` §8:
///
/// ```text
/// git -c protocol.version=2 -C <target_repo> fetch \
///     --no-tags --no-write-fetch-head --quiet \
///     <url> <refspec>
/// ```
///
/// `protocol.version=2` is set on the command line rather than via
/// the user's `~/.gitconfig` so behavior is normalized regardless
/// of the operator's local config. `--no-write-fetch-head` keeps
/// the cache's pool from accumulating `FETCH_HEAD` updates that
/// only matter for interactive workflows.
#[derive(Debug, Default, Clone, Copy)]
pub struct GitCli;

impl GitCli {
    pub fn new() -> Self {
        Self
    }
}

impl GitCacheTransport for GitCli {
    fn fetch(&self, target_repo: &Path, url: &str, refspec: &str) -> Result<FetchedRef, GitError> {
        let dest = parse_dest_ref(refspec)?;
        run_git(
            target_repo,
            &[
                "-c",
                "protocol.version=2",
                "-C",
                &target_repo.display().to_string(),
                "fetch",
                "--no-tags",
                "--no-write-fetch-head",
                "--quiet",
                url,
                refspec,
            ],
            &format!("git fetch {url} {refspec}"),
        )?;
        let oid = read_ref_oid(target_repo, &dest)?;
        Ok(FetchedRef {
            ref_name: dest,
            oid,
        })
    }
}

/// Run `git update-ref <ref_name> <oid>` inside `repo_path`. Used
/// by [`GitCache::fetch`](crate::GitCache::fetch) to mirror a
/// resolved OID from the pool's namespaced ref into the per-object
/// repo's `refs/heads/<branch>`. Public to the crate but not
/// exposed externally — callers go through the cache.
pub(crate) fn update_ref(repo_path: &Path, ref_name: &str, oid: &str) -> Result<(), GitError> {
    run_git(
        repo_path,
        &[
            "-C",
            &repo_path.display().to_string(),
            "update-ref",
            ref_name,
            oid,
        ],
        &format!("git update-ref {ref_name} {oid}"),
    )
}

/// Parse the destination side of a `<src>:<dst>` refspec, stripping
/// an optional leading `+` (force-update flag). Globs are rejected
/// here because v1 transports do not support glob expansion.
fn parse_dest_ref(refspec: &str) -> Result<String, GitError> {
    let trimmed = refspec.strip_prefix('+').unwrap_or(refspec);
    let (_, dst) = trimmed
        .split_once(':')
        .ok_or_else(|| GitError::CacheGitInvocation {
            command: format!("parse refspec {refspec:?}"),
            stderr: "transport requires explicit src:dst refspec".to_owned(),
            exit_code: None,
        })?;
    if dst.contains('*')
        || trimmed
            .split_once(':')
            .map(|(s, _)| s.contains('*'))
            .unwrap_or(false)
    {
        return Err(GitError::CacheGitInvocation {
            command: format!("parse refspec {refspec:?}"),
            stderr: "v1 transport does not expand refspec globs".to_owned(),
            exit_code: None,
        });
    }
    Ok(dst.to_owned())
}

fn read_ref_oid(repo_path: &Path, ref_name: &str) -> Result<String, GitError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("rev-parse")
        .arg("--verify")
        .arg(ref_name)
        .output()
        .map_err(|source| match source.kind() {
            std::io::ErrorKind::NotFound => GitError::CacheGitMissing { source },
            _ => GitError::CacheIo {
                path: repo_path.to_path_buf(),
                source,
            },
        })?;
    if !output.status.success() {
        return Err(GitError::CacheGitInvocation {
            command: format!("git rev-parse --verify {ref_name}"),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            exit_code: output.status.code(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn run_git(cwd_for_io_error: &Path, args: &[&str], label: &str) -> Result<(), GitError> {
    let output = Command::new("git")
        .args(args)
        .output()
        .map_err(|source| match source.kind() {
            std::io::ErrorKind::NotFound => GitError::CacheGitMissing { source },
            _ => GitError::CacheIo {
                path: cwd_for_io_error.to_path_buf(),
                source,
            },
        })?;
    if !output.status.success() {
        return Err(GitError::CacheGitInvocation {
            command: label.to_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            exit_code: output.status.code(),
        });
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use kairo_test_support::git::{init_source_repo, skip_if_no_git};
    use std::process::Command;
    use tempfile::TempDir;

    /// Initialize a bare repo at `path` to serve as the fetch target.
    fn init_bare(path: &std::path::Path) {
        let status = Command::new("git")
            .args(["init", "--bare", "--quiet"])
            .arg(path)
            .status()
            .expect("git init bare");
        assert!(status.success());
    }

    #[test]
    fn fetch_lands_ref_in_target_repo() {
        if skip_if_no_git() {
            return;
        }
        let (_src, url, branch, head_oid) = init_source_repo();
        let target_dir = TempDir::new().expect("tempdir");
        let target = target_dir.path().join("target.git");
        init_bare(&target);

        let refspec = format!("refs/heads/{branch}:refs/kairo/test/{branch}");
        let result = GitCli::new().fetch(&target, &url, &refspec).expect("fetch");

        assert_eq!(result.ref_name, format!("refs/kairo/test/{branch}"));
        assert_eq!(result.oid, head_oid);

        // The destination ref must actually be present in the target repo.
        let output = Command::new("git")
            .args(["-C"])
            .arg(&target)
            .args([
                "rev-parse",
                "--verify",
                &format!("refs/kairo/test/{branch}"),
            ])
            .output()
            .expect("rev-parse");
        assert!(output.status.success());
        let resolved = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        assert_eq!(resolved, head_oid);
    }

    #[test]
    fn fetch_unknown_branch_errors_with_stderr() {
        if skip_if_no_git() {
            return;
        }
        let (_src, url, _branch, _head) = init_source_repo();
        let target_dir = TempDir::new().expect("tempdir");
        let target = target_dir.path().join("target.git");
        init_bare(&target);

        let refspec = "refs/heads/no-such-branch:refs/kairo/test/no-such-branch";
        let err = GitCli::new()
            .fetch(&target, &url, refspec)
            .expect_err("fetch must error");
        match err {
            GitError::CacheGitInvocation { stderr, .. } => {
                // git's exact wording varies but always names the missing ref.
                assert!(
                    stderr.contains("no-such-branch") || stderr.contains("couldn't find"),
                    "stderr should describe missing ref, got: {stderr}"
                );
            }
            other => panic!("expected CacheGitInvocation, got {other:?}"),
        }
    }

    #[test]
    fn fetch_missing_remote_errors() {
        if skip_if_no_git() {
            return;
        }
        let target_dir = TempDir::new().expect("tempdir");
        let target = target_dir.path().join("target.git");
        init_bare(&target);

        let bogus_url = format!("file://{}/does-not-exist", target_dir.path().display());
        let err = GitCli::new()
            .fetch(&target, &bogus_url, "refs/heads/main:refs/kairo/test/main")
            .expect_err("fetch must error");
        assert!(matches!(err, GitError::CacheGitInvocation { .. }));
    }

    #[test]
    fn parse_dest_ref_strips_force_marker() {
        let dst = parse_dest_ref("+refs/heads/main:refs/kairo/zz/main").expect("parse");
        assert_eq!(dst, "refs/kairo/zz/main");
    }

    #[test]
    fn parse_dest_ref_rejects_missing_colon() {
        assert!(matches!(
            parse_dest_ref("refs/heads/main"),
            Err(GitError::CacheGitInvocation { .. })
        ));
    }

    #[test]
    fn parse_dest_ref_rejects_globs() {
        assert!(matches!(
            parse_dest_ref("refs/heads/*:refs/kairo/zz/*"),
            Err(GitError::CacheGitInvocation { .. })
        ));
    }
}
