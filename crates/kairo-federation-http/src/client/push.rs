//! Uploads (`specs/DECISIONS.md` §26 sub-decisions 6 and 8): the
//! pushing side, which plans with the receiver and sends a closure
//! bundle of just the delta, and the receiving side's ingest of that
//! bundle through the pull verifier.

use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use kairo_bundle::closure::{
    write_tar, ClosureBundle, ClosureBundleWriter, ClosureGit, ClosureManifest,
    CLOSURE_BUNDLE_SCHEMA,
};
use kairo_core::{ActorId, ObjectId, Timestamp};
use kairo_federation_core::{
    random_node_nonce, ClosureInventory, InventoryActor, NodeAuthorization, NodeRequest,
    PinnedInventory, PushPlan, PushPlanRequest, RequestBody, API_PREFIX, PUSH_PATH, PUSH_PLAN_PATH,
    SUPPORTS_PUSH,
};
use kairo_identity::json::ActorGenesisJson;
use kairo_statement::GitRevision;
use kairo_store::{ActorStore, BlobStore, FilesystemStore};
use sha2::{Digest, Sha256};

use super::http::Peer;
use super::{authenticated, connect, NodeSigner, Pull, PullError, PullOptions, PullReport, Source};

/// The inventory of `object`'s published closure in `store`: what a
/// pull from this node would list (`DECISIONS.md` §26 sub-decision 6).
pub fn closure_inventory(
    store: &FilesystemStore,
    object: &ObjectId,
) -> Result<ClosureInventory, PullError> {
    let closure = store.object_closure(object).map_err(PullError::Store)?;
    inventory_of(store, closure)
}

/// The inventories of the sources `object`'s revisions pin, as a pull
/// from this node would serve them with it (`DECISIONS.md` §38
/// sub-decision 10).
pub fn pinned_inventories(
    store: &FilesystemStore,
    object: &ObjectId,
) -> Result<Vec<PinnedInventory>, PullError> {
    let closure = store.object_closure(object).map_err(PullError::Store)?;
    crate::pinned_source_closures(store, object, &closure.statements)
        .map_err(PullError::Store)?
        .into_iter()
        .map(|(source, closure)| {
            Ok(PinnedInventory {
                object: source,
                inventory: inventory_of(store, closure)?,
            })
        })
        .collect()
}

fn inventory_of(
    store: &FilesystemStore,
    closure: kairo_store::ObjectClosure,
) -> Result<ClosureInventory, PullError> {
    let actors = closure
        .actors
        .iter()
        .map(|actor| {
            Ok(InventoryActor {
                actor: actor.clone(),
                key_events: store.actor_key_event_ids(actor).map_err(PullError::Store)?,
            })
        })
        .collect::<Result<Vec<_>, PullError>>()?;
    Ok(ClosureInventory {
        statements: closure.statements.into_iter().collect(),
        authority: closure.authority.into_iter().collect(),
        actors,
        blobs: closure.blobs.into_iter().collect(),
        commits: closure.commits.iter().map(ToString::to_string).collect(),
    })
}

/// What an upload sent, the pinned sources' records included.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PushSent {
    pub genesis: bool,
    /// Pinned sources the bundle carries (`DECISIONS.md` §38
    /// sub-decision 10).
    pub sources: usize,
    pub actors: usize,
    pub statements: usize,
    pub blobs: usize,
    /// Commits the Git pack carries history for; the receiver had the
    /// rest.
    pub commits: usize,
    /// Bytes of the uploaded closure bundle.
    pub bytes: u64,
}

/// What the receiver said to an upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushReceipt {
    pub peer_url: String,
    /// The receiver, authenticated by its node document.
    pub peer_actor: ActorId,
    /// The inventory's size, against which `sent` is the delta.
    pub inventory: usize,
    pub sent: PushSent,
    /// The receiver's inbox entry; `None` when the plan asked for
    /// nothing, so nothing was uploaded.
    pub entry: Option<String>,
}

/// Push `object`'s published closure to the node at `peer_url`, sending
/// only what its plan says it lacks (`DECISIONS.md` §26). `scratch` is
/// a directory this call may create and remove, under the store. A
/// refusal is [`PullError::Status`] with the receiver's code.
pub fn push(
    store: &FilesystemStore,
    peer_url: &str,
    object: &ObjectId,
    signer: NodeSigner<'_>,
    options: &PullOptions,
) -> Result<PushReceipt, PullError> {
    let (peer, document) = connect(peer_url, options)?;
    if !document.supports(SUPPORTS_PUSH) {
        return Err(PullError::Unsupported {
            feature: SUPPORTS_PUSH,
        });
    }
    let pull = authenticated(store, peer, &document, &options.limits)?;
    let Source::Peer(peer) = &pull.source else {
        return Err(PullError::Bundle("push needs a peer".to_owned()));
    };

    let inventory = closure_inventory(store, object)?;
    let sources = pinned_inventories(store, object)?;
    let request = PushPlanRequest {
        schema: Default::default(),
        object: object.clone(),
        inventory: inventory.clone(),
        sources: sources.clone(),
    };
    let plan_path = format!("{API_PREFIX}{PUSH_PLAN_PATH}");
    let body = serde_json::to_vec(&request).map_err(|error| PullError::Parse {
        url: format!("{}{plan_path}", peer.base_url()),
        message: error.to_string(),
    })?;
    let response = signed_post(
        peer,
        signer,
        &plan_path,
        RequestBody::Bytes(&body),
        Upload::Bytes(&body),
        options.limits.max_json_bytes,
    )?;
    let plan: PushPlan = serde_json::from_slice(&response).map_err(|error| PullError::Parse {
        url: format!("{}{plan_path}", peer.base_url()),
        message: error.to_string(),
    })?;

    let wanted_commits = wanted(&inventory, &plan)?;
    // Each pinned source the plan answers for, with what it wants.
    let mut source_plans = Vec::new();
    for source in &sources {
        if let Some(answer) = plan
            .sources
            .iter()
            .find(|answer| answer.object == source.object)
        {
            let commits = wanted(&source.inventory, &answer.plan)?;
            source_plans.push((source, &answer.plan, commits));
        }
    }
    let mut receipt = PushReceipt {
        peer_url: peer.base_url().to_owned(),
        peer_actor: document.node_actor.clone(),
        inventory: inventory.len()
            + sources
                .iter()
                .map(|source| source.inventory.len())
                .sum::<usize>(),
        sent: PushSent::default(),
        entry: None,
    };
    if plan.wants_no_records()
        && wanted_commits.is_empty()
        && source_plans
            .iter()
            .all(|(_, _, commits)| commits.is_empty())
    {
        return Ok(receipt);
    }

    let scratch = Scratch::create(store)?;
    let bundle_dir = scratch.path().join("bundle");
    receipt.sent = write_bundle(
        store,
        object,
        &inventory,
        &plan,
        &wanted_commits,
        &source_plans,
        &bundle_dir,
    )?;
    // The plan may want commits this node has no history for; then
    // there may be nothing to send at all.
    if receipt.sent == PushSent::default() {
        return Ok(receipt);
    }
    let tar_path = scratch.path().join("bundle.tar");
    let mut tar = fs::File::create(&tar_path).map_err(bundle_io(&tar_path))?;
    write_tar(&bundle_dir, &mut tar).map_err(|error| PullError::Bundle(error.to_string()))?;
    drop(tar);
    let (digest, bytes) = sha256_file(&tar_path)?;
    receipt.sent.bytes = bytes;

    let push_path = format!("{API_PREFIX}{PUSH_PATH}?object={object}");
    let file = fs::File::open(&tar_path).map_err(bundle_io(&tar_path))?;
    let response = signed_post(
        peer,
        signer,
        &push_path,
        RequestBody::Sha256(digest),
        Upload::File(file, bytes),
        options.limits.max_json_bytes,
    )?;
    let accepted: kairo_federation_core::PushAccepted =
        serde_json::from_slice(&response).map_err(|error| PullError::Parse {
            url: format!("{}{push_path}", peer.base_url()),
            message: error.to_string(),
        })?;
    receipt.entry = Some(accepted.entry);
    Ok(receipt)
}

/// A request body to send: bytes, or a file of known length.
enum Upload<'a> {
    Bytes(&'a [u8]),
    File(fs::File, u64),
}

fn signed_post(
    peer: &Peer,
    signer: NodeSigner<'_>,
    path: &str,
    signed_body: RequestBody<'_>,
    upload: Upload<'_>,
    limit: u64,
) -> Result<Vec<u8>, PullError> {
    let nonce = random_node_nonce().map_err(|error| PullError::Signing(error.to_string()))?;
    let request = NodeRequest {
        method: "POST",
        receiver_base_url: peer.base_url(),
        path_and_query: path,
        body: signed_body,
    };
    let authorization = NodeAuthorization::sign(
        &request,
        signer.node.clone(),
        signer.key,
        Timestamp::now().seconds(),
        nonce,
    )
    .format();
    match upload {
        Upload::Bytes(bytes) => peer.post_signed(path, bytes, &authorization, limit),
        Upload::File(file, length) => {
            peer.post_signed_file(path, file, length, &authorization, limit)
        }
    }
}

/// The inventory's commits the plan doesn't say the receiver has.
fn wanted(inventory: &ClosureInventory, plan: &PushPlan) -> Result<Vec<GitRevision>, PullError> {
    let have: BTreeSet<&str> = plan.have_commits.iter().map(String::as_str).collect();
    inventory
        .commits
        .iter()
        .filter(|commit| !have.contains(commit.as_str()))
        .map(|commit| parse_commit(commit))
        .collect()
}

fn bundle_failed(error: kairo_bundle::closure::ClosureBundleError) -> PullError {
    PullError::Bundle(error.to_string())
}

/// Write the closure bundle at `dir`: `object`'s delta, and inside it
/// the delta of each pinned source that has one.
fn write_bundle(
    store: &FilesystemStore,
    object: &ObjectId,
    inventory: &ClosureInventory,
    plan: &PushPlan,
    wanted_commits: &[GitRevision],
    sources: &[(&PinnedInventory, &PushPlan, Vec<GitRevision>)],
    dir: &Path,
) -> Result<PushSent, PullError> {
    let writer = ClosureBundleWriter::create(dir).map_err(bundle_failed)?;
    let mut sent = PushSent::default();
    let format = write_delta(
        store,
        object,
        inventory,
        plan,
        wanted_commits,
        &writer,
        &mut sent,
    )?;
    let mut carried = Vec::new();
    for (source, source_plan, commits) in sources {
        let nested = writer.source(&source.object).map_err(bundle_failed)?;
        let mut part = PushSent::default();
        let source_format = write_delta(
            store,
            &source.object,
            &source.inventory,
            source_plan,
            commits,
            &nested,
            &mut part,
        )?;
        if part == PushSent::default() {
            drop(nested);
            let _ = fs::remove_dir_all(dir.join("sources").join(source.object.as_str()));
            continue;
        }
        nested
            .finish(closure_manifest(
                &source.object,
                &source.inventory,
                source_format,
            ))
            .map_err(bundle_failed)?;
        carried.push(source.object.to_string());
        sent.sources += 1;
        sent.actors += part.actors;
        sent.statements += part.statements;
        sent.blobs += part.blobs;
        sent.commits += part.commits;
    }
    let mut manifest = closure_manifest(object, inventory, format);
    manifest.sources = carried;
    writer.finish(manifest).map_err(bundle_failed)?;
    Ok(sent)
}

fn closure_manifest(
    object: &ObjectId,
    inventory: &ClosureInventory,
    format: Option<kairo_statement::GitObjectFormat>,
) -> ClosureManifest {
    ClosureManifest {
        schema: CLOSURE_BUNDLE_SCHEMA.to_owned(),
        object: object.to_string(),
        created_at: Timestamp::now().to_string(),
        inventory: inventory.clone(),
        git: ClosureGit {
            format: format.map(|format| format.to_string()),
            pack: false,
        },
        sources: Vec::new(),
    }
}

/// Write the records `plan` asks for, and a Git pack of the wanted
/// commits' history less what the receiver has, with `writer`, adding
/// to `sent`; the commits' format.
fn write_delta(
    store: &FilesystemStore,
    object: &ObjectId,
    inventory: &ClosureInventory,
    plan: &PushPlan,
    wanted_commits: &[GitRevision],
    writer: &ClosureBundleWriter,
    sent: &mut PushSent,
) -> Result<Option<kairo_statement::GitObjectFormat>, PullError> {
    if plan.genesis {
        // A native object's genesis, or a Git or file source's.
        let bytes = store
            .get_any_genesis_bytes(object)
            .map_err(PullError::Store)?;
        writer.put_genesis(&bytes).map_err(bundle_failed)?;
        sent.genesis = true;
    }
    // Only ids this node listed: a receiver can't make it send records
    // outside the closure.
    let listed_actors: BTreeSet<&ActorId> = inventory.actors.iter().map(|a| &a.actor).collect();
    for actor in plan.actors.iter().filter(|a| listed_actors.contains(a)) {
        let genesis = store.get_actor(actor).map_err(PullError::Store)?;
        let bytes =
            serde_json::to_vec(&ActorGenesisJson::from_body(&genesis)).map_err(|error| {
                PullError::Bundle(format!("could not encode actor {actor}: {error}"))
            })?;
        writer.put_actor(actor, &bytes).map_err(bundle_failed)?;
        sent.actors += 1;
    }
    let listed: BTreeSet<_> = inventory.statement_ids().collect();
    for id in plan.statements.iter().filter(|id| listed.contains(id)) {
        let bytes = store.get_statement_bytes(id).map_err(PullError::Store)?;
        writer.put_statement(id, &bytes).map_err(bundle_failed)?;
        sent.statements += 1;
    }
    let listed_blobs: BTreeSet<_> = inventory.blobs.iter().collect();
    for id in plan.blobs.iter().filter(|id| listed_blobs.contains(id)) {
        let bytes = store.get_blob(id).map_err(PullError::Store)?;
        writer.put_blob(id, &bytes).map_err(bundle_failed)?;
        sent.blobs += 1;
    }

    let format = wanted_commits.first().map(GitRevision::format);
    if let Some(format) = format {
        let cache = kairo_git::GitCache::open(crate::git_root(store))
            .map_err(|e| PullError::Git(e.to_string()))?;
        // Without a repo for the object there's no history to send; the
        // receiver reports the commits as unavailable.
        if cache
            .repo_format(object.as_str())
            .map_err(|e| PullError::Git(e.to_string()))?
            .is_some()
        {
            let haves: Vec<GitRevision> = plan
                .have_commits
                .iter()
                .filter_map(|commit| parse_commit(commit).ok())
                .filter(|commit: &GitRevision| commit.format() == format)
                .collect();
            let pack_path = writer.pack_path();
            let pack = fs::File::create(&pack_path).map_err(bundle_io(&pack_path))?;
            cache
                .pack_delta_to(object.as_str(), wanted_commits, &haves, pack)
                .map_err(|e| PullError::Git(e.to_string()))?;
            sent.commits += wanted_commits.len();
        }
    }
    Ok(format)
}

/// Ingest a closure bundle `pusher` uploaded (`DECISIONS.md` §26
/// sub-decision 4): the pull verifier over the unpacked bundle, with
/// provenance naming `push:<pusher>`. The pusher's own key state must
/// already be in `store` (its request was authenticated against it).
pub fn ingest_closure_bundle(
    store: &FilesystemStore,
    bundle: ClosureBundle,
    pusher: &ActorId,
    options: &PullOptions,
) -> Result<PullReport, PullError> {
    let object = bundle.object().clone();
    // The pinned sources it carries, each its own bundle (`DECISIONS.md`
    // §38 sub-decision 10).
    let carried: std::collections::BTreeMap<ObjectId, Box<dyn super::BundleFiles>> = bundle
        .sources()
        .map_err(|error| PullError::Bundle(error.to_string()))?
        .into_iter()
        .map(|source| {
            let id = source.object().clone();
            (id, Box::new(source) as Box<dyn super::BundleFiles>)
        })
        .collect();
    let listed: Vec<ObjectId> = carried.keys().cloned().collect();
    let mut pull = Pull::from_source(
        store,
        Source::Bundle(Box::new(bundle)),
        (format!("push:{pusher}"), pusher.clone()),
        false,
        &object,
        &options.limits,
    );
    pull.run(&object, 0)?;
    // Only those its own copy of the manifests now pins, at exactly
    // those commits, as a pull would.
    pull.pinned_sources(&object, &listed, Some(carried))?;
    if options.require_complete && !pull.report.incomplete.is_empty() {
        return Err(PullError::IncompleteChains {
            chains: pull.report.incomplete,
        });
    }
    Ok(pull.report)
}

/// A scratch directory under `<store>/federation/outgoing/`, removed on
/// drop.
struct Scratch(PathBuf);

impl Scratch {
    fn create(store: &FilesystemStore) -> Result<Self, PullError> {
        let nonce = random_node_nonce().map_err(|error| PullError::Signing(error.to_string()))?;
        let name: String = nonce.iter().map(|byte| format!("{byte:02x}")).collect();
        let path = store.root().join("federation").join("outgoing").join(name);
        fs::create_dir_all(&path).map_err(bundle_io(&path))?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A typed `git:<format>:<oid>` revision.
pub(crate) fn parse_commit(value: &str) -> Result<GitRevision, PullError> {
    match GitRevision::parse(value) {
        Ok(Some(revision)) => Ok(revision),
        Ok(None) => Err(PullError::Git(format!("{value:?} is not a git: revision"))),
        Err(error) => Err(PullError::Git(error.to_string())),
    }
}

fn bundle_io(path: &Path) -> impl FnOnce(std::io::Error) -> PullError + '_ {
    move |error| PullError::Bundle(format!("{}: {error}", path.display()))
}

fn sha256_file(path: &Path) -> Result<([u8; 32], u64), PullError> {
    let mut file = fs::File::open(path).map_err(bundle_io(path))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    let mut total = 0u64;
    loop {
        let read = file.read(&mut buffer).map_err(bundle_io(path))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        total += read as u64;
    }
    Ok((hasher.finalize().into(), total))
}
