//! `kairo federation ...` and `kairo fetch` runners
//! (`specs/DECISIONS.md` §16, `specs/PHASE_2_FEDERATION.md`
//! slices 1 and 3): this node's publish list, provenance lookups, and
//! pulling an object from a peer.
//!
//! All of it is local store state, so these commands always run
//! direct; there is no daemon route for them.

use kairo_core::{KairoRef, ObjectId, Timestamp};
use kairo_federation_http::client::{pull_object, GitOutcome, Limits, PullReport};
use kairo_store::{ProvenanceStore, PublishedObject, PublishedObjectStore};

use crate::cli::FederationCommand;
use crate::error::CliError;
use crate::store_paths::{open_store, StorePaths};

/// Printed whenever an object stops being served, so nobody reads
/// `unpublish` as a recall.
const UNPUBLISH_NOTE: &str = "peers that already fetched this object keep their copy";

pub(crate) fn run_federation_command(
    command: FederationCommand,
    paths: &StorePaths,
) -> Result<String, CliError> {
    match command {
        FederationCommand::Publish { object } => run_publish(&parse_object(object)?, paths),
        FederationCommand::Unpublish { object } => run_unpublish(&parse_object(object)?, paths),
        FederationCommand::Published => run_published(paths),
        FederationCommand::Provenance { record } => run_provenance(record, paths),
    }
}

fn parse_object(object: String) -> Result<ObjectId, CliError> {
    ObjectId::new(object.clone()).map_err(|source| CliError::ParseObjectId { object, source })
}

fn run_publish(object: &ObjectId, paths: &StorePaths) -> Result<String, CliError> {
    let store = open_store(paths)?;
    let added = store
        .publish_object(object, Timestamp::now())
        .map_err(|source| CliError::PublishObject {
            object: object.clone(),
            source,
        })?;
    let status = if added {
        "published"
    } else {
        "already_published"
    };
    Ok(format!("object = {object}\nstatus = {status}\n"))
}

fn run_unpublish(object: &ObjectId, paths: &StorePaths) -> Result<String, CliError> {
    let store = open_store(paths)?;
    let removed = store
        .unpublish_object(object)
        .map_err(|source| CliError::UnpublishObject {
            object: object.clone(),
            source,
        })?;
    if removed {
        Ok(format!(
            "object = {object}\nstatus = unpublished\nnote = {UNPUBLISH_NOTE}\n"
        ))
    } else {
        Ok(format!("object = {object}\nstatus = not_published\n"))
    }
}

fn run_published(paths: &StorePaths) -> Result<String, CliError> {
    let store = open_store(paths)?;
    let objects = store
        .list_published_objects()
        .map_err(|source| CliError::ListPublishedObjects { source })?;
    Ok(format_published(&objects))
}

fn format_published(objects: &[PublishedObject]) -> String {
    let mut output = format!("published = {}\n", objects.len());
    for entry in objects {
        output.push_str(&format!(
            "  object={} published_at={}\n",
            entry.object_id, entry.published_at
        ));
    }
    output
}

/// `kairo fetch --object <id> --from <url>`: pull one published
/// object's closure (`specs/PHASE_2_FEDERATION.md` slice 3). Always
/// direct: the pull writes the local store, like `bundle import`.
pub(crate) fn run_fetch(
    object: &str,
    from: &str,
    json: bool,
    paths: &StorePaths,
) -> Result<String, CliError> {
    let object = parse_object(object.to_owned())?;
    let store = open_store(paths)?;
    let report = pull_object(&store, from, &object, &Limits::default())
        .map_err(|error| CliError::Fetch(Box::new(error)))?;
    Ok(if json {
        format_fetch_json(&report)
    } else {
        format_fetch(&report)
    })
}

fn format_fetch(report: &PullReport) -> String {
    let mut output = format!(
        "object = {}\npeer = {}\npeer_actor = {} (claimed)\nfetched = {}\nalready_held = {}\nskipped = {}\n",
        report.object,
        report.peer_url,
        report.peer_actor,
        report.fetched,
        report.already_held,
        report.skipped.len(),
    );
    for skipped in &report.skipped {
        output.push_str(&format!(
            "  record={} reason={}\n",
            skipped.record, skipped.reason
        ));
    }
    output.push_str(&format!("git = {}\n", describe_git(&report.git)));
    output
}

fn describe_git(git: &GitOutcome) -> String {
    match git {
        GitOutcome::NoGitRevisions => "no git revisions".to_owned(),
        GitOutcome::AlreadyHeld { commits } => format!("already held ({commits} commits)"),
        GitOutcome::Ingested { commits } => format!("ingested ({commits} commits)"),
        GitOutcome::Unavailable { missing } => format!(
            "unavailable from peer ({missing} commits missing; content verification stays \
             indeterminate)"
        ),
    }
}

fn git_json(git: &GitOutcome) -> serde_json::Value {
    match git {
        GitOutcome::NoGitRevisions => serde_json::json!({ "status": "no_git_revisions" }),
        GitOutcome::AlreadyHeld { commits } => {
            serde_json::json!({ "status": "already_held", "commits": commits })
        }
        GitOutcome::Ingested { commits } => {
            serde_json::json!({ "status": "ingested", "commits": commits })
        }
        GitOutcome::Unavailable { missing } => {
            serde_json::json!({ "status": "unavailable", "missing": missing })
        }
    }
}

fn format_fetch_json(report: &PullReport) -> String {
    let value = serde_json::json!({
        "object": report.object.to_string(),
        "peer_url": report.peer_url,
        "peer_actor": report.peer_actor.to_string(),
        "fetched": report.fetched,
        "already_held": report.already_held,
        "skipped": report
            .skipped
            .iter()
            .map(|s| serde_json::json!({ "record": s.record.to_string(), "reason": s.reason }))
            .collect::<Vec<_>>(),
        "git": git_json(&report.git),
    });
    let mut output = serde_json::to_string_pretty(&value).unwrap_or_default();
    output.push('\n');
    output
}

fn run_provenance(record: String, paths: &StorePaths) -> Result<String, CliError> {
    let parsed: KairoRef = record.parse().map_err(|source| CliError::ParseRecordRef {
        record: record.clone(),
        source,
    })?;
    let store = open_store(paths)?;
    let entries = store
        .get_provenance(&parsed)
        .map_err(|source| CliError::ReadProvenance {
            record: record.clone(),
            source,
        })?;
    let mut output = format!("record = {parsed}\npeers = {}\n", entries.len());
    if entries.is_empty() {
        output.push_str("note = not fetched from a peer; authored or imported locally\n");
    }
    for entry in &entries {
        output.push_str(&format!(
            "  peer_url={} peer_actor={} fetched_at={}\n",
            entry.peer_url, entry.peer_actor, entry.fetched_at
        ));
    }
    Ok(output)
}
