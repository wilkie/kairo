//! `kairo federation ...` and `kairo fetch` runners
//! (`specs/DECISIONS.md` §16, `specs/PHASE_2_FEDERATION.md`
//! slices 1 and 3): this node's publish list, provenance lookups, and
//! pulling an object from a peer.
//!
//! All of it is local store state, so these commands always run
//! direct; there is no daemon route for them.

use std::time::Duration;

use kairo_config::ConfigSource;
use kairo_core::{KairoRef, ObjectId, Timestamp};
use kairo_federation_http::client::{
    normalize_peer_url, pull_object, GitOutcome, Limits, PullOptions, PullReport,
};
use kairo_store::{
    ProvenanceStore, PublishedObject, PublishedObjectStore, PullLogStore, PullRecord,
};

use crate::cli::FederationCommand;
use crate::commands::config::load_config;
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
        FederationCommand::Status => run_status(paths),
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

pub(crate) struct FetchArgs<'a> {
    pub(crate) object: &'a str,
    pub(crate) from: &'a str,
    pub(crate) allow_peer: bool,
    pub(crate) require_complete: bool,
    pub(crate) json: bool,
    /// The global `--offline` flag.
    pub(crate) offline: bool,
}

/// `kairo fetch --object <id> --from <url>`: pull one published
/// object's closure (`specs/PHASE_2_FEDERATION.md` slices 3 and 5).
/// Always direct: the pull writes the local store, like `bundle
/// import`.
///
/// Policy comes from `<store>/config.toml` and is checked before any
/// network I/O: federation must be enabled, `--offline` refuses, and
/// the peer must be listed unless `--allow-peer`. A listed peer's
/// `node_actor` pin is passed to the pull, which refuses a different
/// claim before fetching anything.
pub(crate) fn run_fetch(args: &FetchArgs<'_>, paths: &StorePaths) -> Result<String, CliError> {
    let object = parse_object(args.object.to_owned())?;
    if args.offline {
        return Err(CliError::OfflineFetch);
    }
    let (config, source) = load_config(paths)?;
    if !config.federation.enabled {
        return Err(CliError::FederationDisabled {
            config: source.path().to_path_buf(),
        });
    }
    let url = normalize_peer_url(args.from).map_err(|error| CliError::Fetch(Box::new(error)))?;
    // Compare config entries the way the pull normalizes `--from`.
    let peer = config
        .federation
        .peers
        .iter()
        .find(|peer| normalize_peer_url(&peer.url).is_ok_and(|normalized| normalized == url));
    if peer.is_none() && !args.allow_peer {
        return Err(CliError::PeerNotAllowed {
            url,
            config: source.path().to_path_buf(),
        });
    }
    let pinned = peer.and_then(|peer| peer.node_actor.clone());
    let options = PullOptions {
        limits: limits_from(&config.federation.limits),
        expected_node_actor: pinned.clone(),
        require_complete: args.require_complete,
    };

    let store = open_store(paths)?;
    let report = pull_object(&store, &url, &object, &options)
        .map_err(|error| CliError::Fetch(Box::new(error)))?;
    store
        .record_pull(&PullRecord {
            peer_url: report.peer_url.clone(),
            object: report.object.clone(),
            peer_actor: report.peer_actor.clone(),
            at: Timestamp::now(),
            fetched: report.fetched as u64,
            already_held: report.already_held as u64,
        })
        .map_err(|source| CliError::RecordPull { source })?;

    let pin = Pin {
        listed: peer.is_some(),
        pinned: pinned.is_some(),
    };
    Ok(if args.json {
        format_fetch_json(&report, pin)
    } else {
        format_fetch(&report, pin)
    })
}

/// How the peer stood in the config for this pull.
#[derive(Debug, Clone, Copy)]
struct Pin {
    listed: bool,
    pinned: bool,
}

impl Pin {
    fn describe(self, report: &PullReport) -> String {
        match (self.listed, self.pinned) {
            // The node document is unsigned until node authentication
            // lands (DECISIONS.md §16 sub-decision 9), so a matching pin
            // catches a re-pointed URL, not an impersonator.
            (_, true) => "pinned; claim matches, node not authenticated".to_owned(),
            (true, false) => format!(
                "claimed, unpinned; add node_actor = \"{}\" to its [[federation.peers]] entry \
                 to pin it",
                report.peer_actor
            ),
            (false, false) => "claimed; peer not in config (--allow-peer)".to_owned(),
        }
    }

    fn json(self) -> &'static str {
        match (self.listed, self.pinned) {
            (_, true) => "pinned",
            (true, false) => "unpinned",
            (false, false) => "unlisted",
        }
    }
}

/// Config overrides on top of the client's defaults.
fn limits_from(config: &kairo_config::LimitsConfig) -> Limits {
    let defaults = Limits::default();
    let secs = |value: Option<u64>, default: Duration| value.map_or(default, Duration::from_secs);
    Limits {
        max_json_bytes: config.max_json_bytes.unwrap_or(defaults.max_json_bytes),
        max_blob_bytes: config.max_blob_bytes.unwrap_or(defaults.max_blob_bytes),
        max_git_bytes: config.max_git_bytes.unwrap_or(defaults.max_git_bytes),
        max_records: config.max_records.map_or(defaults.max_records, |n| {
            usize::try_from(n).unwrap_or(usize::MAX)
        }),
        request_timeout: secs(config.request_timeout_secs, defaults.request_timeout),
        git_timeout: secs(config.git_timeout_secs, defaults.git_timeout),
    }
}

/// `kairo federation status`.
fn run_status(paths: &StorePaths) -> Result<String, CliError> {
    let (config, source) = load_config(paths)?;
    let store = open_store(paths)?;
    let published = store
        .list_published_objects()
        .map_err(|source| CliError::ListPublishedObjects { source })?;
    let pulls = store
        .list_pulls()
        .map_err(|source| CliError::ListPulls { source })?;

    let mut output = format!(
        "federation = {}
",
        if config.federation.enabled {
            "enabled"
        } else {
            "disabled"
        }
    );
    output.push_str(&match &source {
        ConfigSource::File(path) => format!(
            "config = {}
",
            path.display()
        ),
        ConfigSource::Defaults(path) => {
            format!(
                "config = {} (absent; defaults apply)
",
                path.display()
            )
        }
    });
    output.push_str(&format!(
        "published = {}
",
        published.len()
    ));
    for entry in &published {
        output.push_str(&format!(
            "  object={} published_at={}
",
            entry.object_id, entry.published_at
        ));
    }

    let last_pull = |url: &str| pulls.iter().find(|pull| pull.peer_url == url);
    let describe_pull = |pull: Option<&PullRecord>| match pull {
        Some(pull) => format!(
            "{} object={} fetched={} already_held={} claimed={}",
            pull.at, pull.object, pull.fetched, pull.already_held, pull.peer_actor
        ),
        None => "never".to_owned(),
    };
    output.push_str(&format!(
        "peers = {}
",
        config.federation.peers.len()
    ));
    for peer in &config.federation.peers {
        let pin = peer
            .node_actor
            .as_ref()
            .map_or_else(|| "unpinned".to_owned(), ToString::to_string);
        // Compare with the normalized URL the pull log uses.
        let key = normalize_peer_url(&peer.url).unwrap_or_else(|_| peer.url.clone());
        output.push_str(&format!(
            "  url={} node_actor={pin} last_pull={}
",
            peer.url,
            describe_pull(last_pull(&key))
        ));
    }
    let unlisted: Vec<&PullRecord> = pulls
        .iter()
        .filter(|pull| {
            !config
                .federation
                .peers
                .iter()
                .any(|peer| normalize_peer_url(&peer.url).is_ok_and(|url| url == pull.peer_url))
        })
        .collect();
    if !unlisted.is_empty() {
        output.push_str(&format!(
            "unlisted_peers = {}
",
            unlisted.len()
        ));
        for pull in unlisted {
            output.push_str(&format!(
                "  url={} last_pull={}
",
                pull.peer_url,
                describe_pull(Some(pull))
            ));
        }
    }
    Ok(output)
}

fn format_fetch(report: &PullReport, pin: Pin) -> String {
    let mut output = format!(
        "object = {}\npeer = {}\npeer_actor = {} ({})\nfetched = {}\nalready_held = {}\nflagged = {}\n",
        report.object,
        report.peer_url,
        report.peer_actor,
        pin.describe(report),
        report.fetched,
        report.already_held,
        report.flagged.len(),
    );
    for flagged in &report.flagged {
        output.push_str(&format!(
            "  record={} reason={}\n",
            flagged.record, flagged.reason
        ));
    }
    output.push_str(&format!("git = {}\n", describe_git(&report.git)));
    output.push_str(&format!(
        "incomplete_chains = {}\n",
        report.incomplete.len()
    ));
    for chain in &report.incomplete {
        let missing: Vec<String> = chain.missing.iter().map(ToString::to_string).collect();
        output.push_str(&format!(
            "  signer={} scope={} missing={}\n",
            chain.signer,
            chain.scope,
            missing.join(",")
        ));
    }
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

fn format_fetch_json(report: &PullReport, pin: Pin) -> String {
    let value = serde_json::json!({
        "object": report.object.to_string(),
        "peer_url": report.peer_url,
        "peer_actor": report.peer_actor.to_string(),
        "peer_pin": pin.json(),
        "fetched": report.fetched,
        "already_held": report.already_held,
        "flagged": report
            .flagged
            .iter()
            .map(|s| serde_json::json!({ "record": s.record.to_string(), "reason": s.reason }))
            .collect::<Vec<_>>(),
        "git": git_json(&report.git),
        "incomplete_chains": report
            .incomplete
            .iter()
            .map(|chain| serde_json::json!({
                "signer": chain.signer.to_string(),
                "scope": chain.scope.key(),
                "missing": chain.missing.iter().map(ToString::to_string).collect::<Vec<_>>(),
            }))
            .collect::<Vec<_>>(),
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
