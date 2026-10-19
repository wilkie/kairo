//! `git-remote-kairo` (`specs/DECISIONS.md` §31): a Git remote helper
//! for `kairo://<host>/<object-id>` URLs.
//!
//! On `list`, the helper pulls the object through the Kairo verifier
//! (`kairo fetch`'s checks: an authenticated node, verified records and
//! chains, Git commits admitted through quarantine) into the user's
//! store. It then lists refs from the signed branches and tags whose
//! commits the store's Git cache holds, and `fetch` copies commits from
//! that cache into the clone. Nothing goes from the peer to Git
//! directly. `push` is refused.
//!
//! The protocol is Git's remote-helper protocol (`gitremote-helpers(7)`):
//! `capabilities`, `option`, `list`, `fetch` batches, and a blank line
//! to end.

// Dev-dependencies used only by `tests/`.
#[cfg(test)]
use kairo_identity as _;
#[cfg(test)]
use kairo_test_support as _;
#[cfg(test)]
use tempfile as _;
#[cfg(test)]
use tokio as _;

use std::collections::BTreeSet;
use std::fmt;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use kairo_config::FederationConfig;
use kairo_core::ObjectId;
use kairo_federation_http::client::{normalize_peer_url, pull_object, PullOptions, PullReport};
use kairo_git::GitCache;
use kairo_statement::{GitObjectFormat, GitRevision};
use kairo_store::{
    BranchResolver, FilesystemStore, ObjectStore, StatementStore, VersionTagResolver,
};

/// What the helper needs from its environment.
#[derive(Debug, Clone)]
pub struct Context {
    /// The remote's URL, `kairo://<host>/<object-id>`.
    pub url: String,
    /// The Kairo store root.
    pub store: PathBuf,
    /// The clone's `$GIT_DIR`.
    pub git_dir: PathBuf,
    /// `kairo.allowPeer` for this repository.
    pub allow_peer: bool,
}

#[derive(Debug)]
pub enum RemoteError {
    Url(String),
    Policy(String),
    Store(String),
    Pull(String),
    Git(String),
    Protocol(String),
    Io(std::io::Error),
}

impl fmt::Display for RemoteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Url(message) | Self::Policy(message) => write!(f, "{message}"),
            Self::Store(message) => write!(f, "kairo store: {message}"),
            Self::Pull(message) => write!(f, "kairo pull failed: {message}"),
            Self::Git(message) => write!(f, "git: {message}"),
            Self::Protocol(message) => write!(f, "remote-helper protocol: {message}"),
            Self::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for RemoteError {}

impl From<std::io::Error> for RemoteError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// `kairo://<host>[:port]/<object-id>` → the peer's base URL and the
/// object. A loopback host uses `http://`, as `kairo fetch` allows.
pub fn parse_url(url: &str) -> Result<(String, ObjectId), RemoteError> {
    let bad = |reason: &str| {
        RemoteError::Url(format!(
            "{url:?} is not kairo://<host>/<object-id>: {reason}"
        ))
    };
    let rest = url
        .strip_prefix("kairo://")
        .ok_or_else(|| bad("wrong scheme"))?;
    let (host, object) = rest.split_once('/').ok_or_else(|| bad("no object id"))?;
    let object = object.trim_end_matches('/');
    if host.is_empty() || object.is_empty() || object.contains('/') {
        return Err(bad("expected exactly a host and an object id"));
    }
    let object = ObjectId::new(object.to_owned()).map_err(|error| bad(&error.to_string()))?;
    let loopback = {
        let name = host.rsplit_once(':').map_or(host, |(name, _)| name);
        let name = name.trim_start_matches('[').trim_end_matches(']');
        name.eq_ignore_ascii_case("localhost")
            || name
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    };
    let scheme = if loopback { "http" } else { "https" };
    let base = normalize_peer_url(&format!("{scheme}://{host}"))
        .map_err(|error| bad(&error.to_string()))?;
    Ok((base, object))
}

/// One ref the clone sees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedRef {
    pub name: String,
    pub commit: GitRevision,
}

/// What `list` found: the refs, the object's Git format, the ref
/// `HEAD` points at, and what was left out (said on stderr).
#[derive(Debug, Default)]
pub struct Listing {
    pub refs: Vec<ListedRef>,
    pub format: Option<GitObjectFormat>,
    pub head: Option<String>,
    pub skipped: Vec<String>,
}

/// Pull the object, then list its refs from what's verified and held
/// (§31 sub-decisions 1 and 3).
pub fn pull_and_list(
    context: &Context,
    config: &FederationConfig,
) -> Result<(PullReport, Listing), RemoteError> {
    let (base_url, object) = parse_url(&context.url)?;
    let store = FilesystemStore::open(&context.store)
        .map_err(|error| RemoteError::Store(error.to_string()))?;
    let options = PullOptions {
        limits: kairo_federation_service::limits_from(&config.limits),
        expected_node_actor: config
            .peers
            .iter()
            .find(|peer| normalize_peer_url(&peer.url).is_ok_and(|url| url == base_url))
            .and_then(|peer| peer.node_actor.clone()),
        require_complete: false,
    };
    let report = pull_object(&store, &base_url, &object, &options)
        .map_err(|error| RemoteError::Pull(error.to_string()))?;
    let listing = list_refs(&store, &object)?;
    Ok((report, listing))
}

/// The refs for `object` from the store: the creator's branches and
/// tags, other actors' branches namespaced, each only if its revision's
/// commit is in the cache.
pub fn list_refs(store: &FilesystemStore, object: &ObjectId) -> Result<Listing, RemoteError> {
    let store_error = |error: kairo_store::StoreError| RemoteError::Store(error.to_string());
    let genesis = store.get_object_genesis(object).map_err(store_error)?;
    let creator = genesis.body().created_by().clone();
    let cache =
        GitCache::open(store.root().join("git")).map_err(|e| RemoteError::Git(e.to_string()))?;
    let mut listing = Listing::default();
    let commit_of = |revision: &kairo_core::StatementId,
                     label: &str,
                     listing: &mut Listing|
     -> Result<Option<GitRevision>, RemoteError> {
        let signed = store.get_object_revision(revision).map_err(store_error)?;
        let Ok(Some(commit)) = signed.unsigned().body().revision().git() else {
            listing
                .skipped
                .push(format!("{label}: its revision names no Git commit"));
            return Ok(None);
        };
        if !cache
            .has_commit(object.as_str(), &commit)
            .map_err(|e| RemoteError::Git(e.to_string()))?
        {
            listing
                .skipped
                .push(format!("{label}: commit {commit} isn't held"));
            return Ok(None);
        }
        Ok(Some(commit))
    };

    for tip in store.list_branches(object).map_err(store_error)? {
        let name = if tip.actor == creator {
            format!("refs/heads/{}", tip.name)
        } else {
            format!("refs/kairo/actors/{}/{}", tip.actor, tip.name)
        };
        let branch = store
            .get_object_branch(&tip.statement_id)
            .map_err(store_error)?;
        if let Some(commit) = commit_of(branch.unsigned().body().revision(), &name, &mut listing)? {
            if tip.actor == creator && tip.name == "head" {
                listing.head = Some(name.clone());
            }
            listing.refs.push(ListedRef { name, commit });
        }
    }
    for tag in store.list_version_tags(object).map_err(store_error)? {
        if tag.actor != creator {
            continue;
        }
        let signed = store
            .get_object_version_tag(&tag.statement_id)
            .map_err(store_error)?;
        let Some(target) = signed.unsigned().body().target() else {
            continue; // revoked
        };
        let name = format!("refs/tags/{}", tag.version);
        if let Some(commit) = commit_of(target, &name, &mut listing)? {
            listing.refs.push(ListedRef { name, commit });
        }
    }
    let formats: BTreeSet<GitObjectFormat> =
        listing.refs.iter().map(|r| r.commit.format()).collect();
    listing.format = formats.iter().next().copied();
    if formats.len() > 1 {
        return Err(RemoteError::Git(format!(
            "object {object}'s refs mix Git object formats"
        )));
    }
    listing.refs.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(listing)
}

/// Copy `wants` (and what they reach) from the cache's repository for
/// `object` into the clone at `git_dir`.
pub fn fetch_into(
    store: &Path,
    object: &ObjectId,
    wants: &[GitRevision],
    git_dir: &Path,
) -> Result<(), RemoteError> {
    if wants.is_empty() {
        return Ok(());
    }
    let cache = GitCache::open(store.join("git")).map_err(|e| RemoteError::Git(e.to_string()))?;
    let haves = clone_tips(&cache, object, git_dir, wants[0].format())?;
    let mut child = Command::new("git")
        .arg("--git-dir")
        .arg(git_dir)
        .args(["index-pack", "--stdin", "--fix-thin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| RemoteError::Git("index-pack stdin not captured".to_owned()))?;
    let packed = cache.pack_delta_to(object.as_str(), wants, &haves, stdin);
    let output = child.wait_with_output()?;
    packed.map_err(|e| RemoteError::Git(e.to_string()))?;
    if !output.status.success() {
        return Err(RemoteError::Git(format!(
            "index-pack failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

/// The clone's ref tips that the cache also holds: what a later
/// `git fetch` already has, so the pack is thin against them and only
/// new history moves (`index-pack --fix-thin` completes it from the
/// clone's own objects).
fn clone_tips(
    cache: &GitCache,
    object: &ObjectId,
    git_dir: &Path,
    format: GitObjectFormat,
) -> Result<Vec<GitRevision>, RemoteError> {
    let output = Command::new("git")
        .arg("--git-dir")
        .arg(git_dir)
        .args(["for-each-ref", "--format=%(objectname)"])
        .output()?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let mut tips = Vec::new();
    for oid in String::from_utf8_lossy(&output.stdout).lines() {
        let Ok(tip) = GitRevision::new(format, oid.trim()) else {
            continue;
        };
        if !tips.contains(&tip)
            && cache
                .has_commit(object.as_str(), &tip)
                .map_err(|e| RemoteError::Git(e.to_string()))?
        {
            tips.push(tip);
        }
    }
    Ok(tips)
}

/// The policy check (§31 sub-decision 4): federation enabled, and the
/// host listed unless `kairo.allowPeer`.
pub fn check_policy(context: &Context, config: &kairo_config::Config) -> Result<(), RemoteError> {
    if !config.federation.enabled {
        return Err(RemoteError::Policy(
            "federation is disabled ([federation] enabled = false in the Kairo store's config.toml)"
                .to_owned(),
        ));
    }
    let (base_url, _) = parse_url(&context.url)?;
    let listed = config
        .federation
        .peers
        .iter()
        .any(|peer| normalize_peer_url(&peer.url).is_ok_and(|url| url == base_url));
    if !listed && !context.allow_peer {
        return Err(RemoteError::Policy(format!(
            "{base_url} is not a listed peer; add a [[federation.peers]] entry for it to the Kairo \
             store's config.toml, or run `git config kairo.allowPeer true` for this repository"
        )));
    }
    Ok(())
}

/// Run the helper protocol over `input` / `output` until Git closes it.
pub fn serve(
    context: &Context,
    input: impl BufRead,
    mut output: impl Write,
    mut log: impl Write,
) -> Result<(), RemoteError> {
    let (config, _) = kairo_config::load(&context.store)
        .map_err(|error| RemoteError::Policy(error.to_string()))?;
    let (_, object) = parse_url(&context.url)?;
    let mut listing: Option<Listing> = None;
    let mut object_format = false;
    let mut wants: Vec<GitRevision> = Vec::new();
    let mut lines = input.lines();
    while let Some(line) = lines.next() {
        let line = line?;
        let (command, rest) = line.split_once(' ').unwrap_or((line.as_str(), ""));
        match command {
            "" if !wants.is_empty() => {
                fetch_into(&context.store, &object, &wants, &context.git_dir)?;
                wants.clear();
                writeln!(output)?;
                output.flush()?;
            }
            "" => return Ok(()),
            "capabilities" => {
                writeln!(output, "fetch\noption\npush\nobject-format\n")?;
                output.flush()?;
            }
            "option" => {
                let (name, value) = rest.split_once(' ').unwrap_or((rest, ""));
                let answer = match name {
                    // Git 2.43 sends the option bare, meaning `true`.
                    "object-format" => {
                        object_format = value.is_empty() || value == "true";
                        "ok"
                    }
                    "verbosity" | "progress" | "cloning" | "check-connectivity" => "ok",
                    _ => "unsupported",
                };
                writeln!(output, "{answer}")?;
                output.flush()?;
            }
            "list" if rest == "for-push" => {
                writeln!(output)?;
                output.flush()?;
            }
            "list" => {
                check_policy(context, &config)?;
                let (report, found) = pull_and_list(context, &config.federation)?;
                writeln!(
                    log,
                    "kairo: pulled {object} from {}: {} fetched, {} already held",
                    report.peer_url, report.fetched, report.already_held
                )?;
                for skipped in &found.skipped {
                    writeln!(log, "kairo: left out {skipped}")?;
                }
                if object_format {
                    if let Some(format) = found.format {
                        writeln!(output, ":object-format {format}")?;
                    }
                }
                for listed in &found.refs {
                    writeln!(output, "{} {}", listed.commit.oid(), listed.name)?;
                }
                if let Some(head) = &found.head {
                    writeln!(output, "@{head} HEAD")?;
                }
                writeln!(output)?;
                output.flush()?;
                listing = Some(found);
            }
            "fetch" => {
                let oid = rest.split(' ').next().unwrap_or_default();
                let found = listing
                    .as_ref()
                    .ok_or_else(|| RemoteError::Protocol("fetch before list".to_owned()))?;
                let commit = found
                    .refs
                    .iter()
                    .find(|listed| listed.commit.oid() == oid)
                    .map(|listed| listed.commit.clone())
                    .ok_or_else(|| RemoteError::Protocol(format!("{oid} wasn't listed")))?;
                if !wants.contains(&commit) {
                    wants.push(commit);
                }
            }
            "push" => {
                // Read the whole push batch, refusing each ref.
                let mut refs = vec![rest.to_owned()];
                for line in lines.by_ref() {
                    let line = line?;
                    if line.is_empty() {
                        break;
                    }
                    refs.push(line.trim_start_matches("push ").to_owned());
                }
                for spec in refs {
                    let dst = spec.rsplit(':').next().unwrap_or_default();
                    writeln!(
                        output,
                        "error {dst} kairo:// remotes are read-only; publish with `kairo push`"
                    )?;
                }
                writeln!(output)?;
                output.flush()?;
            }
            other => {
                return Err(RemoteError::Protocol(format!("unknown command {other:?}")));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_name_a_host_and_an_object() {
        let object = ObjectId::from_sha256_digest([1; 32]);
        let parsed = |url: String| parse_url(&url).map(|(base, id)| (base, id.to_string()));
        assert_eq!(
            parsed(format!("kairo://archive.example.org/{object}")).ok(),
            Some(("https://archive.example.org".to_owned(), object.to_string()))
        );
        assert_eq!(
            parsed(format!("kairo://127.0.0.1:7880/{object}/")).ok(),
            Some(("http://127.0.0.1:7880".to_owned(), object.to_string()))
        );
        assert_eq!(
            parsed(format!("kairo://localhost:7880/{object}")).ok(),
            Some(("http://localhost:7880".to_owned(), object.to_string()))
        );
        for bad in [
            format!("https://archive.example.org/{object}"),
            "kairo://archive.example.org/".to_owned(),
            format!("kairo:///{object}"),
            format!("kairo://archive.example.org/a/{object}"),
            "kairo://archive.example.org/not-an-id".to_owned(),
        ] {
            assert!(parse_url(&bad).is_err(), "{bad}");
        }
    }
}
