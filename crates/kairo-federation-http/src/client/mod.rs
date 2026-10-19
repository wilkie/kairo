//! The pulling side of federation (`specs/PHASE_2_FEDERATION.md`
//! slice 3, `specs/DECISIONS.md` §16 sub-decisions 7 and 8).
//!
//! [`pull_object`] fetches one published object's closure from a peer
//! into the local store:
//!
//! 1. the node document, validated (the node actor derives from the
//!    genesis it ships, the node speaks `kairo-fed/1`, and its
//!    `base_url` is the URL pulled from);
//! 2. the node actor's genesis and key events, then the document's
//!    signature against them (`specs/DECISIONS.md` §25), so the peer
//!    is authenticated as its node actor;
//! 3. every page of the object listing;
//! 4. each listed actor: its genesis, then every key event the store
//!    lacks, in `created_at` order so each verifies against the keys
//!    before it;
//! 5. the object's genesis;
//! 6. authority (grants, then revocations), then the object
//!    statements (revisions, then branches and tags);
//! 7. blobs, checked against their content hash.
//!
//! Only records the store lacks are fetched, which makes a re-pull
//! incremental. Records go in dependency order, and each is stored as
//! soon as it verifies, so an interrupted pull leaves a consistent
//! store and a re-run resumes. A fixity, signature, schema, or
//! section failure fails the pull at that record, without storing it.
//! A limit being exceeded also fails the pull. In both cases the
//! records already stored stay.
//!
//! Every record the pull stores or finds already held gets a
//! provenance entry naming this peer.
//!
//! The same connection steps (1 and 2) also serve [`learn_peer`],
//! which stores a peer's authenticated node actor so the peer's
//! `KairoNode1` requests can be checked, and [`announce`], which sends
//! a signed announcement (`specs/DECISIONS.md` §26). [`PullError`] is
//! the error for all three.

mod http;
mod ingest;
mod push;

pub use self::push::{closure_inventory, ingest_closure_bundle, push, PushReceipt, PushSent};

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::Duration;

use kairo_bundle::closure::{ClosureBundle, ClosureBundleError};
use kairo_core::{ActorId, BlobId, KairoRef, ObjectId, StatementId, Timestamp};
use kairo_federation_core::GitAvailability;
use kairo_federation_core::{
    random_node_nonce, ActorListing, Announcement, AnnouncementAccepted, FedErrorCode,
    NodeAuthorization, NodeDocument, NodeDocumentError, NodeRequest, ObjectListing, RequestBody,
    StatementBatch, StatementBatchRequest, ANNOUNCE_PATH, API_PREFIX, MAX_BATCH_IDS,
    MAX_PAGE_LIMIT, STATEMENT_BATCH_PATH, SUPPORTS_ANNOUNCE, SUPPORTS_OBJECT_PULL,
    SUPPORTS_STATEMENT_BATCH, WELL_KNOWN_PATH,
};
use kairo_identity::SecretSigningKey;
use kairo_object::OBJECT_MANIFEST_DOMAIN;
use kairo_statement::{ChainScope, GitRevision, StatementKind};
use kairo_store::{
    ActorStore, BlobStore, ChainCompleteness, FilesystemStore, ObjectStore, ProvenanceEntry,
    ProvenanceStore, StatementStore, StoreError,
};

use self::http::Peer;
use self::ingest::{ingest_genesis, ingest_statement, peek, Ingested, Peek, Section};

/// Per-pull limits (`specs/PHASE_2_FEDERATION.md` §2.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    /// Largest statement, genesis, document, or listing page.
    pub max_json_bytes: u64,
    /// Largest statement batch response (`DECISIONS.md` §24). Each
    /// statement in it is still held to `max_json_bytes`.
    pub max_batch_bytes: u64,
    pub max_blob_bytes: u64,
    /// Most records (listed ids plus key events) one pull may name.
    pub max_records: usize,
    /// Per request, connect to last byte.
    pub request_timeout: Duration,
    /// Most bytes one Git fetch may write.
    pub max_git_bytes: u64,
    /// For the whole Git fetch, which can be large.
    pub git_timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_json_bytes: 1024 * 1024,
            max_batch_bytes: 32 * 1024 * 1024,
            max_blob_bytes: 256 * 1024 * 1024,
            max_records: 100_000,
            request_timeout: Duration::from_secs(30),
            max_git_bytes: 1024 * 1024 * 1024,
            git_timeout: Duration::from_secs(30 * 60),
        }
    }
}

/// How to run a pull.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PullOptions {
    pub limits: Limits,
    /// The node actor the peer must claim (a pin from the node's
    /// config). `None` accepts any valid claim; the report carries
    /// it so the operator can pin it.
    pub expected_node_actor: Option<ActorId>,
    /// Fail the pull if any actor chain it depends on is incomplete
    /// (`DECISIONS.md` §23 sub-decision 8). What verified is still
    /// stored.
    pub require_complete: bool,
}

/// Normalize a peer base URL the way [`pull_object`] does: `https://`
/// anywhere, `http://` only on loopback, origin only, trailing `/`
/// dropped. Callers use it to match config entries against
/// `--from`.
pub fn normalize_peer_url(raw: &str) -> Result<String, PullError> {
    http::normalize_base_url(raw)
}

/// An actor chain the pull depends on that names statements this node
/// still lacks (`DECISIONS.md` §23): a peer withheld them, or doesn't
/// have them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncompleteChain {
    pub signer: ActorId,
    pub scope: ChainScope,
    /// Named by some held statement's `previous` and not held. A
    /// missing grant stands in for a revocation that waits on it.
    pub missing: Vec<StatementId>,
}

/// A record the pull stored but that doesn't verify as valid: a
/// statement signed with a key revoked at its `created_at`, or one
/// whose `previous` names a statement outside its chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlaggedRecord {
    pub record: KairoRef,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullReport {
    pub object: ObjectId,
    pub peer_url: String,
    /// The peer's node actor, authenticated by its signed node
    /// document (`DECISIONS.md` §25).
    pub peer_actor: ActorId,
    /// Records fetched, verified, and stored.
    pub fetched: usize,
    /// Listed records the store already held (not re-fetched).
    pub already_held: usize,
    /// Stored records (counted in `fetched`) that verification will
    /// report as invalid.
    pub flagged: Vec<FlaggedRecord>,
    /// Chains of this object's actors (its `object:` scope, and each
    /// actor's `keys`) that are incomplete after the pull, by signer
    /// then scope.
    pub incomplete: Vec<IncompleteChain>,
    pub git: GitOutcome,
}

/// What the pull did about the object's Git history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitOutcome {
    /// No pulled revision names a `git:sha256:` commit.
    NoGitRevisions,
    /// Every revision's commit was already in the local cache.
    AlreadyHeld { commits: usize },
    /// The missing commits were fetched; every revision's commit now
    /// resolves locally and is pinned under `refs/kairo/imported/`.
    Ingested { commits: usize },
    /// The peer has no Git history for the object. `missing` commits stay
    /// unresolved, so `verify object` reports content as
    /// indeterminate until they come from elsewhere.
    Unavailable { missing: usize },
}

/// Why one record was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordRejection {
    /// Didn't parse as its kind's shape.
    Schema(String),
    /// Its content derives a different id than the one requested.
    Fixity {
        actual: String,
    },
    /// Kind or target doesn't match where the listing put it.
    WrongSection(String),
    Signature(String),
    /// The local store failed while checking or storing it.
    Store(String),
}

impl fmt::Display for RecordRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Schema(message) => write!(f, "malformed: {message}"),
            Self::Fixity { actual } => write!(f, "content hashes to {actual}"),
            Self::WrongSection(message) => write!(f, "does not belong in the closure: {message}"),
            Self::Signature(message) => write!(f, "signature does not verify: {message}"),
            Self::Store(message) => write!(f, "local store error: {message}"),
        }
    }
}

#[derive(Debug)]
pub enum PullError {
    InvalidPeerUrl {
        url: String,
        reason: String,
    },
    Transport {
        url: String,
        message: String,
    },
    /// A non-2xx response, with the federation error code if the body
    /// was an envelope.
    Status {
        url: String,
        status: u16,
        code: Option<FedErrorCode>,
        message: Option<String>,
    },
    TooLarge {
        url: String,
        limit: u64,
    },
    TooManyRecords {
        limit: usize,
    },
    Parse {
        url: String,
        message: String,
    },
    NodeDocument(NodeDocumentError),
    /// The node document's `base_url` isn't the URL pulled from, so
    /// it doesn't vouch for this URL.
    BaseUrlMismatch {
        pulled: String,
        document: String,
    },
    /// The node doesn't advertise this `supports` feature.
    Unsupported {
        feature: &'static str,
    },
    /// A request nonce couldn't be generated.
    Signing(String),
    /// A pushed bundle lists a record this node lacks but doesn't carry
    /// it (the push plan was stale, or the pusher left it out).
    NotInBundle {
        record: KairoRef,
    },
    /// A pushed bundle couldn't be read or built.
    Bundle(String),
    /// The peer claims a different node actor than the one pinned
    /// for its URL.
    NodeActorMismatch {
        expected: ActorId,
        claimed: ActorId,
    },
    /// A listing named a different object than requested.
    ListingMismatch {
        expected: ObjectId,
        found: ObjectId,
    },
    /// An actor listing doesn't check out (genesis doesn't derive the
    /// listed id, or names a different actor).
    BadActor {
        actor: ActorId,
        message: String,
    },
    Record {
        record: KairoRef,
        reason: RecordRejection,
    },
    Store(StoreError),
    /// Local Git cache operation failed (opening, `index-pack`,
    /// pinning).
    Git(String),
    /// The fetch didn't bring every pulled revision commit's closure.
    GitIncomplete {
        missing: Vec<String>,
    },
    /// `require_complete` was set and some chain is still incomplete.
    /// Everything that verified is stored.
    IncompleteChains {
        chains: Vec<IncompleteChain>,
    },
}

impl fmt::Display for PullError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPeerUrl { url, reason } => write!(f, "peer URL {url:?} {reason}"),
            Self::Transport { url, message } => write!(f, "request to {url} failed: {message}"),
            Self::Status {
                url,
                status,
                code: Some(FedErrorCode::FedNotFound),
                ..
            } => write!(
                f,
                "{url} returned {status}: the peer does not publish this record"
            ),
            Self::Status {
                url,
                status,
                code,
                message,
            } => {
                write!(f, "{url} returned {status}")?;
                if let Some(code) = code {
                    write!(f, " {code}")?;
                }
                if let Some(message) = message {
                    write!(f, ": {message}")?;
                }
                Ok(())
            }
            Self::TooLarge { url, limit } => {
                write!(f, "response from {url} exceeds the {limit}-byte limit")
            }
            Self::TooManyRecords { limit } => {
                write!(f, "the object's closure names more than {limit} records")
            }
            Self::Parse { url, message } => write!(f, "unexpected response from {url}: {message}"),
            Self::NodeDocument(error) => write!(f, "peer's node document is invalid: {error}"),
            Self::BaseUrlMismatch { pulled, document } => write!(
                f,
                "pulled from {pulled}, but the peer's node document is for {document:?}; \
                 nothing was fetched"
            ),
            Self::Unsupported { feature } => write!(f, "peer does not support {feature}"),
            Self::Signing(message) => write!(f, "could not sign the request: {message}"),
            Self::NotInBundle { record } => write!(
                f,
                "the bundle lists {record}, which this node lacks, but doesn't carry it"
            ),
            Self::Bundle(message) => write!(f, "closure bundle: {message}"),
            Self::NodeActorMismatch { expected, claimed } => write!(
                f,
                "peer claims node actor {claimed}, but this node pins {expected} for it; \
                 nothing was fetched"
            ),
            Self::ListingMismatch { expected, found } => {
                write!(f, "asked for object {expected}, peer listed {found}")
            }
            Self::BadActor { actor, message } => write!(f, "actor {actor}: {message}"),
            Self::Record { record, reason } => write!(f, "refused {record}: {reason}"),
            Self::Store(error) => write!(f, "local store error: {error}"),
            Self::Git(message) => write!(f, "local Git cache error: {message}"),
            Self::GitIncomplete { missing } => write!(
                f,
                "the Git fetch lacks part of {} revision commit(s): {}",
                missing.len(),
                missing.join(", ")
            ),
            Self::IncompleteChains { chains } => {
                write!(f, "{} actor chain(s) are incomplete:", chains.len())?;
                for chain in chains {
                    let missing: Vec<String> =
                        chain.missing.iter().map(ToString::to_string).collect();
                    write!(
                        f,
                        " {} in {} lacks {};",
                        chain.signer,
                        chain.scope,
                        missing.join(", ")
                    )?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for PullError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NodeDocument(error) => Some(error),
            Self::Store(error) => Some(error),
            _ => None,
        }
    }
}

/// Order statements so each one's dependencies come first: kind rank,
/// then `created_at`, then id.
fn rank(kind: StatementKind) -> u8 {
    match kind {
        StatementKind::ActorCapabilityGrant => 0,
        StatementKind::ActorCapabilityRevocation => 1,
        StatementKind::ObjectRevision => 2,
        _ => 3,
    }
}

/// Where a pull's records come from: a peer over HTTP, or an unpacked
/// closure bundle a node pushed (`DECISIONS.md` §26 sub-decision 8).
/// Every check after reading a record is the same for both.
enum Source {
    Peer(Peer),
    Bundle(ClosureBundle),
}

impl Source {
    fn peer(&self) -> Option<&Peer> {
        match self {
            Self::Peer(peer) => Some(peer),
            Self::Bundle(_) => None,
        }
    }
}

struct Pull<'a> {
    store: &'a FilesystemStore,
    source: Source,
    /// The peer advertises `statement-batch` (`DECISIONS.md` §24).
    batch: bool,
    limits: &'a Limits,
    provenance: ProvenanceEntry,
    report: PullReport,
}

/// Pull `object`'s published closure from the peer at `peer_url` into
/// `store`. See the module docs for the order and failure rules.
pub fn pull_object(
    store: &FilesystemStore,
    peer_url: &str,
    object: &ObjectId,
    options: &PullOptions,
) -> Result<PullReport, PullError> {
    let (peer, document) = connect(peer_url, options)?;
    if !document.supports(SUPPORTS_OBJECT_PULL) {
        return Err(PullError::Unsupported {
            feature: SUPPORTS_OBJECT_PULL,
        });
    }
    let mut pull = Pull::new(store, peer, &document, object, &options.limits);
    let mut budget = 0;
    pull.authenticate(&document, &mut budget)?;
    pull.run(object, budget)?;
    if options.require_complete && !pull.report.incomplete.is_empty() {
        return Err(PullError::IncompleteChains {
            chains: pull.report.incomplete,
        });
    }
    Ok(pull.report)
}

/// Fetch and check a peer's node document: it validates, matches the
/// pin, and is for this URL. Nothing is stored.
fn connect(peer_url: &str, options: &PullOptions) -> Result<(Peer, NodeDocument), PullError> {
    let limits = &options.limits;
    let peer = Peer::new(peer_url, limits)?;

    let document: NodeDocument = peer.get_json(WELL_KNOWN_PATH, limits.max_json_bytes)?;
    document.validate().map_err(PullError::NodeDocument)?;
    // Checked before anything is fetched or stored, so a peer whose
    // claimed identity changed contributes nothing.
    if let Some(expected) = &options.expected_node_actor {
        if expected != &document.node_actor {
            return Err(PullError::NodeActorMismatch {
                expected: expected.clone(),
                claimed: document.node_actor.clone(),
            });
        }
    }
    // The signature binds the document to its `base_url`; a document
    // for another URL says nothing about this one.
    if http::normalize_base_url(&document.base_url).ok().as_deref() != Some(peer.base_url()) {
        return Err(PullError::BaseUrlMismatch {
            pulled: peer.base_url().to_owned(),
            document: document.base_url.clone(),
        });
    }
    Ok((peer, document))
}

/// A peer [`learn_peer`] authenticated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LearnedPeer {
    pub peer_url: String,
    pub node_actor: ActorId,
    /// The node actor's genesis and key events newly stored.
    pub fetched: usize,
    pub already_held: usize,
}

/// Authenticate the peer at `peer_url` and store its node actor's
/// genesis and key events, with provenance. A receiver needs these to
/// check the peer's `KairoNode1` requests (`DECISIONS.md` §25), and
/// re-learning picks up a rotated key. `options.expected_node_actor`
/// pins as for a pull.
pub fn learn_peer(
    store: &FilesystemStore,
    peer_url: &str,
    options: &PullOptions,
) -> Result<LearnedPeer, PullError> {
    let (peer, document) = connect(peer_url, options)?;
    let pull = authenticated(store, peer, &document, &options.limits)?;
    Ok(LearnedPeer {
        peer_url: pull.report.peer_url,
        node_actor: document.node_actor,
        fetched: pull.report.fetched,
        already_held: pull.report.already_held,
    })
}

/// A [`Pull`] that has authenticated the peer and pulls nothing more.
/// Its report counts the node actor's records and names no object.
fn authenticated<'a>(
    store: &'a FilesystemStore,
    peer: Peer,
    document: &NodeDocument,
    limits: &'a Limits,
) -> Result<Pull<'a>, PullError> {
    let none = ObjectId::from_sha256_digest([0; 32]);
    let mut pull = Pull::new(store, peer, document, &none, limits);
    let mut budget = 0;
    pull.authenticate(document, &mut budget)?;
    Ok(pull)
}

/// The local node actor and the key it signs requests with.
#[derive(Debug, Clone, Copy)]
pub struct NodeSigner<'a> {
    pub node: &'a ActorId,
    pub key: &'a SecretSigningKey,
}

/// What a receiver said to an announcement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnounceReceipt {
    pub peer_url: String,
    /// The receiver, authenticated by its node document.
    pub peer_actor: ActorId,
    /// The receiver's inbox entry.
    pub entry: String,
}

/// Send `announcement` to the node at `peer_url`, signed by `signer`
/// (`DECISIONS.md` §26). The receiver is authenticated first, as for
/// a pull, so its node actor is stored here too; the signature names
/// its base URL. A refusal is [`PullError::Status`] with the
/// receiver's code (`fed_unauthenticated`, `fed_not_trusted`,
/// `fed_not_accepted`, `fed_busy`).
pub fn announce(
    store: &FilesystemStore,
    peer_url: &str,
    announcement: &Announcement,
    signer: NodeSigner<'_>,
    options: &PullOptions,
) -> Result<AnnounceReceipt, PullError> {
    let (peer, document) = connect(peer_url, options)?;
    if !document.supports(SUPPORTS_ANNOUNCE) {
        return Err(PullError::Unsupported {
            feature: SUPPORTS_ANNOUNCE,
        });
    }
    let pull = authenticated(store, peer, &document, &options.limits)?;
    let Some(peer) = pull.source.peer() else {
        return Err(PullError::Bundle("announce needs a peer".to_owned()));
    };
    let path = format!("{API_PREFIX}{ANNOUNCE_PATH}");
    let body = serde_json::to_vec(announcement).map_err(|error| PullError::Parse {
        url: format!("{}{path}", peer.base_url()),
        message: error.to_string(),
    })?;
    let nonce = random_node_nonce().map_err(|error| PullError::Signing(error.to_string()))?;
    let request = NodeRequest {
        method: "POST",
        receiver_base_url: peer.base_url(),
        path_and_query: &path,
        body: RequestBody::Bytes(&body),
    };
    let authorization = NodeAuthorization::sign(
        &request,
        signer.node.clone(),
        signer.key,
        Timestamp::now().seconds(),
        nonce,
    );
    let response = peer.post_signed(
        &path,
        &body,
        &authorization.format(),
        options.limits.max_json_bytes,
    )?;
    let accepted: AnnouncementAccepted =
        serde_json::from_slice(&response).map_err(|error| PullError::Parse {
            url: format!("{}{path}", peer.base_url()),
            message: error.to_string(),
        })?;
    Ok(AnnounceReceipt {
        peer_url: peer.base_url().to_owned(),
        peer_actor: document.node_actor,
        entry: accepted.entry,
    })
}

impl<'a> Pull<'a> {
    fn new(
        store: &'a FilesystemStore,
        peer: Peer,
        document: &NodeDocument,
        object: &ObjectId,
        limits: &'a Limits,
    ) -> Self {
        let peer_url = peer.base_url().to_owned();
        let batch = document.supports(SUPPORTS_STATEMENT_BATCH);
        Self::from_source(
            store,
            Source::Peer(peer),
            (peer_url, document.node_actor.clone()),
            batch,
            object,
            limits,
        )
    }

    /// `from` is the peer URL (or `push:<node>`) and node actor that
    /// provenance and the report name.
    fn from_source(
        store: &'a FilesystemStore,
        source: Source,
        from: (String, ActorId),
        batch: bool,
        object: &ObjectId,
        limits: &'a Limits,
    ) -> Self {
        let (peer_url, peer_actor) = from;
        let provenance = ProvenanceEntry {
            peer_url: peer_url.clone(),
            peer_actor: peer_actor.clone(),
            fetched_at: Timestamp::now(),
        };
        let report = PullReport {
            object: object.clone(),
            peer_url,
            peer_actor,
            fetched: 0,
            already_held: 0,
            flagged: Vec::new(),
            incomplete: Vec::new(),
            git: GitOutcome::NoGitRevisions,
        };
        Pull {
            store,
            batch,
            source,
            limits,
            provenance,
            report,
        }
    }
}

impl Pull<'_> {
    /// Fetch the node actor's genesis and key events (always served,
    /// `DECISIONS.md` §25 sub-decision 2), then check the document's
    /// signature against the key they make active at `signed_at`.
    /// The node actor's records are self-verifying, so they're stored
    /// even if the document's signature then fails.
    fn authenticate(
        &mut self,
        document: &NodeDocument,
        budget: &mut usize,
    ) -> Result<(), PullError> {
        *budget += 1;
        self.check_budget(*budget)?;
        self.actor(&document.node_actor, budget)?;
        document
            .verify_signature(self.store)
            .map_err(PullError::NodeDocument)?;
        Ok(())
    }

    fn run(&mut self, object: &ObjectId, mut budget: usize) -> Result<(), PullError> {
        let listing = self.listing(object)?;
        budget += listing.len();
        self.check_budget(budget)?;

        for actor in &listing.actors {
            // Already fetched by `authenticate`.
            if actor == &self.report.peer_actor {
                continue;
            }
            self.actor(actor, &mut budget)?;
        }
        self.genesis(object)?;

        let grants: BTreeSet<StatementId> = listing.authority.iter().cloned().collect();
        let authority = Section::Authority {
            object,
            grants: &grants,
        };
        self.statements(&listing.authority, authority)?;
        self.statements(&listing.statements, Section::Object(object))?;

        for blob in &listing.blobs {
            self.blob(blob)?;
        }

        let git_available = listing.git.is_some_and(|git| git.available);
        self.report.git = self.git(object, &listing.statements, git_available)?;
        self.chains(object, &listing.actors)
    }

    /// Check the chains this object depends on: each closure actor's
    /// `object:` chain and its `keys` chain (`DECISIONS.md` §23
    /// sub-decision 8). Gaps are reported; contradictions are flagged.
    fn chains(&mut self, object: &ObjectId, actors: &[ActorId]) -> Result<(), PullError> {
        let store = self.store;
        let discount = |id: &StatementId| store.signed_with_revoked_key(id).unwrap_or(false);
        for actor in actors {
            for scope in [ChainScope::Object(object.clone()), ChainScope::Keys] {
                let report = store
                    .actor_chain_report(actor, &scope, discount)
                    .map_err(PullError::Store)?;
                for id in report.contradictions {
                    self.report.flagged.push(FlaggedRecord {
                        record: KairoRef::Statement(id),
                        reason: format!(
                            "its previous names a statement outside {actor}'s {scope} chain"
                        ),
                    });
                }
                if let ChainCompleteness::Incomplete { missing } = report.completeness {
                    self.report.incomplete.push(IncompleteChain {
                        signer: actor.clone(),
                        scope,
                        missing,
                    });
                }
            }
        }
        Ok(())
    }

    /// Make every pulled revision's commit resolve in the local Git
    /// cache (`specs/DECISIONS.md` §19). Git is only fetched when some
    /// commit is missing, with `git fetch` over the peer's smart-HTTP
    /// endpoint (§22), in the object's format.
    ///
    /// The fetch lands in quarantine and only moves into the pool if
    /// every missing commit's full closure (commit, ancestors, trees,
    /// blobs) is in it. Only then is the object's repo created
    /// and the commits pinned, so a failed pull leaves neither a repo
    /// to advertise nor stray objects in the pool.
    fn git(
        &mut self,
        object: &ObjectId,
        statements: &[StatementId],
        available: bool,
    ) -> Result<GitOutcome, PullError> {
        let git_root = crate::git_root(self.store);
        let mut commits: BTreeSet<GitRevision> = BTreeSet::new();
        for id in statements {
            let bytes = match self.store.get_statement_bytes(id) {
                Ok(bytes) => bytes,
                // Refused earlier in this pull.
                Err(StoreError::Missing) => continue,
                Err(error) => return Err(PullError::Store(error)),
            };
            if !matches!(
                peek(&bytes),
                Ok(Peek {
                    kind: StatementKind::ObjectRevision,
                    ..
                })
            ) {
                continue;
            }
            let revision = self
                .store
                .get_object_revision(id)
                .map_err(PullError::Store)?;
            // A malformed `git:` revision names no commit; `verify
            // object` reports it. Nothing to transfer for it.
            if let Ok(Some(git)) = revision.unsigned().body().revision().git() {
                commits.insert(git);
            }
        }
        let Some(format) = commits.iter().next().map(GitRevision::format) else {
            return Ok(GitOutcome::NoGitRevisions);
        };
        if commits.iter().any(|commit| commit.format() != format) {
            return Err(PullError::Git(format!(
                "object {object}'s revisions mix Git object formats; one object's history \
                 has one format (DECISIONS.md §19)"
            )));
        }

        let mut missing = Vec::new();
        for commit in &commits {
            if !local_has_commit(&git_root, object, commit)? {
                missing.push(commit.clone());
            }
        }
        if missing.is_empty() {
            return Ok(GitOutcome::AlreadyHeld {
                commits: commits.len(),
            });
        }
        if !available {
            return Ok(GitOutcome::Unavailable {
                missing: missing.len(),
            });
        }

        let cache =
            kairo_git::GitCache::open(&git_root).map_err(|e| PullError::Git(e.to_string()))?;
        if let Some(existing) = cache
            .repo_format(object.as_str())
            .map_err(|e| PullError::Git(e.to_string()))?
        {
            if existing != format {
                return Err(PullError::Git(format!(
                    "the local cache holds object {object} as {existing}, but its revisions \
                     are {format}"
                )));
            }
        }
        // Git's own smart HTTP (DECISIONS.md §22): negotiation sends
        // the commits this cache already holds for the object as
        // `have`s, so the peer sends only what's missing, and the
        // fetch lands in a quarantine that reaches the pool only if
        // every missing commit's full closure arrived (§21).
        let peer = match &self.source {
            Source::Peer(peer) => peer,
            Source::Bundle(bundle) => {
                // The bundle's pack, possibly thin against what this
                // cache holds (§26 sub-decision 6), through the same
                // quarantine.
                let Some(pack) = bundle.pack() else {
                    return Ok(GitOutcome::Unavailable {
                        missing: missing.len(),
                    });
                };
                let file = std::fs::File::open(&pack)
                    .map_err(|error| PullError::Git(format!("{}: {error}", pack.display())))?;
                cache
                    .ingest_pack_from(format, file, &missing)
                    .map_err(|error| match error {
                        kairo_git::GitError::IncompletePack { missing } => {
                            PullError::GitIncomplete { missing }
                        }
                        other => PullError::Git(other.to_string()),
                    })?;
                return self.pin(&cache, object, &missing);
            }
        };
        let url = format!("{}{API_PREFIX}/objects/{object}/git", peer.base_url());
        let limits = kairo_git::FetchLimits {
            max_bytes: self.limits.max_git_bytes,
            timeout: self.limits.git_timeout,
        };
        cache
            .fetch_commits(object.as_str(), &url, &missing, &limits)
            .map_err(|error| match error {
                kairo_git::GitError::IncompletePack { missing } => {
                    PullError::GitIncomplete { missing }
                }
                kairo_git::GitError::TransferTooLarge { limit } => PullError::TooLarge {
                    url: url.clone(),
                    limit,
                },
                kairo_git::GitError::TransferTimedOut { .. } => PullError::Transport {
                    url: url.clone(),
                    message: error.to_string(),
                },
                other => PullError::Git(other.to_string()),
            })?;
        self.pin(&cache, object, &missing)
    }

    /// Pin each newly ingested commit under `refs/kairo/imported/`.
    fn pin(
        &self,
        cache: &kairo_git::GitCache,
        object: &ObjectId,
        missing: &[GitRevision],
    ) -> Result<GitOutcome, PullError> {
        for commit in missing {
            cache
                .set_ref(
                    object.as_str(),
                    &format!("refs/kairo/imported/{}", commit.oid()),
                    commit,
                )
                .map_err(|e| PullError::Git(e.to_string()))?;
        }
        Ok(GitOutcome::Ingested {
            commits: missing.len(),
        })
    }

    fn check_budget(&self, records: usize) -> Result<(), PullError> {
        if records > self.limits.max_records {
            Err(PullError::TooManyRecords {
                limit: self.limits.max_records,
            })
        } else {
            Ok(())
        }
    }

    /// Every page of the object listing, merged. A bundle's listing is
    /// its manifest's inventory.
    fn listing(&self, object: &ObjectId) -> Result<ObjectListing, PullError> {
        let peer = match &self.source {
            Source::Peer(peer) => peer,
            Source::Bundle(bundle) => {
                let inventory = &bundle.manifest().inventory;
                let mut listing = ObjectListing {
                    schema: Default::default(),
                    object: bundle.object().clone(),
                    statements: inventory.statements.clone(),
                    authority: inventory.authority.clone(),
                    actors: inventory.actors.iter().map(|a| a.actor.clone()).collect(),
                    blobs: inventory.blobs.clone(),
                    git: Some(GitAvailability {
                        available: bundle.pack().is_some(),
                    }),
                    next_cursor: None,
                };
                if &listing.object != object {
                    return Err(PullError::ListingMismatch {
                        expected: object.clone(),
                        found: listing.object,
                    });
                }
                self.check_budget(listing.len())?;
                for list in [&mut listing.statements, &mut listing.authority] {
                    dedup(list);
                }
                dedup(&mut listing.actors);
                dedup(&mut listing.blobs);
                return Ok(listing);
            }
        };
        let base = format!("{API_PREFIX}/objects/{object}?limit={MAX_PAGE_LIMIT}");
        let mut path = base.clone();
        let mut merged: Option<ObjectListing> = None;
        loop {
            let page: ObjectListing = peer.get_json(&path, self.limits.max_json_bytes)?;
            if &page.object != object {
                return Err(PullError::ListingMismatch {
                    expected: object.clone(),
                    found: page.object,
                });
            }
            let next = page.next_cursor.clone();
            let total = match merged.as_mut() {
                None => {
                    let len = page.len();
                    merged = Some(page);
                    len
                }
                Some(all) => {
                    all.statements.extend(page.statements);
                    all.authority.extend(page.authority);
                    all.actors.extend(page.actors);
                    all.blobs.extend(page.blobs);
                    all.len()
                }
            };
            self.check_budget(total)?;
            match next {
                Some(cursor) => path = format!("{base}&cursor={cursor}"),
                None => break,
            }
        }
        let mut listing = merged.ok_or_else(|| PullError::Parse {
            url: base,
            message: "empty listing".to_owned(),
        })?;
        // A well-behaved node pages without overlap; dedup anyway so a
        // misbehaving one can't make us count a record twice.
        for list in [&mut listing.statements, &mut listing.authority] {
            dedup(list);
        }
        dedup(&mut listing.actors);
        dedup(&mut listing.blobs);
        Ok(listing)
    }

    fn note(&self, record: &KairoRef) -> Result<(), PullError> {
        self.store
            .record_provenance(record, &self.provenance)
            .map(|_| ())
            .map_err(PullError::Store)
    }

    fn held<T>(result: Result<T, StoreError>) -> Result<bool, PullError> {
        match result {
            Ok(_) => Ok(true),
            Err(StoreError::Missing) => Ok(false),
            Err(error) => Err(PullError::Store(error)),
        }
    }

    fn actor(&mut self, actor: &ActorId, budget: &mut usize) -> Result<(), PullError> {
        let listing = self.actor_listing(actor)?;
        let bad = |message: String| PullError::BadActor {
            actor: actor.clone(),
            message,
        };
        if &listing.actor != actor {
            return Err(bad(format!("peer answered for {}", listing.actor)));
        }
        let genesis = listing.validate().map_err(|e| bad(e.to_string()))?;
        let record = KairoRef::Actor(actor.clone());
        if Self::held(self.store.get_actor(actor))? {
            self.report.already_held += 1;
        } else {
            self.store.put_actor(&genesis).map_err(PullError::Store)?;
            self.report.fetched += 1;
        }
        self.note(&record)?;

        *budget += listing.key_events.len();
        self.check_budget(*budget)?;

        // Key events verify against the keys before them, so fetch the
        // missing ones and ingest in chain order (`created_at` between
        // statements the chain leaves unordered).
        let mut wanted = Vec::new();
        for id in dedup_ids(&listing.key_events) {
            if Self::held(self.store.get_statement_bytes(&id))? {
                self.report.already_held += 1;
                self.note(&KairoRef::Statement(id))?;
                continue;
            }
            wanted.push(id);
        }
        let mut missing: Vec<Pending<Timestamp>> = Vec::new();
        for (id, bytes) in self.fetch_statements(&wanted)? {
            let peeked = peek(&bytes).map_err(|reason| PullError::Record {
                record: KairoRef::Statement(id.clone()),
                reason,
            })?;
            missing.push(Pending {
                key: peeked.created_at,
                id,
                previous: peeked.previous,
                bytes,
            });
        }
        for (id, bytes) in chain_order(missing) {
            self.ingest(&id, &bytes, Section::KeyEvent(actor))?;
        }
        Ok(())
    }

    fn genesis(&mut self, object: &ObjectId) -> Result<(), PullError> {
        let record = KairoRef::Object(object.clone());
        if Self::held(self.store.get_object_genesis(object))? {
            self.report.already_held += 1;
        } else {
            let bytes = match &self.source {
                Source::Peer(peer) => peer.get_bytes(
                    &format!("{API_PREFIX}/objects/{object}/genesis"),
                    self.limits.max_json_bytes,
                )?,
                Source::Bundle(bundle) => {
                    bundled(bundle.genesis(), KairoRef::Object(object.clone()))?
                }
            };
            ingest_genesis(self.store, object, &bytes).map_err(|reason| PullError::Record {
                record: record.clone(),
                reason,
            })?;
            self.report.fetched += 1;
        }
        self.note(&record)
    }

    fn fetch_statement(&self, id: &StatementId) -> Result<Vec<u8>, PullError> {
        match &self.source {
            Source::Peer(peer) => peer.get_bytes(
                &format!("{API_PREFIX}/statements/{id}"),
                self.limits.max_json_bytes,
            ),
            Source::Bundle(bundle) => {
                bundled(bundle.statement(id), KairoRef::Statement(id.clone()))
            }
        }
    }

    /// An actor's genesis and key-event ids: the peer's actor listing,
    /// or, from a bundle, the bundled genesis (or the one this store
    /// already holds) with the inventory's key events.
    fn actor_listing(&self, actor: &ActorId) -> Result<ActorListing, PullError> {
        let bundle = match &self.source {
            Source::Peer(peer) => {
                return peer.get_json(
                    &format!("{API_PREFIX}/actors/{actor}"),
                    self.limits.max_json_bytes,
                )
            }
            Source::Bundle(bundle) => bundle,
        };
        let key_events = bundle
            .manifest()
            .inventory
            .actors
            .iter()
            .find(|entry| &entry.actor == actor)
            .map(|entry| entry.key_events.clone())
            .unwrap_or_default();
        let record = KairoRef::Actor(actor.clone());
        let genesis = match bundle.actor(actor).map_err(bundle_error)? {
            Some(bytes) => serde_json::from_slice(&bytes).map_err(|error| PullError::Record {
                record,
                reason: RecordRejection::Schema(error.to_string()),
            })?,
            None => match self.store.get_actor(actor) {
                Ok(genesis) => kairo_identity::json::ActorGenesisJson::from_body(&genesis),
                Err(StoreError::Missing) => return Err(PullError::NotInBundle { record }),
                Err(error) => return Err(PullError::Store(error)),
            },
        };
        Ok(ActorListing {
            schema: Default::default(),
            actor: actor.clone(),
            genesis,
            key_events,
        })
    }

    /// Fetch `ids`, in batches of [`MAX_BATCH_IDS`] when the peer
    /// supports it (`DECISIONS.md` §24), otherwise one at a time. An id
    /// a batch leaves out is fetched on its own, so a listed statement
    /// the peer won't serve fails the pull with the same `404` as
    /// before. Returned in `ids` order.
    fn fetch_statements(
        &self,
        ids: &[StatementId],
    ) -> Result<Vec<(StatementId, Vec<u8>)>, PullError> {
        let peer = match &self.source {
            Source::Peer(peer) if self.batch => peer,
            _ => {
                return ids
                    .iter()
                    .map(|id| Ok((id.clone(), self.fetch_statement(id)?)))
                    .collect()
            }
        };
        let path = format!("{API_PREFIX}{STATEMENT_BATCH_PATH}");
        let mut fetched = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(MAX_BATCH_IDS) {
            let bytes = peer.post_json(
                &path,
                &StatementBatchRequest::new(chunk.to_vec()),
                self.limits.max_batch_bytes,
            )?;
            let batch: StatementBatch =
                serde_json::from_slice(&bytes).map_err(|error| PullError::Parse {
                    url: format!("{}{path}", peer.base_url()),
                    message: error.to_string(),
                })?;
            let mut served: BTreeMap<StatementId, Vec<u8>> = batch
                .statements
                .into_iter()
                .map(|entry| (entry.id, entry.statement.get().as_bytes().to_vec()))
                .collect();
            for id in chunk {
                let bytes = match served.remove(id) {
                    Some(bytes) if bytes.len() as u64 <= self.limits.max_json_bytes => bytes,
                    Some(_) => {
                        return Err(PullError::TooLarge {
                            url: format!("{}{path}", peer.base_url()),
                            limit: self.limits.max_json_bytes,
                        })
                    }
                    None => self.fetch_statement(id)?,
                };
                fetched.push((id.clone(), bytes));
            }
        }
        Ok(fetched)
    }

    fn statements(&mut self, ids: &[StatementId], section: Section<'_>) -> Result<(), PullError> {
        let mut wanted = Vec::new();
        for id in ids {
            if Self::held(self.store.get_statement_bytes(id))? {
                self.report.already_held += 1;
                self.note(&KairoRef::Statement(id.clone()))?;
                continue;
            }
            wanted.push(id.clone());
        }
        let mut missing: Vec<Pending<(u8, Timestamp)>> = Vec::new();
        for (id, bytes) in self.fetch_statements(&wanted)? {
            let peeked = peek(&bytes).map_err(|reason| PullError::Record {
                record: KairoRef::Statement(id.clone()),
                reason,
            })?;
            missing.push(Pending {
                key: (rank(peeked.kind), peeked.created_at),
                id,
                previous: peeked.previous,
                bytes,
            });
        }
        for (id, bytes) in chain_order(missing) {
            self.ingest(&id, &bytes, section)?;
        }
        Ok(())
    }

    fn ingest(
        &mut self,
        id: &StatementId,
        bytes: &[u8],
        section: Section<'_>,
    ) -> Result<(), PullError> {
        let record = KairoRef::Statement(id.clone());
        match ingest_statement(self.store, id, bytes, section) {
            Ok(Ingested::Stored) => {
                self.report.fetched += 1;
                self.note(&record)
            }
            Ok(Ingested::StoredRevokedKey) => {
                self.report.fetched += 1;
                self.report.flagged.push(FlaggedRecord {
                    record: record.clone(),
                    reason: "signing key is revoked at the statement's created_at".to_owned(),
                });
                self.note(&record)
            }
            Err(reason) => Err(PullError::Record { record, reason }),
        }
    }

    fn blob(&mut self, id: &BlobId) -> Result<(), PullError> {
        let record = KairoRef::Blob(id.clone());
        if Self::held(self.store.open_blob(id))? {
            self.report.already_held += 1;
            return self.note(&record);
        }
        let bytes = match &self.source {
            Source::Peer(peer) => peer.get_bytes(
                &format!("{API_PREFIX}/blobs/{id}"),
                self.limits.max_blob_bytes,
            )?,
            Source::Bundle(bundle) => bundled(bundle.blob(id), record.clone())?,
        };
        // Every MVP blob is a revision manifest.
        let actual = BlobId::from_bytes(OBJECT_MANIFEST_DOMAIN, &bytes);
        if &actual != id {
            return Err(PullError::Record {
                record,
                reason: RecordRejection::Fixity {
                    actual: actual.to_string(),
                },
            });
        }
        self.store.put_blob(id, &bytes).map_err(PullError::Store)?;
        self.report.fetched += 1;
        self.note(&record)
    }
}

/// A bundled record, or [`PullError::NotInBundle`] when the bundle
/// doesn't carry it.
fn bundled(
    read: Result<Option<Vec<u8>>, ClosureBundleError>,
    record: KairoRef,
) -> Result<Vec<u8>, PullError> {
    read.map_err(bundle_error)?
        .ok_or(PullError::NotInBundle { record })
}

fn bundle_error(error: ClosureBundleError) -> PullError {
    PullError::Bundle(error.to_string())
}

/// A fetched statement waiting to be ingested.
struct Pending<K> {
    /// Order between statements no chain orders.
    key: K,
    id: StatementId,
    previous: Vec<StatementId>,
    bytes: Vec<u8>,
}

/// `pending` in chain order (`DECISIONS.md` §23 sub-decision 7): a
/// statement after every pending statement its `previous` names, and
/// otherwise by `(key, id)`. Names outside the batch impose nothing
/// (already held, or missing). Content-addressed ids can't form a
/// cycle; if a peer's records claim one anyway, the rest are ingested
/// in `(key, id)` order and verification judges each statement.
fn chain_order<K: Ord + Clone>(pending: Vec<Pending<K>>) -> Vec<(StatementId, Vec<u8>)> {
    let ids: BTreeSet<StatementId> = pending.iter().map(|p| p.id.clone()).collect();
    let mut waiting_on: BTreeMap<StatementId, usize> = BTreeMap::new();
    let mut unblocks: BTreeMap<StatementId, Vec<StatementId>> = BTreeMap::new();
    let mut by_id: BTreeMap<StatementId, Pending<K>> = BTreeMap::new();
    for item in pending {
        let named: BTreeSet<&StatementId> = item
            .previous
            .iter()
            .filter(|p| ids.contains(*p) && **p != item.id)
            .collect();
        waiting_on.insert(item.id.clone(), named.len());
        for p in named {
            unblocks.entry(p.clone()).or_default().push(item.id.clone());
        }
        by_id.insert(item.id.clone(), item);
    }
    let mut ready: BTreeSet<(K, StatementId)> = by_id
        .values()
        .filter(|item| waiting_on.get(&item.id) == Some(&0))
        .map(|item| (item.key.clone(), item.id.clone()))
        .collect();
    let mut ordered = Vec::with_capacity(by_id.len());
    while !by_id.is_empty() {
        let next = match ready.pop_first() {
            Some((_, id)) => id,
            // A claimed cycle: release the least remaining statement.
            None => match by_id
                .values()
                .map(|item| (item.key.clone(), item.id.clone()))
                .min()
            {
                Some((_, id)) => id,
                None => break,
            },
        };
        let Some(item) = by_id.remove(&next) else {
            continue;
        };
        for later in unblocks.remove(&next).unwrap_or_default() {
            if let Some(count) = waiting_on.get_mut(&later) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    if let Some(waiting) = by_id.get(&later) {
                        ready.insert((waiting.key.clone(), later.clone()));
                    }
                }
            }
        }
        ordered.push((item.id, item.bytes));
    }
    ordered
}

fn dedup<T: Ord + Clone>(list: &mut Vec<T>) {
    let mut seen = BTreeSet::new();
    list.retain(|item| seen.insert(item.clone()));
}

fn dedup_ids(ids: &[StatementId]) -> Vec<StatementId> {
    let mut list = ids.to_vec();
    dedup(&mut list);
    list
}

/// Whether `commit` resolves in `object`'s per-object cache repo
/// (including its pool through alternates). Probed without opening
/// the cache, so a pull that needs no Git data never requires the
/// `git` binary, the same rule `verify object` follows. A repo in
/// another format doesn't hold it.
fn local_has_commit(
    git_root: &std::path::Path,
    object: &ObjectId,
    commit: &GitRevision,
) -> Result<bool, PullError> {
    let Ok(path) = kairo_git::object_repo_path(git_root, object.as_str()) else {
        return Ok(false);
    };
    if !path.exists() {
        return Ok(false);
    }
    let repo = kairo_git::open(&path).map_err(|e| PullError::Git(e.to_string()))?;
    if repo
        .object_format()
        .map_err(|e| PullError::Git(e.to_string()))?
        != commit.format()
    {
        return Ok(false);
    }
    repo.find_commit(commit)
        .map(|found| found.is_some())
        .map_err(|e| PullError::Git(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{chain_order, Pending};
    use kairo_core::StatementId;

    fn id(n: u8) -> StatementId {
        StatementId::from_sha256_digest([n; 32])
    }

    fn pending(n: u8, key: u8, previous: &[u8]) -> Pending<u8> {
        Pending {
            key,
            id: id(n),
            previous: previous.iter().map(|p| id(*p)).collect(),
            bytes: vec![n],
        }
    }

    fn order(items: Vec<Pending<u8>>) -> Vec<u8> {
        chain_order(items)
            .into_iter()
            .map(|(_, bytes)| bytes[0])
            .collect()
    }

    #[test]
    fn a_chain_orders_before_the_tiebreak_key() {
        // 3 names 2, which names 1, though their keys say the reverse.
        assert_eq!(
            order(vec![
                pending(1, 9, &[]),
                pending(2, 5, &[1]),
                pending(3, 1, &[2]),
            ]),
            vec![1, 2, 3]
        );
        // Unrelated statements, and names outside the batch, fall back
        // to the key.
        assert_eq!(
            order(vec![
                pending(1, 3, &[]),
                pending(2, 1, &[7]),
                pending(4, 2, &[]),
            ]),
            vec![2, 4, 1]
        );
        // A merge waits for both tips.
        assert_eq!(
            order(vec![
                pending(4, 0, &[2, 3]),
                pending(3, 2, &[1]),
                pending(2, 1, &[1]),
                pending(1, 5, &[]),
            ]),
            vec![1, 2, 3, 4]
        );
    }

    #[test]
    fn a_claimed_cycle_still_ingests_everything_once() {
        let ordered = order(vec![
            pending(1, 1, &[2]),
            pending(2, 2, &[1]),
            pending(3, 0, &[]),
        ]);
        assert_eq!(ordered, vec![3, 1, 2]);
    }
}
