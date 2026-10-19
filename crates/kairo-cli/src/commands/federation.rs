//! `kairo federation ...` and `kairo fetch` runners
//! (`specs/DECISIONS.md` §16, §25, §26, `specs/PHASE_2_FEDERATION.md`
//! slices 1 and 3): this node's publish list, provenance lookups,
//! pulling an object from a peer, learning a peer's node actor, and
//! announcing and receiving pushes.
//!
//! All of it is local store state, so these commands always run
//! direct; there is no daemon route for them.

use kairo_config::{Config, ConfigSource};
use kairo_core::{ActorId, KairoRef, ObjectId, Timestamp};
use kairo_federation_core::Announcement;
use kairo_federation_http::client::{
    announce, export_actor, fetch_trust, learn_from_export, learn_peer, normalize_peer_url,
    pull_object, push, GitOutcome, NodeSigner, PullOptions, PullReport,
};
use kairo_federation_service::{limits_from, DiscoverOutcome, ReceiveOutcome, Received, Via};
use kairo_identity::ActorKind;
use kairo_keystore::Keystore;
use kairo_statement::TrustScope;
use kairo_store::{
    ActorStore, Follow, FollowStore, PeerOutcome, ProvenanceStore, PublishedObject,
    PublishedObjectStore, PullLogStore, PullRecord, TrustTransportStore,
};

use crate::cli::FederationCommand;
use crate::commands::config::load_config;
use crate::error::CliError;
use crate::store_paths::{open_keystore, open_store, StorePaths};

/// Printed whenever an object stops being served, so nobody reads
/// `unpublish` as a recall.
const UNPUBLISH_NOTE: &str = "peers that already fetched this object keep their copy";

pub(crate) fn run_federation_command(
    command: FederationCommand,
    paths: &StorePaths,
    offline: bool,
) -> Result<String, CliError> {
    match command {
        FederationCommand::Learn {
            from: Some(from),
            allow_peer,
            ..
        } => run_learn(&from, allow_peer, offline, paths),
        FederationCommand::Learn {
            file: Some(file),
            node_actor,
            ..
        } => run_learn_file(&file, node_actor.as_deref(), paths),
        FederationCommand::Learn { .. } => Err(CliError::NodeActorChoice {
            message: "federation learn needs --from <url> or --file <path>".to_owned(),
        }),
        FederationCommand::Announce {
            objects,
            to,
            base_url,
            node,
            allow_peer,
        } => run_announce(
            &AnnounceArgs {
                objects,
                to: &to,
                base_url,
                node: node.as_deref(),
                allow_peer,
                offline,
            },
            paths,
        ),
        FederationCommand::Receive => run_receive(offline, paths),
        FederationCommand::Discover => run_discover(offline, paths),
        FederationCommand::Holders { object } => run_holders(&parse_object(object)?, paths),
        FederationCommand::Follow { target } => run_follow(
            &follow_of(target.object, target.trust_by, target.scope)?,
            true,
            paths,
        ),
        FederationCommand::Unfollow { target } => run_follow(
            &follow_of(target.object, target.trust_by, target.scope)?,
            false,
            paths,
        ),
        FederationCommand::Sync { target } => {
            let only = match (&target.object, &target.trust_by) {
                (None, None) => None,
                _ => Some(follow_of(target.object, target.trust_by, target.scope)?),
            };
            run_sync(only.as_ref(), offline, paths)
        }
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
    let (config, url, peer) = peer_policy(args.from, args.allow_peer, args.offline, paths)
        .map_err(|error| error.into_cli(CliError::Fetch))?;
    let pinned = peer.and_then(|peer| peer.node_actor);
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
        listed: peer_listed(&config, &url),
        pinned: pinned.is_some(),
    };
    Ok(if args.json {
        format_fetch_json(&report, pin)
    } else {
        format_fetch(&report, pin)
    })
}

/// A policy refusal, or a peer URL that doesn't normalize (which each
/// command wraps in its own error).
enum PolicyError {
    Cli(CliError),
    Url(kairo_federation_http::client::PullError),
}

impl PolicyError {
    fn into_cli(
        self,
        wrap: impl FnOnce(Box<kairo_federation_http::client::PullError>) -> CliError,
    ) -> CliError {
        match self {
            Self::Cli(error) => error,
            Self::Url(error) => wrap(Box::new(error)),
        }
    }
}

/// Federation policy before any network I/O (`DECISIONS.md` §17
/// sub-decision 4): `--offline` refuses, federation must be enabled,
/// and the peer must be listed unless `allow_peer`. Returns the config,
/// the normalized URL, and the peer's entry.
fn peer_policy(
    raw_url: &str,
    allow_peer: bool,
    offline: bool,
    paths: &StorePaths,
) -> Result<(Config, String, Option<kairo_config::PeerConfig>), PolicyError> {
    if offline {
        return Err(PolicyError::Cli(CliError::OfflineFetch));
    }
    let (config, source) = load_config(paths).map_err(PolicyError::Cli)?;
    if !config.federation.enabled {
        return Err(PolicyError::Cli(CliError::FederationDisabled {
            config: source.path().to_path_buf(),
        }));
    }
    let url = normalize_peer_url(raw_url).map_err(PolicyError::Url)?;
    let peer = peer_entry(&config, &url);
    if peer.is_none() && !allow_peer {
        return Err(PolicyError::Cli(CliError::PeerNotAllowed {
            url,
            config: source.path().to_path_buf(),
        }));
    }
    Ok((config, url, peer))
}

/// The config entry for a normalized URL, compared the way the pull
/// normalizes `--from`.
fn peer_entry(config: &Config, url: &str) -> Option<kairo_config::PeerConfig> {
    config
        .federation
        .peers
        .iter()
        .find(|peer| normalize_peer_url(&peer.url).is_ok_and(|normalized| normalized == url))
        .cloned()
}

fn peer_listed(config: &Config, url: &str) -> bool {
    peer_entry(config, url).is_some()
}

/// `kairo federation learn --from <url>`: authenticate the peer and
/// store its node actor's key state (`DECISIONS.md` §25).
fn run_learn(
    from: &str,
    allow_peer: bool,
    offline: bool,
    paths: &StorePaths,
) -> Result<String, CliError> {
    let wrap = |source| CliError::PeerRequest {
        action: "learning the peer",
        source,
    };
    let (config, url, peer) =
        peer_policy(from, allow_peer, offline, paths).map_err(|error| error.into_cli(wrap))?;
    let pinned = peer.and_then(|peer| peer.node_actor);
    let options = PullOptions {
        limits: limits_from(&config.federation.limits),
        expected_node_actor: pinned.clone(),
        require_complete: false,
    };
    let store = open_store(paths)?;
    let learned = learn_peer(&store, &url, &options).map_err(|error| wrap(Box::new(error)))?;
    let pin = match (peer_listed(&config, &url), pinned.is_some()) {
        (_, true) => "pinned, authenticated".to_owned(),
        (true, false) => format!(
            "authenticated, unpinned; add node_actor = \"{}\" to its [[federation.peers]] \
             entry to pin it",
            learned.node_actor
        ),
        (false, false) => "authenticated; peer not in config (--allow-peer)".to_owned(),
    };
    Ok(format!(
        "peer = {}\nnode_actor = {} ({pin})\nfetched = {}\nalready_held = {}\n",
        learned.peer_url, learned.node_actor, learned.fetched, learned.already_held
    ))
}

/// `kairo federation learn --file`: learn a node from its actor export
/// (§26 sub-decision 7). No network, so `--offline` doesn't refuse it.
fn run_learn_file(
    file: &std::path::Path,
    expected: Option<&str>,
    paths: &StorePaths,
) -> Result<String, CliError> {
    let wrap = |source| CliError::PeerRequest {
        action: "learning the node",
        source,
    };
    let bad_file = |message: String| CliError::ActorExportFile {
        path: file.to_path_buf(),
        message,
    };
    let bytes = std::fs::read(file).map_err(|error| bad_file(error.to_string()))?;
    let export: kairo_federation_core::ActorExport =
        serde_json::from_slice(&bytes).map_err(|error| bad_file(error.to_string()))?;
    let expected_node_actor = expected
        .map(|raw| {
            ActorId::new(raw.to_owned()).map_err(|source| CliError::ParseActorId {
                actor: raw.to_owned(),
                source,
            })
        })
        .transpose()?;
    let (config, _) = load_config(paths)?;
    let options = PullOptions {
        limits: limits_from(&config.federation.limits),
        expected_node_actor,
        require_complete: false,
    };
    let store = open_store(paths)?;
    let learned = learn_from_export(&store, export, format!("file:{}", file.display()), &options)
        .map_err(|error| wrap(Box::new(error)))?;
    let pin = if expected.is_some() {
        "pinned"
    } else {
        "unpinned; check this id out of band before trusting it"
    };
    Ok(format!(
        "file = {}\nnode_actor = {} ({pin})\nfetched = {}\nalready_held = {}\n",
        file.display(),
        learned.node_actor,
        learned.fetched,
        learned.already_held
    ))
}

/// `kairo actor export`: an actor's genesis and key events in one file.
pub(crate) fn run_actor_export(
    actor: &str,
    out: &std::path::Path,
    paths: &StorePaths,
) -> Result<String, CliError> {
    let actor = ActorId::new(actor.to_owned()).map_err(|source| CliError::ParseActorId {
        actor: actor.to_owned(),
        source,
    })?;
    let store = open_store(paths)?;
    let export = export_actor(&store, &actor).map_err(|source| CliError::PeerRequest {
        action: "exporting the actor",
        source: Box::new(source),
    })?;
    let bad_file = |message: String| CliError::ActorExportFile {
        path: out.to_path_buf(),
        message,
    };
    let mut bytes =
        serde_json::to_vec_pretty(&export).map_err(|error| bad_file(error.to_string()))?;
    bytes.push(b'\n');
    std::fs::write(out, bytes).map_err(|error| bad_file(error.to_string()))?;
    Ok(format!(
        "actor = {actor}\nkey_events = {}\nout = {}\n",
        export.key_events.len(),
        out.display()
    ))
}

fn parse_actor(raw: &str) -> Result<ActorId, CliError> {
    ActorId::new(raw.to_owned()).map_err(|source| CliError::ParseActorId {
        actor: raw.to_owned(),
        source,
    })
}

/// `kairo trust publish` / `unpublish` (`DECISIONS.md` §27
/// sub-decision 1).
pub(crate) fn run_trust_publish(
    by: &str,
    scope: &TrustScope,
    publish: bool,
    paths: &StorePaths,
) -> Result<String, CliError> {
    let by = parse_actor(by)?;
    let store = open_store(paths)?;
    let status = if publish {
        let added = store
            .publish_trust(&by, scope, Timestamp::now())
            .map_err(CliError::ReadActorTrust)?;
        let chain = store
            .trust_chain_ids(&by, scope)
            .map_err(CliError::ReadActorTrust)?
            .len();
        let status = if added {
            "published"
        } else {
            "already_published"
        };
        format!("status = {status}\nopinions = {chain}\n")
    } else if store
        .unpublish_trust(&by, scope)
        .map_err(CliError::ReadActorTrust)?
    {
        format!("status = unpublished\nnote = {UNPUBLISH_NOTE}\n")
    } else {
        "status = not_published\n".to_owned()
    };
    Ok(format!("by_actor = {by}\nscope = {scope}\n{status}"))
}

/// `kairo trust fetch` (§27 sub-decision 3).
pub(crate) fn run_trust_fetch(
    by: &str,
    scope: &TrustScope,
    from: &str,
    allow_peer: bool,
    offline: bool,
    paths: &StorePaths,
) -> Result<String, CliError> {
    let wrap = |source| CliError::PeerRequest {
        action: "fetching trust",
        source,
    };
    let by = parse_actor(by)?;
    let (config, url, peer) =
        peer_policy(from, allow_peer, offline, paths).map_err(|error| error.into_cli(wrap))?;
    let options = PullOptions {
        limits: limits_from(&config.federation.limits),
        expected_node_actor: peer.and_then(|peer| peer.node_actor),
        require_complete: false,
    };
    let store = open_store(paths)?;
    let report =
        fetch_trust(&store, &url, &by, scope, &options).map_err(|error| wrap(Box::new(error)))?;
    let mut output = format!(
        "by_actor = {by}\nscope = {scope}\npeer = {}\npeer_actor = {}\nopinions = {}\n\
         fetched = {}\nalready_held = {}\nflagged = {}\n",
        report.peer_url,
        report.peer_actor,
        report.chain,
        report.fetched,
        report.already_held,
        report.flagged.len()
    );
    for flagged in &report.flagged {
        output.push_str(&format!(
            "  record={} reason={}\n",
            flagged.record, flagged.reason
        ));
    }
    output.push_str(
        "note = imported opinions stay this truster's; nothing this node trusts changed\n",
    );
    Ok(output)
}

/// `kairo trust forget` (§27 sub-decision 5).
pub(crate) fn run_trust_forget(
    by: &str,
    scope: Option<&TrustScope>,
    paths: &StorePaths,
) -> Result<String, CliError> {
    let by = parse_actor(by)?;
    let keystore = open_keystore(paths)?;
    if keystore.has_signing_key(&by).unwrap_or(false) {
        return Err(CliError::NodeActorChoice {
            message: format!(
                "{by} has a key in this node's keystore, so its opinions are this node's own; \
                 withdraw them with `kairo trust withdraw` instead"
            ),
        });
    }
    let store = open_store(paths)?;
    let forgotten = store
        .forget_trust(&by, scope)
        .map_err(CliError::ReadActorTrust)?;
    // Background sync must not bring it back (§28 sub-decision 5).
    let unfollowed = store
        .remove_trust_follows(&by, scope)
        .map_err(|source| CliError::Follows { source })?;
    let scope = scope.map_or_else(|| "all".to_owned(), ToString::to_string);
    Ok(format!(
        "by_actor = {by}\nscope = {scope}\nforgotten = {forgotten}\nunfollowed = {unfollowed}\n"
    ))
}

struct AnnounceArgs<'a> {
    objects: Vec<String>,
    to: &'a str,
    base_url: String,
    node: Option<&'a str>,
    allow_peer: bool,
    offline: bool,
}

/// `kairo federation announce`: tell a peer to pull objects this node
/// publishes (`DECISIONS.md` §26 sub-decision 5).
fn run_announce(args: &AnnounceArgs<'_>, paths: &StorePaths) -> Result<String, CliError> {
    let wrap = |source| CliError::PeerRequest {
        action: "announcing",
        source,
    };
    let (config, url, peer) = peer_policy(args.to, args.allow_peer, args.offline, paths)
        .map_err(|error| error.into_cli(wrap))?;
    let objects = args
        .objects
        .iter()
        .map(|object| parse_object(object.clone()))
        .collect::<Result<Vec<_>, _>>()?;
    let store = open_store(paths)?;
    let published = store
        .list_published_objects()
        .map_err(|source| CliError::ListPublishedObjects { source })?;
    for object in &objects {
        if !published.iter().any(|entry| &entry.object_id == object) {
            return Err(CliError::NotPublished {
                object: object.clone(),
            });
        }
    }
    let keystore = open_keystore(paths)?;
    let node = choose_node_actor(&store, &keystore, args.node)?;
    let key = keystore
        .get_signing_key(&node)
        .map_err(|source| CliError::ReadKey {
            actor: node.clone(),
            source,
        })?;

    let options = PullOptions {
        limits: limits_from(&config.federation.limits),
        expected_node_actor: peer.and_then(|peer| peer.node_actor),
        require_complete: false,
    };
    let announcement = Announcement::new(args.base_url.clone(), objects);
    let receipt = announce(
        &store,
        &url,
        &announcement,
        NodeSigner {
            node: &node,
            key: &key,
        },
        &options,
    )
    .map_err(|error| wrap(Box::new(error)))?;
    Ok(format!(
        "peer = {}\npeer_actor = {}\nnode = {node}\nobjects = {}\nentry = {}\n",
        receipt.peer_url,
        receipt.peer_actor,
        announcement.objects.len(),
        receipt.entry
    ))
}

pub(crate) struct PushArgs<'a> {
    pub(crate) object: &'a str,
    pub(crate) to: &'a str,
    pub(crate) node: Option<&'a str>,
    pub(crate) allow_peer: bool,
    pub(crate) offline: bool,
}

/// `kairo push`: plan with the peer, then upload a closure bundle of
/// what it lacks (`DECISIONS.md` §26 sub-decisions 5 and 6).
pub(crate) fn run_push(args: &PushArgs<'_>, paths: &StorePaths) -> Result<String, CliError> {
    let wrap = |source| CliError::PeerRequest {
        action: "pushing",
        source,
    };
    let (config, url, peer) = peer_policy(args.to, args.allow_peer, args.offline, paths)
        .map_err(|error| error.into_cli(wrap))?;
    let object = parse_object(args.object.to_owned())?;
    let store = open_store(paths)?;
    let published = store
        .list_published_objects()
        .map_err(|source| CliError::ListPublishedObjects { source })?;
    if !published.iter().any(|entry| entry.object_id == object) {
        return Err(CliError::NotPublished { object });
    }
    let keystore = open_keystore(paths)?;
    let node = choose_node_actor(&store, &keystore, args.node)?;
    let key = keystore
        .get_signing_key(&node)
        .map_err(|source| CliError::ReadKey {
            actor: node.clone(),
            source,
        })?;
    let options = PullOptions {
        limits: limits_from(&config.federation.limits),
        expected_node_actor: peer.and_then(|peer| peer.node_actor),
        require_complete: false,
    };
    let receipt = push(
        &store,
        &url,
        &object,
        NodeSigner {
            node: &node,
            key: &key,
        },
        &options,
    )
    .map_err(|error| wrap(Box::new(error)))?;
    let sent = &receipt.sent;
    Ok(format!(
        "object = {object}\npeer = {}\npeer_actor = {}\nnode = {node}\ninventory = {}\n\
         sent = genesis={} actors={} statements={} blobs={} commits={} bytes={}\nentry = {}\n",
        receipt.peer_url,
        receipt.peer_actor,
        receipt.inventory,
        sent.genesis,
        sent.actors,
        sent.statements,
        sent.blobs,
        sent.commits,
        sent.bytes,
        receipt
            .entry
            .as_deref()
            .unwrap_or("none (the peer holds everything)"),
    ))
}

/// `--node`, or the one keystore actor whose genesis is kind `node`.
fn choose_node_actor(
    store: &kairo_store::FilesystemStore,
    keystore: &kairo_keystore::FilesystemKeystore,
    explicit: Option<&str>,
) -> Result<ActorId, CliError> {
    if let Some(raw) = explicit {
        return ActorId::new(raw.to_owned()).map_err(|source| CliError::ParseActorId {
            actor: raw.to_owned(),
            source,
        });
    }
    let actors = keystore.list_actors().map_err(CliError::ListKeystore)?;
    let nodes: Vec<ActorId> = actors
        .into_iter()
        .filter(|actor| {
            store
                .get_actor(actor)
                .is_ok_and(|genesis| genesis.actor_kind() == &ActorKind::node())
        })
        .collect();
    match nodes.as_slice() {
        [node] => Ok(node.clone()),
        [] => Err(CliError::NodeActorChoice {
            message: "the keystore holds no node actor; create one with `kairo actor create \
                      --kind node`"
                .to_owned(),
        }),
        _ => Err(CliError::NodeActorChoice {
            message: format!(
                "the keystore holds {} node actors; pick one with --node",
                nodes.len()
            ),
        }),
    }
}

/// `kairo federation receive`: the engine's inbox pass
/// (`DECISIONS.md` §26 sub-decision 4, §28 sub-decision 3), run now.
fn run_receive(offline: bool, paths: &StorePaths) -> Result<String, CliError> {
    if offline {
        return Err(CliError::OfflineFetch);
    }
    let (config, source) = load_config(paths)?;
    let store = open_store(paths)?;
    let received = kairo_federation_service::receive_inbox(
        &store,
        &config.federation,
        source.path().to_path_buf(),
    )
    .map_err(|error| match error {
        kairo_federation_service::SyncError::Disabled { config } => {
            CliError::FederationDisabled { config }
        }
        kairo_federation_service::SyncError::Store(source) => CliError::ListInbox { source },
    })?;
    let entries: std::collections::BTreeSet<&str> =
        received.iter().map(|item| item.entry.as_str()).collect();
    let mut output = format!("received = {}\n", entries.len());
    let mut failed = 0;
    for item in &received {
        let line = describe_received(item);
        if !matches!(item.outcome, ReceiveOutcome::Received { .. }) {
            failed += 1;
        }
        output.push_str(&format!("  entry={} {line}\n", item.entry));
    }
    output.push_str(&format!("not_pulled = {failed}\n"));
    Ok(output)
}

/// One `receive` line: `pulled` (announcement) or `ingested`
/// (upload), `refused`, or `failed`.
fn describe_received(item: &Received) -> String {
    let (object, from) = (&item.object, &item.from);
    match &item.outcome {
        ReceiveOutcome::Received {
            report,
            published,
            pull_log_error,
        } => {
            let verb = match item.via {
                Via::Announcement => "pulled",
                Via::Upload => "ingested",
            };
            let mut line = format!(
                "{verb} object={object} from={from} fetched={} already_held={} flagged={} \
                 incomplete_chains={} git=\"{}\" published={published}",
                report.fetched,
                report.already_held,
                report.flagged.len(),
                report.incomplete.len(),
                describe_git(&report.git),
            );
            if let Some(error) = pull_log_error {
                line.push_str(&format!(" pull_log_error=\"{error}\""));
            }
            line
        }
        ReceiveOutcome::Refused { reason } => {
            format!("object={object} refused from={from} reason=\"{reason}\"")
        }
        ReceiveOutcome::Failed { reason } => {
            format!("object={object} failed from={from} reason=\"{reason}\"")
        }
    }
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
            // The pull authenticated the node actor by its signed node
            // document (DECISIONS.md §25), so a pin holds the URL to
            // that identity.
            (_, true) => "pinned, authenticated".to_owned(),
            (true, false) => format!(
                "authenticated, unpinned; add node_actor = \"{}\" to its [[federation.peers]] \
                 entry to pin it",
                report.peer_actor
            ),
            (false, false) => "authenticated; peer not in config (--allow-peer)".to_owned(),
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

/// A follow from its CLI spelling.
fn follow_of(
    object: Option<String>,
    trust_by: Option<String>,
    scope: Option<String>,
) -> Result<Follow, CliError> {
    match (object, trust_by, scope) {
        (Some(object), None, None) => Ok(Follow::Object(parse_object(object)?)),
        (None, Some(by), Some(scope)) => Ok(Follow::Trust {
            by: parse_actor(&by)?,
            scope: TrustScope::parse(&scope).map_err(CliError::ParseTrustScope)?,
        }),
        _ => Err(CliError::NodeActorChoice {
            message: "name --object <id>, or --trust-by <actor> with --scope <scope>".to_owned(),
        }),
    }
}

/// `kairo federation follow` / `unfollow` (`DECISIONS.md` §28
/// sub-decision 1).
fn run_follow(follow: &Follow, add: bool, paths: &StorePaths) -> Result<String, CliError> {
    let store = open_store(paths)?;
    let follows = |source| CliError::Follows { source };
    let status = if add {
        if store
            .add_follow(follow, Timestamp::now())
            .map_err(follows)?
        {
            "following"
        } else {
            "already_following"
        }
    } else if store.remove_follow(follow).map_err(follows)? {
        "unfollowed"
    } else {
        "not_following"
    };
    Ok(format!("follow = {follow}\nstatus = {status}\n"))
}

/// `kairo federation sync` (§28 sub-decision 3): the engine, run now.
fn run_sync(only: Option<&Follow>, offline: bool, paths: &StorePaths) -> Result<String, CliError> {
    if offline {
        return Err(CliError::OfflineFetch);
    }
    let (config, source) = load_config(paths)?;
    let store = open_store(paths)?;
    if let Some(only) = only {
        let followed = store
            .list_follows()
            .map_err(|source| CliError::Follows { source })?
            .iter()
            .any(|record| &record.follow == only);
        if !followed {
            return Err(CliError::NodeActorChoice {
                message: format!(
                    "{only} is not followed; follow it with `kairo federation follow` first"
                ),
            });
        }
    }
    let results = kairo_federation_service::sync_follows(
        &store,
        &config.federation,
        source.path().to_path_buf(),
        only,
    )
    .map_err(|error| match error {
        kairo_federation_service::SyncError::Disabled { config } => {
            CliError::FederationDisabled { config }
        }
        kairo_federation_service::SyncError::Store(source) => CliError::Follows { source },
    })?;
    let mut output = format!("synced = {}\n", results.len());
    for (follow, attempt) in &results {
        let status = if attempt.succeeded() {
            "current"
        } else {
            "not_current"
        };
        output.push_str(&format!("  follow={follow} status={status}\n"));
        for peer in &attempt.peers {
            output.push_str(&format!("    {}\n", describe_peer_outcome(peer)));
        }
    }
    Ok(output)
}

fn describe_peer_outcome(outcome: &PeerOutcome) -> String {
    match outcome {
        PeerOutcome::Fetched {
            peer_url,
            fetched,
            already_held,
            incomplete_chains,
            flagged,
        } => format!(
            "peer={peer_url} outcome=fetched fetched={fetched} already_held={already_held} \
             incomplete_chains={incomplete_chains} flagged={flagged}"
        ),
        PeerOutcome::NotPublished { peer_url } => {
            format!("peer={peer_url} outcome=not_published")
        }
        PeerOutcome::Failed { peer_url, reason } => {
            format!("peer={peer_url} outcome=failed reason=\"{reason}\"")
        }
    }
}

/// `kairo federation discover` (`DECISIONS.md` §29 sub-decision 3).
fn run_discover(offline: bool, paths: &StorePaths) -> Result<String, CliError> {
    if offline {
        return Err(CliError::OfflineFetch);
    }
    let (config, source) = load_config(paths)?;
    let store = open_store(paths)?;
    let outcomes =
        kairo_federation_service::discover(&store, &config.federation, source.path().to_path_buf())
            .map_err(|error| match error {
                kairo_federation_service::SyncError::Disabled { config } => {
                    CliError::FederationDisabled { config }
                }
                kairo_federation_service::SyncError::Store(source) => CliError::Follows { source },
            })?;
    let sources: std::collections::BTreeSet<&str> =
        outcomes.iter().map(DiscoverOutcome::source_url).collect();
    let mut output = format!("sources = {}\n", sources.len());
    for outcome in &outcomes {
        output.push_str(&match outcome {
            DiscoverOutcome::Learned {
                source_url,
                node_actor,
                objects,
                newer,
            } => format!(
                "  source={source_url} outcome=learned node_actor={node_actor} \
                 objects={objects} newer={newer}\n"
            ),
            DiscoverOutcome::Relayed {
                source_url,
                via,
                node_actor,
                objects,
                newer,
            } => format!(
                "  source={source_url} outcome=relayed via={via} node_actor={node_actor} \
                 objects={objects} newer={newer}\n"
            ),
            DiscoverOutcome::RelayRefused {
                source_url,
                node_actor,
                reason,
            } => format!(
                "  source={source_url} outcome=relay_refused node_actor={} reason=\"{reason}\"\n",
                node_actor
                    .as_ref()
                    .map_or_else(|| "unknown".to_owned(), ToString::to_string)
            ),
            DiscoverOutcome::Unsupported { source_url } => {
                format!("  source={source_url} outcome=unsupported\n")
            }
            DiscoverOutcome::Failed { source_url, reason } => {
                format!("  source={source_url} outcome=failed reason=\"{reason}\"\n")
            }
        });
    }
    output.push_str("note = hints say who has what; they grant nothing\n");
    Ok(output)
}

/// `kairo federation holders --object` (§29 sub-decision 5).
fn run_holders(object: &ObjectId, paths: &StorePaths) -> Result<String, CliError> {
    let (config, _) = load_config(paths)?;
    let store = open_store(paths)?;
    let hints = kairo_federation_service::holders(&store, &config.federation, object)
        .map_err(|source| CliError::Follows { source })?;
    let mut output = format!("object = {object}\nholders = {}\n", hints.len());
    for hint in &hints {
        let url = normalize_peer_url(&hint.base_url).unwrap_or_else(|_| hint.base_url.clone());
        let listed = peer_entry(&config, &url).is_some();
        output.push_str(&format!(
            "  node_actor={} base_url={} signed_at={} learned_from={} listed={listed}\n",
            hint.node_actor, hint.base_url, hint.signed_at, hint.learned_from,
        ));
    }
    Ok(output)
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
            "{} object={} fetched={} already_held={} node_actor={}",
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
    let follows = store
        .list_follows()
        .map_err(|source| CliError::Follows { source })?;
    output.push_str(&format!("follows = {}\n", follows.len()));
    for record in &follows {
        let last =
            |at: Option<Timestamp>| at.map_or_else(|| "never".to_owned(), |at| at.to_string());
        output.push_str(&format!(
            "  follow={} last_attempt={} last_success={}\n",
            record.follow,
            last(record.last_attempt.as_ref().map(|attempt| attempt.at)),
            last(record.last_success),
        ));
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
