//! `kairo source ...` runners (`specs/DECISIONS.md` §37): source
//! objects, external Git repositories archived faithfully and
//! identified by their canonical URL.

use std::fmt::Write as _;

use kairo_core::canonical::CanonicalEncode;
use kairo_core::{KairoRef, ObjectId, Timestamp};
use kairo_git::{FetchLimits, GitCache, GitObjectFormat};
use kairo_statement::{
    ObservedRef, Signature, SignedStatement, SourceGenesis, SourceObservationBody, SourceUrl,
    UnsignedStatement,
};
use kairo_store::{SourceStore, StatementStore};

use crate::cli::SourceCommand;
use crate::commands::config::load_config;
use crate::error::CliError;
use crate::store_paths::{open_keystore, open_store, StorePaths};

pub(crate) fn run_source_command(
    command: SourceCommand,
    paths: &StorePaths,
) -> Result<String, CliError> {
    match command {
        SourceCommand::Id { url } => {
            let url = SourceUrl::parse(&url).map_err(CliError::ParseSourceUrl)?;
            let source = SourceGenesis::new(url.clone()).object_id();
            Ok(format!("url = {url}\nsource = {source}\n"))
        }
        SourceCommand::Fetch {
            url,
            actor,
            object_format,
        } => run_fetch(paths, &url, &actor, object_format.as_deref()),
        SourceCommand::Show { source, json } => run_show(paths, &source, json),
    }
}

fn git_error(source: kairo_git::GitError) -> CliError {
    CliError::GitOperation { source }
}

fn run_fetch(
    paths: &StorePaths,
    url: &str,
    actor: &str,
    flag: Option<&str>,
) -> Result<String, CliError> {
    let url = SourceUrl::parse(url).map_err(CliError::ParseSourceUrl)?;
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
    let genesis = SourceGenesis::new(url.clone());
    let source = store
        .put_source(&genesis)
        .map_err(|source| CliError::Sources { source })?;

    // The format: the flag, the source's cache repo, and the remote
    // must agree where present (§19 sub-decision 7).
    let cache = GitCache::open(paths.git_root()).map_err(git_error)?;
    let flag = flag
        .map(|raw| {
            raw.parse::<GitObjectFormat>()
                .map_err(|source| CliError::InvalidGitRevision {
                    revision: format!("--object-format {raw}"),
                    source,
                })
        })
        .transpose()?;
    let repo = cache.repo_format(source.as_str()).map_err(git_error)?;
    let remote = GitCache::source_format(url.as_str()).map_err(git_error)?;
    let mut chosen: Option<(&str, GitObjectFormat)> = None;
    for (what, format) in [
        ("--object-format", flag),
        ("the cache repo", repo),
        ("the remote", remote),
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
    // fetch; the observation records that it was empty.
    let refs = match chosen.filter(|_| remote.is_some()) {
        None => Vec::new(),
        Some((_, format)) => {
            cache
                .fetch_source(
                    source.as_str(),
                    url.as_str(),
                    format,
                    &FetchLimits {
                        max_bytes: limits.max_git_bytes,
                        timeout: limits.git_timeout,
                    },
                )
                .map_err(git_error)?
                .refs
        }
    };

    let observed = refs
        .iter()
        .map(|(name, target)| ObservedRef::new(name.clone(), target.clone()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| CliError::ObservationShape(error.to_string()))?;
    let previous = latest_refs(&store, &source)?;
    let body = SourceObservationBody::new(url.clone(), observed)
        .map_err(|error| CliError::ObservationShape(error.to_string()))?;
    let subject: KairoRef =
        format!("object:{source}")
            .parse()
            .map_err(|error| CliError::BuildSubjectRef {
                object: source.clone(),
                source: error,
            })?;
    let unsigned = store
        .link_to_chain(UnsignedStatement::new(
            actor_id.clone(),
            subject,
            Timestamp::now(),
            body,
        ))
        .map_err(|error| CliError::ReadActorChain {
            actor: actor_id.clone(),
            source: error,
        })?;
    let signature_bytes = secret.sign(&unsigned.canonical_bytes());
    let signature = Signature::new(
        actor_id.clone(),
        secret.public_key().key_id().to_string(),
        "ed25519",
        signature_bytes.bytes().to_vec(),
    );
    let signed = SignedStatement::new(unsigned, signature);
    let observation = store
        .put_source_observation(&signed)
        .map_err(|source| CliError::Sources { source })?;

    let mut out = String::new();
    let _ = writeln!(out, "url = {url}");
    let _ = writeln!(out, "source = {source}");
    let _ = writeln!(out, "observation = {observation}");
    let _ = writeln!(
        out,
        "object_format = {}",
        signed
            .unsigned()
            .body()
            .format()
            .map_or("none (no refs)".to_owned(), |format| format.to_string())
    );
    let _ = writeln!(out, "refs = {}", signed.unsigned().body().refs().len());
    // Against the newest earlier observation: what changed upstream.
    let current = signed.unsigned().body().refs();
    let mut changes = Vec::new();
    for observed in current {
        match previous.iter().find(|(name, _)| name == observed.name()) {
            None if !previous.is_empty() => {
                changes.push(format!("  + {} {}", observed.name(), observed.target()));
            }
            Some((_, old)) if old != &observed.target().to_string() => changes.push(format!(
                "  ~ {} {old} -> {}",
                observed.name(),
                observed.target()
            )),
            _ => {}
        }
    }
    for (name, old) in &previous {
        if !current.iter().any(|observed| observed.name() == name) {
            changes.push(format!("  - {name} {old} (still held)"));
        }
    }
    if previous.is_empty() {
        let _ = writeln!(out, "first_observation = yes");
    } else {
        let _ = writeln!(out, "changes = {}", changes.len());
        for change in changes {
            let _ = writeln!(out, "{change}");
        }
    }
    Ok(out)
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

fn run_show(paths: &StorePaths, source: &str, json: bool) -> Result<String, CliError> {
    let source_id = match source.parse::<KairoRef>() {
        Ok(KairoRef::Object(object)) => object,
        _ => SourceGenesis::new(SourceUrl::parse(source).map_err(CliError::ParseSourceUrl)?)
            .object_id(),
    };
    let store = open_store(paths)?;
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
        let refs = statement.unsigned().body().refs();
        let _ = writeln!(out, "refs = {} (newest observation)", refs.len());
        for observed in refs {
            let _ = writeln!(out, "  {} {}", observed.name(), observed.target());
        }
    }
    Ok(out)
}
