//! `kairo source ...` runners (`specs/DECISIONS.md` §37–§38): source
//! objects, external Git repositories archived faithfully and
//! identified by their canonical URL, and single files identified by
//! their SHA-256.

use std::fmt::Write as _;

use kairo_core::canonical::CanonicalEncode;
use kairo_core::{KairoRef, ObjectId, Timestamp};
use kairo_git::{FetchLimits, GitCache, GitObjectFormat};
use kairo_statement::{
    FileObservationBody, FileSourceGenesis, FileUrl, ObservedRef, Sha256Digest, Signature,
    SignedStatement, SourceGenesis, SourceObservationBody, SourceUrl, UnsignedStatement,
};
use kairo_store::{SourceStore, StatementStore};

use crate::cli::SourceCommand;
use crate::commands::config::load_config;
use crate::error::CliError;
use crate::store_paths::{open_keystore, open_store, StorePaths};

pub(crate) fn run_source_command(
    command: SourceCommand,
    paths: &StorePaths,
    offline: bool,
) -> Result<String, CliError> {
    match command {
        SourceCommand::Id { url } if Sha256Digest::parse(&url).is_ok() => {
            let sha256 = Sha256Digest::parse(&url)
                .map_err(|error| CliError::ParseFileSource(error.to_string()))?;
            let source = FileSourceGenesis::new(sha256).object_id();
            Ok(format!("sha256 = {sha256}\nsource = {source}\n"))
        }
        SourceCommand::Id { url } => {
            let url = SourceUrl::parse(&url).map_err(CliError::ParseSourceUrl)?;
            let source = SourceGenesis::new(url.clone()).object_id();
            Ok(format!("url = {url}\nsource = {source}\n"))
        }
        SourceCommand::Fetch {
            url,
            actor,
            object_format,
            no_recurse_submodules,
            max_depth,
            require_complete,
            mirrors,
        } => run_fetch(
            paths,
            &url,
            &actor,
            object_format.as_deref(),
            !no_recurse_submodules,
            max_depth,
            require_complete,
            &mirrors,
        ),
        SourceCommand::FetchFile { url, sha256, actor } => {
            run_fetch_file(paths, &url, sha256.as_deref(), &actor)
        }
        SourceCommand::Show { source, json } => run_show(paths, &source, json),
        SourceCommand::Pull {
            url,
            from,
            allow_peer,
            json,
        } => {
            // A file's hash names a file source (§38); anything else is
            // a repository URL.
            let source = match Sha256Digest::parse(&url) {
                Ok(sha256) => FileSourceGenesis::new(sha256).object_id().to_string(),
                Err(_) => {
                    let url = SourceUrl::parse(&url).map_err(CliError::ParseSourceUrl)?;
                    SourceGenesis::new(url).object_id().to_string()
                }
            };
            crate::commands::federation::run_fetch(
                &crate::commands::federation::FetchArgs {
                    object: &source,
                    from: from.as_deref(),
                    allow_peer,
                    require_complete: false,
                    json,
                    offline,
                },
                paths,
            )
        }
    }
}

fn git_error(source: kairo_git::GitError) -> CliError {
    CliError::GitOperation { source }
}

/// What one `kairo source fetch` run shares across the sources it
/// observes.
struct Fetcher<'a> {
    store: &'a kairo_store::FilesystemStore,
    cache: GitCache,
    actor: kairo_core::ActorId,
    secret: kairo_identity::SecretSigningKey,
    limits: FetchLimits,
    /// Mirrors to fetch a source through when its own URL fails
    /// (`DECISIONS.md` §40).
    mirrors: std::collections::BTreeMap<SourceUrl, SourceUrl>,
}

/// What one fetch from one URL brought.
struct Fetched {
    refs: Vec<(String, kairo_git::GitRevision)>,
    pinned: Vec<kairo_git::GitRevision>,
    missing: Vec<(kairo_git::GitRevision, String)>,
}

/// One source observed in this run.
struct Observed {
    url: SourceUrl,
    source: ObjectId,
    observation: kairo_core::StatementId,
    body: SourceObservationBody,
    /// The newest observation's refs before this one.
    previous: Vec<(String, String)>,
    /// Required commits that couldn't be archived, with why.
    missing: Vec<(kairo_git::GitRevision, String)>,
}

impl Fetcher<'_> {
    /// Fetch `url`'s refs, then any of `required` (commits a parent
    /// pins) its refs don't reach, by id; sign one observation of it
    /// all.
    fn observe(
        &self,
        url: &SourceUrl,
        flag: Option<GitObjectFormat>,
        required: &[kairo_git::GitRevision],
    ) -> Result<Observed, CliError> {
        let source = self
            .store
            .put_source(&SourceGenesis::new(url.clone()))
            .map_err(|source| CliError::Sources { source })?;

        // From the source's own URL, or failing that through its mirror,
        // which the observation then names.
        let (fetched, via) = match self.fetch_from(&source, url.as_str(), flag, required) {
            Ok(fetched) => (fetched, None),
            Err(error) => match self.mirrors.get(url) {
                None => return Err(error),
                Some(mirror) => {
                    eprintln!("{url}: {error}; fetching through the mirror {mirror}");
                    let fetched = self.fetch_from(&source, mirror.as_str(), flag, required)?;
                    (fetched, Some(mirror.clone()))
                }
            },
        };
        let Fetched {
            refs,
            pinned,
            missing,
        } = fetched;

        let observed = refs
            .iter()
            .map(|(name, target)| ObservedRef::new(name.clone(), target.clone()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| CliError::ObservationShape(error.to_string()))?;
        let previous = latest_refs(self.store, &source)?;
        let mut body = SourceObservationBody::new(url.clone(), observed)
            .and_then(|body| body.with_pinned(pinned))
            .map_err(|error| CliError::ObservationShape(error.to_string()))?;
        if let Some(via) = via {
            body = body.with_via(via);
        }
        let statement = sign(self.store, &self.actor, &self.secret, &source, body.clone())?;
        let observation = self
            .store
            .put_source_observation(&statement)
            .map_err(|source| CliError::Sources { source })?;
        Ok(Observed {
            url: url.clone(),
            source,
            observation,
            body,
            previous,
            missing,
        })
    }

    /// Fetch `source`'s refs from `fetch_url`, then any of `required`
    /// its refs don't reach, by id, into its cache repo.
    fn fetch_from(
        &self,
        source: &ObjectId,
        fetch_url: &str,
        flag: Option<GitObjectFormat>,
        required: &[kairo_git::GitRevision],
    ) -> Result<Fetched, CliError> {
        // The format: the flag, the source's cache repo, the remote,
        // and the commits a parent pins must agree where present (§19
        // sub-decision 7).
        let repo = self.cache.repo_format(source.as_str()).map_err(git_error)?;
        let remote = GitCache::source_format(fetch_url).map_err(git_error)?;
        let pinned_format = required.first().map(kairo_git::GitRevision::format);
        let mut chosen: Option<(&str, GitObjectFormat)> = None;
        for (what, format) in [
            ("--object-format", flag),
            ("the cache repo", repo),
            ("the remote", remote),
            ("the parent's gitlink", pinned_format),
        ] {
            let Some(format) = format else { continue };
            match chosen {
                None => chosen = Some((what, format)),
                Some((first, existing)) if existing != format => {
                    return Err(CliError::GitFormatConflict {
                        object: source.to_string(),
                        message: format!("{first} says {existing} but {what} says {format}"),
                    })
                }
                Some(_) => {}
            }
        }

        // A remote with no refs reports no format and holds nothing to
        // fetch by name; the observation records that it was empty.
        let refs = match chosen.filter(|_| remote.is_some()) {
            None => Vec::new(),
            Some((_, format)) => {
                self.cache
                    .fetch_source(source.as_str(), fetch_url, format, &self.limits)
                    .map_err(git_error)?
                    .refs
            }
        };

        // Pinned commits the refs don't reach are fetched by id.
        let mut pinned = Vec::new();
        let mut missing = Vec::new();
        let held = |commit: &kairo_git::GitRevision| {
            self.cache
                .has_commit(source.as_str(), commit)
                .unwrap_or(false)
        };
        let wanted: Vec<kairo_git::GitRevision> = required
            .iter()
            .filter(|commit| !held(commit))
            .cloned()
            .collect();
        if !wanted.is_empty() {
            match self
                .cache
                .fetch_source_commits(source.as_str(), fetch_url, &wanted, &self.limits)
            {
                Ok(()) => pinned.extend(wanted),
                // One at a time, so one unavailable commit doesn't cost
                // the others.
                Err(_) => {
                    for commit in wanted {
                        match self.cache.fetch_source_commits(
                            source.as_str(),
                            fetch_url,
                            std::slice::from_ref(&commit),
                            &self.limits,
                        ) {
                            Ok(()) => pinned.push(commit),
                            Err(error) => missing.push((commit, error.to_string())),
                        }
                    }
                }
            }
        }

        Ok(Fetched {
            refs,
            pinned,
            missing,
        })
    }

    /// The submodules each of `commits` pins, as `(link, its source
    /// URL)`; a link whose URL is absent or has no source identity is
    /// returned with the reason instead.
    fn links(
        &self,
        parent: &Observed,
        commits: &[kairo_git::GitRevision],
    ) -> Result<Vec<Link>, CliError> {
        let repo_path = self
            .cache
            .path_for(parent.source.as_str())
            .map_err(git_error)?;
        if !repo_path.exists() {
            return Ok(Vec::new());
        }
        let repo = kairo_git::open(&repo_path).map_err(git_error)?;
        let mut links = Vec::new();
        for commit in commits {
            for link in repo.submodules_at(commit).map_err(git_error)? {
                let url = match &link.url {
                    None => Err("no .gitmodules entry for this path".to_owned()),
                    Some(raw) => parent.url.join(raw).map_err(|error| error.to_string()),
                };
                links.push(Link {
                    parent: parent.url.clone(),
                    parent_commit: commit.clone(),
                    path: link.path,
                    commit: link.commit,
                    url,
                });
            }
        }
        Ok(links)
    }
}

struct Link {
    parent: SourceUrl,
    parent_commit: kairo_git::GitRevision,
    path: String,
    commit: kairo_git::GitRevision,
    url: Result<SourceUrl, String>,
}

#[allow(clippy::too_many_arguments)]
fn run_fetch(
    paths: &StorePaths,
    url: &str,
    actor: &str,
    flag: Option<&str>,
    recurse: bool,
    max_depth: u32,
    require_complete: bool,
    mirrors: &[String],
) -> Result<String, CliError> {
    let url = SourceUrl::parse(url).map_err(CliError::ParseSourceUrl)?;
    let mirrors = mirrors
        .iter()
        .map(|pair| {
            let (source, mirror) = pair.split_once('=').ok_or_else(|| {
                CliError::ParseFileSource(format!(
                    "--mirror {pair:?} is not <source url>=<mirror url>"
                ))
            })?;
            let source = SourceUrl::parse(source).map_err(CliError::ParseSourceUrl)?;
            let mirror = SourceUrl::parse(mirror).map_err(CliError::ParseSourceUrl)?;
            Ok((source, mirror))
        })
        .collect::<Result<std::collections::BTreeMap<_, _>, CliError>>()?;
    let actor_id =
        kairo_core::ActorId::new(actor.to_owned()).map_err(|source| CliError::ParseActorId {
            actor: actor.to_owned(),
            source,
        })?;
    let store = open_store(paths)?;
    let keystore = open_keystore(paths)?;
    let secret = crate::require_active_signing_key(&store, &keystore, &actor_id)?;
    let (config, _) = load_config(paths)?;
    let limits = kairo_federation_service::limits_from(&config.federation.limits);
    let flag = flag
        .map(|raw| {
            raw.parse::<GitObjectFormat>()
                .map_err(|source| CliError::InvalidGitRevision {
                    revision: format!("--object-format {raw}"),
                    source,
                })
        })
        .transpose()?;
    let fetcher = Fetcher {
        store: &store,
        cache: GitCache::open(paths.git_root()).map_err(git_error)?,
        actor: actor_id,
        secret,
        limits: FetchLimits {
            max_bytes: limits.max_git_bytes,
            timeout: limits.git_timeout,
        },
        mirrors,
    };

    let top = fetcher.observe(&url, flag, &[])?;
    let mut out = String::new();
    let _ = writeln!(out, "url = {}", top.url);
    let _ = writeln!(out, "source = {}", top.source);
    let _ = writeln!(out, "observation = {}", top.observation);
    let _ = writeln!(
        out,
        "object_format = {}",
        top.body
            .format()
            .map_or("none (no refs)".to_owned(), |format| format.to_string())
    );
    let _ = writeln!(out, "refs = {}", top.body.refs().len());
    if let Some(via) = top.body.via() {
        let _ = writeln!(out, "via = {via}");
    }
    push_changes(&mut out, &top);
    if !recurse {
        return Ok(out);
    }

    // Submodules (§37 sub-decision 5), breadth first: the top source's
    // ref tips, then at each level the commits the level above pins,
    // grouped by source so each source is observed once per level.
    let repo_path = fetcher
        .cache
        .path_for(top.source.as_str())
        .map_err(git_error)?;
    let tips: Vec<kairo_git::GitRevision> = if repo_path.exists() {
        let repo = kairo_git::open(&repo_path).map_err(git_error)?;
        let mut tips = Vec::new();
        for observed in top.body.refs() {
            if let Some(commit) = repo.peel_to_commit(observed.target()).map_err(git_error)? {
                if !tips.contains(&commit) {
                    tips.push(commit);
                }
            }
        }
        tips
    } else {
        Vec::new()
    };
    let mut visited: std::collections::BTreeSet<(String, String)> = Default::default();
    let mut links = fetcher.links(&top, &tips)?;
    let mut lines = Vec::new();
    let mut observed_lines = Vec::new();
    let mut incomplete = 0usize;
    let mut depth = 1;
    while !links.is_empty() {
        let mut by_source: std::collections::BTreeMap<
            String,
            (SourceUrl, Vec<kairo_git::GitRevision>),
        > = Default::default();
        for link in &links {
            match &link.url {
                Err(reason) => {
                    incomplete += 1;
                    lines.push(format!(
                        "  {}@{} {} -> {}: unresolved ({reason})",
                        link.parent,
                        link.parent_commit.oid(),
                        link.path,
                        link.commit
                    ));
                }
                Ok(sub) => {
                    if depth > max_depth {
                        incomplete += 1;
                        lines.push(format!(
                            "  {} {} -> {sub} {}: not fetched (depth limit {max_depth})",
                            link.parent, link.path, link.commit
                        ));
                        continue;
                    }
                    if !visited.insert((sub.to_string(), link.commit.oid().to_owned())) {
                        continue;
                    }
                    let entry = by_source
                        .entry(sub.to_string())
                        .or_insert_with(|| (sub.clone(), Vec::new()));
                    if !entry.1.contains(&link.commit) {
                        entry.1.push(link.commit.clone());
                    }
                    lines.push(format!(
                        "  {} {} -> {sub} {}",
                        link.parent, link.path, link.commit
                    ));
                }
            }
        }
        let mut next = Vec::new();
        for (sub, required) in by_source.into_values() {
            let observed = match fetcher.observe(&sub, None, &required) {
                Ok(observed) => observed,
                Err(error) => {
                    incomplete += required.len();
                    lines.push(format!("  {sub}: not archived ({error})"));
                    continue;
                }
            };
            observed_lines.push(format!(
                "  {} = {} (refs {}, pinned {}{})",
                observed.url,
                observed.observation,
                observed.body.refs().len(),
                observed.body.pinned().len(),
                observed
                    .body
                    .via()
                    .map(|via| format!(", via {via}"))
                    .unwrap_or_default()
            ));
            for (commit, reason) in &observed.missing {
                incomplete += 1;
                lines.push(format!("  {sub} {commit}: not archived ({reason})"));
            }
            let held: Vec<kairo_git::GitRevision> = required
                .iter()
                .filter(|commit| {
                    !observed
                        .missing
                        .iter()
                        .any(|(missing, _)| missing == *commit)
                })
                .cloned()
                .collect();
            next.extend(fetcher.links(&observed, &held)?);
        }
        links = next;
        depth += 1;
    }
    let _ = writeln!(out, "submodules = {}", lines.len());
    for line in &lines {
        let _ = writeln!(out, "{line}");
    }
    if !observed_lines.is_empty() {
        let _ = writeln!(out, "observed:");
        for line in &observed_lines {
            let _ = writeln!(out, "{line}");
        }
    }
    let _ = writeln!(
        out,
        "complete = {}",
        if incomplete == 0 { "yes" } else { "no" }
    );
    if incomplete > 0 && require_complete {
        return Err(CliError::SourceIncomplete {
            summary: out,
            missing: incomplete,
        });
    }
    Ok(out)
}

/// What changed upstream since the newest earlier observation.
fn push_changes(out: &mut String, observed: &Observed) {
    let previous = &observed.previous;
    if previous.is_empty() {
        let _ = writeln!(out, "first_observation = yes");
        return;
    }
    let current = observed.body.refs();
    let mut changes = Vec::new();
    for now in current {
        match previous.iter().find(|(name, _)| name == now.name()) {
            None => changes.push(format!("  + {} {}", now.name(), now.target())),
            Some((_, old)) if old != &now.target().to_string() => {
                changes.push(format!("  ~ {} {old} -> {}", now.name(), now.target()));
            }
            _ => {}
        }
    }
    for (name, old) in previous {
        if !current.iter().any(|now| now.name() == name) {
            changes.push(format!("  - {name} {old} (still held)"));
        }
    }
    let _ = writeln!(out, "changes = {}", changes.len());
    for change in changes {
        let _ = writeln!(out, "{change}");
    }
}

/// The refs of the newest observation of `source`, by anyone.
fn latest_refs(
    store: &kairo_store::FilesystemStore,
    source: &ObjectId,
) -> Result<Vec<(String, String)>, CliError> {
    let heads = store
        .list_source_observations(source)
        .map_err(|source| CliError::Sources { source })?;
    let Some(newest) = heads.first() else {
        return Ok(Vec::new());
    };
    let statement = store
        .get_source_observation(&newest.statement_id)
        .map_err(|source| CliError::Sources { source })?;
    Ok(statement
        .unsigned()
        .body()
        .refs()
        .iter()
        .map(|observed| (observed.name().to_owned(), observed.target().to_string()))
        .collect())
}

/// Sign `body` as `actor`, linked to its chain.
fn sign<B: kairo_statement::StatementBody + Clone>(
    store: &kairo_store::FilesystemStore,
    actor: &kairo_core::ActorId,
    secret: &kairo_identity::SecretSigningKey,
    source: &ObjectId,
    body: B,
) -> Result<SignedStatement<B>, CliError> {
    let subject: KairoRef =
        format!("object:{source}")
            .parse()
            .map_err(|error| CliError::BuildSubjectRef {
                object: source.clone(),
                source: error,
            })?;
    let unsigned = store
        .link_to_chain(UnsignedStatement::new(
            actor.clone(),
            subject,
            Timestamp::now(),
            body,
        ))
        .map_err(|error| CliError::ReadActorChain {
            actor: actor.clone(),
            source: error,
        })?;
    let signature_bytes = secret.sign(&unsigned.canonical_bytes());
    let signature = Signature::new(
        actor.clone(),
        secret.public_key().key_id().to_string(),
        "ed25519",
        signature_bytes.bytes().to_vec(),
    );
    Ok(SignedStatement::new(unsigned, signature))
}

/// `kairo source fetch-file` (§38 sub-decision 2).
fn run_fetch_file(
    paths: &StorePaths,
    url: &str,
    expected: Option<&str>,
    actor: &str,
) -> Result<String, CliError> {
    let url = FileUrl::parse(url).map_err(|error| CliError::ParseFileSource(error.to_string()))?;
    let expected = expected
        .map(Sha256Digest::parse)
        .transpose()
        .map_err(|error| CliError::ParseFileSource(error.to_string()))?;
    let actor_id =
        kairo_core::ActorId::new(actor.to_owned()).map_err(|source| CliError::ParseActorId {
            actor: actor.to_owned(),
            source,
        })?;
    let store = open_store(paths)?;
    let keystore = open_keystore(paths)?;
    let secret = crate::require_active_signing_key(&store, &keystore, &actor_id)?;
    let (config, _) = load_config(paths)?;
    let limits = kairo_federation_service::limits_from(&config.federation.limits);

    let bytes = download(&url, limits.max_file_bytes, limits.file_timeout)?;
    let sha256 = {
        use sha2::Digest as _;
        Sha256Digest::from_bytes(sha2::Sha256::digest(&bytes).into())
    };
    if let Some(expected) = expected {
        if expected != sha256 {
            return Err(CliError::FileHashMismatch {
                url: url.to_string(),
                expected: expected.to_string(),
                actual: sha256.to_string(),
            });
        }
    }
    let source = store
        .put_file_source(&FileSourceGenesis::new(sha256), &bytes)
        .map_err(|source| CliError::Sources { source })?;
    let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let statement = sign(
        &store,
        &actor_id,
        &secret,
        &source,
        FileObservationBody::new(url.clone(), sha256, size),
    )?;
    let observation = store
        .put_file_observation(&statement)
        .map_err(|source| CliError::Sources { source })?;
    let observations = store
        .list_file_observations(&source)
        .map_err(|source| CliError::Sources { source })?
        .len();
    Ok(format!(
        "url = {url}\nsha256 = {sha256}\nsize = {size}\nsource = {source}\nobservation = {observation}\nobservations = {observations}\n"
    ))
}

/// GET `url`, following at most one redirect, to a URL the same rules
/// accept; at most `limit` bytes within `timeout`.
fn download(url: &FileUrl, limit: u64, timeout: std::time::Duration) -> Result<Vec<u8>, CliError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into();
    let fail = |at: &FileUrl, message: String| CliError::FileFetch {
        url: at.to_string(),
        message,
    };
    let mut current = url.clone();
    for hop in 0..2 {
        let mut response = agent
            .get(current.as_str())
            .call()
            .map_err(|error| fail(&current, error.to_string()))?;
        let status = response.status();
        if status.is_redirection() {
            if hop > 0 {
                return Err(fail(url, "redirected more than once".to_owned()));
            }
            let location = response
                .headers()
                .get("location")
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| fail(&current, format!("{status} without a Location")))?;
            let target = if location.starts_with('/') {
                let origin_end = current.as_str()["https://".len()..]
                    .find('/')
                    .map_or(current.as_str().len(), |at| at + "https://".len());
                format!("{}{location}", &current.as_str()[..origin_end])
            } else {
                location.to_owned()
            };
            current = FileUrl::parse(&target)
                .map_err(|error| fail(url, format!("refused redirect: {error}")))?;
            continue;
        }
        if !status.is_success() {
            return Err(fail(&current, format!("server answered {status}")));
        }
        return response
            .body_mut()
            .with_config()
            .limit(limit)
            .read_to_vec()
            .map_err(|error| match error {
                ureq::Error::BodyExceedsLimit(_) => {
                    fail(&current, format!("larger than the {limit}-byte limit"))
                }
                other => fail(&current, other.to_string()),
            });
    }
    Err(fail(url, "redirected more than once".to_owned()))
}

fn run_show(paths: &StorePaths, source: &str, json: bool) -> Result<String, CliError> {
    let store = open_store(paths)?;
    let file_source = match Sha256Digest::parse(source) {
        Ok(sha256) => Some(FileSourceGenesis::new(sha256).object_id()),
        Err(_) => match source.parse::<KairoRef>() {
            Ok(KairoRef::Object(object)) if store.get_file_source(&object).is_ok() => Some(object),
            _ => None,
        },
    };
    if let Some(file_source) = file_source {
        return show_file_source(&store, &file_source, json);
    }
    let source_id = match source.parse::<KairoRef>() {
        Ok(KairoRef::Object(object)) => object,
        _ => SourceGenesis::new(SourceUrl::parse(source).map_err(CliError::ParseSourceUrl)?)
            .object_id(),
    };
    let genesis = store
        .get_source(&source_id)
        .map_err(|source| CliError::Sources { source })?;
    let heads = store
        .list_source_observations(&source_id)
        .map_err(|source| CliError::Sources { source })?;
    let newest = heads
        .first()
        .map(|head| store.get_source_observation(&head.statement_id))
        .transpose()
        .map_err(|source| CliError::Sources { source })?;
    if json {
        let value = serde_json::json!({
            "url": genesis.url().as_str(),
            "source": source_id.to_string(),
            "observations": heads.iter().map(|head| serde_json::json!({
                "statement": head.statement_id.to_string(),
                "observer": head.actor.to_string(),
                "observed_at": head.created_at.to_string(),
            })).collect::<Vec<_>>(),
            "via": newest.as_ref().and_then(|statement| statement.unsigned().body().via().map(ToString::to_string)),
            "refs": newest.as_ref().map(|statement| statement.unsigned().body().refs().iter().map(|observed| {
                serde_json::json!({ "name": observed.name(), "target": observed.target().to_string() })
            }).collect::<Vec<_>>()),
        });
        return Ok(format!(
            "{}\n",
            serde_json::to_string_pretty(&value).unwrap_or_default()
        ));
    }
    let mut out = String::new();
    let _ = writeln!(out, "url = {}", genesis.url());
    let _ = writeln!(out, "source = {source_id}");
    let _ = writeln!(out, "observations = {}", heads.len());
    for head in &heads {
        let _ = writeln!(
            out,
            "  {} by {} at {}",
            head.statement_id, head.actor, head.created_at
        );
    }
    if let Some(statement) = newest {
        if let Some(via) = statement.unsigned().body().via() {
            let _ = writeln!(out, "via = {via} (newest observation)");
        }
        let refs = statement.unsigned().body().refs();
        let _ = writeln!(out, "refs = {} (newest observation)", refs.len());
        for observed in refs {
            let _ = writeln!(out, "  {} {}", observed.name(), observed.target());
        }
    }
    Ok(out)
}

/// A file source's hash and every observation of it: each mirror it was
/// fetched from, by whom, and when.
fn show_file_source(
    store: &kairo_store::FilesystemStore,
    source: &ObjectId,
    json: bool,
) -> Result<String, CliError> {
    let genesis = store
        .get_file_source(source)
        .map_err(|source| CliError::Sources { source })?;
    let mut observations = Vec::new();
    for head in store
        .list_file_observations(source)
        .map_err(|source| CliError::Sources { source })?
    {
        let statement = store
            .get_file_observation(&head.statement_id)
            .map_err(|source| CliError::Sources { source })?;
        observations.push((head, statement));
    }
    let size = observations
        .first()
        .map(|(_, statement)| statement.unsigned().body().size());
    if json {
        let value = serde_json::json!({
            "sha256": genesis.sha256().to_string(),
            "source": source.to_string(),
            "size": size,
            "observations": observations.iter().map(|(head, statement)| serde_json::json!({
                "statement": head.statement_id.to_string(),
                "observer": head.actor.to_string(),
                "observed_at": head.created_at.to_string(),
                "url": statement.unsigned().body().url().as_str(),
            })).collect::<Vec<_>>(),
        });
        return Ok(format!(
            "{}\n",
            serde_json::to_string_pretty(&value).unwrap_or_default()
        ));
    }
    let mut out = String::new();
    let _ = writeln!(out, "sha256 = {}", genesis.sha256());
    let _ = writeln!(out, "source = {source}");
    if let Some(size) = size {
        let _ = writeln!(out, "size = {size}");
    }
    let _ = writeln!(out, "observations = {}", observations.len());
    for (head, statement) in &observations {
        let _ = writeln!(
            out,
            "  {} by {} at {}: {}",
            head.statement_id,
            head.actor,
            head.created_at,
            statement.unsigned().body().url()
        );
    }
    Ok(out)
}
