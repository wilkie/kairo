//! The pulling side of federation (`specs/PHASE_2_FEDERATION.md`
//! slice 3, `specs/DECISIONS.md` §16 sub-decisions 7 and 8).
//!
//! [`pull_object`] fetches one published object's closure from a peer
//! into the local store:
//!
//! 1. the node document, validated (the node actor derives from the
//!    genesis it ships, and the node speaks `kairo-fed/1`);
//! 2. every page of the object listing;
//! 3. each listed actor: its genesis, then every key event the store
//!    lacks, in `created_at` order so each verifies against the keys
//!    before it;
//! 4. the object's genesis;
//! 5. authority (grants, then revocations), then the object
//!    statements (revisions, then branches and tags);
//! 6. blobs, checked against their content hash.
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

mod http;
mod ingest;

use std::collections::BTreeSet;
use std::fmt;
use std::time::Duration;

use kairo_core::{ActorId, BlobId, KairoRef, ObjectId, StatementId, Timestamp};
use kairo_federation_core::{
    ActorListing, FedErrorCode, NodeDocument, NodeDocumentError, ObjectListing, API_PREFIX,
    MAX_PAGE_LIMIT, SUPPORTS_OBJECT_PULL, WELL_KNOWN_PATH,
};
use kairo_object::OBJECT_MANIFEST_DOMAIN;
use kairo_statement::StatementKind;
use kairo_store::{
    ActorStore, BlobStore, FilesystemStore, ObjectStore, ProvenanceEntry, ProvenanceStore,
    StoreError,
};

use self::http::Peer;
use self::ingest::{ingest_genesis, ingest_statement, peek, Ingested, Section};

/// Per-pull limits (`specs/PHASE_2_FEDERATION.md` §2.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    /// Largest statement, genesis, document, or listing page.
    pub max_json_bytes: u64,
    pub max_blob_bytes: u64,
    /// Most records (listed ids plus key events) one pull may name.
    pub max_records: usize,
    /// Per request, connect to last byte.
    pub request_timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_json_bytes: 1024 * 1024,
            max_blob_bytes: 256 * 1024 * 1024,
            max_records: 100_000,
            request_timeout: Duration::from_secs(30),
        }
    }
}

/// A record the pull verified but did not store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedRecord {
    pub record: KairoRef,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullReport {
    pub object: ObjectId,
    pub peer_url: String,
    /// Claimed by the peer's unsigned node document.
    pub peer_actor: ActorId,
    /// Records fetched, verified, and stored.
    pub fetched: usize,
    /// Listed records the store already held (not re-fetched).
    pub already_held: usize,
    pub skipped: Vec<SkippedRecord>,
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
    /// The node doesn't advertise `object-pull`.
    Unsupported,
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
            Self::Unsupported => write!(f, "peer does not support {SUPPORTS_OBJECT_PULL}"),
            Self::ListingMismatch { expected, found } => {
                write!(f, "asked for object {expected}, peer listed {found}")
            }
            Self::BadActor { actor, message } => write!(f, "actor {actor}: {message}"),
            Self::Record { record, reason } => write!(f, "refused {record}: {reason}"),
            Self::Store(error) => write!(f, "local store error: {error}"),
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

struct Pull<'a> {
    store: &'a FilesystemStore,
    peer: Peer,
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
    limits: &Limits,
) -> Result<PullReport, PullError> {
    let peer = Peer::new(peer_url, limits)?;

    let document: NodeDocument = peer.get_json(WELL_KNOWN_PATH, limits.max_json_bytes)?;
    document.validate().map_err(PullError::NodeDocument)?;
    if !document.supports(SUPPORTS_OBJECT_PULL) {
        return Err(PullError::Unsupported);
    }

    let provenance = ProvenanceEntry {
        peer_url: peer.base_url().to_owned(),
        peer_actor: document.node_actor.clone(),
        fetched_at: Timestamp::now(),
    };
    let report = PullReport {
        object: object.clone(),
        peer_url: peer.base_url().to_owned(),
        peer_actor: document.node_actor.clone(),
        fetched: 0,
        already_held: 0,
        skipped: Vec::new(),
    };
    let mut pull = Pull {
        store,
        peer,
        limits,
        provenance,
        report,
    };
    pull.run(object)?;
    Ok(pull.report)
}

impl Pull<'_> {
    fn run(&mut self, object: &ObjectId) -> Result<(), PullError> {
        let listing = self.listing(object)?;
        let mut budget = listing.len();
        self.check_budget(budget)?;

        for actor in &listing.actors {
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
        Ok(())
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

    /// Every page of the object listing, merged.
    fn listing(&self, object: &ObjectId) -> Result<ObjectListing, PullError> {
        let base = format!("{API_PREFIX}/objects/{object}?limit={MAX_PAGE_LIMIT}");
        let mut path = base.clone();
        let mut merged: Option<ObjectListing> = None;
        loop {
            let page: ObjectListing = self.peer.get_json(&path, self.limits.max_json_bytes)?;
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
        let listing: ActorListing = self.peer.get_json(
            &format!("{API_PREFIX}/actors/{actor}"),
            self.limits.max_json_bytes,
        )?;
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
        // missing ones and ingest in `created_at` order.
        let mut missing: Vec<(Timestamp, StatementId, Vec<u8>)> = Vec::new();
        for id in dedup_ids(&listing.key_events) {
            if Self::held(self.store.get_statement_bytes(&id))? {
                self.report.already_held += 1;
                self.note(&KairoRef::Statement(id))?;
                continue;
            }
            let bytes = self.fetch_statement(&id)?;
            let (_, created_at) = peek(&bytes).map_err(|reason| PullError::Record {
                record: KairoRef::Statement(id.clone()),
                reason,
            })?;
            missing.push((created_at, id, bytes));
        }
        missing.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
        for (_, id, bytes) in missing {
            self.ingest(&id, &bytes, Section::KeyEvent(actor))?;
        }
        Ok(())
    }

    fn genesis(&mut self, object: &ObjectId) -> Result<(), PullError> {
        let record = KairoRef::Object(object.clone());
        if Self::held(self.store.get_object_genesis(object))? {
            self.report.already_held += 1;
        } else {
            let bytes = self.peer.get_bytes(
                &format!("{API_PREFIX}/objects/{object}/genesis"),
                self.limits.max_json_bytes,
            )?;
            ingest_genesis(self.store, object, &bytes).map_err(|reason| PullError::Record {
                record: record.clone(),
                reason,
            })?;
            self.report.fetched += 1;
        }
        self.note(&record)
    }

    fn fetch_statement(&self, id: &StatementId) -> Result<Vec<u8>, PullError> {
        self.peer.get_bytes(
            &format!("{API_PREFIX}/statements/{id}"),
            self.limits.max_json_bytes,
        )
    }

    fn statements(&mut self, ids: &[StatementId], section: Section<'_>) -> Result<(), PullError> {
        let mut missing: Vec<((u8, Timestamp), StatementId, Vec<u8>)> = Vec::new();
        for id in ids {
            if Self::held(self.store.get_statement_bytes(id))? {
                self.report.already_held += 1;
                self.note(&KairoRef::Statement(id.clone()))?;
                continue;
            }
            let bytes = self.fetch_statement(id)?;
            let (kind, created_at) = peek(&bytes).map_err(|reason| PullError::Record {
                record: KairoRef::Statement(id.clone()),
                reason,
            })?;
            missing.push(((rank(kind), created_at), id.clone(), bytes));
        }
        missing.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
        for (_, id, bytes) in missing {
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
            Ok(Ingested::SkippedRevokedKey) => {
                self.report.skipped.push(SkippedRecord {
                    record,
                    reason: "signing key is revoked at the statement's created_at".to_owned(),
                });
                Ok(())
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
        let bytes = self.peer.get_bytes(
            &format!("{API_PREFIX}/blobs/{id}"),
            self.limits.max_blob_bytes,
        )?;
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

fn dedup<T: Ord + Clone>(list: &mut Vec<T>) {
    let mut seen = BTreeSet::new();
    list.retain(|item| seen.insert(item.clone()));
}

fn dedup_ids(ids: &[StatementId]) -> Vec<StatementId> {
    let mut list = ids.to_vec();
    dedup(&mut list);
    list
}
