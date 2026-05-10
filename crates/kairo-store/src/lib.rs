//! Local filesystem store for Kairo statements, actors, and blobs.
//!
//! The MVP is a direct-mode (single-process) store rooted at a path on
//! disk. Records are sharded under each record-type directory using two
//! levels of two base58 characters — see [`shard`] for details.
//!
//! Independent traits split the responsibilities:
//!
//! - [`ActorStore`] — actor genesis bodies, indexed by [`ActorId`].
//! - [`ObjectStore`] — signed `ObjectGenesis` statements, indexed by
//!   [`ObjectId`].
//! - [`StatementStore`] — signed envelope statements (`ObjectRevision`,
//!   `ObjectBranch`, `ObjectVersionTag`, `ActorTrust`,
//!   `ActorCapabilityGrant`, `ActorCapabilityRevocation`), indexed by
//!   [`StatementId`].
//! - [`BlobStore`] — raw bytes addressed by a caller-supplied [`BlobId`].
//!
//! [`BranchResolver`] resolves the current `(actor, object, name)`
//! `ObjectBranch` tip via a per-object materialized index that
//! `put_object_branch` keeps in sync. [`VersionTagResolver`] does the
//! same for `(actor, object, semver)` `ObjectVersionTag` heads via a
//! parallel index maintained by `put_object_version_tag`.
//! [`TrustResolver`] resolves the current `(by_actor, trusted_actor)`
//! `ActorTrust` head via a per-truster materialized index maintained
//! by `put_actor_trust`. [`CapabilityResolver`] resolves the current
//! `(grantor, grantee, scope)` `ActorCapabilityGrant` head — and any
//! effective `ActorCapabilityRevocation` for a grant — via a per-
//! grantor materialized index maintained by
//! `put_actor_capability_grant` / `put_actor_capability_revocation`,
//! plus a per-object cross-cutting reverse index maintained by
//! `put_actor_capability_grant` for the object-scoped grants. The
//! reverse index drives the §6.1 "for grantee `B` on object `O`, is
//! there any covering grant?" hot path.
//!
//! [`FilesystemStore`] implements all of these. It also implements
//! [`ActorResolver`] so `kairo-statement::verify` consumes a store directly.
//!
//! Future work (revisit at the moments noted):
//! - Multi-process safety (file locks): land soon after MVP structure is
//!   stable; the trait boundary already isolates callers from this change.
//! - SQLite or other index backends: defer until query patterns demand it.
//! - Canonical-binary sidecars (`.canonical` files) preserving exact signed
//!   bytes for federation/package exchange: revisit at TODO §9.

mod branches;
mod capabilities;
mod capabilities_by_object;
pub mod error;
mod keys;
mod lock;
mod shard;
mod statements_by_actor;
mod tags;
mod trust;

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use kairo_core::{ActorId, BlobId, ObjectId, StatementId, Timestamp};
use kairo_identity::json::ActorGenesisJson;
use kairo_identity::{
    ActorGenesisBody, ActorResolveError, ActorResolver, AttestationKeyAddEntry,
    AttestationKeyRevocationEntry, KeyRevocationEntry, KeyRotationEntry, KeySurface,
};
use kairo_statement::json::{
    ActorAttestationKeyAddStatementJson, ActorAttestationKeyRevocationStatementJson,
    ActorAttestationThresholdChangeStatementJson, ActorCapabilityGrantStatementJson,
    ActorCapabilityRevocationStatementJson, ActorEmergencyKeyRevocationStatementJson,
    ActorEmergencyKeyRotationStatementJson, ActorKeyRevocationStatementJson,
    ActorKeyRotationStatementJson, ActorTrustStatementJson, ObjectBranchStatementJson,
    ObjectGenesisStatementJson, ObjectRevisionStatementJson, ObjectVersionTagStatementJson,
};
use kairo_statement::{
    ActorAttestationKeyAddBody, ActorAttestationKeyRevocationBody,
    ActorAttestationThresholdChangeBody, ActorCapabilityGrantBody, ActorCapabilityRevocationBody,
    ActorEmergencyKeyRevocationBody, ActorEmergencyKeyRotationBody, ActorKeyRevocationBody,
    ActorKeyRotationBody, ActorTrustBody, CapabilityScope, MultiSignedStatement, ObjectBranchBody,
    ObjectGenesisStatement, ObjectRevisionBody, ObjectVersionTagBody, SignedStatement,
    StatementKind,
};

pub use branches::BranchTip;
pub use capabilities::{CapabilityHead, CapabilityRevocationRecord};
pub use capabilities_by_object::CapabilityByObjectHead;
pub use error::{CorruptReason, StoreError};
pub use statements_by_actor::StatementByActor;
pub use tags::VersionTagHead;
pub use trust::TrustHead;

/// Crate version string, drawn from `Cargo.toml`. Surfaced in
/// daemon `/api/v1/version` responses and similar diagnostics.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// On-disk schema version recorded in `<store>/version.txt` when
/// a store is opened. Surfaced in daemon `/api/v1/status`. Bumped
/// when the on-disk layout changes incompatibly.
pub const SCHEMA_VERSION: &str = "1";

const STORE_VERSION: &str = SCHEMA_VERSION;
const VERSION_FILE: &str = "version.txt";

const ACTORS_DIR: &str = "actors";
const OBJECTS_DIR: &str = "objects";
const STATEMENTS_DIR: &str = "statements";
const BRANCHES_DIR: &str = "branches";
const VERSION_TAGS_DIR: &str = "version_tags";
const TRUST_DIR: &str = "trust";
const ACTOR_CAPABILITY_DIR: &str = "actor_capability";
const ACTOR_CAPABILITY_BY_OBJECT_DIR: &str = "actor_capability_by_object";
const ACTOR_KEYS_DIR: &str = "actor_keys";
const STATEMENTS_BY_ACTOR_DIR: &str = "statements_by_actor";
const BLOBS_DIR: &str = "blobs";

const JSON_SUFFIX: &str = ".json";
const BLOB_SUFFIX: &str = "";

/// Persistence interface for actor genesis bodies.
pub trait ActorStore {
    fn put_actor(&self, genesis: &ActorGenesisBody) -> Result<ActorId, StoreError>;
    fn get_actor(&self, id: &ActorId) -> Result<ActorGenesisBody, StoreError>;
}

/// Persistence interface for object-level identity-deriving records.
///
/// `ObjectGenesisStatement` is identity-deriving: its body's canonical bytes
/// derive the `ObjectId`. Stored under `<root>/objects/<XX>/<YY>/<id>.json`,
/// parallel to actors.
///
/// The store does **not** verify the genesis signature on read. Callers that
/// need that guarantee should run signature verification separately when a
/// `Verifiable` trait lands.
pub trait ObjectStore {
    fn put_object_genesis(
        &self,
        statement: &ObjectGenesisStatement,
    ) -> Result<ObjectId, StoreError>;

    fn get_object_genesis(&self, id: &ObjectId) -> Result<ObjectGenesisStatement, StoreError>;
}

/// Persistence interface for envelope-wrapped signed statements.
///
/// Today `ObjectRevision`, `ObjectBranch`, `ObjectVersionTag`,
/// `ActorTrust`, `ActorCapabilityGrant`, and `ActorCapabilityRevocation`
/// are supported. New statement types add new methods alongside; do
/// not collapse them into a single generic until the shape of more
/// statement types is known.
///
/// `put_object_branch`, `put_object_version_tag`, `put_actor_trust`,
/// `put_actor_capability_grant`, and `put_actor_capability_revocation`
/// also update their respective materialized indices, so later
/// `BranchResolver` / `VersionTagResolver` / `TrustResolver` /
/// `CapabilityResolver` calls return the latest head without scanning
/// all underlying statements.
pub trait StatementStore {
    fn put_object_revision(
        &self,
        statement: &SignedStatement<ObjectRevisionBody>,
    ) -> Result<StatementId, StoreError>;

    fn get_object_revision(
        &self,
        id: &StatementId,
    ) -> Result<SignedStatement<ObjectRevisionBody>, StoreError>;

    fn put_object_branch(
        &self,
        statement: &SignedStatement<ObjectBranchBody>,
    ) -> Result<StatementId, StoreError>;

    fn get_object_branch(
        &self,
        id: &StatementId,
    ) -> Result<SignedStatement<ObjectBranchBody>, StoreError>;

    fn put_object_version_tag(
        &self,
        statement: &SignedStatement<ObjectVersionTagBody>,
    ) -> Result<StatementId, StoreError>;

    fn get_object_version_tag(
        &self,
        id: &StatementId,
    ) -> Result<SignedStatement<ObjectVersionTagBody>, StoreError>;

    fn put_actor_trust(
        &self,
        statement: &SignedStatement<ActorTrustBody>,
    ) -> Result<StatementId, StoreError>;

    fn get_actor_trust(
        &self,
        id: &StatementId,
    ) -> Result<SignedStatement<ActorTrustBody>, StoreError>;

    fn put_actor_capability_grant(
        &self,
        statement: &SignedStatement<ActorCapabilityGrantBody>,
    ) -> Result<StatementId, StoreError>;

    fn get_actor_capability_grant(
        &self,
        id: &StatementId,
    ) -> Result<SignedStatement<ActorCapabilityGrantBody>, StoreError>;

    fn put_actor_capability_revocation(
        &self,
        statement: &SignedStatement<ActorCapabilityRevocationBody>,
    ) -> Result<StatementId, StoreError>;

    fn get_actor_capability_revocation(
        &self,
        id: &StatementId,
    ) -> Result<SignedStatement<ActorCapabilityRevocationBody>, StoreError>;

    fn put_actor_key_rotation(
        &self,
        statement: &SignedStatement<ActorKeyRotationBody>,
    ) -> Result<StatementId, StoreError>;

    fn get_actor_key_rotation(
        &self,
        id: &StatementId,
    ) -> Result<SignedStatement<ActorKeyRotationBody>, StoreError>;

    fn put_actor_key_revocation(
        &self,
        statement: &SignedStatement<ActorKeyRevocationBody>,
    ) -> Result<StatementId, StoreError>;

    fn get_actor_key_revocation(
        &self,
        id: &StatementId,
    ) -> Result<SignedStatement<ActorKeyRevocationBody>, StoreError>;

    fn put_actor_emergency_key_rotation(
        &self,
        statement: &MultiSignedStatement<ActorEmergencyKeyRotationBody>,
    ) -> Result<StatementId, StoreError>;

    fn get_actor_emergency_key_rotation(
        &self,
        id: &StatementId,
    ) -> Result<MultiSignedStatement<ActorEmergencyKeyRotationBody>, StoreError>;

    fn put_actor_emergency_key_revocation(
        &self,
        statement: &MultiSignedStatement<ActorEmergencyKeyRevocationBody>,
    ) -> Result<StatementId, StoreError>;

    fn get_actor_emergency_key_revocation(
        &self,
        id: &StatementId,
    ) -> Result<MultiSignedStatement<ActorEmergencyKeyRevocationBody>, StoreError>;

    fn put_actor_attestation_key_add(
        &self,
        statement: &MultiSignedStatement<ActorAttestationKeyAddBody>,
    ) -> Result<StatementId, StoreError>;

    fn get_actor_attestation_key_add(
        &self,
        id: &StatementId,
    ) -> Result<MultiSignedStatement<ActorAttestationKeyAddBody>, StoreError>;

    /// Persist an `ActorAttestationKeyRevocation` and update the actor's
    /// per-actor key-event index.
    ///
    /// The store enforces the **non-empty attestation set** rule from
    /// `ACTORS.md` §5.5.2 here: if applying this revocation would leave
    /// the actor with zero attestation keys at `created_at`, the call
    /// returns [`StoreError::Rejected`] and nothing is written. This is
    /// the protocol-layer enforcement point; the CLI also refuses such
    /// statements upstream, but direct callers cannot bypass this guard.
    fn put_actor_attestation_key_revocation(
        &self,
        statement: &MultiSignedStatement<ActorAttestationKeyRevocationBody>,
    ) -> Result<StatementId, StoreError>;

    fn get_actor_attestation_key_revocation(
        &self,
        id: &StatementId,
    ) -> Result<MultiSignedStatement<ActorAttestationKeyRevocationBody>, StoreError>;

    /// Persist an `ActorAttestationThresholdChange` and update the
    /// per-actor key-event index.
    ///
    /// The store enforces the **asymmetric authority rule** from
    /// `ACTORS.md` §5.5.3 here: a raise (new > current) requires the
    /// envelope to carry `>= max(current, new)` distinct signatures; a
    /// lower or equal change requires `>= current`. Both are checked
    /// against the live attestation set at `created_at`. If the
    /// envelope falls short the call returns
    /// [`StoreError::Rejected`] and nothing is written. Signature
    /// validity itself is left to the verifier — the store only
    /// counts distinct attestation-set members.
    fn put_actor_attestation_threshold_change(
        &self,
        statement: &MultiSignedStatement<ActorAttestationThresholdChangeBody>,
    ) -> Result<StatementId, StoreError>;

    fn get_actor_attestation_threshold_change(
        &self,
        id: &StatementId,
    ) -> Result<MultiSignedStatement<ActorAttestationThresholdChangeBody>, StoreError>;
}

/// Resolver for the current `(actor, object, name)` branch tip.
///
/// Backed by the per-object tip index materialized in
/// `<root>/branches/<XX>/<YY>/<object-id>.json`. The MVP implementation
/// always queries the index; rebuilding the index from underlying
/// statements is future work.
pub trait BranchResolver {
    /// Latest `ObjectBranch` statement for `(actor, object, name)`.
    /// Returns `None` if no branch with that name has been published.
    fn latest_branch(
        &self,
        actor: &ActorId,
        object: &ObjectId,
        name: &str,
    ) -> Result<Option<SignedStatement<ObjectBranchBody>>, StoreError>;

    /// All known `(actor, name)` branch tips for an object.
    fn list_branches(&self, object: &ObjectId) -> Result<Vec<BranchTip>, StoreError>;
}

/// Resolver for the current `(actor, object, version)` version-tag head.
///
/// Backed by the per-object head index materialized in
/// `<root>/version_tags/<XX>/<YY>/<object-id>.json`. The MVP
/// implementation always queries the index; rebuilding from underlying
/// statements is future work.
///
/// The returned statement may be a bind (`target` present) or a
/// revocation (`target` absent). Callers walk the `supersedes` chain via
/// `get_object_version_tag` to surface the full audit history.
pub trait VersionTagResolver {
    /// Latest `ObjectVersionTag` statement for `(actor, object, version)`.
    /// Returns `None` if no tag with that version has been published.
    fn latest_version_tag(
        &self,
        actor: &ActorId,
        object: &ObjectId,
        version: &str,
    ) -> Result<Option<SignedStatement<ObjectVersionTagBody>>, StoreError>;

    /// All known `(actor, version)` tag heads for an object.
    fn list_version_tags(&self, object: &ObjectId) -> Result<Vec<VersionTagHead>, StoreError>;
}

/// Resolver for the current `(by_actor, trusted_actor)` trust head.
///
/// Backed by the per-trusted-actor head index materialized in
/// `<root>/trust/<XX>/<YY>/<trusted-actor-id>.json`. Each file is keyed
/// by truster, so opinions about a single trusted actor cluster
/// together — federation aggregation ("what does the world say about
/// Y?") is O(1).
///
/// The returned statement may be a grant (`decision = "trusted"`),
/// block (`decision = "untrusted"`), or withdrawal
/// (`decision = null` — the chain leaf retracts any prior decision).
/// Callers walk the `supersedes` chain via `get_actor_trust` to
/// surface the full audit history.
///
/// Cross-actor `supersedes` is invalid for trust at the canonical
/// schema layer; the per-truster keying inside each index file
/// enforces this structurally — the chain resolver only ever walks
/// entries under one truster key.
pub trait TrustResolver {
    /// Latest `ActorTrust` statement for `(by_actor, trusted_actor)`.
    /// Returns `None` if `by_actor` has never published a trust
    /// statement about `trusted_actor`.
    fn latest_trust(
        &self,
        by_actor: &ActorId,
        trusted_actor: &ActorId,
    ) -> Result<Option<SignedStatement<ActorTrustBody>>, StoreError>;

    /// All known trust heads signed by `by_actor`. Each head is the
    /// chain leaf for one `trusted_actor`.
    ///
    /// MVP implementation walks the trust directory; this is
    /// O(trusted-actor count). A per-truster reverse index can land
    /// when this query becomes hot.
    fn list_trust(&self, by_actor: &ActorId) -> Result<Vec<TrustHead>, StoreError>;

    /// All known trust heads about `trusted_actor`, one per truster
    /// who has expressed an opinion. O(1) given the sharding choice;
    /// useful for federation aggregation and for surfacing peer
    /// opinions in UIs.
    fn list_opinions_about(&self, trusted_actor: &ActorId) -> Result<Vec<TrustHead>, StoreError>;
}

/// Resolver for the current `(grantor, grantee, scope)`
/// `ActorCapabilityGrant` head and any effective
/// `ActorCapabilityRevocation` for a particular grant.
///
/// Backed by the per-grantor materialized index in
/// `<root>/actor_capability/<XX>/<YY>/<grantor-id>.json`. Each file
/// nests `grantee -> scope -> chain entries`, so the dominant
/// "for this `(grantor, grantee, scope)` triple, what is in effect?"
/// query is O(1) once the file is loaded. Sharding on the *grantor*
/// matches the duty model: the grantor maintains and revokes the
/// grants they issue (`specs/CAPABILITIES.md` Decision A, §9).
///
/// Resolution honors chain precedence (a successor with `supersedes`
/// wins over its predecessor regardless of `(created_at, statement_id)`
/// order); fork tiebreak falls back to greatest `(created_at,
/// statement_id)`. This mirrors `TrustResolver` and `VersionTagResolver`.
///
/// Revocations: only the original grantor may revoke (v1; see
/// `specs/CAPABILITIES.md` §5.2). Multiple revocations targeting the
/// same grant are tolerated (replay tolerance, §6.3); the most-
/// restrictive wins on read.
///
/// This resolver answers the structural questions ("is there a chain
/// leaf?", "is it revoked?"). The full
/// `evaluate_capability(grantee, target, at)` resolver from
/// `specs/CAPABILITIES.md` §6.1 — including delegation-depth checks,
/// expiration, and recursive grantor-authority verification — lands
/// in `kairo-statement::verify` on top of these primitives.
pub trait CapabilityResolver {
    /// Latest `ActorCapabilityGrant` for `(grantor, grantee, scope)`,
    /// resolved by chain precedence. Returns `None` if `grantor` has
    /// never issued a grant for that triple.
    fn latest_capability(
        &self,
        grantor: &ActorId,
        grantee: &ActorId,
        scope: &CapabilityScope,
    ) -> Result<Option<SignedStatement<ActorCapabilityGrantBody>>, StoreError>;

    /// Most-restrictive `ActorCapabilityRevocation` issued by
    /// `grantor` against `revoked_grant`, if any. Returns `None` if
    /// no revocation is known.
    fn latest_capability_revocation(
        &self,
        grantor: &ActorId,
        revoked_grant: &StatementId,
    ) -> Result<Option<SignedStatement<ActorCapabilityRevocationBody>>, StoreError>;

    /// All known `(grantee, scope)` chain heads issued by `grantor`.
    /// One head per triple. Drives the §7.1 audit query (enumerate a
    /// grantor's outstanding grants for key-compromise cleanup).
    fn list_capabilities_from(&self, grantor: &ActorId) -> Result<Vec<CapabilityHead>, StoreError>;

    /// All known `(grantor, grantee)` chain heads on `object` —
    /// across every grantor that has issued an object-scoped grant
    /// on it. Drives the §6.1 capability evaluator's hot path: when
    /// asking "does grantee `B` hold a covering grant on `O`?", the
    /// resolver enumerates these heads and evaluates each. Returns
    /// an empty vector if no grants target the object.
    ///
    /// Actor-scoped grants are not surfaced here (no statement kinds
    /// are valid for actor scope in v1; see `specs/CAPABILITIES.md`
    /// §4.3).
    fn list_capabilities_for_object(
        &self,
        object: &ObjectId,
    ) -> Result<Vec<CapabilityByObjectHead>, StoreError>;
}

/// Resolver for the per-actor signed-statement audit list.
///
/// Backed by the per-actor materialized index in
/// `<root>/statements_by_actor/<XX>/<YY>/<actor-id>.json`. Every
/// `put_*` for a signed envelope (single- or multi-signer) appends one
/// entry under the envelope's `actor`; `ObjectGenesis` is intentionally
/// excluded because it carries `created_by` instead of the envelope
/// `actor` field every other statement type uses, and the `objects/`
/// directory already provides the per-creator scan path (see
/// `statements_by_actor.rs` doc comment for the full rationale).
///
/// This resolver answers the inspector's "what has this actor signed?"
/// query in O(entries-for-this-actor) — no full statement-tree scan
/// per page load, even on large stores.
pub trait StatementByActorResolver {
    /// Every signed statement authored by `actor`, sorted by
    /// `(created_at, statement_id)` ascending. Returns an empty vector
    /// if `actor` has never been the envelope signer for a stored
    /// statement.
    fn list_statements_by_actor(
        &self,
        actor: &ActorId,
    ) -> Result<Vec<StatementByActor>, StoreError>;
}

/// Summary of a [`FilesystemStore::rebuild_indexes`] run.
///
/// `statements_scanned` counts every JSON file under
/// `statements/` that the rebuild successfully replayed; `by_kind`
/// breaks that count down by `StatementKind`. The two are kept
/// separate so a caller can sanity-check both the total work
/// done (e.g. compare to a known fixture's size) and the kind
/// distribution (e.g. assert that a store thought to contain
/// only object-revisions doesn't surprise with capability
/// statements).
///
/// `ObjectGenesis` never appears in `by_kind` — genesis is
/// stored under `objects/`, not `statements/`, and the rebuild
/// rejects (rather than silently absorbs) any genesis it finds
/// misplaced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RebuildReport {
    pub statements_scanned: usize,
    pub by_kind: BTreeMap<StatementKind, usize>,
}

/// Persistence interface for raw byte blobs.
///
/// `BlobId` is domain-prefixed (`sha256(domain || bytes)`), so the store
/// cannot fixity-check a blob without external context. Callers should
/// re-derive at the appropriate boundary (e.g. after parsing a manifest).
pub trait BlobStore {
    fn put_blob(&self, id: &BlobId, bytes: &[u8]) -> Result<(), StoreError>;
    fn get_blob(&self, id: &BlobId) -> Result<Vec<u8>, StoreError>;
}

/// Filesystem-backed Kairo store rooted at a single directory.
#[derive(Debug, Clone)]
pub struct FilesystemStore {
    root: PathBuf,
}

impl FilesystemStore {
    /// Open or initialize a store at `root`.
    ///
    /// - If the directory does not exist, it is created.
    /// - If `version.txt` is absent, it is written with the current store
    ///   version.
    /// - If `version.txt` exists with a different version, returns
    ///   `StoreError::Corrupt` (refuses to open).
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let root = root.into();
        fs::create_dir_all(&root)?;

        let version_path = root.join(VERSION_FILE);
        match fs::read_to_string(&version_path) {
            Ok(contents) => {
                let recorded = contents.trim();
                if recorded != STORE_VERSION {
                    return Err(StoreError::Corrupt {
                        id: VERSION_FILE.to_owned(),
                        reason: CorruptReason::SchemaMismatch,
                    });
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::write(&version_path, format!("{STORE_VERSION}\n"))?;
            }
            Err(error) => return Err(StoreError::Unavailable(error)),
        }

        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Read the raw JSON bytes for the statement with id `id`,
    /// without dispatching to the typed `get_*` getter for its
    /// kind. Used by the daemon's polymorphic
    /// `GET /api/v1/statements/{id}` route, where the on-disk
    /// JSON is the wire format and the response just streams
    /// it back inside the success envelope.
    ///
    /// Does not validate the file's hash matches `id` — fixity
    /// re-derivation belongs in the typed getters that decode
    /// the body. Callers that want fixity should prefer those.
    pub fn get_statement_bytes(&self, id: &StatementId) -> Result<Vec<u8>, StoreError> {
        let path = self.shard_path(STATEMENTS_DIR, id.as_str(), JSON_SUFFIX)?;
        read_or_missing(&path)
    }

    /// List every `ObjectRevision` statement on disk whose body
    /// targets `object`, sorted by `created_at` ascending (ties
    /// broken by `statement_id`).
    ///
    /// This is a full directory scan over `<root>/statements/`
    /// — there's no per-object revision index in the v1 store
    /// layout, so every call walks the sharded statement tree
    /// once. The cost is acceptable for inspector tooling on
    /// MVP-sized stores; if a future workload demands faster
    /// listing, add a `RevisionResolver` trait backed by a
    /// per-object materialized index (mirroring how
    /// `BranchResolver` / `VersionTagResolver` work) and move
    /// the method there.
    ///
    /// I/O errors surface as [`StoreError::Unavailable`]; an
    /// individual file that fails to parse as
    /// `ObjectRevisionStatementJson` surfaces as
    /// [`StoreError::Corrupt`] with the offending statement id
    /// in the error.
    pub fn list_object_revisions(
        &self,
        object: &ObjectId,
    ) -> Result<Vec<SignedStatement<ObjectRevisionBody>>, StoreError> {
        let statements_dir = self.root().join(STATEMENTS_DIR);
        let mut found = Vec::new();
        let level1 = match fs::read_dir(&statements_dir) {
            Ok(level1) => level1,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(found),
            Err(error) => return Err(StoreError::Unavailable(error)),
        };
        for shard1 in level1 {
            let shard1 = shard1?;
            if !shard1.path().is_dir() {
                continue;
            }
            for shard2 in fs::read_dir(shard1.path())? {
                let shard2 = shard2?;
                if !shard2.path().is_dir() {
                    continue;
                }
                for entry in fs::read_dir(shard2.path())? {
                    let entry = entry?;
                    let path = entry.path();
                    if path.extension().and_then(|s| s.to_str()) != Some("json") {
                        continue;
                    }
                    let bytes = fs::read(&path)?;
                    let dto: ObjectRevisionStatementJson = match serde_json::from_slice(&bytes) {
                        Ok(dto) => dto,
                        // Parse failure here means either a non-revision
                        // statement (we share the directory across all
                        // statement kinds) or a genuinely corrupt file. We
                        // can't tell from the deserializer alone — let
                        // through anything that doesn't decode as a
                        // revision shape. Per-kind parse errors don't
                        // belong to "list revisions for object".
                        Err(_) => continue,
                    };
                    let statement_id = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or_default();
                    let signed = dto.to_statement().map_err(|reason| StoreError::Corrupt {
                        id: statement_id.to_owned(),
                        reason: CorruptReason::Parse(reason.to_string()),
                    })?;
                    if signed.unsigned().body().object() == object {
                        found.push(signed);
                    }
                }
            }
        }
        found.sort_by(|a, b| {
            a.unsigned()
                .created_at()
                .cmp(&b.unsigned().created_at())
                .then_with(|| {
                    a.unsigned()
                        .statement_id()
                        .cmp(&b.unsigned().statement_id())
                })
        });
        Ok(found)
    }

    /// Rebuild every materialized index by replaying the on-disk
    /// `statements/` tree. Wipes each index directory first, then
    /// walks each statement file and dispatches it to the matching
    /// `index_*` helper — the same helpers `put_*` calls — so the
    /// resulting indexes are byte-for-byte equivalent to what
    /// continuous `put_*` would have produced.
    ///
    /// Use cases:
    ///
    /// - **Recovery from a corrupt index.** A partial write, an
    ///   external `rm`, a disk error — anything that leaves an
    ///   index file mangled. The underlying signed statements stay
    ///   on disk under `statements/` regardless, so a rebuild
    ///   restores the index without losing data.
    /// - **Adding a new index dimension.** When a new
    ///   materialized index lands (e.g. the §13 `statements_by_actor`
    ///   pass) existing stores have no entries until something
    ///   touches them. A one-shot rebuild backfills every dimension
    ///   over the historical statement set.
    /// - **Catching put/index drift.** If a `put_*` ever diverged
    ///   from its `index_*` helper in the past, a rebuild produces
    ///   the canonically correct state and the diff between the
    ///   pre- and post-rebuild trees is the bug.
    ///
    /// Wipe-then-rebuild semantics: this call is destructive on the
    /// index dirs, but the statements/ tree is the source of truth
    /// — rebuild is therefore safe to retry. It is *not* safe to
    /// run concurrently with `put_*`. The caller must arrange for
    /// no writers (stop the daemon, fail loud if rebuild is
    /// requested while one is running) — there's no global store
    /// lock yet (see `lock.rs` for the per-record advisory lock).
    ///
    /// Validation is intentionally skipped during replay: rebuild
    /// treats whatever made it onto disk under `statements/` as
    /// canonical history. The validation guards in
    /// `put_actor_attestation_*` are entry-point gates, not
    /// invariants over already-accepted statements.
    ///
    /// Returns a [`RebuildReport`] with the total scan count and
    /// per-kind breakdown, so callers (the CLI, monitoring) can
    /// assert the rebuild touched what they expected.
    pub fn rebuild_indexes(&self) -> Result<RebuildReport, StoreError> {
        // Wipe every index directory. We don't try to be clever
        // about preserving locks or partial state; if the rebuild
        // fails partway through, re-running it is the recovery
        // path.
        for dir in [
            BRANCHES_DIR,
            VERSION_TAGS_DIR,
            TRUST_DIR,
            ACTOR_CAPABILITY_DIR,
            ACTOR_CAPABILITY_BY_OBJECT_DIR,
            ACTOR_KEYS_DIR,
            STATEMENTS_BY_ACTOR_DIR,
        ] {
            let path = self.root.join(dir);
            match fs::remove_dir_all(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(StoreError::Unavailable(error)),
            }
        }

        let mut report = RebuildReport::default();
        let statements_dir = self.root.join(STATEMENTS_DIR);
        let level1 = match fs::read_dir(&statements_dir) {
            Ok(level1) => level1,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(report),
            Err(error) => return Err(StoreError::Unavailable(error)),
        };
        for shard1 in level1 {
            let shard1 = shard1?;
            if !shard1.path().is_dir() {
                continue;
            }
            for shard2 in fs::read_dir(shard1.path())? {
                let shard2 = shard2?;
                if !shard2.path().is_dir() {
                    continue;
                }
                for entry in fs::read_dir(shard2.path())? {
                    let entry = entry?;
                    let path = entry.path();
                    if path.extension().and_then(|s| s.to_str()) != Some("json") {
                        continue;
                    }
                    self.rebuild_one_statement(&path, &mut report)?;
                }
            }
        }
        Ok(report)
    }

    fn rebuild_one_statement(
        &self,
        path: &Path,
        report: &mut RebuildReport,
    ) -> Result<(), StoreError> {
        let bytes = fs::read(path)?;
        let file_stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_owned();

        // Two-pass parse: peek the `type` discriminator, then parse
        // into the typed shape. The dispatch arms below use the
        // peeked kind so a corrupted file with an unknown kind
        // surfaces as `Corrupt` rather than a confusing
        // wrong-shape parse error.
        #[derive(serde::Deserialize)]
        struct PeekedKind {
            #[serde(rename = "type")]
            kind: String,
        }
        let peek: PeekedKind =
            serde_json::from_slice(&bytes).map_err(|error| StoreError::Corrupt {
                id: file_stem.clone(),
                reason: CorruptReason::Parse(format!(
                    "missing or invalid 'type' discriminator: {error}"
                )),
            })?;
        let kind = StatementKind::parse(&peek.kind).map_err(|error| StoreError::Corrupt {
            id: file_stem.clone(),
            reason: CorruptReason::Parse(format!("unknown statement kind: {error}")),
        })?;

        // Per-kind dispatch. Each arm parses into the kind's typed
        // JSON shape and hands the result to the matching `index_*`
        // helper. The local macro keeps each arm to one line so the
        // table reads as a dispatch table; the per-arm body is
        // identical except for the DTO and index-method names.
        macro_rules! rebuild_arm {
            ($dto:ty, $index_fn:ident) => {{
                let json: $dto =
                    serde_json::from_slice(&bytes).map_err(|error| StoreError::Corrupt {
                        id: file_stem.clone(),
                        reason: CorruptReason::Parse(error.to_string()),
                    })?;
                let signed = json.to_statement().map_err(|error| StoreError::Corrupt {
                    id: file_stem.clone(),
                    reason: CorruptReason::Parse(error.to_string()),
                })?;
                let id = signed.statement_id();
                self.$index_fn(&signed, &id)?;
            }};
        }

        match kind {
            StatementKind::ObjectGenesis => {
                // ObjectGenesis is stored under objects/ (it is identity-
                // deriving, not envelope-wrapped). Finding one under
                // statements/ means a tool wrote it to the wrong place
                // — refuse to silently absorb the corruption.
                return Err(StoreError::Corrupt {
                    id: file_stem,
                    reason: CorruptReason::Parse(
                        "ObjectGenesis must not appear in statements/; expected under objects/"
                            .to_owned(),
                    ),
                });
            }
            StatementKind::ObjectRevision => {
                rebuild_arm!(ObjectRevisionStatementJson, index_object_revision);
            }
            StatementKind::ObjectBranch => {
                rebuild_arm!(ObjectBranchStatementJson, index_object_branch);
            }
            StatementKind::ObjectVersionTag => {
                rebuild_arm!(ObjectVersionTagStatementJson, index_object_version_tag);
            }
            StatementKind::ActorTrust => {
                rebuild_arm!(ActorTrustStatementJson, index_actor_trust);
            }
            StatementKind::ActorCapabilityGrant => {
                rebuild_arm!(
                    ActorCapabilityGrantStatementJson,
                    index_actor_capability_grant
                );
            }
            StatementKind::ActorCapabilityRevocation => {
                rebuild_arm!(
                    ActorCapabilityRevocationStatementJson,
                    index_actor_capability_revocation
                );
            }
            StatementKind::ActorKeyRotation => {
                rebuild_arm!(ActorKeyRotationStatementJson, index_actor_key_rotation);
            }
            StatementKind::ActorKeyRevocation => {
                rebuild_arm!(ActorKeyRevocationStatementJson, index_actor_key_revocation);
            }
            StatementKind::ActorEmergencyKeyRotation => {
                rebuild_arm!(
                    ActorEmergencyKeyRotationStatementJson,
                    index_actor_emergency_key_rotation
                );
            }
            StatementKind::ActorEmergencyKeyRevocation => {
                rebuild_arm!(
                    ActorEmergencyKeyRevocationStatementJson,
                    index_actor_emergency_key_revocation
                );
            }
            StatementKind::ActorAttestationKeyAdd => {
                rebuild_arm!(
                    ActorAttestationKeyAddStatementJson,
                    index_actor_attestation_key_add
                );
            }
            StatementKind::ActorAttestationKeyRevocation => {
                rebuild_arm!(
                    ActorAttestationKeyRevocationStatementJson,
                    index_actor_attestation_key_revocation
                );
            }
            StatementKind::ActorAttestationThresholdChange => {
                rebuild_arm!(
                    ActorAttestationThresholdChangeStatementJson,
                    index_actor_attestation_threshold_change
                );
            }
        }

        report.statements_scanned += 1;
        *report.by_kind.entry(kind).or_insert(0) += 1;
        Ok(())
    }

    /// Open the on-disk blob for `id` for streaming reads.
    ///
    /// Returns the opened [`std::fs::File`] so callers can drive
    /// the read without materializing the whole blob. The daemon
    /// uses this for `GET /api/v1/blobs/{id}` (chunked transfer);
    /// large packs and bundles will use it post-v1.
    ///
    /// Returns [`StoreError::Missing`] when the blob is not on
    /// disk, mirroring the semantic / I/O split the typed
    /// getters use. Other open failures surface as
    /// [`StoreError::Unavailable`].
    ///
    /// Does not validate fixity. The blob hash is domain-prefixed
    /// (`sha256(domain || bytes)`); the store cannot recompute
    /// without the caller's domain context (`BlobStore` doc).
    pub fn open_blob(&self, id: &BlobId) -> Result<fs::File, StoreError> {
        let path = self.shard_path(BLOBS_DIR, id.as_str(), BLOB_SUFFIX)?;
        match fs::File::open(&path) {
            Ok(file) => Ok(file),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Err(StoreError::Missing),
            Err(error) => Err(StoreError::Unavailable(error)),
        }
    }

    fn shard_path(&self, type_dir: &str, id: &str, suffix: &str) -> Result<PathBuf, StoreError> {
        shard::shard_path(&self.root, type_dir, id, suffix).map_err(|error| {
            StoreError::Unavailable(io::Error::new(
                io::ErrorKind::InvalidInput,
                error.to_string(),
            ))
        })
    }
}

impl ActorStore for FilesystemStore {
    fn put_actor(&self, genesis: &ActorGenesisBody) -> Result<ActorId, StoreError> {
        let id = genesis.actor_id();
        let json = ActorGenesisJson::from_body(genesis);
        let bytes = serde_json::to_vec_pretty(&json).map_err(json_to_corrupt(&id))?;
        let path = self.shard_path(ACTORS_DIR, id.as_str(), JSON_SUFFIX)?;
        atomic_write(&path, &bytes)?;
        Ok(id)
    }

    fn get_actor(&self, id: &ActorId) -> Result<ActorGenesisBody, StoreError> {
        let path = self.shard_path(ACTORS_DIR, id.as_str(), JSON_SUFFIX)?;
        let bytes = read_or_missing(&path)?;
        let json: ActorGenesisJson = serde_json::from_slice(&bytes).map_err(json_to_corrupt(id))?;
        let body = json.to_body().map_err(|error| StoreError::Corrupt {
            id: id.to_string(),
            reason: CorruptReason::Parse(error.to_string()),
        })?;
        let derived = body.actor_id();
        if &derived != id {
            return Err(StoreError::Corrupt {
                id: id.to_string(),
                reason: CorruptReason::HashMismatch {
                    expected: id.to_string(),
                    actual: derived.to_string(),
                },
            });
        }
        Ok(body)
    }
}

impl ObjectStore for FilesystemStore {
    fn put_object_genesis(
        &self,
        statement: &ObjectGenesisStatement,
    ) -> Result<ObjectId, StoreError> {
        let id = statement.object_id();
        let json = ObjectGenesisStatementJson::from_statement(statement);
        let bytes = serde_json::to_vec_pretty(&json).map_err(json_to_corrupt(&id))?;
        let path = self.shard_path(OBJECTS_DIR, id.as_str(), JSON_SUFFIX)?;
        atomic_write(&path, &bytes)?;
        Ok(id)
    }

    fn get_object_genesis(&self, id: &ObjectId) -> Result<ObjectGenesisStatement, StoreError> {
        let path = self.shard_path(OBJECTS_DIR, id.as_str(), JSON_SUFFIX)?;
        let bytes = read_or_missing(&path)?;
        let json: ObjectGenesisStatementJson =
            serde_json::from_slice(&bytes).map_err(json_to_corrupt(id))?;
        let statement = json.to_statement().map_err(|error| StoreError::Corrupt {
            id: id.to_string(),
            reason: CorruptReason::Parse(error.to_string()),
        })?;
        let derived = statement.object_id();
        if &derived != id {
            return Err(StoreError::Corrupt {
                id: id.to_string(),
                reason: CorruptReason::HashMismatch {
                    expected: id.to_string(),
                    actual: derived.to_string(),
                },
            });
        }
        Ok(statement)
    }
}

impl StatementStore for FilesystemStore {
    fn put_object_revision(
        &self,
        statement: &SignedStatement<ObjectRevisionBody>,
    ) -> Result<StatementId, StoreError> {
        let id = statement.statement_id();
        let json = ObjectRevisionStatementJson::from_statement(statement);
        let bytes = serde_json::to_vec_pretty(&json).map_err(json_to_corrupt(&id))?;
        let path = self.shard_path(STATEMENTS_DIR, id.as_str(), JSON_SUFFIX)?;
        atomic_write(&path, &bytes)?;

        self.index_object_revision(statement, &id)?;

        Ok(id)
    }

    fn get_object_revision(
        &self,
        id: &StatementId,
    ) -> Result<SignedStatement<ObjectRevisionBody>, StoreError> {
        let path = self.shard_path(STATEMENTS_DIR, id.as_str(), JSON_SUFFIX)?;
        let bytes = read_or_missing(&path)?;
        let json: ObjectRevisionStatementJson =
            serde_json::from_slice(&bytes).map_err(json_to_corrupt(id))?;
        let signed = json.to_statement().map_err(|error| StoreError::Corrupt {
            id: id.to_string(),
            reason: CorruptReason::Parse(error.to_string()),
        })?;
        let derived = signed.statement_id();
        if &derived != id {
            return Err(StoreError::Corrupt {
                id: id.to_string(),
                reason: CorruptReason::HashMismatch {
                    expected: id.to_string(),
                    actual: derived.to_string(),
                },
            });
        }
        Ok(signed)
    }

    fn put_object_branch(
        &self,
        statement: &SignedStatement<ObjectBranchBody>,
    ) -> Result<StatementId, StoreError> {
        let id = statement.statement_id();
        let json = ObjectBranchStatementJson::from_statement(statement);
        let bytes = serde_json::to_vec_pretty(&json).map_err(json_to_corrupt(&id))?;
        let path = self.shard_path(STATEMENTS_DIR, id.as_str(), JSON_SUFFIX)?;
        atomic_write(&path, &bytes)?;

        self.index_object_branch(statement, &id)?;

        Ok(id)
    }

    fn get_object_branch(
        &self,
        id: &StatementId,
    ) -> Result<SignedStatement<ObjectBranchBody>, StoreError> {
        let path = self.shard_path(STATEMENTS_DIR, id.as_str(), JSON_SUFFIX)?;
        let bytes = read_or_missing(&path)?;
        let json: ObjectBranchStatementJson =
            serde_json::from_slice(&bytes).map_err(json_to_corrupt(id))?;
        let signed = json.to_statement().map_err(|error| StoreError::Corrupt {
            id: id.to_string(),
            reason: CorruptReason::Parse(error.to_string()),
        })?;
        let derived = signed.statement_id();
        if &derived != id {
            return Err(StoreError::Corrupt {
                id: id.to_string(),
                reason: CorruptReason::HashMismatch {
                    expected: id.to_string(),
                    actual: derived.to_string(),
                },
            });
        }
        Ok(signed)
    }

    fn put_object_version_tag(
        &self,
        statement: &SignedStatement<ObjectVersionTagBody>,
    ) -> Result<StatementId, StoreError> {
        let id = statement.statement_id();
        let json = ObjectVersionTagStatementJson::from_statement(statement);
        let bytes = serde_json::to_vec_pretty(&json).map_err(json_to_corrupt(&id))?;
        let path = self.shard_path(STATEMENTS_DIR, id.as_str(), JSON_SUFFIX)?;
        atomic_write(&path, &bytes)?;

        self.index_object_version_tag(statement, &id)?;

        Ok(id)
    }

    fn get_object_version_tag(
        &self,
        id: &StatementId,
    ) -> Result<SignedStatement<ObjectVersionTagBody>, StoreError> {
        let path = self.shard_path(STATEMENTS_DIR, id.as_str(), JSON_SUFFIX)?;
        let bytes = read_or_missing(&path)?;
        let json: ObjectVersionTagStatementJson =
            serde_json::from_slice(&bytes).map_err(json_to_corrupt(id))?;
        let signed = json.to_statement().map_err(|error| StoreError::Corrupt {
            id: id.to_string(),
            reason: CorruptReason::Parse(error.to_string()),
        })?;
        let derived = signed.statement_id();
        if &derived != id {
            return Err(StoreError::Corrupt {
                id: id.to_string(),
                reason: CorruptReason::HashMismatch {
                    expected: id.to_string(),
                    actual: derived.to_string(),
                },
            });
        }
        Ok(signed)
    }

    fn put_actor_trust(
        &self,
        statement: &SignedStatement<ActorTrustBody>,
    ) -> Result<StatementId, StoreError> {
        let id = statement.statement_id();
        let json = ActorTrustStatementJson::from_statement(statement);
        let bytes = serde_json::to_vec_pretty(&json).map_err(json_to_corrupt(&id))?;
        let path = self.shard_path(STATEMENTS_DIR, id.as_str(), JSON_SUFFIX)?;
        atomic_write(&path, &bytes)?;

        self.index_actor_trust(statement, &id)?;

        Ok(id)
    }

    fn get_actor_trust(
        &self,
        id: &StatementId,
    ) -> Result<SignedStatement<ActorTrustBody>, StoreError> {
        let path = self.shard_path(STATEMENTS_DIR, id.as_str(), JSON_SUFFIX)?;
        let bytes = read_or_missing(&path)?;
        let json: ActorTrustStatementJson =
            serde_json::from_slice(&bytes).map_err(json_to_corrupt(id))?;
        let signed = json.to_statement().map_err(|error| StoreError::Corrupt {
            id: id.to_string(),
            reason: CorruptReason::Parse(error.to_string()),
        })?;
        let derived = signed.statement_id();
        if &derived != id {
            return Err(StoreError::Corrupt {
                id: id.to_string(),
                reason: CorruptReason::HashMismatch {
                    expected: id.to_string(),
                    actual: derived.to_string(),
                },
            });
        }
        Ok(signed)
    }

    fn put_actor_capability_grant(
        &self,
        statement: &SignedStatement<ActorCapabilityGrantBody>,
    ) -> Result<StatementId, StoreError> {
        let id = statement.statement_id();
        let json = ActorCapabilityGrantStatementJson::from_statement(statement);
        let bytes = serde_json::to_vec_pretty(&json).map_err(json_to_corrupt(&id))?;
        let path = self.shard_path(STATEMENTS_DIR, id.as_str(), JSON_SUFFIX)?;
        atomic_write(&path, &bytes)?;

        self.index_actor_capability_grant(statement, &id)?;

        Ok(id)
    }

    fn get_actor_capability_grant(
        &self,
        id: &StatementId,
    ) -> Result<SignedStatement<ActorCapabilityGrantBody>, StoreError> {
        let path = self.shard_path(STATEMENTS_DIR, id.as_str(), JSON_SUFFIX)?;
        let bytes = read_or_missing(&path)?;
        let json: ActorCapabilityGrantStatementJson =
            serde_json::from_slice(&bytes).map_err(json_to_corrupt(id))?;
        let signed = json.to_statement().map_err(|error| StoreError::Corrupt {
            id: id.to_string(),
            reason: CorruptReason::Parse(error.to_string()),
        })?;
        let derived = signed.statement_id();
        if &derived != id {
            return Err(StoreError::Corrupt {
                id: id.to_string(),
                reason: CorruptReason::HashMismatch {
                    expected: id.to_string(),
                    actual: derived.to_string(),
                },
            });
        }
        Ok(signed)
    }

    fn put_actor_capability_revocation(
        &self,
        statement: &SignedStatement<ActorCapabilityRevocationBody>,
    ) -> Result<StatementId, StoreError> {
        let id = statement.statement_id();
        let json = ActorCapabilityRevocationStatementJson::from_statement(statement);
        let bytes = serde_json::to_vec_pretty(&json).map_err(json_to_corrupt(&id))?;
        let path = self.shard_path(STATEMENTS_DIR, id.as_str(), JSON_SUFFIX)?;
        atomic_write(&path, &bytes)?;

        self.index_actor_capability_revocation(statement, &id)?;

        Ok(id)
    }

    fn get_actor_capability_revocation(
        &self,
        id: &StatementId,
    ) -> Result<SignedStatement<ActorCapabilityRevocationBody>, StoreError> {
        let path = self.shard_path(STATEMENTS_DIR, id.as_str(), JSON_SUFFIX)?;
        let bytes = read_or_missing(&path)?;
        let json: ActorCapabilityRevocationStatementJson =
            serde_json::from_slice(&bytes).map_err(json_to_corrupt(id))?;
        let signed = json.to_statement().map_err(|error| StoreError::Corrupt {
            id: id.to_string(),
            reason: CorruptReason::Parse(error.to_string()),
        })?;
        let derived = signed.statement_id();
        if &derived != id {
            return Err(StoreError::Corrupt {
                id: id.to_string(),
                reason: CorruptReason::HashMismatch {
                    expected: id.to_string(),
                    actual: derived.to_string(),
                },
            });
        }
        Ok(signed)
    }

    fn put_actor_key_rotation(
        &self,
        statement: &SignedStatement<ActorKeyRotationBody>,
    ) -> Result<StatementId, StoreError> {
        let id = statement.statement_id();
        let json = ActorKeyRotationStatementJson::from_statement(statement);
        let bytes = serde_json::to_vec_pretty(&json).map_err(json_to_corrupt(&id))?;
        let path = self.shard_path(STATEMENTS_DIR, id.as_str(), JSON_SUFFIX)?;
        atomic_write(&path, &bytes)?;

        self.index_actor_key_rotation(statement, &id)?;

        Ok(id)
    }

    fn get_actor_key_rotation(
        &self,
        id: &StatementId,
    ) -> Result<SignedStatement<ActorKeyRotationBody>, StoreError> {
        let path = self.shard_path(STATEMENTS_DIR, id.as_str(), JSON_SUFFIX)?;
        let bytes = read_or_missing(&path)?;
        let json: ActorKeyRotationStatementJson =
            serde_json::from_slice(&bytes).map_err(json_to_corrupt(id))?;
        let signed = json.to_statement().map_err(|error| StoreError::Corrupt {
            id: id.to_string(),
            reason: CorruptReason::Parse(error.to_string()),
        })?;
        let derived = signed.statement_id();
        if &derived != id {
            return Err(StoreError::Corrupt {
                id: id.to_string(),
                reason: CorruptReason::HashMismatch {
                    expected: id.to_string(),
                    actual: derived.to_string(),
                },
            });
        }
        Ok(signed)
    }

    fn put_actor_key_revocation(
        &self,
        statement: &SignedStatement<ActorKeyRevocationBody>,
    ) -> Result<StatementId, StoreError> {
        let id = statement.statement_id();
        let json = ActorKeyRevocationStatementJson::from_statement(statement);
        let bytes = serde_json::to_vec_pretty(&json).map_err(json_to_corrupt(&id))?;
        let path = self.shard_path(STATEMENTS_DIR, id.as_str(), JSON_SUFFIX)?;
        atomic_write(&path, &bytes)?;

        self.index_actor_key_revocation(statement, &id)?;

        Ok(id)
    }

    fn get_actor_key_revocation(
        &self,
        id: &StatementId,
    ) -> Result<SignedStatement<ActorKeyRevocationBody>, StoreError> {
        let path = self.shard_path(STATEMENTS_DIR, id.as_str(), JSON_SUFFIX)?;
        let bytes = read_or_missing(&path)?;
        let json: ActorKeyRevocationStatementJson =
            serde_json::from_slice(&bytes).map_err(json_to_corrupt(id))?;
        let signed = json.to_statement().map_err(|error| StoreError::Corrupt {
            id: id.to_string(),
            reason: CorruptReason::Parse(error.to_string()),
        })?;
        let derived = signed.statement_id();
        if &derived != id {
            return Err(StoreError::Corrupt {
                id: id.to_string(),
                reason: CorruptReason::HashMismatch {
                    expected: id.to_string(),
                    actual: derived.to_string(),
                },
            });
        }
        Ok(signed)
    }

    fn put_actor_emergency_key_rotation(
        &self,
        statement: &MultiSignedStatement<ActorEmergencyKeyRotationBody>,
    ) -> Result<StatementId, StoreError> {
        let id = statement.statement_id();
        let json = ActorEmergencyKeyRotationStatementJson::from_statement(statement);
        let bytes = serde_json::to_vec_pretty(&json).map_err(json_to_corrupt(&id))?;
        let path = self.shard_path(STATEMENTS_DIR, id.as_str(), JSON_SUFFIX)?;
        atomic_write(&path, &bytes)?;

        self.index_actor_emergency_key_rotation(statement, &id)?;

        Ok(id)
    }

    fn get_actor_emergency_key_rotation(
        &self,
        id: &StatementId,
    ) -> Result<MultiSignedStatement<ActorEmergencyKeyRotationBody>, StoreError> {
        let path = self.shard_path(STATEMENTS_DIR, id.as_str(), JSON_SUFFIX)?;
        let bytes = read_or_missing(&path)?;
        let json: ActorEmergencyKeyRotationStatementJson =
            serde_json::from_slice(&bytes).map_err(json_to_corrupt(id))?;
        let signed = json.to_statement().map_err(|error| StoreError::Corrupt {
            id: id.to_string(),
            reason: CorruptReason::Parse(error.to_string()),
        })?;
        let derived = signed.statement_id();
        if &derived != id {
            return Err(StoreError::Corrupt {
                id: id.to_string(),
                reason: CorruptReason::HashMismatch {
                    expected: id.to_string(),
                    actual: derived.to_string(),
                },
            });
        }
        Ok(signed)
    }

    fn put_actor_emergency_key_revocation(
        &self,
        statement: &MultiSignedStatement<ActorEmergencyKeyRevocationBody>,
    ) -> Result<StatementId, StoreError> {
        let id = statement.statement_id();
        let json = ActorEmergencyKeyRevocationStatementJson::from_statement(statement);
        let bytes = serde_json::to_vec_pretty(&json).map_err(json_to_corrupt(&id))?;
        let path = self.shard_path(STATEMENTS_DIR, id.as_str(), JSON_SUFFIX)?;
        atomic_write(&path, &bytes)?;

        self.index_actor_emergency_key_revocation(statement, &id)?;

        Ok(id)
    }

    fn get_actor_emergency_key_revocation(
        &self,
        id: &StatementId,
    ) -> Result<MultiSignedStatement<ActorEmergencyKeyRevocationBody>, StoreError> {
        let path = self.shard_path(STATEMENTS_DIR, id.as_str(), JSON_SUFFIX)?;
        let bytes = read_or_missing(&path)?;
        let json: ActorEmergencyKeyRevocationStatementJson =
            serde_json::from_slice(&bytes).map_err(json_to_corrupt(id))?;
        let signed = json.to_statement().map_err(|error| StoreError::Corrupt {
            id: id.to_string(),
            reason: CorruptReason::Parse(error.to_string()),
        })?;
        let derived = signed.statement_id();
        if &derived != id {
            return Err(StoreError::Corrupt {
                id: id.to_string(),
                reason: CorruptReason::HashMismatch {
                    expected: id.to_string(),
                    actual: derived.to_string(),
                },
            });
        }
        Ok(signed)
    }

    fn put_actor_attestation_key_add(
        &self,
        statement: &MultiSignedStatement<ActorAttestationKeyAddBody>,
    ) -> Result<StatementId, StoreError> {
        let id = statement.statement_id();
        let json = ActorAttestationKeyAddStatementJson::from_statement(statement);
        let bytes = serde_json::to_vec_pretty(&json).map_err(json_to_corrupt(&id))?;
        let path = self.shard_path(STATEMENTS_DIR, id.as_str(), JSON_SUFFIX)?;
        atomic_write(&path, &bytes)?;

        self.index_actor_attestation_key_add(statement, &id)?;

        Ok(id)
    }

    fn get_actor_attestation_key_add(
        &self,
        id: &StatementId,
    ) -> Result<MultiSignedStatement<ActorAttestationKeyAddBody>, StoreError> {
        let path = self.shard_path(STATEMENTS_DIR, id.as_str(), JSON_SUFFIX)?;
        let bytes = read_or_missing(&path)?;
        let json: ActorAttestationKeyAddStatementJson =
            serde_json::from_slice(&bytes).map_err(json_to_corrupt(id))?;
        let signed = json.to_statement().map_err(|error| StoreError::Corrupt {
            id: id.to_string(),
            reason: CorruptReason::Parse(error.to_string()),
        })?;
        let derived = signed.statement_id();
        if &derived != id {
            return Err(StoreError::Corrupt {
                id: id.to_string(),
                reason: CorruptReason::HashMismatch {
                    expected: id.to_string(),
                    actual: derived.to_string(),
                },
            });
        }
        Ok(signed)
    }

    fn put_actor_attestation_key_revocation(
        &self,
        statement: &MultiSignedStatement<ActorAttestationKeyRevocationBody>,
    ) -> Result<StatementId, StoreError> {
        let id = statement.statement_id();
        let actor = statement.unsigned().actor();
        let body = statement.unsigned().body();
        let created_at = statement.unsigned().created_at();

        // Non-empty-set guard (§5.5.2). Compute what the attestation
        // set will look like at `created_at` after this revocation
        // lands; if it would be empty, refuse to persist. The genesis
        // is the source of truth for the initial set; existing index
        // entries (adds, prior revocations) compose the rest.
        let genesis = match self.get_actor(actor) {
            Ok(genesis) => genesis,
            Err(StoreError::Missing) => {
                return Err(StoreError::Rejected {
                    reason: format!(
                        "actor {actor} has no genesis; cannot apply attestation revocation",
                    ),
                });
            }
            Err(error) => return Err(error),
        };
        let mut projected: std::collections::BTreeSet<kairo_identity::KeyId> =
            std::collections::BTreeSet::new();
        for key in genesis.attestation_keys() {
            projected.insert(key.key_id());
        }
        let index = self.read_key_index_or_default(actor)?;
        for entry in index.decode_attestation_adds(actor.as_str())? {
            if entry.created_at <= created_at {
                projected.insert(entry.new_key.key_id());
            }
        }
        for entry in index.decode_attestation_revocations()? {
            if entry.created_at <= created_at {
                projected.remove(&entry.revoked_key);
            }
        }
        // If the revoked key isn't in the projected set, the revocation
        // is a redundant no-op (per the spec). It does not shrink the
        // set, so it cannot empty it; allow it to persist.
        if projected.contains(body.revoked_key()) {
            projected.remove(body.revoked_key());
            if projected.is_empty() {
                return Err(StoreError::Rejected {
                    reason: format!(
                        "ActorAttestationKeyRevocation for {} would empty {actor}'s attestation key set; \
                         add a replacement attestation key first (ACTORS.md §5.5.2)",
                        body.revoked_key()
                    ),
                });
            }
        }

        let json = ActorAttestationKeyRevocationStatementJson::from_statement(statement);
        let bytes = serde_json::to_vec_pretty(&json).map_err(json_to_corrupt(&id))?;
        let path = self.shard_path(STATEMENTS_DIR, id.as_str(), JSON_SUFFIX)?;
        atomic_write(&path, &bytes)?;

        self.index_actor_attestation_key_revocation(statement, &id)?;

        Ok(id)
    }

    fn get_actor_attestation_key_revocation(
        &self,
        id: &StatementId,
    ) -> Result<MultiSignedStatement<ActorAttestationKeyRevocationBody>, StoreError> {
        let path = self.shard_path(STATEMENTS_DIR, id.as_str(), JSON_SUFFIX)?;
        let bytes = read_or_missing(&path)?;
        let json: ActorAttestationKeyRevocationStatementJson =
            serde_json::from_slice(&bytes).map_err(json_to_corrupt(id))?;
        let signed = json.to_statement().map_err(|error| StoreError::Corrupt {
            id: id.to_string(),
            reason: CorruptReason::Parse(error.to_string()),
        })?;
        let derived = signed.statement_id();
        if &derived != id {
            return Err(StoreError::Corrupt {
                id: id.to_string(),
                reason: CorruptReason::HashMismatch {
                    expected: id.to_string(),
                    actual: derived.to_string(),
                },
            });
        }
        Ok(signed)
    }

    fn put_actor_attestation_threshold_change(
        &self,
        statement: &MultiSignedStatement<ActorAttestationThresholdChangeBody>,
    ) -> Result<StatementId, StoreError> {
        let id = statement.statement_id();
        let actor = statement.unsigned().actor();
        let body = statement.unsigned().body();
        let created_at = statement.unsigned().created_at();
        let new_threshold = body.new_threshold();

        // Asymmetric authority rule (§5.5.3). Resolve the live
        // attestation set and threshold at `created_at`; raises require
        // `max(current, new)` distinct signatures from set members,
        // lowers/equal require `current`. Attestation-set membership is
        // checked here; signature *validity* is the verifier's job.
        let current_threshold = self.attestation_threshold_at(actor, created_at)?;
        let required = if new_threshold > current_threshold {
            new_threshold
        } else {
            current_threshold
        };
        let attestation_set = self.attestation_keys_at_internal(actor, created_at)?;
        let mut distinct_signers: std::collections::BTreeSet<String> =
            std::collections::BTreeSet::new();
        for signature in statement.signatures() {
            let key_id = kairo_identity::KeyId::new(signature.key_id().to_owned());
            if attestation_set.contains(&key_id) {
                distinct_signers.insert(signature.key_id().to_owned());
            }
        }
        // Threshold and signer counts are bounded by the attestation
        // set size, which is small (single-digit in practice). Saturate
        // explicitly so the comparison below remains well-defined even
        // if a future caller hands us a pathological set.
        let provided = u8::try_from(distinct_signers.len()).unwrap_or(u8::MAX);
        if provided < required {
            return Err(StoreError::Rejected {
                reason: format!(
                    "ActorAttestationThresholdChange for {actor} carries {provided} attestation-set \
                     signature(s); required {required} (current threshold {current_threshold}, \
                     new threshold {new_threshold}; ACTORS.md §5.5.3)",
                ),
            });
        }
        // Sanity guard: do not allow lowering a threshold below 1 or
        // raising it above the projected attestation-set size at
        // `created_at`. The latter would brick future emergency events
        // by definition.
        if new_threshold < 1 {
            return Err(StoreError::Rejected {
                reason: format!(
                    "ActorAttestationThresholdChange new_threshold must be >= 1 (got {new_threshold})",
                ),
            });
        }
        let projected_set_size = u8::try_from(attestation_set.len()).unwrap_or(u8::MAX);
        if new_threshold > projected_set_size {
            return Err(StoreError::Rejected {
                reason: format!(
                    "ActorAttestationThresholdChange new_threshold {new_threshold} exceeds \
                     attestation set size {projected_set_size} at this causal position; add more \
                     attestation keys first (ACTORS.md §5.5.3)",
                ),
            });
        }

        let json = ActorAttestationThresholdChangeStatementJson::from_statement(statement);
        let bytes = serde_json::to_vec_pretty(&json).map_err(json_to_corrupt(&id))?;
        let path = self.shard_path(STATEMENTS_DIR, id.as_str(), JSON_SUFFIX)?;
        atomic_write(&path, &bytes)?;

        self.index_actor_attestation_threshold_change(statement, &id)?;

        Ok(id)
    }

    fn get_actor_attestation_threshold_change(
        &self,
        id: &StatementId,
    ) -> Result<MultiSignedStatement<ActorAttestationThresholdChangeBody>, StoreError> {
        let path = self.shard_path(STATEMENTS_DIR, id.as_str(), JSON_SUFFIX)?;
        let bytes = read_or_missing(&path)?;
        let json: ActorAttestationThresholdChangeStatementJson =
            serde_json::from_slice(&bytes).map_err(json_to_corrupt(id))?;
        let signed = json.to_statement().map_err(|error| StoreError::Corrupt {
            id: id.to_string(),
            reason: CorruptReason::Parse(error.to_string()),
        })?;
        let derived = signed.statement_id();
        if &derived != id {
            return Err(StoreError::Corrupt {
                id: id.to_string(),
                reason: CorruptReason::HashMismatch {
                    expected: id.to_string(),
                    actual: derived.to_string(),
                },
            });
        }
        Ok(signed)
    }
}

impl FilesystemStore {
    // ---------------------------------------------------------------------
    // Per-kind index update helpers.
    //
    // Each `index_*` method takes a parsed statement that has already
    // passed validation and been written to `statements/`, and updates
    // every materialized index that kind needs. The corresponding
    // `put_*` method validates, writes the statement file, and then
    // calls `index_*`. The single entry-point shape lets
    // `rebuild_indexes()` reuse the same dispatch — wipe the index
    // dirs, walk `statements/`, and call `index_*` on each parsed
    // envelope. Without this split, rebuild would have to duplicate
    // the put-time index bookkeeping and the two would silently drift.
    //
    // Validation lives in `put_*` only — rebuild treats whatever made
    // it onto disk as canonical history. If a rule needs to re-check
    // on rebuild (e.g. a future schema-version migration), add a
    // separate replay validator rather than wiring it through here.

    fn index_object_revision(
        &self,
        statement: &SignedStatement<ObjectRevisionBody>,
        id: &StatementId,
    ) -> Result<(), StoreError> {
        let actor = statement.unsigned().actor();
        let created_at = statement.unsigned().created_at();
        self.upsert_statement_by_actor_index(actor, id, StatementKind::ObjectRevision, created_at)
    }

    fn index_object_branch(
        &self,
        statement: &SignedStatement<ObjectBranchBody>,
        id: &StatementId,
    ) -> Result<(), StoreError> {
        let actor = statement.unsigned().actor();
        let body = statement.unsigned().body();
        let created_at = statement.unsigned().created_at();
        self.upsert_branch_index(
            body.object(),
            actor,
            body.name(),
            id,
            created_at,
            body.supersedes(),
        )?;
        self.upsert_statement_by_actor_index(actor, id, StatementKind::ObjectBranch, created_at)
    }

    fn index_object_version_tag(
        &self,
        statement: &SignedStatement<ObjectVersionTagBody>,
        id: &StatementId,
    ) -> Result<(), StoreError> {
        let actor = statement.unsigned().actor();
        let body = statement.unsigned().body();
        let created_at = statement.unsigned().created_at();
        self.upsert_version_tag_index(
            body.object(),
            actor,
            body.version().as_str(),
            id,
            created_at,
            body.supersedes(),
        )?;
        self.upsert_statement_by_actor_index(actor, id, StatementKind::ObjectVersionTag, created_at)
    }

    fn index_actor_trust(
        &self,
        statement: &SignedStatement<ActorTrustBody>,
        id: &StatementId,
    ) -> Result<(), StoreError> {
        let by_actor = statement.unsigned().actor();
        let body = statement.unsigned().body();
        let created_at = statement.unsigned().created_at();
        self.upsert_trust_index(
            body.trusted_actor(),
            by_actor,
            id,
            created_at,
            body.decision().map(|d| d.as_str()),
            body.supersedes(),
        )?;
        self.upsert_statement_by_actor_index(by_actor, id, StatementKind::ActorTrust, created_at)
    }

    fn index_actor_capability_grant(
        &self,
        statement: &SignedStatement<ActorCapabilityGrantBody>,
        id: &StatementId,
    ) -> Result<(), StoreError> {
        let grantor = statement.unsigned().actor();
        let body = statement.unsigned().body();
        let grantee = body.grantee();
        let scope = body.capability().scope();
        let created_at = statement.unsigned().created_at();
        let supersedes = body.supersedes();
        self.upsert_capability_grant_index(grantor, grantee, scope, id, created_at, supersedes)?;
        // Object-scoped grants also land in the cross-cutting reverse
        // index. Actor-scoped grants are skipped — see
        // `capabilities_by_object.rs` doc and `specs/CAPABILITIES.md`
        // §4.3 (no statement kinds are valid for actor scope in v1).
        if let CapabilityScope::Object(object) = scope {
            self.upsert_capability_by_object_index(
                object, grantee, grantor, id, created_at, supersedes,
            )?;
        }
        self.upsert_statement_by_actor_index(
            grantor,
            id,
            StatementKind::ActorCapabilityGrant,
            created_at,
        )
    }

    fn index_actor_capability_revocation(
        &self,
        statement: &SignedStatement<ActorCapabilityRevocationBody>,
        id: &StatementId,
    ) -> Result<(), StoreError> {
        let grantor = statement.unsigned().actor();
        let body = statement.unsigned().body();
        let created_at = statement.unsigned().created_at();
        self.upsert_capability_revocation_index(
            grantor,
            body.revoked_grant(),
            id,
            created_at,
            body.retroactive(),
        )?;
        self.upsert_statement_by_actor_index(
            grantor,
            id,
            StatementKind::ActorCapabilityRevocation,
            created_at,
        )
    }

    fn index_actor_key_rotation(
        &self,
        statement: &SignedStatement<ActorKeyRotationBody>,
        id: &StatementId,
    ) -> Result<(), StoreError> {
        let actor = statement.unsigned().actor();
        let body = statement.unsigned().body();
        let created_at = statement.unsigned().created_at();
        self.upsert_key_rotation_index(
            actor,
            id,
            body.next_key(),
            created_at,
            body.supersedes(),
            KeySurface::Operational,
        )?;
        self.upsert_statement_by_actor_index(actor, id, StatementKind::ActorKeyRotation, created_at)
    }

    fn index_actor_key_revocation(
        &self,
        statement: &SignedStatement<ActorKeyRevocationBody>,
        id: &StatementId,
    ) -> Result<(), StoreError> {
        let actor = statement.unsigned().actor();
        let body = statement.unsigned().body();
        let created_at = statement.unsigned().created_at();
        self.upsert_key_revocation_index(
            actor,
            id,
            body.revoked_key(),
            body.retroactive(),
            created_at,
            KeySurface::Operational,
        )?;
        self.upsert_statement_by_actor_index(
            actor,
            id,
            StatementKind::ActorKeyRevocation,
            created_at,
        )
    }

    fn index_actor_emergency_key_rotation(
        &self,
        statement: &MultiSignedStatement<ActorEmergencyKeyRotationBody>,
        id: &StatementId,
    ) -> Result<(), StoreError> {
        let actor = statement.unsigned().actor();
        let body = statement.unsigned().body();
        let created_at = statement.unsigned().created_at();
        self.upsert_key_rotation_index(
            actor,
            id,
            body.next_key(),
            created_at,
            body.supersedes(),
            KeySurface::Attestation,
        )?;
        self.upsert_statement_by_actor_index(
            actor,
            id,
            StatementKind::ActorEmergencyKeyRotation,
            created_at,
        )
    }

    fn index_actor_emergency_key_revocation(
        &self,
        statement: &MultiSignedStatement<ActorEmergencyKeyRevocationBody>,
        id: &StatementId,
    ) -> Result<(), StoreError> {
        let actor = statement.unsigned().actor();
        let body = statement.unsigned().body();
        let created_at = statement.unsigned().created_at();
        self.upsert_key_revocation_index(
            actor,
            id,
            body.revoked_key(),
            body.retroactive(),
            created_at,
            KeySurface::Attestation,
        )?;
        self.upsert_statement_by_actor_index(
            actor,
            id,
            StatementKind::ActorEmergencyKeyRevocation,
            created_at,
        )
    }

    fn index_actor_attestation_key_add(
        &self,
        statement: &MultiSignedStatement<ActorAttestationKeyAddBody>,
        id: &StatementId,
    ) -> Result<(), StoreError> {
        let actor = statement.unsigned().actor();
        let body = statement.unsigned().body();
        let created_at = statement.unsigned().created_at();
        self.upsert_attestation_add_index(actor, id, body.new_key(), created_at)?;
        self.upsert_statement_by_actor_index(
            actor,
            id,
            StatementKind::ActorAttestationKeyAdd,
            created_at,
        )
    }

    fn index_actor_attestation_key_revocation(
        &self,
        statement: &MultiSignedStatement<ActorAttestationKeyRevocationBody>,
        id: &StatementId,
    ) -> Result<(), StoreError> {
        let actor = statement.unsigned().actor();
        let body = statement.unsigned().body();
        let created_at = statement.unsigned().created_at();
        self.upsert_attestation_revocation_index(actor, id, body.revoked_key(), created_at)?;
        self.upsert_statement_by_actor_index(
            actor,
            id,
            StatementKind::ActorAttestationKeyRevocation,
            created_at,
        )
    }

    fn index_actor_attestation_threshold_change(
        &self,
        statement: &MultiSignedStatement<ActorAttestationThresholdChangeBody>,
        id: &StatementId,
    ) -> Result<(), StoreError> {
        let actor = statement.unsigned().actor();
        let body = statement.unsigned().body();
        let created_at = statement.unsigned().created_at();
        self.upsert_attestation_threshold_change_index(
            actor,
            id,
            body.new_threshold(),
            created_at,
        )?;
        self.upsert_statement_by_actor_index(
            actor,
            id,
            StatementKind::ActorAttestationThresholdChange,
            created_at,
        )
    }

    fn upsert_branch_index(
        &self,
        object: &ObjectId,
        actor: &ActorId,
        name: &str,
        statement_id: &StatementId,
        created_at: kairo_core::Timestamp,
        supersedes: Option<&StatementId>,
    ) -> Result<(), StoreError> {
        let path = self.shard_path(BRANCHES_DIR, object.as_str(), JSON_SUFFIX)?;
        lock::with_index_lock(&path, || {
            let mut index = match fs::read(&path) {
                Ok(bytes) => serde_json::from_slice::<branches::BranchIndexFile>(&bytes).map_err(
                    |error| StoreError::Corrupt {
                        id: object.to_string(),
                        reason: CorruptReason::Parse(format!("invalid branch index: {error}")),
                    },
                )?,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    branches::BranchIndexFile::default()
                }
                Err(error) => return Err(StoreError::Unavailable(error)),
            };

            let updated = index.upsert(actor, name, statement_id, created_at, supersedes);
            if updated {
                let bytes = serde_json::to_vec_pretty(&index).map_err(json_to_corrupt(object))?;
                atomic_write(&path, &bytes)?;
            }
            Ok(())
        })
    }

    fn read_branch_index(
        &self,
        object: &ObjectId,
    ) -> Result<Option<branches::BranchIndexFile>, StoreError> {
        let path = self.shard_path(BRANCHES_DIR, object.as_str(), JSON_SUFFIX)?;
        match fs::read(&path) {
            Ok(bytes) => {
                let index: branches::BranchIndexFile =
                    serde_json::from_slice(&bytes).map_err(|error| StoreError::Corrupt {
                        id: object.to_string(),
                        reason: CorruptReason::Parse(format!("invalid branch index: {error}")),
                    })?;
                Ok(Some(index))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(StoreError::Unavailable(error)),
        }
    }

    fn upsert_version_tag_index(
        &self,
        object: &ObjectId,
        actor: &ActorId,
        version: &str,
        statement_id: &StatementId,
        created_at: kairo_core::Timestamp,
        supersedes: Option<&StatementId>,
    ) -> Result<(), StoreError> {
        let path = self.shard_path(VERSION_TAGS_DIR, object.as_str(), JSON_SUFFIX)?;
        lock::with_index_lock(&path, || {
            let mut index = match fs::read(&path) {
                Ok(bytes) => serde_json::from_slice::<tags::VersionTagIndexFile>(&bytes).map_err(
                    |error| StoreError::Corrupt {
                        id: object.to_string(),
                        reason: CorruptReason::Parse(format!("invalid version tag index: {error}")),
                    },
                )?,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    tags::VersionTagIndexFile::default()
                }
                Err(error) => return Err(StoreError::Unavailable(error)),
            };

            let updated = index.upsert(actor, version, statement_id, created_at, supersedes);
            if updated {
                let bytes = serde_json::to_vec_pretty(&index).map_err(json_to_corrupt(object))?;
                atomic_write(&path, &bytes)?;
            }
            Ok(())
        })
    }

    fn read_version_tag_index(
        &self,
        object: &ObjectId,
    ) -> Result<Option<tags::VersionTagIndexFile>, StoreError> {
        let path = self.shard_path(VERSION_TAGS_DIR, object.as_str(), JSON_SUFFIX)?;
        match fs::read(&path) {
            Ok(bytes) => {
                let index: tags::VersionTagIndexFile =
                    serde_json::from_slice(&bytes).map_err(|error| StoreError::Corrupt {
                        id: object.to_string(),
                        reason: CorruptReason::Parse(format!("invalid version tag index: {error}")),
                    })?;
                Ok(Some(index))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(StoreError::Unavailable(error)),
        }
    }

    fn upsert_statement_by_actor_index(
        &self,
        actor: &ActorId,
        statement_id: &StatementId,
        kind: StatementKind,
        created_at: kairo_core::Timestamp,
    ) -> Result<(), StoreError> {
        let path = self.shard_path(STATEMENTS_BY_ACTOR_DIR, actor.as_str(), JSON_SUFFIX)?;
        lock::with_index_lock(&path, || {
            let mut index = match fs::read(&path) {
                Ok(bytes) => {
                    serde_json::from_slice::<statements_by_actor::StatementByActorIndexFile>(&bytes)
                        .map_err(|error| StoreError::Corrupt {
                            id: actor.to_string(),
                            reason: CorruptReason::Parse(format!(
                                "invalid statements_by_actor index: {error}"
                            )),
                        })?
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    statements_by_actor::StatementByActorIndexFile::default()
                }
                Err(error) => return Err(StoreError::Unavailable(error)),
            };

            let updated = index.upsert(statement_id, kind, created_at);
            if updated {
                let bytes = serde_json::to_vec_pretty(&index).map_err(json_to_corrupt(actor))?;
                atomic_write(&path, &bytes)?;
            }
            Ok(())
        })
    }

    fn read_statement_by_actor_index(
        &self,
        actor: &ActorId,
    ) -> Result<Option<statements_by_actor::StatementByActorIndexFile>, StoreError> {
        let path = self.shard_path(STATEMENTS_BY_ACTOR_DIR, actor.as_str(), JSON_SUFFIX)?;
        match fs::read(&path) {
            Ok(bytes) => {
                let index: statements_by_actor::StatementByActorIndexFile =
                    serde_json::from_slice(&bytes).map_err(|error| StoreError::Corrupt {
                        id: actor.to_string(),
                        reason: CorruptReason::Parse(format!(
                            "invalid statements_by_actor index: {error}"
                        )),
                    })?;
                Ok(Some(index))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(StoreError::Unavailable(error)),
        }
    }

    fn upsert_trust_index(
        &self,
        trusted_actor: &ActorId,
        by_actor: &ActorId,
        statement_id: &StatementId,
        created_at: kairo_core::Timestamp,
        decision: Option<&str>,
        supersedes: Option<&StatementId>,
    ) -> Result<(), StoreError> {
        let path = self.shard_path(TRUST_DIR, trusted_actor.as_str(), JSON_SUFFIX)?;
        lock::with_index_lock(&path, || {
            let mut index =
                match fs::read(&path) {
                    Ok(bytes) => serde_json::from_slice::<trust::TrustIndexFile>(&bytes).map_err(
                        |error| StoreError::Corrupt {
                            id: trusted_actor.to_string(),
                            reason: CorruptReason::Parse(format!("invalid trust index: {error}")),
                        },
                    )?,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        trust::TrustIndexFile::default()
                    }
                    Err(error) => return Err(StoreError::Unavailable(error)),
                };

            let updated = index.upsert(by_actor, statement_id, created_at, decision, supersedes);
            if updated {
                let bytes =
                    serde_json::to_vec_pretty(&index).map_err(json_to_corrupt(trusted_actor))?;
                atomic_write(&path, &bytes)?;
            }
            Ok(())
        })
    }

    fn read_trust_index(
        &self,
        trusted_actor: &ActorId,
    ) -> Result<Option<trust::TrustIndexFile>, StoreError> {
        let path = self.shard_path(TRUST_DIR, trusted_actor.as_str(), JSON_SUFFIX)?;
        match fs::read(&path) {
            Ok(bytes) => {
                let index: trust::TrustIndexFile =
                    serde_json::from_slice(&bytes).map_err(|error| StoreError::Corrupt {
                        id: trusted_actor.to_string(),
                        reason: CorruptReason::Parse(format!("invalid trust index: {error}")),
                    })?;
                Ok(Some(index))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(StoreError::Unavailable(error)),
        }
    }

    fn upsert_key_rotation_index(
        &self,
        actor: &ActorId,
        statement_id: &StatementId,
        next_key: &kairo_identity::PublicKey,
        created_at: kairo_core::Timestamp,
        supersedes: Option<&StatementId>,
        surface: kairo_identity::KeySurface,
    ) -> Result<(), StoreError> {
        let path = self.shard_path(ACTOR_KEYS_DIR, actor.as_str(), JSON_SUFFIX)?;
        lock::with_index_lock(&path, || {
            let mut index = self.read_key_index_or_default(actor)?;
            let updated =
                index.upsert_rotation(statement_id, next_key, created_at, supersedes, surface);
            if updated {
                let bytes = serde_json::to_vec_pretty(&index).map_err(json_to_corrupt(actor))?;
                atomic_write(&path, &bytes)?;
            }
            Ok(())
        })
    }

    fn upsert_key_revocation_index(
        &self,
        actor: &ActorId,
        statement_id: &StatementId,
        revoked_key: &kairo_identity::KeyId,
        retroactive: bool,
        created_at: kairo_core::Timestamp,
        surface: kairo_identity::KeySurface,
    ) -> Result<(), StoreError> {
        let path = self.shard_path(ACTOR_KEYS_DIR, actor.as_str(), JSON_SUFFIX)?;
        lock::with_index_lock(&path, || {
            let mut index = self.read_key_index_or_default(actor)?;
            let updated = index.upsert_revocation(
                statement_id,
                revoked_key,
                retroactive,
                created_at,
                surface,
            );
            if updated {
                let bytes = serde_json::to_vec_pretty(&index).map_err(json_to_corrupt(actor))?;
                atomic_write(&path, &bytes)?;
            }
            Ok(())
        })
    }

    fn upsert_attestation_add_index(
        &self,
        actor: &ActorId,
        statement_id: &StatementId,
        new_key: &kairo_identity::PublicKey,
        created_at: kairo_core::Timestamp,
    ) -> Result<(), StoreError> {
        let path = self.shard_path(ACTOR_KEYS_DIR, actor.as_str(), JSON_SUFFIX)?;
        lock::with_index_lock(&path, || {
            let mut index = self.read_key_index_or_default(actor)?;
            let updated = index.upsert_attestation_add(statement_id, new_key, created_at);
            if updated {
                let bytes = serde_json::to_vec_pretty(&index).map_err(json_to_corrupt(actor))?;
                atomic_write(&path, &bytes)?;
            }
            Ok(())
        })
    }

    fn upsert_attestation_revocation_index(
        &self,
        actor: &ActorId,
        statement_id: &StatementId,
        revoked_key: &kairo_identity::KeyId,
        created_at: kairo_core::Timestamp,
    ) -> Result<(), StoreError> {
        let path = self.shard_path(ACTOR_KEYS_DIR, actor.as_str(), JSON_SUFFIX)?;
        lock::with_index_lock(&path, || {
            let mut index = self.read_key_index_or_default(actor)?;
            let updated =
                index.upsert_attestation_revocation(statement_id, revoked_key, created_at);
            if updated {
                let bytes = serde_json::to_vec_pretty(&index).map_err(json_to_corrupt(actor))?;
                atomic_write(&path, &bytes)?;
            }
            Ok(())
        })
    }

    fn upsert_attestation_threshold_change_index(
        &self,
        actor: &ActorId,
        statement_id: &StatementId,
        new_threshold: u8,
        created_at: kairo_core::Timestamp,
    ) -> Result<(), StoreError> {
        let path = self.shard_path(ACTOR_KEYS_DIR, actor.as_str(), JSON_SUFFIX)?;
        lock::with_index_lock(&path, || {
            let mut index = self.read_key_index_or_default(actor)?;
            let updated =
                index.upsert_attestation_threshold_change(statement_id, new_threshold, created_at);
            if updated {
                let bytes = serde_json::to_vec_pretty(&index).map_err(json_to_corrupt(actor))?;
                atomic_write(&path, &bytes)?;
            }
            Ok(())
        })
    }

    /// Resolve the actor's attestation threshold at `at` directly from
    /// the key-event index. Returns the genesis threshold when no
    /// change precedes `at`. Used by `put_actor_attestation_threshold_change`
    /// to enforce the asymmetric authority rule before the new entry
    /// is written.
    fn attestation_threshold_at(
        &self,
        actor: &ActorId,
        at: kairo_core::Timestamp,
    ) -> Result<u8, StoreError> {
        let genesis = self.get_actor(actor)?;
        let index = self.read_key_index_or_default(actor)?;
        let changes = index.decode_attestation_threshold_changes()?;
        let mut latest: Option<&kairo_identity::AttestationThresholdChangeEntry> = None;
        for entry in &changes {
            if entry.created_at > at {
                continue;
            }
            latest = Some(match latest {
                None => entry,
                Some(current)
                    if entry.created_at > current.created_at
                        || (entry.created_at == current.created_at
                            && entry.statement_id > current.statement_id) =>
                {
                    entry
                }
                Some(current) => current,
            });
        }
        Ok(match latest {
            Some(entry) => entry.new_threshold,
            None => genesis.attestation_threshold(),
        })
    }

    /// Materialize the actor's attestation key-id set at `at` from the
    /// key-event index plus the genesis. The set is used at put time
    /// to count distinct attestation-set signers on attestation-surface
    /// envelopes.
    fn attestation_keys_at_internal(
        &self,
        actor: &ActorId,
        at: kairo_core::Timestamp,
    ) -> Result<std::collections::BTreeSet<kairo_identity::KeyId>, StoreError> {
        let genesis = self.get_actor(actor)?;
        let mut set: std::collections::BTreeSet<kairo_identity::KeyId> =
            std::collections::BTreeSet::new();
        for key in genesis.attestation_keys() {
            set.insert(key.key_id());
        }
        let index = self.read_key_index_or_default(actor)?;
        for entry in index.decode_attestation_adds(actor.as_str())? {
            if entry.created_at <= at {
                set.insert(entry.new_key.key_id());
            }
        }
        for entry in index.decode_attestation_revocations()? {
            if entry.created_at <= at {
                set.remove(&entry.revoked_key);
            }
        }
        Ok(set)
    }

    fn read_key_index_or_default(
        &self,
        actor: &ActorId,
    ) -> Result<keys::KeyEventIndexFile, StoreError> {
        let path = self.shard_path(ACTOR_KEYS_DIR, actor.as_str(), JSON_SUFFIX)?;
        match fs::read(&path) {
            Ok(bytes) => {
                serde_json::from_slice::<keys::KeyEventIndexFile>(&bytes).map_err(|error| {
                    StoreError::Corrupt {
                        id: actor.to_string(),
                        reason: CorruptReason::Parse(format!("invalid key event index: {error}")),
                    }
                })
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Ok(keys::KeyEventIndexFile::default())
            }
            Err(error) => Err(StoreError::Unavailable(error)),
        }
    }

    fn upsert_capability_grant_index(
        &self,
        grantor: &ActorId,
        grantee: &ActorId,
        scope: &CapabilityScope,
        statement_id: &StatementId,
        created_at: kairo_core::Timestamp,
        supersedes: Option<&StatementId>,
    ) -> Result<(), StoreError> {
        let path = self.shard_path(ACTOR_CAPABILITY_DIR, grantor.as_str(), JSON_SUFFIX)?;
        lock::with_index_lock(&path, || {
            let mut index = self.read_capability_index_or_default(grantor)?;
            let updated = index.upsert_grant(grantee, scope, statement_id, created_at, supersedes);
            if updated {
                let bytes = serde_json::to_vec_pretty(&index).map_err(json_to_corrupt(grantor))?;
                atomic_write(&path, &bytes)?;
            }
            Ok(())
        })
    }

    fn upsert_capability_revocation_index(
        &self,
        grantor: &ActorId,
        revoked_grant: &StatementId,
        statement_id: &StatementId,
        created_at: kairo_core::Timestamp,
        retroactive: bool,
    ) -> Result<(), StoreError> {
        let path = self.shard_path(ACTOR_CAPABILITY_DIR, grantor.as_str(), JSON_SUFFIX)?;
        lock::with_index_lock(&path, || {
            let mut index = self.read_capability_index_or_default(grantor)?;
            let updated =
                index.upsert_revocation(revoked_grant, statement_id, created_at, retroactive);
            if updated {
                let bytes = serde_json::to_vec_pretty(&index).map_err(json_to_corrupt(grantor))?;
                atomic_write(&path, &bytes)?;
            }
            Ok(())
        })
    }

    fn read_capability_index_or_default(
        &self,
        grantor: &ActorId,
    ) -> Result<capabilities::CapabilityIndexFile, StoreError> {
        Ok(self.read_capability_index(grantor)?.unwrap_or_default())
    }

    fn read_capability_index(
        &self,
        grantor: &ActorId,
    ) -> Result<Option<capabilities::CapabilityIndexFile>, StoreError> {
        let path = self.shard_path(ACTOR_CAPABILITY_DIR, grantor.as_str(), JSON_SUFFIX)?;
        match fs::read(&path) {
            Ok(bytes) => {
                let index: capabilities::CapabilityIndexFile = serde_json::from_slice(&bytes)
                    .map_err(|error| StoreError::Corrupt {
                        id: grantor.to_string(),
                        reason: CorruptReason::Parse(format!("invalid capability index: {error}")),
                    })?;
                Ok(Some(index))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(StoreError::Unavailable(error)),
        }
    }

    fn upsert_capability_by_object_index(
        &self,
        object: &ObjectId,
        grantee: &ActorId,
        grantor: &ActorId,
        statement_id: &StatementId,
        created_at: kairo_core::Timestamp,
        supersedes: Option<&StatementId>,
    ) -> Result<(), StoreError> {
        let path = self.shard_path(ACTOR_CAPABILITY_BY_OBJECT_DIR, object.as_str(), JSON_SUFFIX)?;
        lock::with_index_lock(&path, || {
            let mut index = match fs::read(&path) {
                Ok(bytes) => serde_json::from_slice::<
                    capabilities_by_object::CapabilityByObjectIndexFile,
                >(&bytes)
                .map_err(|error| StoreError::Corrupt {
                    id: object.to_string(),
                    reason: CorruptReason::Parse(format!(
                        "invalid capabilities-by-object index: {error}"
                    )),
                })?,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    capabilities_by_object::CapabilityByObjectIndexFile::default()
                }
                Err(error) => return Err(StoreError::Unavailable(error)),
            };

            let updated = index.upsert(grantee, grantor, statement_id, created_at, supersedes);
            if updated {
                let bytes = serde_json::to_vec_pretty(&index).map_err(json_to_corrupt(object))?;
                atomic_write(&path, &bytes)?;
            }
            Ok(())
        })
    }

    fn read_capability_by_object_index(
        &self,
        object: &ObjectId,
    ) -> Result<Option<capabilities_by_object::CapabilityByObjectIndexFile>, StoreError> {
        let path = self.shard_path(ACTOR_CAPABILITY_BY_OBJECT_DIR, object.as_str(), JSON_SUFFIX)?;
        match fs::read(&path) {
            Ok(bytes) => {
                let index: capabilities_by_object::CapabilityByObjectIndexFile =
                    serde_json::from_slice(&bytes).map_err(|error| StoreError::Corrupt {
                        id: object.to_string(),
                        reason: CorruptReason::Parse(format!(
                            "invalid capabilities-by-object index: {error}"
                        )),
                    })?;
                Ok(Some(index))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(StoreError::Unavailable(error)),
        }
    }

    /// Walk the trust directory and apply `for_each` to each
    /// `(trusted_actor, index)` pair. Used by `list_trust(by_actor)`
    /// in the absence of a per-truster reverse index.
    fn walk_trust_indices(
        &self,
        mut for_each: impl FnMut(ActorId, trust::TrustIndexFile) -> Result<(), StoreError>,
    ) -> Result<(), StoreError> {
        let trust_root = self.root.join(TRUST_DIR);
        let level1 = match fs::read_dir(&trust_root) {
            Ok(iter) => iter,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(StoreError::Unavailable(error)),
        };
        for entry in level1 {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            for entry2 in fs::read_dir(entry.path())? {
                let entry2 = entry2?;
                if !entry2.file_type()?.is_dir() {
                    continue;
                }
                for entry3 in fs::read_dir(entry2.path())? {
                    let entry3 = entry3?;
                    let path = entry3.path();
                    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                        continue;
                    };
                    if path.extension().and_then(|s| s.to_str()) != Some("json") {
                        continue;
                    }
                    let bytes = fs::read(&path)?;
                    let index: trust::TrustIndexFile =
                        serde_json::from_slice(&bytes).map_err(|error| StoreError::Corrupt {
                            id: stem.to_string(),
                            reason: CorruptReason::Parse(format!("invalid trust index: {error}")),
                        })?;
                    let trusted_actor =
                        ActorId::new(stem.to_string()).map_err(|error| StoreError::Corrupt {
                            id: stem.to_string(),
                            reason: CorruptReason::Parse(format!(
                                "invalid trusted actor id in trust path: {error}"
                            )),
                        })?;
                    for_each(trusted_actor, index)?;
                }
            }
        }
        Ok(())
    }
}

impl BranchResolver for FilesystemStore {
    fn latest_branch(
        &self,
        actor: &ActorId,
        object: &ObjectId,
        name: &str,
    ) -> Result<Option<SignedStatement<ObjectBranchBody>>, StoreError> {
        let Some(index) = self.read_branch_index(object)? else {
            return Ok(None);
        };
        let Some(entry) = index.lookup_head(actor, name) else {
            return Ok(None);
        };

        // Per `specs/CAPABILITIES.md` §6.2: from the same-actor chain
        // leaf, follow authorized cross-actor supersedes edges
        // forward. Same-actor sup is automatic; cross-actor sup is
        // honored iff the successor's signer holds an `ObjectBranch`
        // capability on this object at the successor's `created_at`.
        let final_entry = self.walk_authorized_branch_chain(object, name, entry, &index)?;
        let statement_id = StatementId::new(final_entry.statement_id.clone()).map_err(|error| {
            StoreError::Corrupt {
                id: final_entry.statement_id.clone(),
                reason: CorruptReason::Parse(format!(
                    "invalid statement id in branch index: {error}"
                )),
            }
        })?;
        let signed = self.get_object_branch(&statement_id)?;

        // The walk may legitimately leave us pointing at a statement
        // signed by a different actor than the query — that's the
        // whole point of §6.2. Object and name must still match,
        // since they're the resolution key.
        if signed.unsigned().body().object() != object || signed.unsigned().body().name() != name {
            return Err(StoreError::Corrupt {
                id: statement_id.to_string(),
                reason: CorruptReason::Parse(
                    "branch index points at a statement with mismatched (object, name)".to_owned(),
                ),
            });
        }

        Ok(Some(signed))
    }

    fn list_branches(&self, object: &ObjectId) -> Result<Vec<BranchTip>, StoreError> {
        let Some(index) = self.read_branch_index(object)? else {
            return Ok(Vec::new());
        };
        // Per `(perspective_actor, name)`, walk same-actor leaf
        // forward through authorized cross-actor supersedes edges.
        // The returned tip's `actor` is the perspective actor; the
        // tip's `statement_id` may have been signed by an authorized
        // delegate.
        let mut tips: Vec<BranchTip> = Vec::new();
        for (actor_str, by_name) in &index.by_actor {
            let actor = ActorId::new(actor_str.clone()).map_err(|error| StoreError::Corrupt {
                id: actor_str.clone(),
                reason: CorruptReason::Parse(format!("invalid actor id in branch index: {error}")),
            })?;
            for name in by_name.keys() {
                let Some(entry) = index.lookup_head(&actor, name) else {
                    continue;
                };
                let final_entry = self.walk_authorized_branch_chain(object, name, entry, &index)?;
                let statement_id =
                    StatementId::new(final_entry.statement_id.clone()).map_err(|error| {
                        StoreError::Corrupt {
                            id: final_entry.statement_id.clone(),
                            reason: CorruptReason::Parse(format!(
                                "invalid statement id in branch index: {error}"
                            )),
                        }
                    })?;
                let created_at: Timestamp =
                    final_entry
                        .created_at
                        .parse()
                        .map_err(|error| StoreError::Corrupt {
                            id: final_entry.statement_id.clone(),
                            reason: CorruptReason::Parse(format!(
                                "invalid created_at in branch index: {error}"
                            )),
                        })?;
                tips.push(BranchTip {
                    actor: actor.clone(),
                    object: object.clone(),
                    name: name.clone(),
                    statement_id,
                    created_at,
                });
            }
        }
        Ok(tips)
    }
}

impl FilesystemStore {
    /// Walk the supersedes chain forward from `start` for the given
    /// `(object, name)`. Same-actor sups are honored automatically;
    /// cross-actor sups are honored iff the successor's signer holds
    /// an `ObjectBranch` capability on `object` at the successor's
    /// `created_at` (per `specs/CAPABILITIES.md` §6.2).
    ///
    /// Mirrors `walk_authorized_tag_chain` for `ObjectVersionTag`.
    /// The walk is bounded by total entry count for the branch name,
    /// so a malformed cycle in `supersedes` cannot loop forever.
    fn walk_authorized_branch_chain(
        &self,
        object: &ObjectId,
        name: &str,
        start: &branches::BranchEntry,
        index: &branches::BranchIndexFile,
    ) -> Result<branches::BranchEntry, StoreError> {
        let entries = index.entries_for_name(name);
        let max_steps = entries.len();
        let mut current_signer: Option<String> = entries
            .iter()
            .find(|(_, e)| e.statement_id == start.statement_id)
            .map(|(actor_str, _)| (*actor_str).to_string());
        let mut current = start.clone();

        for _ in 0..max_steps {
            let mut best: Option<(String, &branches::BranchEntry)> = None;
            for (actor_str, entry) in &entries {
                if entry.supersedes.as_deref() != Some(current.statement_id.as_str()) {
                    continue;
                }
                let is_same_actor = current_signer
                    .as_deref()
                    .is_some_and(|cur| cur == *actor_str);
                let authorized = if is_same_actor {
                    true
                } else {
                    let signer = ActorId::new(actor_str.to_string()).map_err(|error| {
                        StoreError::Corrupt {
                            id: actor_str.to_string(),
                            reason: CorruptReason::Parse(format!(
                                "invalid actor id in branch index: {error}"
                            )),
                        }
                    })?;
                    let entry_at: Timestamp =
                        entry
                            .created_at
                            .parse()
                            .map_err(|error| StoreError::Corrupt {
                                id: entry.statement_id.clone(),
                                reason: CorruptReason::Parse(format!(
                                    "invalid created_at in branch index: {error}"
                                )),
                            })?;
                    let evaluation = kairo_statement::verify::evaluate_capability(
                        &signer,
                        &kairo_statement::verify::ResolutionTarget::Object {
                            id: object.clone(),
                            kind: kairo_statement::StatementKind::ObjectBranch,
                        },
                        entry_at,
                        self,
                    )?;
                    matches!(
                        evaluation,
                        kairo_statement::verify::CapabilityEvaluation::Held
                    )
                };
                if !authorized {
                    continue;
                }
                let candidate = ((*actor_str).to_string(), *entry);
                match best {
                    None => best = Some(candidate),
                    Some((_, existing)) if branch_entry_greater_than_for_walk(entry, existing) => {
                        best = Some(candidate);
                    }
                    _ => {}
                }
            }
            match best {
                None => return Ok(current),
                Some((actor_str, entry)) => {
                    current_signer = Some(actor_str);
                    current = entry.clone();
                }
            }
        }
        Ok(current)
    }
}

fn branch_entry_greater_than_for_walk(
    candidate: &branches::BranchEntry,
    current: &branches::BranchEntry,
) -> bool {
    match (
        candidate.created_at.parse::<Timestamp>(),
        current.created_at.parse::<Timestamp>(),
    ) {
        (Ok(a), Ok(b)) => {
            if a > b {
                return true;
            }
            if a < b {
                return false;
            }
            candidate.statement_id > current.statement_id
        }
        _ => candidate.statement_id > current.statement_id,
    }
}

impl VersionTagResolver for FilesystemStore {
    fn latest_version_tag(
        &self,
        actor: &ActorId,
        object: &ObjectId,
        version: &str,
    ) -> Result<Option<SignedStatement<ObjectVersionTagBody>>, StoreError> {
        let Some(index) = self.read_version_tag_index(object)? else {
            return Ok(None);
        };
        let Some(entry) = index.lookup_head(actor, version) else {
            return Ok(None);
        };

        // Step 6 (`specs/CAPABILITIES.md` §6.2): from the same-actor
        // chain leaf, follow authorized cross-actor supersedes edges
        // forward. Same-actor sup is automatic; cross-actor sup is
        // honored iff the successor's signer holds an
        // ObjectVersionTag capability on this object at the
        // successor's `created_at`.
        let final_entry = self.walk_authorized_tag_chain(object, version, entry, &index)?;
        let statement_id = StatementId::new(final_entry.statement_id.clone()).map_err(|error| {
            StoreError::Corrupt {
                id: final_entry.statement_id.clone(),
                reason: CorruptReason::Parse(format!(
                    "invalid statement id in version tag index: {error}"
                )),
            }
        })?;
        let signed = self.get_object_version_tag(&statement_id)?;

        // The walk may legitimately leave us pointing at a statement
        // signed by a different actor than the query — that's the
        // whole point of §6.2. Object and version must still match,
        // since they're the resolution key.
        if signed.unsigned().body().object() != object
            || signed.unsigned().body().version().as_str() != version
        {
            return Err(StoreError::Corrupt {
                id: statement_id.to_string(),
                reason: CorruptReason::Parse(
                    "version tag index points at a statement with mismatched (object, version)"
                        .to_owned(),
                ),
            });
        }

        Ok(Some(signed))
    }

    fn list_version_tags(&self, object: &ObjectId) -> Result<Vec<VersionTagHead>, StoreError> {
        let Some(index) = self.read_version_tag_index(object)? else {
            return Ok(Vec::new());
        };
        // Per `(perspective_actor, version)`, walk same-actor leaf
        // forward through authorized cross-actor supersedes edges.
        // The returned head's `actor` field is the perspective actor;
        // the head's `statement_id` may have been signed by an
        // authorized delegate.
        let mut heads: Vec<VersionTagHead> = Vec::new();
        for (actor_str, by_version) in &index.by_actor {
            let actor = ActorId::new(actor_str.clone()).map_err(|error| StoreError::Corrupt {
                id: actor_str.clone(),
                reason: CorruptReason::Parse(format!(
                    "invalid actor id in version tag index: {error}"
                )),
            })?;
            for version in by_version.keys() {
                let Some(entry) = index.lookup_head(&actor, version) else {
                    continue;
                };
                let final_entry = self.walk_authorized_tag_chain(object, version, entry, &index)?;
                let statement_id =
                    StatementId::new(final_entry.statement_id.clone()).map_err(|error| {
                        StoreError::Corrupt {
                            id: final_entry.statement_id.clone(),
                            reason: CorruptReason::Parse(format!(
                                "invalid statement id in version tag index: {error}"
                            )),
                        }
                    })?;
                let created_at: Timestamp =
                    final_entry
                        .created_at
                        .parse()
                        .map_err(|error| StoreError::Corrupt {
                            id: final_entry.statement_id.clone(),
                            reason: CorruptReason::Parse(format!(
                                "invalid created_at in version tag index: {error}"
                            )),
                        })?;
                heads.push(VersionTagHead {
                    actor: actor.clone(),
                    object: object.clone(),
                    version: version.clone(),
                    statement_id,
                    created_at,
                });
            }
        }
        Ok(heads)
    }
}

impl FilesystemStore {
    /// Walk the supersedes chain forward from `start` for the given
    /// `(object, version)`. Same-actor sups are honored automatically;
    /// cross-actor sups are honored iff the successor's signer holds
    /// an `ObjectVersionTag` capability on `object` at the successor's
    /// `created_at` (per `specs/CAPABILITIES.md` §6.2).
    ///
    /// Returns the final leaf entry. The walk is bounded by the total
    /// entry count for the version, so a malformed cycle in
    /// `supersedes` cannot loop forever.
    fn walk_authorized_tag_chain(
        &self,
        object: &ObjectId,
        version: &str,
        start: &tags::VersionTagEntry,
        index: &tags::VersionTagIndexFile,
    ) -> Result<tags::VersionTagEntry, StoreError> {
        let entries = index.entries_for_version(version);
        let max_steps = entries.len();
        let mut current_signer: Option<String> = entries
            .iter()
            .find(|(_, e)| e.statement_id == start.statement_id)
            .map(|(actor_str, _)| (*actor_str).to_string());
        let mut current = start.clone();

        for _ in 0..max_steps {
            // Candidates: entries whose supersedes names current.
            let mut best: Option<(String, &tags::VersionTagEntry)> = None;
            for (actor_str, entry) in &entries {
                if entry.supersedes.as_deref() != Some(current.statement_id.as_str()) {
                    continue;
                }
                let is_same_actor = current_signer
                    .as_deref()
                    .is_some_and(|cur| cur == *actor_str);
                let authorized = if is_same_actor {
                    true
                } else {
                    let signer = ActorId::new(actor_str.to_string()).map_err(|error| {
                        StoreError::Corrupt {
                            id: actor_str.to_string(),
                            reason: CorruptReason::Parse(format!(
                                "invalid actor id in version tag index: {error}"
                            )),
                        }
                    })?;
                    let entry_at: Timestamp =
                        entry
                            .created_at
                            .parse()
                            .map_err(|error| StoreError::Corrupt {
                                id: entry.statement_id.clone(),
                                reason: CorruptReason::Parse(format!(
                                    "invalid created_at in version tag index: {error}"
                                )),
                            })?;
                    let evaluation = kairo_statement::verify::evaluate_capability(
                        &signer,
                        &kairo_statement::verify::ResolutionTarget::Object {
                            id: object.clone(),
                            kind: kairo_statement::StatementKind::ObjectVersionTag,
                        },
                        entry_at,
                        self,
                    )?;
                    matches!(
                        evaluation,
                        kairo_statement::verify::CapabilityEvaluation::Held
                    )
                };
                if !authorized {
                    continue;
                }
                let candidate = ((*actor_str).to_string(), *entry);
                match best {
                    None => best = Some(candidate),
                    Some((_, existing)) if entry_greater_than_for_walk(entry, existing) => {
                        best = Some(candidate);
                    }
                    _ => {}
                }
            }
            match best {
                None => return Ok(current),
                Some((actor_str, entry)) => {
                    current_signer = Some(actor_str);
                    current = entry.clone();
                }
            }
        }
        Ok(current)
    }
}

fn entry_greater_than_for_walk(
    candidate: &tags::VersionTagEntry,
    current: &tags::VersionTagEntry,
) -> bool {
    match (
        candidate.created_at.parse::<Timestamp>(),
        current.created_at.parse::<Timestamp>(),
    ) {
        (Ok(a), Ok(b)) => {
            if a > b {
                return true;
            }
            if a < b {
                return false;
            }
            candidate.statement_id > current.statement_id
        }
        _ => candidate.statement_id > current.statement_id,
    }
}

impl TrustResolver for FilesystemStore {
    fn latest_trust(
        &self,
        by_actor: &ActorId,
        trusted_actor: &ActorId,
    ) -> Result<Option<SignedStatement<ActorTrustBody>>, StoreError> {
        let Some(index) = self.read_trust_index(trusted_actor)? else {
            return Ok(None);
        };
        let Some(entry) = index.lookup_head(by_actor) else {
            return Ok(None);
        };
        let statement_id =
            StatementId::new(entry.statement_id.clone()).map_err(|error| StoreError::Corrupt {
                id: entry.statement_id.clone(),
                reason: CorruptReason::Parse(format!(
                    "invalid statement id in trust index: {error}"
                )),
            })?;
        let signed = self.get_actor_trust(&statement_id)?;

        if signed.unsigned().actor() != by_actor
            || signed.unsigned().body().trusted_actor() != trusted_actor
        {
            return Err(StoreError::Corrupt {
                id: statement_id.to_string(),
                reason: CorruptReason::Parse(
                    "trust index points at a statement with mismatched (by_actor, trusted_actor)"
                        .to_owned(),
                ),
            });
        }

        Ok(Some(signed))
    }

    fn list_trust(&self, by_actor: &ActorId) -> Result<Vec<TrustHead>, StoreError> {
        let mut heads = Vec::new();
        self.walk_trust_indices(|trusted_actor, index| {
            for head in index.into_heads(&trusted_actor)? {
                if &head.by_actor == by_actor {
                    heads.push(head);
                }
            }
            Ok(())
        })?;
        Ok(heads)
    }

    fn list_opinions_about(&self, trusted_actor: &ActorId) -> Result<Vec<TrustHead>, StoreError> {
        let Some(index) = self.read_trust_index(trusted_actor)? else {
            return Ok(Vec::new());
        };
        index.into_heads(trusted_actor)
    }
}

impl CapabilityResolver for FilesystemStore {
    fn latest_capability(
        &self,
        grantor: &ActorId,
        grantee: &ActorId,
        scope: &CapabilityScope,
    ) -> Result<Option<SignedStatement<ActorCapabilityGrantBody>>, StoreError> {
        let Some(index) = self.read_capability_index(grantor)? else {
            return Ok(None);
        };
        let Some(entry) = index.lookup_grant_head(grantee, scope) else {
            return Ok(None);
        };
        let statement_id =
            StatementId::new(entry.statement_id.clone()).map_err(|error| StoreError::Corrupt {
                id: entry.statement_id.clone(),
                reason: CorruptReason::Parse(format!(
                    "invalid statement id in capability index: {error}"
                )),
            })?;
        let signed = self.get_actor_capability_grant(&statement_id)?;

        if signed.unsigned().actor() != grantor
            || signed.unsigned().body().grantee() != grantee
            || signed.unsigned().body().capability().scope() != scope
        {
            return Err(StoreError::Corrupt {
                id: statement_id.to_string(),
                reason: CorruptReason::Parse(
                    "capability index points at a statement with mismatched (grantor, grantee, scope)"
                        .to_owned(),
                ),
            });
        }

        Ok(Some(signed))
    }

    fn latest_capability_revocation(
        &self,
        grantor: &ActorId,
        revoked_grant: &StatementId,
    ) -> Result<Option<SignedStatement<ActorCapabilityRevocationBody>>, StoreError> {
        let Some(index) = self.read_capability_index(grantor)? else {
            return Ok(None);
        };
        let Some(entry) = index.lookup_revocation(revoked_grant) else {
            return Ok(None);
        };
        let statement_id =
            StatementId::new(entry.statement_id.clone()).map_err(|error| StoreError::Corrupt {
                id: entry.statement_id.clone(),
                reason: CorruptReason::Parse(format!(
                    "invalid statement id in capability index: {error}"
                )),
            })?;
        let signed = self.get_actor_capability_revocation(&statement_id)?;

        if signed.unsigned().actor() != grantor
            || signed.unsigned().body().revoked_grant() != revoked_grant
        {
            return Err(StoreError::Corrupt {
                id: statement_id.to_string(),
                reason: CorruptReason::Parse(
                    "capability index points at a revocation with mismatched (grantor, revoked_grant)"
                        .to_owned(),
                ),
            });
        }

        Ok(Some(signed))
    }

    fn list_capabilities_from(&self, grantor: &ActorId) -> Result<Vec<CapabilityHead>, StoreError> {
        let Some(index) = self.read_capability_index(grantor)? else {
            return Ok(Vec::new());
        };
        index.into_heads(grantor)
    }

    fn list_capabilities_for_object(
        &self,
        object: &ObjectId,
    ) -> Result<Vec<CapabilityByObjectHead>, StoreError> {
        let Some(index) = self.read_capability_by_object_index(object)? else {
            return Ok(Vec::new());
        };
        index.into_heads(object)
    }
}

impl StatementByActorResolver for FilesystemStore {
    fn list_statements_by_actor(
        &self,
        actor: &ActorId,
    ) -> Result<Vec<StatementByActor>, StoreError> {
        let Some(index) = self.read_statement_by_actor_index(actor)? else {
            return Ok(Vec::new());
        };
        index.into_summaries(actor)
    }
}

impl BlobStore for FilesystemStore {
    fn put_blob(&self, id: &BlobId, bytes: &[u8]) -> Result<(), StoreError> {
        let path = self.shard_path(BLOBS_DIR, id.as_str(), BLOB_SUFFIX)?;
        atomic_write(&path, bytes)?;
        Ok(())
    }

    fn get_blob(&self, id: &BlobId) -> Result<Vec<u8>, StoreError> {
        let path = self.shard_path(BLOBS_DIR, id.as_str(), BLOB_SUFFIX)?;
        read_or_missing(&path)
    }
}

impl ActorResolver for FilesystemStore {
    fn actor_genesis(
        &self,
        actor: &ActorId,
    ) -> Result<Option<ActorGenesisBody>, ActorResolveError> {
        match self.get_actor(actor) {
            Ok(body) => Ok(Some(body)),
            Err(StoreError::Missing) => Ok(None),
            Err(error) => Err(ActorResolveError::Unavailable(error.to_string())),
        }
    }

    fn key_rotations(&self, actor: &ActorId) -> Result<Vec<KeyRotationEntry>, ActorResolveError> {
        let index = self
            .read_key_index_or_default(actor)
            .map_err(|error| ActorResolveError::Unavailable(error.to_string()))?;
        index
            .decode_rotations(actor.as_str())
            .map_err(|error| ActorResolveError::Unavailable(error.to_string()))
    }

    fn key_revocations(
        &self,
        actor: &ActorId,
    ) -> Result<Vec<KeyRevocationEntry>, ActorResolveError> {
        let index = self
            .read_key_index_or_default(actor)
            .map_err(|error| ActorResolveError::Unavailable(error.to_string()))?;
        index
            .decode_revocations()
            .map_err(|error| ActorResolveError::Unavailable(error.to_string()))
    }

    fn attestation_key_adds(
        &self,
        actor: &ActorId,
    ) -> Result<Vec<AttestationKeyAddEntry>, ActorResolveError> {
        let index = self
            .read_key_index_or_default(actor)
            .map_err(|error| ActorResolveError::Unavailable(error.to_string()))?;
        index
            .decode_attestation_adds(actor.as_str())
            .map_err(|error| ActorResolveError::Unavailable(error.to_string()))
    }

    fn attestation_key_revocations(
        &self,
        actor: &ActorId,
    ) -> Result<Vec<AttestationKeyRevocationEntry>, ActorResolveError> {
        let index = self
            .read_key_index_or_default(actor)
            .map_err(|error| ActorResolveError::Unavailable(error.to_string()))?;
        index
            .decode_attestation_revocations()
            .map_err(|error| ActorResolveError::Unavailable(error.to_string()))
    }

    fn attestation_threshold_changes(
        &self,
        actor: &ActorId,
    ) -> Result<Vec<kairo_identity::AttestationThresholdChangeEntry>, ActorResolveError> {
        let index = self
            .read_key_index_or_default(actor)
            .map_err(|error| ActorResolveError::Unavailable(error.to_string()))?;
        index
            .decode_attestation_threshold_changes()
            .map_err(|error| ActorResolveError::Unavailable(error.to_string()))
    }
}

impl kairo_statement::verify::TrustResolver for FilesystemStore {
    type Error = StoreError;

    fn latest_trust(
        &self,
        by_actor: &ActorId,
        trusted_actor: &ActorId,
    ) -> Result<Option<SignedStatement<ActorTrustBody>>, Self::Error> {
        TrustResolver::latest_trust(self, by_actor, trusted_actor)
    }
}

impl kairo_statement::verify::CapabilityResolver for FilesystemStore {
    type Error = StoreError;

    fn latest_capability(
        &self,
        grantor: &ActorId,
        grantee: &ActorId,
        scope: &CapabilityScope,
    ) -> Result<Option<SignedStatement<ActorCapabilityGrantBody>>, Self::Error> {
        CapabilityResolver::latest_capability(self, grantor, grantee, scope)
    }

    fn latest_capability_revocation(
        &self,
        grantor: &ActorId,
        revoked_grant: &StatementId,
    ) -> Result<Option<SignedStatement<ActorCapabilityRevocationBody>>, Self::Error> {
        CapabilityResolver::latest_capability_revocation(self, grantor, revoked_grant)
    }

    fn capability_grantors_for(
        &self,
        object: &ObjectId,
        grantee: &ActorId,
    ) -> Result<Vec<ActorId>, Self::Error> {
        Ok(self
            .list_capabilities_for_object(object)?
            .into_iter()
            .filter(|head| &head.grantee == grantee)
            .map(|head| head.grantor)
            .collect())
    }

    fn object_root_authority(
        &self,
        object: &ObjectId,
    ) -> Result<Option<Vec<ActorId>>, Self::Error> {
        match self.get_object_genesis(object) {
            Ok(statement) => Ok(Some(vec![statement.body().created_by().clone()])),
            Err(StoreError::Missing) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn is_key_revoked_at(
        &self,
        actor: &ActorId,
        key_id: &kairo_identity::KeyId,
        at: kairo_core::Timestamp,
    ) -> Result<bool, Self::Error> {
        ActorResolver::is_key_revoked_at(self, actor, key_id, at)
            .map_err(|error| StoreError::Unavailable(io::Error::other(error.to_string())))
    }
}

fn read_or_missing(path: &Path) -> Result<Vec<u8>, StoreError> {
    match fs::read(path) {
        Ok(bytes) => Ok(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Err(StoreError::Missing),
        Err(error) => Err(StoreError::Unavailable(error)),
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let parent = match path.parent() {
        Some(parent) => parent,
        None => {
            return Err(StoreError::Unavailable(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path has no parent",
            )));
        }
    };
    fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("anon");
    let tmp = parent.join(format!(".{file_name}.tmp"));
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

fn json_to_corrupt<T: ToString>(id: &T) -> impl FnOnce(serde_json::Error) -> StoreError {
    let id = id.to_string();
    move |error| StoreError::Corrupt {
        id,
        reason: CorruptReason::Parse(error.to_string()),
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic, clippy::cast_possible_truncation)]
mod tests {
    use std::fs;

    use ed25519_dalek::{Signer, SigningKey};
    use kairo_core::canonical::CanonicalEncode;
    use kairo_core::{BlobId, KairoRef, ObjectId, Timestamp};
    use kairo_identity::{ActorGenesisBody, ActorKind, PublicKey};
    use kairo_statement::verify::{
        evaluate_capability, verify_envelope_statement, ActorResolution, CapabilityEvaluation,
        ResolutionTarget, SignatureStatus,
    };
    use kairo_statement::{
        ActorCapabilityGrantBody, ActorCapabilityRevocationBody, ActorTrustBody, Capability,
        CapabilityScope, ObjectGenesisBody, ObjectGenesisStatement, ObjectKind, ObjectRevisionBody,
        ObjectVersionTagBody, RevisionId, SemverVersion, Signature, SignedStatement, StatementKind,
        TrustDecision, UnsignedStatement,
    };
    use tempfile::TempDir;

    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    const OBJECT_ID: &str = "zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk";
    const BLOB_ID: &str = "zQmfBE2w2UqKJhGZAxK4ZWb4JuCrvxnN9P4YKuvzfbPSvD5";

    fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&[7; 32])
    }

    fn public_key() -> PublicKey {
        PublicKey::ed25519(signing_key().verifying_key().to_bytes())
    }

    fn attestation_key() -> PublicKey {
        PublicKey::ed25519(
            SigningKey::from_bytes(&[200; 32])
                .verifying_key()
                .to_bytes(),
        )
    }

    fn timestamp() -> Timestamp {
        Timestamp::from_seconds(1_700_000_000)
    }

    fn fresh_genesis() -> ActorGenesisBody {
        ActorGenesisBody::new(
            ActorKind::person(),
            public_key(),
            vec![attestation_key()],
            1,
            timestamp(),
            [9; 32],
        )
        .expect("genesis well-formed")
    }

    fn open_temp_store() -> Result<(TempDir, FilesystemStore), Box<dyn std::error::Error>> {
        let dir = TempDir::new()?;
        let store = FilesystemStore::open(dir.path())?;
        Ok((dir, store))
    }

    fn signed_revision(
        actor: ActorId,
    ) -> Result<SignedStatement<ObjectRevisionBody>, Box<dyn std::error::Error>> {
        let object_id = ObjectId::new(OBJECT_ID)?;
        let body = ObjectRevisionBody::new(
            object_id,
            RevisionId::new("git:sha256:revision"),
            vec![RevisionId::new("git:sha256:parent")],
            BlobId::new(BLOB_ID)?,
            true,
        );
        let subject: KairoRef = format!("object:{OBJECT_ID}").parse()?;
        let unsigned = UnsignedStatement::new(actor.clone(), subject, timestamp(), body);
        let signature_bytes = signing_key().sign(&unsigned.canonical_bytes()).to_bytes();
        let signature = Signature::new(
            actor,
            public_key().key_id().to_string(),
            "ed25519",
            signature_bytes.to_vec(),
        );
        Ok(SignedStatement::new(unsigned, signature))
    }

    #[test]
    fn open_creates_version_file() -> TestResult {
        let dir = TempDir::new()?;
        let _ = FilesystemStore::open(dir.path())?;
        let version = fs::read_to_string(dir.path().join("version.txt"))?;
        assert_eq!(version.trim(), "1");
        Ok(())
    }

    #[test]
    fn refuses_to_open_with_wrong_version() -> TestResult {
        let dir = TempDir::new()?;
        fs::write(dir.path().join("version.txt"), "9\n")?;
        let result = FilesystemStore::open(dir.path());
        assert!(matches!(
            result,
            Err(StoreError::Corrupt {
                reason: CorruptReason::SchemaMismatch,
                ..
            })
        ));
        Ok(())
    }

    #[test]
    fn round_trips_actor_genesis() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let genesis = fresh_genesis();
        let actor_id = store.put_actor(&genesis)?;
        let loaded = store.get_actor(&actor_id)?;
        assert_eq!(loaded, genesis);
        Ok(())
    }

    #[test]
    fn round_trips_signed_revision() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let genesis = fresh_genesis();
        let actor_id = genesis.actor_id();
        let signed = signed_revision(actor_id)?;
        let id = store.put_object_revision(&signed)?;
        let loaded = store.get_object_revision(&id)?;
        assert_eq!(loaded, signed);
        Ok(())
    }

    #[test]
    fn round_trips_blob() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let bytes = b"hello kairo";
        let id = BlobId::from_bytes(b"kairo.test.blob.v1", bytes);
        store.put_blob(&id, bytes)?;
        let loaded = store.get_blob(&id)?;
        assert_eq!(loaded, bytes);
        Ok(())
    }

    #[test]
    fn missing_actor_returns_missing() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let actor_id = fresh_genesis().actor_id();
        assert!(matches!(
            store.get_actor(&actor_id),
            Err(StoreError::Missing)
        ));
        Ok(())
    }

    #[test]
    fn tampered_actor_file_is_corrupt() -> TestResult {
        let (dir, store) = open_temp_store()?;
        let genesis = fresh_genesis();
        let actor_id = store.put_actor(&genesis)?;
        let path = shard::shard_path(dir.path(), ACTORS_DIR, actor_id.as_str(), JSON_SUFFIX)
            .map_err(|error| error.to_string())?;
        // Replace the file with a different valid actor genesis JSON whose
        // derived ActorId will not match the filename.
        let different = ActorGenesisBody::new(
            ActorKind::person(),
            public_key(),
            vec![attestation_key()],
            1,
            timestamp(),
            [10; 32],
        )
        .expect("genesis well-formed");
        let json = ActorGenesisJson::from_body(&different);
        fs::write(&path, serde_json::to_vec_pretty(&json)?)?;

        assert!(matches!(
            store.get_actor(&actor_id),
            Err(StoreError::Corrupt {
                reason: CorruptReason::HashMismatch { .. },
                ..
            })
        ));
        Ok(())
    }

    #[test]
    fn unparseable_statement_file_is_corrupt() -> TestResult {
        let (dir, store) = open_temp_store()?;
        let signed = signed_revision(fresh_genesis().actor_id())?;
        let id = store.put_object_revision(&signed)?;
        let path = shard::shard_path(dir.path(), STATEMENTS_DIR, id.as_str(), JSON_SUFFIX)
            .map_err(|error| error.to_string())?;
        fs::write(&path, b"not json")?;

        assert!(matches!(
            store.get_object_revision(&id),
            Err(StoreError::Corrupt {
                reason: CorruptReason::Parse(_),
                ..
            })
        ));
        Ok(())
    }

    #[test]
    fn sharded_path_layout_matches_decision() -> TestResult {
        let (dir, store) = open_temp_store()?;
        let genesis = fresh_genesis();
        let actor_id = store.put_actor(&genesis)?;
        let shard1 = &actor_id.as_str()[3..5];
        let shard2 = &actor_id.as_str()[5..7];
        let expected = dir
            .path()
            .join(ACTORS_DIR)
            .join(shard1)
            .join(shard2)
            .join(format!("{actor_id}.json"));
        assert!(expected.exists(), "expected sharded path {expected:?}");
        Ok(())
    }

    #[test]
    fn store_acts_as_actor_resolver_for_verifier() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let genesis = fresh_genesis();
        let actor_id = store.put_actor(&genesis)?;
        let signed = signed_revision(actor_id)?;
        store.put_object_revision(&signed)?;

        let report = verify_envelope_statement(&signed, &store);

        assert_eq!(report.actor, ActorResolution::Resolved);
        assert_eq!(report.signature, SignatureStatus::Valid);
        assert!(report.is_cryptographically_valid());
        Ok(())
    }

    #[test]
    fn resolver_returns_none_for_unknown_actor() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let actor_id = fresh_genesis().actor_id();
        let resolved = ActorResolver::actor_genesis(&store, &actor_id)?;
        assert!(resolved.is_none());
        Ok(())
    }

    fn signed_object_genesis(actor: ActorId) -> ObjectGenesisStatement {
        let body = ObjectGenesisBody::new(
            ObjectKind::software(),
            actor.clone(),
            timestamp(),
            [42; 32],
            None,
        );
        let signature_bytes = signing_key().sign(&body.canonical_bytes()).to_bytes();
        let signature = Signature::new(
            actor,
            public_key().key_id().to_string(),
            "ed25519",
            signature_bytes.to_vec(),
        );
        ObjectGenesisStatement::new(body, signature)
    }

    #[test]
    fn round_trips_object_genesis() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let actor_id = fresh_genesis().actor_id();
        let statement = signed_object_genesis(actor_id);
        let id = store.put_object_genesis(&statement)?;
        assert_eq!(id, statement.object_id());
        let loaded = store.get_object_genesis(&id)?;
        assert_eq!(loaded.object_id(), statement.object_id());
        Ok(())
    }

    #[test]
    fn missing_object_genesis_returns_missing() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let actor_id = fresh_genesis().actor_id();
        let id = signed_object_genesis(actor_id).object_id();
        assert!(matches!(
            store.get_object_genesis(&id),
            Err(StoreError::Missing)
        ));
        Ok(())
    }

    #[test]
    fn tampered_object_genesis_is_corrupt() -> TestResult {
        let (dir, store) = open_temp_store()?;
        let actor_id = fresh_genesis().actor_id();
        let statement = signed_object_genesis(actor_id.clone());
        let id = store.put_object_genesis(&statement)?;

        // Replace the file with a different ObjectGenesis whose body derives a
        // different ObjectId.
        let other_body = ObjectGenesisBody::new(
            ObjectKind::software(),
            actor_id.clone(),
            timestamp(),
            [99; 32],
            None,
        );
        let other_signature = Signature::new(
            actor_id,
            public_key().key_id().to_string(),
            "ed25519",
            signing_key()
                .sign(&other_body.canonical_bytes())
                .to_bytes()
                .to_vec(),
        );
        let other_statement = ObjectGenesisStatement::new(other_body, other_signature);

        let path = shard::shard_path(dir.path(), OBJECTS_DIR, id.as_str(), JSON_SUFFIX)
            .map_err(|error| error.to_string())?;
        let json = ObjectGenesisStatementJson::from_statement(&other_statement);
        fs::write(&path, serde_json::to_vec_pretty(&json)?)?;

        assert!(matches!(
            store.get_object_genesis(&id),
            Err(StoreError::Corrupt {
                reason: CorruptReason::HashMismatch { .. },
                ..
            })
        ));
        Ok(())
    }

    #[test]
    fn object_genesis_path_is_sharded() -> TestResult {
        let (dir, store) = open_temp_store()?;
        let actor_id = fresh_genesis().actor_id();
        let statement = signed_object_genesis(actor_id);
        let id = store.put_object_genesis(&statement)?;

        let shard1 = &id.as_str()[3..5];
        let shard2 = &id.as_str()[5..7];
        let expected = dir
            .path()
            .join(OBJECTS_DIR)
            .join(shard1)
            .join(shard2)
            .join(format!("{id}.json"));
        assert!(expected.exists(), "expected sharded path {expected:?}");
        Ok(())
    }

    fn signed_branch(
        actor: ActorId,
        object: ObjectId,
        name: &str,
        revision: StatementId,
        supersedes: Option<StatementId>,
        created_at: Timestamp,
    ) -> Result<SignedStatement<ObjectBranchBody>, Box<dyn std::error::Error>> {
        let body = ObjectBranchBody::new(object.clone(), name, revision, supersedes);
        let subject: KairoRef = format!("object:{object}").parse()?;
        let unsigned = UnsignedStatement::new(actor.clone(), subject, created_at, body);
        let signature_bytes = signing_key().sign(&unsigned.canonical_bytes()).to_bytes();
        let signature = Signature::new(
            actor,
            public_key().key_id().to_string(),
            "ed25519",
            signature_bytes.to_vec(),
        );
        Ok(SignedStatement::new(unsigned, signature))
    }

    #[test]
    fn round_trips_object_branch() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let actor = fresh_genesis().actor_id();
        let object = ObjectId::new(OBJECT_ID)?;
        let revision = StatementId::from_sha256_digest([0xAA; 32]);
        let signed = signed_branch(actor, object, "head", revision, None, timestamp())?;
        let id = store.put_object_branch(&signed)?;
        let loaded = store.get_object_branch(&id)?;
        assert_eq!(loaded, signed);
        Ok(())
    }

    #[test]
    fn latest_branch_returns_most_recent() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let actor = fresh_genesis().actor_id();
        let object = ObjectId::new(OBJECT_ID)?;
        let earlier = signed_branch(
            actor.clone(),
            object.clone(),
            "head",
            StatementId::from_sha256_digest([0xAA; 32]),
            None,
            Timestamp::from_seconds(timestamp().seconds()),
        )?;
        let earlier_id = store.put_object_branch(&earlier)?;
        let later = signed_branch(
            actor.clone(),
            object.clone(),
            "head",
            StatementId::from_sha256_digest([0xBB; 32]),
            Some(earlier_id),
            Timestamp::from_seconds(timestamp().seconds() + 1),
        )?;
        store.put_object_branch(&later)?;

        let resolved = store.latest_branch(&actor, &object, "head")?;
        assert_eq!(resolved, Some(later));
        Ok(())
    }

    #[test]
    fn earlier_branch_after_later_does_not_supersede() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let actor = fresh_genesis().actor_id();
        let object = ObjectId::new(OBJECT_ID)?;
        let earlier = signed_branch(
            actor.clone(),
            object.clone(),
            "head",
            StatementId::from_sha256_digest([0xAA; 32]),
            None,
            Timestamp::from_seconds(timestamp().seconds()),
        )?;
        let earlier_id = earlier.statement_id();
        let later = signed_branch(
            actor.clone(),
            object.clone(),
            "head",
            StatementId::from_sha256_digest([0xBB; 32]),
            Some(earlier_id.clone()),
            Timestamp::from_seconds(timestamp().seconds() + 1),
        )?;
        // Insert later first, then the earlier write must not move the chain
        // leaf — the supersedes chain still puts `later` at the tip.
        store.put_object_branch(&later)?;
        store.put_object_branch(&earlier)?;

        let resolved = store.latest_branch(&actor, &object, "head")?;
        assert_eq!(resolved, Some(later));
        // Earlier branch is still on disk by its statement id.
        assert!(store.get_object_branch(&earlier_id).is_ok());
        Ok(())
    }

    #[test]
    fn missing_branch_returns_none() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let actor = fresh_genesis().actor_id();
        let object = ObjectId::new(OBJECT_ID)?;
        let resolved = store.latest_branch(&actor, &object, "head")?;
        assert!(resolved.is_none());
        Ok(())
    }

    #[test]
    fn branches_are_independent_per_name() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let actor = fresh_genesis().actor_id();
        let object = ObjectId::new(OBJECT_ID)?;
        let head = signed_branch(
            actor.clone(),
            object.clone(),
            "head",
            StatementId::from_sha256_digest([0xAA; 32]),
            None,
            timestamp(),
        )?;
        let release = signed_branch(
            actor.clone(),
            object.clone(),
            "release",
            StatementId::from_sha256_digest([0xCC; 32]),
            None,
            timestamp(),
        )?;
        store.put_object_branch(&head)?;
        store.put_object_branch(&release)?;

        assert_eq!(store.latest_branch(&actor, &object, "head")?, Some(head));
        assert_eq!(
            store.latest_branch(&actor, &object, "release")?,
            Some(release)
        );
        Ok(())
    }

    #[test]
    fn list_branches_returns_all_tips_for_object() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let actor = fresh_genesis().actor_id();
        let object = ObjectId::new(OBJECT_ID)?;
        let head = signed_branch(
            actor.clone(),
            object.clone(),
            "head",
            StatementId::from_sha256_digest([0xAA; 32]),
            None,
            timestamp(),
        )?;
        let release = signed_branch(
            actor.clone(),
            object.clone(),
            "release",
            StatementId::from_sha256_digest([0xCC; 32]),
            None,
            timestamp(),
        )?;
        store.put_object_branch(&head)?;
        store.put_object_branch(&release)?;

        let tips = store.list_branches(&object)?;
        assert_eq!(tips.len(), 2);
        let names: Vec<_> = tips.iter().map(|tip| tip.name.as_str()).collect();
        assert!(names.contains(&"head"));
        assert!(names.contains(&"release"));
        Ok(())
    }

    #[test]
    fn branch_index_path_is_sharded() -> TestResult {
        let (dir, store) = open_temp_store()?;
        let actor = fresh_genesis().actor_id();
        let object = ObjectId::new(OBJECT_ID)?;
        let signed = signed_branch(
            actor,
            object.clone(),
            "head",
            StatementId::from_sha256_digest([0xAA; 32]),
            None,
            timestamp(),
        )?;
        store.put_object_branch(&signed)?;

        let shard1 = &object.as_str()[3..5];
        let shard2 = &object.as_str()[5..7];
        let expected = dir
            .path()
            .join(BRANCHES_DIR)
            .join(shard1)
            .join(shard2)
            .join(format!("{object}.json"));
        assert!(expected.exists(), "expected branch index at {expected:?}");
        Ok(())
    }

    #[test]
    fn list_branches_for_unknown_object_is_empty() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let object = ObjectId::new(OBJECT_ID)?;
        let tips = store.list_branches(&object)?;
        assert!(tips.is_empty());
        Ok(())
    }

    #[test]
    fn cross_actor_branch_supersedes_without_grant_does_not_replace_per_actor_head() -> TestResult {
        // Actor B signs a branch advance whose supersedes points at
        // actor A's branch advance, but A has not granted B any
        // ObjectBranch capability on the object. evaluate_capability(B,
        // ...) returns NotHeld, so the cross-actor edge is rejected and
        // each actor keeps their own head. Mirrors the tag-side test
        // for `specs/CAPABILITIES.md` §6.2.
        let (_dir, store) = open_temp_store()?;
        let actor_a = fresh_genesis().actor_id();
        let actor_b = ActorGenesisBody::new(
            ActorKind::person(),
            PublicKey::ed25519(SigningKey::from_bytes(&[8; 32]).verifying_key().to_bytes()),
            vec![attestation_key()],
            1,
            timestamp(),
            [11; 32],
        )
        .expect("genesis well-formed")
        .actor_id();
        let object = ObjectId::new(OBJECT_ID)?;
        let a_branch = signed_branch(
            actor_a.clone(),
            object.clone(),
            "head",
            StatementId::from_sha256_digest([0xAA; 32]),
            None,
            timestamp(),
        )?;
        let a_id = store.put_object_branch(&a_branch)?;
        let b_branch = signed_branch(
            actor_b.clone(),
            object.clone(),
            "head",
            StatementId::from_sha256_digest([0xBB; 32]),
            Some(a_id),
            Timestamp::from_seconds(timestamp().seconds() + 1),
        )?;
        store.put_object_branch(&b_branch)?;

        let a_head = store.latest_branch(&actor_a, &object, "head")?;
        assert_eq!(a_head, Some(a_branch));
        let b_head = store.latest_branch(&actor_b, &object, "head")?;
        assert_eq!(b_head, Some(b_branch));
        Ok(())
    }

    #[test]
    fn cross_actor_branch_supersedes_with_grant_replaces_per_actor_head() -> TestResult {
        // Symmetric payoff: A is the object's root authority and grants
        // B an ObjectBranch capability on this object. B then signs a
        // branch advance whose supersedes points at A's. The cross-actor
        // edge is honored; latest_branch(A, ...) returns B's branch.
        let (_dir, store) = open_temp_store()?;
        let a_genesis = fresh_genesis();
        let actor_a = a_genesis.actor_id();
        store.put_actor(&a_genesis)?;
        let object_statement = signed_object_genesis(actor_a.clone());
        let object = store.put_object_genesis(&object_statement)?;

        let actor_b = ActorGenesisBody::new(
            ActorKind::person(),
            PublicKey::ed25519(SigningKey::from_bytes(&[8; 32]).verifying_key().to_bytes()),
            vec![attestation_key()],
            1,
            timestamp(),
            [11; 32],
        )
        .expect("genesis well-formed")
        .actor_id();

        let scope = CapabilityScope::Object(object.clone());
        let grant =
            signed_capability_grant(actor_a.clone(), actor_b.clone(), scope, None, timestamp())?;
        store.put_actor_capability_grant(&grant)?;

        let a_branch = signed_branch(
            actor_a.clone(),
            object.clone(),
            "head",
            StatementId::from_sha256_digest([0xAA; 32]),
            None,
            timestamp(),
        )?;
        let a_id = store.put_object_branch(&a_branch)?;
        let b_branch = signed_branch(
            actor_b.clone(),
            object.clone(),
            "head",
            StatementId::from_sha256_digest([0xBB; 32]),
            Some(a_id),
            Timestamp::from_seconds(timestamp().seconds() + 1),
        )?;
        store.put_object_branch(&b_branch)?;

        let a_head = store.latest_branch(&actor_a, &object, "head")?;
        assert_eq!(a_head, Some(b_branch.clone()));
        let b_head = store.latest_branch(&actor_b, &object, "head")?;
        assert_eq!(b_head, Some(b_branch));
        Ok(())
    }

    #[test]
    fn cross_actor_branch_supersedes_with_revoked_grant_is_not_honored() -> TestResult {
        // A grants B, B supersedes A's branch, then A revokes the grant
        // *before* B's advance was created. With a default (non-
        // retroactive) revocation, B's branch was created strictly after
        // the revocation, so the grant no longer covers it — the
        // cross-actor edge is rejected, A's branch stays the head.
        let (_dir, store) = open_temp_store()?;
        let a_genesis = fresh_genesis();
        let actor_a = a_genesis.actor_id();
        store.put_actor(&a_genesis)?;
        let object_statement = signed_object_genesis(actor_a.clone());
        let object = store.put_object_genesis(&object_statement)?;

        let actor_b = ActorGenesisBody::new(
            ActorKind::person(),
            PublicKey::ed25519(SigningKey::from_bytes(&[8; 32]).verifying_key().to_bytes()),
            vec![attestation_key()],
            1,
            timestamp(),
            [11; 32],
        )
        .expect("genesis well-formed")
        .actor_id();

        let scope = CapabilityScope::Object(object.clone());
        let grant =
            signed_capability_grant(actor_a.clone(), actor_b.clone(), scope, None, timestamp())?;
        let grant_id = store.put_actor_capability_grant(&grant)?;
        let revocation = signed_capability_revocation(
            actor_a.clone(),
            grant_id,
            false,
            Timestamp::from_seconds(timestamp().seconds() + 1),
        )?;
        store.put_actor_capability_revocation(&revocation)?;

        let a_branch = signed_branch(
            actor_a.clone(),
            object.clone(),
            "head",
            StatementId::from_sha256_digest([0xAA; 32]),
            None,
            timestamp(),
        )?;
        let a_id = store.put_object_branch(&a_branch)?;
        let b_branch = signed_branch(
            actor_b,
            object.clone(),
            "head",
            StatementId::from_sha256_digest([0xBB; 32]),
            Some(a_id),
            Timestamp::from_seconds(timestamp().seconds() + 2),
        )?;
        store.put_object_branch(&b_branch)?;

        let a_head = store.latest_branch(&actor_a, &object, "head")?;
        assert_eq!(a_head, Some(a_branch));
        Ok(())
    }

    #[test]
    fn branch_chain_precedence_overrides_timestamp_tiebreak() -> TestResult {
        // A successor with a strictly older `created_at` than the
        // genesis must still win because chain precedence — the
        // supersedes edge — beats the (created_at, statement_id)
        // tiebreak. Mirrors the tag-side chain-precedence rule.
        let (_dir, store) = open_temp_store()?;
        let actor = fresh_genesis().actor_id();
        let object = ObjectId::new(OBJECT_ID)?;
        let genesis = signed_branch(
            actor.clone(),
            object.clone(),
            "head",
            StatementId::from_sha256_digest([0xAA; 32]),
            None,
            Timestamp::from_seconds(timestamp().seconds() + 100),
        )?;
        let genesis_id = store.put_object_branch(&genesis)?;
        let successor = signed_branch(
            actor.clone(),
            object.clone(),
            "head",
            StatementId::from_sha256_digest([0xBB; 32]),
            Some(genesis_id),
            Timestamp::from_seconds(timestamp().seconds()),
        )?;
        store.put_object_branch(&successor)?;

        let resolved = store.latest_branch(&actor, &object, "head")?;
        assert_eq!(resolved, Some(successor));
        Ok(())
    }

    fn signed_version_tag(
        actor: ActorId,
        object: ObjectId,
        version: &str,
        target: Option<StatementId>,
        supersedes: Option<StatementId>,
        created_at: Timestamp,
    ) -> Result<SignedStatement<ObjectVersionTagBody>, Box<dyn std::error::Error>> {
        let semver = SemverVersion::parse(version)?;
        let body = ObjectVersionTagBody::new(object.clone(), semver, target, supersedes)?;
        let subject: KairoRef = format!("object:{object}").parse()?;
        let unsigned = UnsignedStatement::new(actor.clone(), subject, created_at, body);
        let signature_bytes = signing_key().sign(&unsigned.canonical_bytes()).to_bytes();
        let signature = Signature::new(
            actor,
            public_key().key_id().to_string(),
            "ed25519",
            signature_bytes.to_vec(),
        );
        Ok(SignedStatement::new(unsigned, signature))
    }

    #[test]
    fn round_trips_object_version_tag() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let actor = fresh_genesis().actor_id();
        let object = ObjectId::new(OBJECT_ID)?;
        let target = StatementId::from_sha256_digest([0xAA; 32]);
        let signed = signed_version_tag(actor, object, "1.2.3", Some(target), None, timestamp())?;
        let id = store.put_object_version_tag(&signed)?;
        let loaded = store.get_object_version_tag(&id)?;
        assert_eq!(loaded, signed);
        Ok(())
    }

    #[test]
    fn latest_version_tag_returns_most_recent() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let actor = fresh_genesis().actor_id();
        let object = ObjectId::new(OBJECT_ID)?;
        let earlier = signed_version_tag(
            actor.clone(),
            object.clone(),
            "1.2.3",
            Some(StatementId::from_sha256_digest([0xAA; 32])),
            None,
            Timestamp::from_seconds(timestamp().seconds()),
        )?;
        let earlier_id = store.put_object_version_tag(&earlier)?;
        let later = signed_version_tag(
            actor.clone(),
            object.clone(),
            "1.2.3",
            Some(StatementId::from_sha256_digest([0xBB; 32])),
            Some(earlier_id),
            Timestamp::from_seconds(timestamp().seconds() + 1),
        )?;
        store.put_object_version_tag(&later)?;

        let resolved = store.latest_version_tag(&actor, &object, "1.2.3")?;
        assert_eq!(resolved, Some(later));
        Ok(())
    }

    #[test]
    fn revoke_supersedes_bind_at_latest_wins() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let actor = fresh_genesis().actor_id();
        let object = ObjectId::new(OBJECT_ID)?;
        let bind = signed_version_tag(
            actor.clone(),
            object.clone(),
            "1.2.3",
            Some(StatementId::from_sha256_digest([0xAA; 32])),
            None,
            Timestamp::from_seconds(timestamp().seconds()),
        )?;
        let bind_id = store.put_object_version_tag(&bind)?;
        let revoke = signed_version_tag(
            actor.clone(),
            object.clone(),
            "1.2.3",
            None,
            Some(bind_id),
            Timestamp::from_seconds(timestamp().seconds() + 1),
        )?;
        store.put_object_version_tag(&revoke)?;

        let resolved = store.latest_version_tag(&actor, &object, "1.2.3")?;
        assert!(matches!(
            resolved,
            Some(signed) if signed.unsigned().body().is_revocation()
        ));
        Ok(())
    }

    #[test]
    fn missing_version_tag_returns_none() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let actor = fresh_genesis().actor_id();
        let object = ObjectId::new(OBJECT_ID)?;
        let resolved = store.latest_version_tag(&actor, &object, "1.2.3")?;
        assert!(resolved.is_none());
        Ok(())
    }

    #[test]
    fn version_tags_are_independent_per_version_string() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let actor = fresh_genesis().actor_id();
        let object = ObjectId::new(OBJECT_ID)?;
        let one = signed_version_tag(
            actor.clone(),
            object.clone(),
            "1.2.3",
            Some(StatementId::from_sha256_digest([0xAA; 32])),
            None,
            timestamp(),
        )?;
        let two = signed_version_tag(
            actor.clone(),
            object.clone(),
            "1.2.4",
            Some(StatementId::from_sha256_digest([0xCC; 32])),
            None,
            timestamp(),
        )?;
        store.put_object_version_tag(&one)?;
        store.put_object_version_tag(&two)?;

        assert_eq!(
            store.latest_version_tag(&actor, &object, "1.2.3")?,
            Some(one)
        );
        assert_eq!(
            store.latest_version_tag(&actor, &object, "1.2.4")?,
            Some(two)
        );
        Ok(())
    }

    #[test]
    fn list_version_tags_returns_all_heads_for_object() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let actor = fresh_genesis().actor_id();
        let object = ObjectId::new(OBJECT_ID)?;
        let one = signed_version_tag(
            actor.clone(),
            object.clone(),
            "1.2.3",
            Some(StatementId::from_sha256_digest([0xAA; 32])),
            None,
            timestamp(),
        )?;
        let two = signed_version_tag(
            actor.clone(),
            object.clone(),
            "1.2.4",
            Some(StatementId::from_sha256_digest([0xCC; 32])),
            None,
            timestamp(),
        )?;
        store.put_object_version_tag(&one)?;
        store.put_object_version_tag(&two)?;

        let heads = store.list_version_tags(&object)?;
        assert_eq!(heads.len(), 2);
        let versions: Vec<_> = heads.iter().map(|h| h.version.as_str()).collect();
        assert!(versions.contains(&"1.2.3"));
        assert!(versions.contains(&"1.2.4"));
        Ok(())
    }

    #[test]
    fn version_tag_index_path_is_sharded() -> TestResult {
        let (dir, store) = open_temp_store()?;
        let actor = fresh_genesis().actor_id();
        let object = ObjectId::new(OBJECT_ID)?;
        let signed = signed_version_tag(
            actor,
            object.clone(),
            "1.2.3",
            Some(StatementId::from_sha256_digest([0xAA; 32])),
            None,
            timestamp(),
        )?;
        store.put_object_version_tag(&signed)?;

        let shard1 = &object.as_str()[3..5];
        let shard2 = &object.as_str()[5..7];
        let expected = dir
            .path()
            .join(VERSION_TAGS_DIR)
            .join(shard1)
            .join(shard2)
            .join(format!("{object}.json"));
        assert!(
            expected.exists(),
            "expected version tag index at {expected:?}"
        );
        Ok(())
    }

    #[test]
    fn list_version_tags_for_unknown_object_is_empty() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let object = ObjectId::new(OBJECT_ID)?;
        let heads = store.list_version_tags(&object)?;
        assert!(heads.is_empty());
        Ok(())
    }

    #[test]
    fn chain_precedence_overrides_timestamp_tiebreak() -> TestResult {
        // Bind first; revoke second, both at same created_at, with the
        // revoke's statement id sorting *lower* lexicographically. With
        // pure (created_at, statement_id) tiebreak the bind would win;
        // chain-precedence must give the win to the revoke because it
        // explicitly supersedes the bind.
        let (_dir, store) = open_temp_store()?;
        let actor = fresh_genesis().actor_id();
        let object = ObjectId::new(OBJECT_ID)?;
        let bind = signed_version_tag(
            actor.clone(),
            object.clone(),
            "1.2.3",
            Some(StatementId::from_sha256_digest([0xAA; 32])),
            None,
            timestamp(),
        )?;
        let bind_id = store.put_object_version_tag(&bind)?;
        let revoke = signed_version_tag(
            actor.clone(),
            object.clone(),
            "1.2.3",
            None,
            Some(bind_id.clone()),
            timestamp(), // same created_at — would tie under old rule
        )?;
        let revoke_id = store.put_object_version_tag(&revoke)?;
        // Sanity: at least one of these orderings would have caused
        // the bind to win under timestamp+id tiebreak.
        let _ = (bind_id, revoke_id);

        let resolved = store.latest_version_tag(&actor, &object, "1.2.3")?;
        assert!(matches!(
            resolved,
            Some(signed) if signed.unsigned().body().is_revocation()
        ));
        Ok(())
    }

    #[test]
    fn cross_actor_supersedes_without_grant_does_not_replace_per_actor_head() -> TestResult {
        // Actor B signs a tag whose supersedes points at actor A's
        // tag, but A has not granted B any ObjectVersionTag
        // capability on the object. evaluate_capability(B, ...)
        // returns NotHeld, so the cross-actor edge is rejected and
        // each actor keeps their own head. This is the negative half
        // of `specs/CAPABILITIES.md` §6.2.
        let (_dir, store) = open_temp_store()?;
        let actor_a = fresh_genesis().actor_id();
        let actor_b = ActorGenesisBody::new(
            ActorKind::person(),
            PublicKey::ed25519(SigningKey::from_bytes(&[8; 32]).verifying_key().to_bytes()),
            vec![attestation_key()],
            1,
            timestamp(),
            [11; 32],
        )
        .expect("genesis well-formed")
        .actor_id();
        let object = ObjectId::new(OBJECT_ID)?;
        let a_tag = signed_version_tag(
            actor_a.clone(),
            object.clone(),
            "1.2.3",
            Some(StatementId::from_sha256_digest([0xAA; 32])),
            None,
            timestamp(),
        )?;
        let a_id = store.put_object_version_tag(&a_tag)?;
        let b_tag = signed_version_tag(
            actor_b.clone(),
            object.clone(),
            "1.2.3",
            Some(StatementId::from_sha256_digest([0xBB; 32])),
            Some(a_id.clone()),
            Timestamp::from_seconds(timestamp().seconds() + 1),
        )?;
        store.put_object_version_tag(&b_tag)?;

        let a_head = store.latest_version_tag(&actor_a, &object, "1.2.3")?;
        assert_eq!(a_head, Some(a_tag));
        let b_head = store.latest_version_tag(&actor_b, &object, "1.2.3")?;
        assert_eq!(b_head, Some(b_tag));
        Ok(())
    }

    #[test]
    fn cross_actor_supersedes_with_grant_replaces_per_actor_head() -> TestResult {
        // Step 6 / §6.2 payoff: A is the object's root authority and
        // grants B an ObjectVersionTag capability on this object. B
        // then signs a tag whose supersedes points at A's tag. The
        // cross-actor edge is now honored; latest_version_tag(A, ...)
        // returns B's tag.
        let (_dir, store) = open_temp_store()?;
        // Persist A's actor genesis and an ObjectGenesis whose
        // root authority (`created_by`) is A.
        let a_genesis = fresh_genesis();
        let actor_a = a_genesis.actor_id();
        store.put_actor(&a_genesis)?;
        let object_statement = signed_object_genesis(actor_a.clone());
        let object = store.put_object_genesis(&object_statement)?;

        let actor_b = ActorGenesisBody::new(
            ActorKind::person(),
            PublicKey::ed25519(SigningKey::from_bytes(&[8; 32]).verifying_key().to_bytes()),
            vec![attestation_key()],
            1,
            timestamp(),
            [11; 32],
        )
        .expect("genesis well-formed")
        .actor_id();

        // A → B: capability covering ObjectVersionTag on this object.
        let scope = CapabilityScope::Object(object.clone());
        let grant =
            signed_capability_grant(actor_a.clone(), actor_b.clone(), scope, None, timestamp())?;
        store.put_actor_capability_grant(&grant)?;

        // A's tag, then B's tag superseding A's.
        let a_tag = signed_version_tag(
            actor_a.clone(),
            object.clone(),
            "1.2.3",
            Some(StatementId::from_sha256_digest([0xAA; 32])),
            None,
            timestamp(),
        )?;
        let a_id = store.put_object_version_tag(&a_tag)?;
        let b_tag = signed_version_tag(
            actor_b.clone(),
            object.clone(),
            "1.2.3",
            Some(StatementId::from_sha256_digest([0xBB; 32])),
            Some(a_id),
            Timestamp::from_seconds(timestamp().seconds() + 1),
        )?;
        store.put_object_version_tag(&b_tag)?;

        // From A's perspective the head has flipped to B's tag.
        let a_head = store.latest_version_tag(&actor_a, &object, "1.2.3")?;
        assert_eq!(a_head, Some(b_tag.clone()));
        // From B's perspective B's tag is also the head (B's same-actor leaf).
        let b_head = store.latest_version_tag(&actor_b, &object, "1.2.3")?;
        assert_eq!(b_head, Some(b_tag));
        Ok(())
    }

    #[test]
    fn cross_actor_supersedes_with_revoked_grant_is_not_honored() -> TestResult {
        // A grants B, B supersedes A's tag, then A revokes the grant
        // *before* B's tag was created. With a default (non-
        // retroactive) revocation, B's tag was created strictly
        // after the revocation and the grant no longer covers it —
        // the cross-actor edge is rejected, A's tag stays the head.
        let (_dir, store) = open_temp_store()?;
        let a_genesis = fresh_genesis();
        let actor_a = a_genesis.actor_id();
        store.put_actor(&a_genesis)?;
        let object_statement = signed_object_genesis(actor_a.clone());
        let object = store.put_object_genesis(&object_statement)?;

        let actor_b = ActorGenesisBody::new(
            ActorKind::person(),
            PublicKey::ed25519(SigningKey::from_bytes(&[8; 32]).verifying_key().to_bytes()),
            vec![attestation_key()],
            1,
            timestamp(),
            [11; 32],
        )
        .expect("genesis well-formed")
        .actor_id();

        let scope = CapabilityScope::Object(object.clone());
        let grant =
            signed_capability_grant(actor_a.clone(), actor_b.clone(), scope, None, timestamp())?;
        let grant_id = store.put_actor_capability_grant(&grant)?;
        // Revocation at t+1; B's tag at t+2. evaluate_capability at
        // t+2 sees the grant as revoked.
        let revocation = signed_capability_revocation(
            actor_a.clone(),
            grant_id,
            false,
            Timestamp::from_seconds(timestamp().seconds() + 1),
        )?;
        store.put_actor_capability_revocation(&revocation)?;

        let a_tag = signed_version_tag(
            actor_a.clone(),
            object.clone(),
            "1.2.3",
            Some(StatementId::from_sha256_digest([0xAA; 32])),
            None,
            timestamp(),
        )?;
        let a_id = store.put_object_version_tag(&a_tag)?;
        let b_tag = signed_version_tag(
            actor_b,
            object.clone(),
            "1.2.3",
            Some(StatementId::from_sha256_digest([0xBB; 32])),
            Some(a_id),
            Timestamp::from_seconds(timestamp().seconds() + 2),
        )?;
        store.put_object_version_tag(&b_tag)?;

        let a_head = store.latest_version_tag(&actor_a, &object, "1.2.3")?;
        assert_eq!(a_head, Some(a_tag));
        Ok(())
    }

    fn signed_actor_trust(
        by_actor: ActorId,
        trusted_actor: ActorId,
        decision: Option<TrustDecision>,
        reason: Option<&str>,
        supersedes: Option<StatementId>,
        created_at: Timestamp,
    ) -> Result<SignedStatement<ActorTrustBody>, Box<dyn std::error::Error>> {
        let body = ActorTrustBody::new(
            trusted_actor.clone(),
            decision,
            reason.map(|r| r.to_owned()),
            supersedes,
        )?;
        let subject: KairoRef = format!("actor:{trusted_actor}").parse()?;
        let unsigned = UnsignedStatement::new(by_actor.clone(), subject, created_at, body);
        let signature_bytes = signing_key().sign(&unsigned.canonical_bytes()).to_bytes();
        let signature = Signature::new(
            by_actor,
            public_key().key_id().to_string(),
            "ed25519",
            signature_bytes.to_vec(),
        );
        Ok(SignedStatement::new(unsigned, signature))
    }

    fn trusted_actor_id() -> ActorId {
        ActorGenesisBody::new(
            ActorKind::person(),
            PublicKey::ed25519(SigningKey::from_bytes(&[5; 32]).verifying_key().to_bytes()),
            vec![attestation_key()],
            1,
            timestamp(),
            [33; 32],
        )
        .expect("genesis well-formed")
        .actor_id()
    }

    fn other_trusted_actor_id() -> ActorId {
        ActorGenesisBody::new(
            ActorKind::person(),
            PublicKey::ed25519(SigningKey::from_bytes(&[6; 32]).verifying_key().to_bytes()),
            vec![attestation_key()],
            1,
            timestamp(),
            [44; 32],
        )
        .expect("genesis well-formed")
        .actor_id()
    }

    #[test]
    fn round_trips_actor_trust() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let by_actor = fresh_genesis().actor_id();
        let trusted = trusted_actor_id();
        let signed = signed_actor_trust(
            by_actor,
            trusted,
            Some(TrustDecision::Trusted),
            Some("works"),
            None,
            timestamp(),
        )?;
        let id = store.put_actor_trust(&signed)?;
        let loaded = store.get_actor_trust(&id)?;
        assert_eq!(loaded, signed);
        Ok(())
    }

    #[test]
    fn latest_trust_returns_chain_leaf() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let by_actor = fresh_genesis().actor_id();
        let trusted = trusted_actor_id();
        let grant = signed_actor_trust(
            by_actor.clone(),
            trusted.clone(),
            Some(TrustDecision::Trusted),
            None,
            None,
            timestamp(),
        )?;
        let grant_id = store.put_actor_trust(&grant)?;
        let block = signed_actor_trust(
            by_actor.clone(),
            trusted.clone(),
            Some(TrustDecision::Untrusted),
            None,
            Some(grant_id),
            Timestamp::from_seconds(timestamp().seconds() + 1),
        )?;
        store.put_actor_trust(&block)?;

        let resolved = store.latest_trust(&by_actor, &trusted)?;
        assert_eq!(resolved, Some(block));
        Ok(())
    }

    #[test]
    fn withdraw_supersedes_grant_at_latest_wins() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let by_actor = fresh_genesis().actor_id();
        let trusted = trusted_actor_id();
        let grant = signed_actor_trust(
            by_actor.clone(),
            trusted.clone(),
            Some(TrustDecision::Trusted),
            None,
            None,
            timestamp(),
        )?;
        let grant_id = store.put_actor_trust(&grant)?;
        let withdraw = signed_actor_trust(
            by_actor.clone(),
            trusted.clone(),
            None,
            None,
            Some(grant_id),
            Timestamp::from_seconds(timestamp().seconds() + 1),
        )?;
        store.put_actor_trust(&withdraw)?;

        let resolved = store.latest_trust(&by_actor, &trusted)?;
        assert!(matches!(
            resolved,
            Some(signed) if signed.unsigned().body().is_withdrawal()
        ));
        Ok(())
    }

    #[test]
    fn missing_trust_returns_none() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let by_actor = fresh_genesis().actor_id();
        let trusted = trusted_actor_id();
        let resolved = store.latest_trust(&by_actor, &trusted)?;
        assert!(resolved.is_none());
        Ok(())
    }

    #[test]
    fn trust_is_independent_per_trusted_actor() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let by_actor = fresh_genesis().actor_id();
        let trusted_a = trusted_actor_id();
        let trusted_b = other_trusted_actor_id();
        let grant_a = signed_actor_trust(
            by_actor.clone(),
            trusted_a.clone(),
            Some(TrustDecision::Trusted),
            None,
            None,
            timestamp(),
        )?;
        let block_b = signed_actor_trust(
            by_actor.clone(),
            trusted_b.clone(),
            Some(TrustDecision::Untrusted),
            None,
            None,
            timestamp(),
        )?;
        store.put_actor_trust(&grant_a)?;
        store.put_actor_trust(&block_b)?;

        assert_eq!(store.latest_trust(&by_actor, &trusted_a)?, Some(grant_a));
        assert_eq!(store.latest_trust(&by_actor, &trusted_b)?, Some(block_b));
        Ok(())
    }

    #[test]
    fn list_trust_returns_all_heads_for_truster() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let by_actor = fresh_genesis().actor_id();
        let trusted_a = trusted_actor_id();
        let trusted_b = other_trusted_actor_id();
        let grant_a = signed_actor_trust(
            by_actor.clone(),
            trusted_a.clone(),
            Some(TrustDecision::Trusted),
            None,
            None,
            timestamp(),
        )?;
        let block_b = signed_actor_trust(
            by_actor.clone(),
            trusted_b.clone(),
            Some(TrustDecision::Untrusted),
            None,
            None,
            timestamp(),
        )?;
        store.put_actor_trust(&grant_a)?;
        store.put_actor_trust(&block_b)?;

        let heads = store.list_trust(&by_actor)?;
        assert_eq!(heads.len(), 2);
        let trusted_set: Vec<_> = heads.iter().map(|h| h.trusted_actor.clone()).collect();
        assert!(trusted_set.contains(&trusted_a));
        assert!(trusted_set.contains(&trusted_b));
        Ok(())
    }

    #[test]
    fn trust_index_path_is_sharded_on_trusted_actor() -> TestResult {
        let (dir, store) = open_temp_store()?;
        let by_actor = fresh_genesis().actor_id();
        let trusted = trusted_actor_id();
        let signed = signed_actor_trust(
            by_actor,
            trusted.clone(),
            Some(TrustDecision::Trusted),
            None,
            None,
            timestamp(),
        )?;
        store.put_actor_trust(&signed)?;

        let shard1 = &trusted.as_str()[3..5];
        let shard2 = &trusted.as_str()[5..7];
        let expected = dir
            .path()
            .join(TRUST_DIR)
            .join(shard1)
            .join(shard2)
            .join(format!("{trusted}.json"));
        assert!(expected.exists(), "expected trust index at {expected:?}");
        Ok(())
    }

    #[test]
    fn list_trust_for_unknown_truster_is_empty() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let by_actor = fresh_genesis().actor_id();
        let heads = store.list_trust(&by_actor)?;
        assert!(heads.is_empty());
        Ok(())
    }

    #[test]
    fn list_opinions_about_returns_each_truster_head() -> TestResult {
        // Two trusters express opinions about the same trusted actor.
        // list_opinions_about returns both heads.
        let (_dir, store) = open_temp_store()?;
        let truster_a = fresh_genesis().actor_id();
        let truster_b = ActorGenesisBody::new(
            ActorKind::person(),
            PublicKey::ed25519(SigningKey::from_bytes(&[8; 32]).verifying_key().to_bytes()),
            vec![attestation_key()],
            1,
            timestamp(),
            [11; 32],
        )
        .expect("genesis well-formed")
        .actor_id();
        let trusted = trusted_actor_id();

        let a_grant = signed_actor_trust(
            truster_a.clone(),
            trusted.clone(),
            Some(TrustDecision::Trusted),
            None,
            None,
            timestamp(),
        )?;
        let b_block = signed_actor_trust(
            truster_b.clone(),
            trusted.clone(),
            Some(TrustDecision::Untrusted),
            None,
            None,
            timestamp(),
        )?;
        store.put_actor_trust(&a_grant)?;
        store.put_actor_trust(&b_block)?;

        let opinions = store.list_opinions_about(&trusted)?;
        assert_eq!(opinions.len(), 2);
        let trusters: Vec<_> = opinions.iter().map(|h| h.by_actor.clone()).collect();
        assert!(trusters.contains(&truster_a));
        assert!(trusters.contains(&truster_b));
        Ok(())
    }

    #[test]
    fn list_opinions_about_unknown_target_is_empty() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let trusted = trusted_actor_id();
        let opinions = store.list_opinions_about(&trusted)?;
        assert!(opinions.is_empty());
        Ok(())
    }

    #[test]
    fn list_trust_walks_directory_to_find_truster_heads() -> TestResult {
        // Same truster X publishes opinions about two different
        // trusted actors. Each opinion lands in its target's file
        // (different shard paths). list_trust(X) must walk both files.
        let (_dir, store) = open_temp_store()?;
        let truster = fresh_genesis().actor_id();
        let trusted_a = trusted_actor_id();
        let trusted_b = other_trusted_actor_id();
        let about_a = signed_actor_trust(
            truster.clone(),
            trusted_a.clone(),
            Some(TrustDecision::Trusted),
            None,
            None,
            timestamp(),
        )?;
        let about_b = signed_actor_trust(
            truster.clone(),
            trusted_b.clone(),
            Some(TrustDecision::Untrusted),
            None,
            None,
            timestamp(),
        )?;
        store.put_actor_trust(&about_a)?;
        store.put_actor_trust(&about_b)?;

        let heads = store.list_trust(&truster)?;
        assert_eq!(heads.len(), 2);
        let targets: Vec<_> = heads.iter().map(|h| h.trusted_actor.clone()).collect();
        assert!(targets.contains(&trusted_a));
        assert!(targets.contains(&trusted_b));
        Ok(())
    }

    #[test]
    fn separate_trusters_do_not_cross_influence() -> TestResult {
        // Truster A and truster B both publish trust about the same
        // trusted actor. Each truster's index is independent because
        // sharding is on by_actor.
        let (_dir, store) = open_temp_store()?;
        let truster_a = fresh_genesis().actor_id();
        let truster_b = ActorGenesisBody::new(
            ActorKind::person(),
            PublicKey::ed25519(SigningKey::from_bytes(&[8; 32]).verifying_key().to_bytes()),
            vec![attestation_key()],
            1,
            timestamp(),
            [11; 32],
        )
        .expect("genesis well-formed")
        .actor_id();
        let trusted = trusted_actor_id();

        let a_grant = signed_actor_trust(
            truster_a.clone(),
            trusted.clone(),
            Some(TrustDecision::Trusted),
            None,
            None,
            timestamp(),
        )?;
        let b_block = signed_actor_trust(
            truster_b.clone(),
            trusted.clone(),
            Some(TrustDecision::Untrusted),
            None,
            None,
            timestamp(),
        )?;
        store.put_actor_trust(&a_grant)?;
        store.put_actor_trust(&b_block)?;

        let a_head = store.latest_trust(&truster_a, &trusted)?;
        assert!(matches!(
            a_head,
            Some(s) if s.unsigned().body().decision() == Some(TrustDecision::Trusted)
        ));
        let b_head = store.latest_trust(&truster_b, &trusted)?;
        assert!(matches!(
            b_head,
            Some(s) if s.unsigned().body().decision() == Some(TrustDecision::Untrusted)
        ));
        Ok(())
    }

    #[test]
    fn trust_chain_precedence_overrides_timestamp_tiebreak() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let by_actor = fresh_genesis().actor_id();
        let trusted = trusted_actor_id();
        let grant = signed_actor_trust(
            by_actor.clone(),
            trusted.clone(),
            Some(TrustDecision::Trusted),
            None,
            None,
            timestamp(),
        )?;
        let grant_id = store.put_actor_trust(&grant)?;
        let withdraw = signed_actor_trust(
            by_actor.clone(),
            trusted.clone(),
            None,
            None,
            Some(grant_id),
            timestamp(), // same created_at — would tie under pure timestamp+id rule
        )?;
        store.put_actor_trust(&withdraw)?;

        let resolved = store.latest_trust(&by_actor, &trusted)?;
        assert!(matches!(
            resolved,
            Some(s) if s.unsigned().body().is_withdrawal()
        ));
        Ok(())
    }

    fn grantee_actor() -> ActorId {
        ActorGenesisBody::new(
            ActorKind::person(),
            PublicKey::ed25519(SigningKey::from_bytes(&[6; 32]).verifying_key().to_bytes()),
            vec![attestation_key()],
            1,
            timestamp(),
            [55; 32],
        )
        .expect("genesis well-formed")
        .actor_id()
    }

    fn other_grantee_actor() -> ActorId {
        ActorGenesisBody::new(
            ActorKind::person(),
            PublicKey::ed25519(SigningKey::from_bytes(&[7; 32]).verifying_key().to_bytes()),
            vec![attestation_key()],
            1,
            timestamp(),
            [66; 32],
        )
        .expect("genesis well-formed")
        .actor_id()
    }

    fn capability_for(scope: CapabilityScope) -> Result<Capability, Box<dyn std::error::Error>> {
        Ok(Capability::new(
            scope,
            vec![
                StatementKind::ObjectRevision,
                StatementKind::ObjectVersionTag,
                StatementKind::ObjectBranch,
            ],
            true,
            vec![],
        )?)
    }

    fn signed_capability_grant(
        grantor: ActorId,
        grantee: ActorId,
        scope: CapabilityScope,
        supersedes: Option<StatementId>,
        created_at: Timestamp,
    ) -> Result<SignedStatement<ActorCapabilityGrantBody>, Box<dyn std::error::Error>> {
        let capability = capability_for(scope)?;
        let body = ActorCapabilityGrantBody::new(grantee.clone(), capability, supersedes);
        let subject: KairoRef = format!("actor:{grantee}").parse()?;
        let unsigned = UnsignedStatement::new(grantor.clone(), subject, created_at, body);
        let signature_bytes = signing_key().sign(&unsigned.canonical_bytes()).to_bytes();
        let signature = Signature::new(
            grantor,
            public_key().key_id().to_string(),
            "ed25519",
            signature_bytes.to_vec(),
        );
        Ok(SignedStatement::new(unsigned, signature))
    }

    fn signed_capability_revocation(
        grantor: ActorId,
        revoked_grant: StatementId,
        retroactive: bool,
        created_at: Timestamp,
    ) -> Result<SignedStatement<ActorCapabilityRevocationBody>, Box<dyn std::error::Error>> {
        let body = ActorCapabilityRevocationBody::new(revoked_grant.clone(), retroactive, None);
        let subject: KairoRef = format!("statement:{revoked_grant}").parse()?;
        let unsigned = UnsignedStatement::new(grantor.clone(), subject, created_at, body);
        let signature_bytes = signing_key().sign(&unsigned.canonical_bytes()).to_bytes();
        let signature = Signature::new(
            grantor,
            public_key().key_id().to_string(),
            "ed25519",
            signature_bytes.to_vec(),
        );
        Ok(SignedStatement::new(unsigned, signature))
    }

    #[test]
    fn round_trips_actor_capability_grant() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let grantor = fresh_genesis().actor_id();
        let grantee = grantee_actor();
        let scope = CapabilityScope::Object(ObjectId::new(OBJECT_ID)?);
        let signed = signed_capability_grant(grantor, grantee, scope, None, timestamp())?;
        let id = store.put_actor_capability_grant(&signed)?;
        let loaded = store.get_actor_capability_grant(&id)?;
        assert_eq!(loaded, signed);
        Ok(())
    }

    #[test]
    fn round_trips_actor_capability_revocation() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let grantor = fresh_genesis().actor_id();
        let grantee = grantee_actor();
        let scope = CapabilityScope::Object(ObjectId::new(OBJECT_ID)?);
        let grant = signed_capability_grant(grantor.clone(), grantee, scope, None, timestamp())?;
        let grant_id = store.put_actor_capability_grant(&grant)?;
        let revocation = signed_capability_revocation(
            grantor,
            grant_id,
            false,
            Timestamp::from_seconds(timestamp().seconds() + 1),
        )?;
        let revocation_id = store.put_actor_capability_revocation(&revocation)?;
        let loaded = store.get_actor_capability_revocation(&revocation_id)?;
        assert_eq!(loaded, revocation);
        Ok(())
    }

    #[test]
    fn missing_capability_returns_none() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let grantor = fresh_genesis().actor_id();
        let grantee = grantee_actor();
        let scope = CapabilityScope::Object(ObjectId::new(OBJECT_ID)?);
        let resolved = store.latest_capability(&grantor, &grantee, &scope)?;
        assert!(resolved.is_none());
        Ok(())
    }

    #[test]
    fn missing_capability_revocation_returns_none() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let grantor = fresh_genesis().actor_id();
        let revoked_grant = StatementId::from_sha256_digest([0xAA; 32]);
        let resolved = store.latest_capability_revocation(&grantor, &revoked_grant)?;
        assert!(resolved.is_none());
        Ok(())
    }

    #[test]
    fn latest_capability_returns_chain_leaf() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let grantor = fresh_genesis().actor_id();
        let grantee = grantee_actor();
        let scope = CapabilityScope::Object(ObjectId::new(OBJECT_ID)?);
        let genesis = signed_capability_grant(
            grantor.clone(),
            grantee.clone(),
            scope.clone(),
            None,
            timestamp(),
        )?;
        let genesis_id = store.put_actor_capability_grant(&genesis)?;
        let successor = signed_capability_grant(
            grantor.clone(),
            grantee.clone(),
            scope.clone(),
            Some(genesis_id),
            Timestamp::from_seconds(timestamp().seconds() + 1),
        )?;
        store.put_actor_capability_grant(&successor)?;

        let resolved = store.latest_capability(&grantor, &grantee, &scope)?;
        assert_eq!(resolved, Some(successor));
        Ok(())
    }

    #[test]
    fn capability_chain_precedence_overrides_timestamp_tiebreak() -> TestResult {
        // Genesis grant signed first; successor at same created_at
        // explicitly supersedes it. Chain-precedence picks the successor
        // regardless of statement-id ordering.
        let (_dir, store) = open_temp_store()?;
        let grantor = fresh_genesis().actor_id();
        let grantee = grantee_actor();
        let scope = CapabilityScope::Object(ObjectId::new(OBJECT_ID)?);
        let genesis = signed_capability_grant(
            grantor.clone(),
            grantee.clone(),
            scope.clone(),
            None,
            timestamp(),
        )?;
        let genesis_id = store.put_actor_capability_grant(&genesis)?;
        let successor = signed_capability_grant(
            grantor.clone(),
            grantee.clone(),
            scope.clone(),
            Some(genesis_id),
            timestamp(), // same created_at — would tie under pure timestamp+id rule
        )?;
        let successor_id = store.put_actor_capability_grant(&successor)?;

        let resolved = store.latest_capability(&grantor, &grantee, &scope)?;
        assert_eq!(resolved.map(|s| s.statement_id()), Some(successor_id),);
        Ok(())
    }

    #[test]
    fn capabilities_are_independent_per_grantee_and_scope() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let grantor = fresh_genesis().actor_id();
        let grantee_one = grantee_actor();
        let grantee_two = other_grantee_actor();
        let scope = CapabilityScope::Object(ObjectId::new(OBJECT_ID)?);

        let grant_one = signed_capability_grant(
            grantor.clone(),
            grantee_one.clone(),
            scope.clone(),
            None,
            timestamp(),
        )?;
        let grant_two = signed_capability_grant(
            grantor.clone(),
            grantee_two.clone(),
            scope.clone(),
            None,
            timestamp(),
        )?;
        store.put_actor_capability_grant(&grant_one)?;
        store.put_actor_capability_grant(&grant_two)?;

        assert_eq!(
            store.latest_capability(&grantor, &grantee_one, &scope)?,
            Some(grant_one)
        );
        assert_eq!(
            store.latest_capability(&grantor, &grantee_two, &scope)?,
            Some(grant_two)
        );
        Ok(())
    }

    #[test]
    fn revocation_is_recorded_independently_of_grant_chain() -> TestResult {
        // The revocation does not move the grant chain head. The
        // resolver returns the (still-valid) chain leaf and the
        // revocation separately; the full capability evaluator
        // (Step 5) combines them.
        let (_dir, store) = open_temp_store()?;
        let grantor = fresh_genesis().actor_id();
        let grantee = grantee_actor();
        let scope = CapabilityScope::Object(ObjectId::new(OBJECT_ID)?);
        let grant = signed_capability_grant(
            grantor.clone(),
            grantee.clone(),
            scope.clone(),
            None,
            timestamp(),
        )?;
        let grant_id = store.put_actor_capability_grant(&grant)?;
        let revocation = signed_capability_revocation(
            grantor.clone(),
            grant_id.clone(),
            false,
            Timestamp::from_seconds(timestamp().seconds() + 1),
        )?;
        store.put_actor_capability_revocation(&revocation)?;

        // Chain head is still the grant.
        let head = store.latest_capability(&grantor, &grantee, &scope)?;
        assert_eq!(head, Some(grant));
        // But the revocation is reachable via the dedicated lookup.
        let resolved_revocation = store.latest_capability_revocation(&grantor, &grant_id)?;
        assert_eq!(resolved_revocation, Some(revocation));
        Ok(())
    }

    #[test]
    fn retroactive_revocation_overrides_non_retroactive_for_same_grant() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let grantor = fresh_genesis().actor_id();
        let grantee = grantee_actor();
        let scope = CapabilityScope::Object(ObjectId::new(OBJECT_ID)?);
        let grant = signed_capability_grant(grantor.clone(), grantee, scope, None, timestamp())?;
        let grant_id = store.put_actor_capability_grant(&grant)?;
        let non_retroactive = signed_capability_revocation(
            grantor.clone(),
            grant_id.clone(),
            false,
            Timestamp::from_seconds(timestamp().seconds() + 1),
        )?;
        let retroactive = signed_capability_revocation(
            grantor.clone(),
            grant_id.clone(),
            true,
            Timestamp::from_seconds(timestamp().seconds() + 2),
        )?;
        store.put_actor_capability_revocation(&non_retroactive)?;
        store.put_actor_capability_revocation(&retroactive)?;

        let effective = store.latest_capability_revocation(&grantor, &grant_id)?;
        assert!(matches!(
            effective,
            Some(signed) if signed.unsigned().body().retroactive()
        ));
        Ok(())
    }

    #[test]
    fn list_capabilities_returns_all_chain_heads_for_grantor() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let grantor = fresh_genesis().actor_id();
        let grantee_one = grantee_actor();
        let grantee_two = other_grantee_actor();
        let scope = CapabilityScope::Object(ObjectId::new(OBJECT_ID)?);
        let grant_one = signed_capability_grant(
            grantor.clone(),
            grantee_one.clone(),
            scope.clone(),
            None,
            timestamp(),
        )?;
        let grant_two = signed_capability_grant(
            grantor.clone(),
            grantee_two.clone(),
            scope.clone(),
            None,
            timestamp(),
        )?;
        store.put_actor_capability_grant(&grant_one)?;
        store.put_actor_capability_grant(&grant_two)?;

        let heads = store.list_capabilities_from(&grantor)?;
        assert_eq!(heads.len(), 2);
        let grantees: Vec<_> = heads.iter().map(|h| h.grantee.clone()).collect();
        assert!(grantees.contains(&grantee_one));
        assert!(grantees.contains(&grantee_two));
        Ok(())
    }

    #[test]
    fn list_capabilities_for_unknown_grantor_is_empty() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let grantor = fresh_genesis().actor_id();
        let heads = store.list_capabilities_from(&grantor)?;
        assert!(heads.is_empty());
        Ok(())
    }

    #[test]
    fn capability_index_path_is_sharded_on_grantor() -> TestResult {
        let (dir, store) = open_temp_store()?;
        let grantor = fresh_genesis().actor_id();
        let grantee = grantee_actor();
        let scope = CapabilityScope::Object(ObjectId::new(OBJECT_ID)?);
        let signed = signed_capability_grant(grantor.clone(), grantee, scope, None, timestamp())?;
        store.put_actor_capability_grant(&signed)?;

        let shard1 = &grantor.as_str()[3..5];
        let shard2 = &grantor.as_str()[5..7];
        let expected = dir
            .path()
            .join(ACTOR_CAPABILITY_DIR)
            .join(shard1)
            .join(shard2)
            .join(format!("{grantor}.json"));
        assert!(
            expected.exists(),
            "expected capability index at {expected:?}"
        );
        Ok(())
    }

    #[test]
    fn capability_grant_statement_path_is_sharded() -> TestResult {
        let (dir, store) = open_temp_store()?;
        let grantor = fresh_genesis().actor_id();
        let grantee = grantee_actor();
        let scope = CapabilityScope::Object(ObjectId::new(OBJECT_ID)?);
        let signed = signed_capability_grant(grantor, grantee, scope, None, timestamp())?;
        let id = store.put_actor_capability_grant(&signed)?;

        let shard1 = &id.as_str()[3..5];
        let shard2 = &id.as_str()[5..7];
        let expected = dir
            .path()
            .join(STATEMENTS_DIR)
            .join(shard1)
            .join(shard2)
            .join(format!("{id}.json"));
        assert!(expected.exists(), "expected sharded path {expected:?}");
        Ok(())
    }

    #[test]
    fn list_capabilities_for_object_returns_per_grantor_grantee_heads() -> TestResult {
        // Two different grantors each issue an object-scoped grant on
        // the same object, one to grantee_one and one to grantee_two.
        // The reverse index returns both heads.
        let (_dir, store) = open_temp_store()?;
        let grantor_one = fresh_genesis().actor_id();
        let grantor_two = ActorGenesisBody::new(
            ActorKind::person(),
            PublicKey::ed25519(SigningKey::from_bytes(&[8; 32]).verifying_key().to_bytes()),
            vec![attestation_key()],
            1,
            timestamp(),
            [11; 32],
        )
        .expect("genesis well-formed")
        .actor_id();
        let grantee_one = grantee_actor();
        let grantee_two = other_grantee_actor();
        let object = ObjectId::new(OBJECT_ID)?;
        let scope = CapabilityScope::Object(object.clone());

        let grant_one = signed_capability_grant(
            grantor_one.clone(),
            grantee_one.clone(),
            scope.clone(),
            None,
            timestamp(),
        )?;
        let grant_two = signed_capability_grant(
            grantor_two.clone(),
            grantee_two.clone(),
            scope.clone(),
            None,
            timestamp(),
        )?;
        store.put_actor_capability_grant(&grant_one)?;
        store.put_actor_capability_grant(&grant_two)?;

        let heads = store.list_capabilities_for_object(&object)?;
        assert_eq!(heads.len(), 2);
        let pair_one = heads
            .iter()
            .find(|h| h.grantor == grantor_one && h.grantee == grantee_one)
            .expect("(grantor_one, grantee_one) head present");
        assert_eq!(pair_one.statement_id, grant_one.statement_id());
        assert_eq!(pair_one.object, object);
        let pair_two = heads
            .iter()
            .find(|h| h.grantor == grantor_two && h.grantee == grantee_two)
            .expect("(grantor_two, grantee_two) head present");
        assert_eq!(pair_two.statement_id, grant_two.statement_id());
        Ok(())
    }

    #[test]
    fn list_capabilities_for_object_picks_chain_leaf_per_triple() -> TestResult {
        // Same (grantor, grantee, object) triple — successor wins
        // over predecessor in the reverse index.
        let (_dir, store) = open_temp_store()?;
        let grantor = fresh_genesis().actor_id();
        let grantee = grantee_actor();
        let object = ObjectId::new(OBJECT_ID)?;
        let scope = CapabilityScope::Object(object.clone());
        let genesis = signed_capability_grant(
            grantor.clone(),
            grantee.clone(),
            scope.clone(),
            None,
            timestamp(),
        )?;
        let genesis_id = store.put_actor_capability_grant(&genesis)?;
        let successor = signed_capability_grant(
            grantor.clone(),
            grantee.clone(),
            scope,
            Some(genesis_id),
            Timestamp::from_seconds(timestamp().seconds() + 1),
        )?;
        let successor_id = store.put_actor_capability_grant(&successor)?;

        let heads = store.list_capabilities_for_object(&object)?;
        assert_eq!(heads.len(), 1);
        assert_eq!(heads[0].statement_id, successor_id);
        Ok(())
    }

    #[test]
    fn list_capabilities_for_unknown_object_is_empty() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let object = ObjectId::new(OBJECT_ID)?;
        let heads = store.list_capabilities_for_object(&object)?;
        assert!(heads.is_empty());
        Ok(())
    }

    #[test]
    fn capabilities_by_object_index_path_is_sharded() -> TestResult {
        let (dir, store) = open_temp_store()?;
        let grantor = fresh_genesis().actor_id();
        let grantee = grantee_actor();
        let object = ObjectId::new(OBJECT_ID)?;
        let scope = CapabilityScope::Object(object.clone());
        let signed = signed_capability_grant(grantor, grantee, scope, None, timestamp())?;
        store.put_actor_capability_grant(&signed)?;

        let shard1 = &object.as_str()[3..5];
        let shard2 = &object.as_str()[5..7];
        let expected = dir
            .path()
            .join(ACTOR_CAPABILITY_BY_OBJECT_DIR)
            .join(shard1)
            .join(shard2)
            .join(format!("{object}.json"));
        assert!(
            expected.exists(),
            "expected capabilities-by-object index at {expected:?}"
        );
        Ok(())
    }

    #[test]
    fn list_capabilities_for_object_excludes_other_objects() -> TestResult {
        // A grant on object A must not appear when listing object B.
        let (_dir, store) = open_temp_store()?;
        let grantor = fresh_genesis().actor_id();
        let grantee = grantee_actor();
        let object_a = ObjectId::new(OBJECT_ID)?;
        let object_b = ObjectId::from_sha256_digest([99; 32]);
        let scope_a = CapabilityScope::Object(object_a.clone());
        let signed = signed_capability_grant(grantor, grantee, scope_a, None, timestamp())?;
        store.put_actor_capability_grant(&signed)?;

        let heads_b = store.list_capabilities_for_object(&object_b)?;
        assert!(heads_b.is_empty());
        let heads_a = store.list_capabilities_for_object(&object_a)?;
        assert_eq!(heads_a.len(), 1);
        Ok(())
    }

    #[test]
    fn tampered_capability_grant_file_is_corrupt() -> TestResult {
        let (dir, store) = open_temp_store()?;
        let grantor = fresh_genesis().actor_id();
        let grantee = grantee_actor();
        let scope = CapabilityScope::Object(ObjectId::new(OBJECT_ID)?);
        let signed = signed_capability_grant(
            grantor.clone(),
            grantee.clone(),
            scope.clone(),
            None,
            timestamp(),
        )?;
        let id = store.put_actor_capability_grant(&signed)?;

        // Replace the file with a different valid grant whose derived
        // StatementId will not match the original filename.
        let other_scope = CapabilityScope::Object(ObjectId::from_sha256_digest([99; 32]));
        let other = signed_capability_grant(grantor, grantee, other_scope, None, timestamp())?;
        let path = shard::shard_path(dir.path(), STATEMENTS_DIR, id.as_str(), JSON_SUFFIX)
            .map_err(|error| error.to_string())?;
        let json = ActorCapabilityGrantStatementJson::from_statement(&other);
        fs::write(&path, serde_json::to_vec_pretty(&json)?)?;

        assert!(matches!(
            store.get_actor_capability_grant(&id),
            Err(StoreError::Corrupt {
                reason: CorruptReason::HashMismatch { .. },
                ..
            })
        ));
        Ok(())
    }

    #[test]
    fn evaluate_capability_against_filesystem_store_returns_held() -> TestResult {
        // End-to-end: real FilesystemStore is used as the
        // CapabilityResolver behind evaluate_capability. The store's
        // ObjectGenesis pins the root authority; one grant from root
        // to bob is enough for evaluate_capability to return Held.
        let (_dir, store) = open_temp_store()?;
        let root_genesis = fresh_genesis();
        let root_actor = root_genesis.actor_id();
        store.put_actor(&root_genesis)?;

        let object_statement = signed_object_genesis(root_actor.clone());
        let object_id = store.put_object_genesis(&object_statement)?;

        let bob = grantee_actor();
        let scope = CapabilityScope::Object(object_id.clone());
        let grant =
            signed_capability_grant(root_actor.clone(), bob.clone(), scope, None, timestamp())?;
        store.put_actor_capability_grant(&grant)?;

        let evaluation = evaluate_capability(
            &bob,
            &ResolutionTarget::Object {
                id: object_id,
                kind: StatementKind::ObjectVersionTag,
            },
            timestamp(),
            &store,
        )?;
        assert_eq!(evaluation, CapabilityEvaluation::Held);
        Ok(())
    }

    // ---- ActorKeyRotation / ActorKeyRevocation ----

    fn other_public_key() -> PublicKey {
        PublicKey::ed25519(SigningKey::from_bytes(&[8; 32]).verifying_key().to_bytes())
    }

    fn third_public_key() -> PublicKey {
        PublicKey::ed25519(SigningKey::from_bytes(&[10; 32]).verifying_key().to_bytes())
    }

    fn signed_key_rotation(
        actor: ActorId,
        next_key: PublicKey,
        supersedes: Option<StatementId>,
        created_at: Timestamp,
    ) -> Result<SignedStatement<ActorKeyRotationBody>, Box<dyn std::error::Error>> {
        let body = ActorKeyRotationBody::new(next_key, supersedes);
        let subject: KairoRef = format!("actor:{actor}").parse()?;
        let unsigned = UnsignedStatement::new(actor.clone(), subject, created_at, body);
        let signature_bytes = signing_key().sign(&unsigned.canonical_bytes()).to_bytes();
        let signature = Signature::new(
            actor,
            public_key().key_id().to_string(),
            "ed25519",
            signature_bytes.to_vec(),
        );
        Ok(SignedStatement::new(unsigned, signature))
    }

    fn signed_key_revocation(
        actor: ActorId,
        revoked_key: kairo_identity::KeyId,
        retroactive: bool,
        created_at: Timestamp,
    ) -> Result<SignedStatement<ActorKeyRevocationBody>, Box<dyn std::error::Error>> {
        let body = ActorKeyRevocationBody::new(revoked_key, retroactive, None);
        let subject: KairoRef = format!("actor:{actor}").parse()?;
        let unsigned = UnsignedStatement::new(actor.clone(), subject, created_at, body);
        let signature_bytes = signing_key().sign(&unsigned.canonical_bytes()).to_bytes();
        let signature = Signature::new(
            actor,
            public_key().key_id().to_string(),
            "ed25519",
            signature_bytes.to_vec(),
        );
        Ok(SignedStatement::new(unsigned, signature))
    }

    #[test]
    fn round_trips_actor_key_rotation() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let actor = fresh_genesis().actor_id();
        let signed = signed_key_rotation(actor, other_public_key(), None, timestamp())?;
        let id = store.put_actor_key_rotation(&signed)?;
        let loaded = store.get_actor_key_rotation(&id)?;
        assert_eq!(loaded, signed);
        Ok(())
    }

    #[test]
    fn round_trips_actor_key_revocation() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let actor = fresh_genesis().actor_id();
        let signed = signed_key_revocation(actor, other_public_key().key_id(), true, timestamp())?;
        let id = store.put_actor_key_revocation(&signed)?;
        let loaded = store.get_actor_key_revocation(&id)?;
        assert_eq!(loaded, signed);
        Ok(())
    }

    #[test]
    fn key_event_index_path_is_sharded() -> TestResult {
        let (dir, store) = open_temp_store()?;
        let actor = fresh_genesis().actor_id();
        let signed = signed_key_rotation(actor.clone(), other_public_key(), None, timestamp())?;
        store.put_actor_key_rotation(&signed)?;

        let shard1 = &actor.as_str()[3..5];
        let shard2 = &actor.as_str()[5..7];
        let expected = dir
            .path()
            .join(ACTOR_KEYS_DIR)
            .join(shard1)
            .join(shard2)
            .join(format!("{actor}.json"));
        assert!(expected.exists(), "expected key index at {expected:?}");
        Ok(())
    }

    #[test]
    fn active_key_at_falls_back_to_genesis_initial_when_no_rotations() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let actor = fresh_genesis().actor_id();
        store.put_actor(&fresh_genesis())?;

        let resolved = ActorResolver::active_key_at(
            &store,
            &actor,
            Timestamp::from_seconds(timestamp().seconds() + 100),
        )?;
        assert_eq!(resolved, Some(public_key()));
        Ok(())
    }

    #[test]
    fn active_key_at_walks_persisted_rotation_chain() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let actor = fresh_genesis().actor_id();
        store.put_actor(&fresh_genesis())?;

        // First rotation at t+10 to other_public_key.
        let r1 = signed_key_rotation(
            actor.clone(),
            other_public_key(),
            None,
            Timestamp::from_seconds(timestamp().seconds() + 10),
        )?;
        let r1_id = store.put_actor_key_rotation(&r1)?;
        // Successor at t+20 to third_public_key, supersedes r1.
        let r2 = signed_key_rotation(
            actor.clone(),
            third_public_key(),
            Some(r1_id),
            Timestamp::from_seconds(timestamp().seconds() + 20),
        )?;
        store.put_actor_key_rotation(&r2)?;

        // Before any rotation: genesis initial.
        assert_eq!(
            ActorResolver::active_key_at(&store, &actor, timestamp())?,
            Some(public_key())
        );
        // Between rotations: r1 wins.
        assert_eq!(
            ActorResolver::active_key_at(
                &store,
                &actor,
                Timestamp::from_seconds(timestamp().seconds() + 15)
            )?,
            Some(other_public_key())
        );
        // After both rotations: r2 wins.
        assert_eq!(
            ActorResolver::active_key_at(
                &store,
                &actor,
                Timestamp::from_seconds(timestamp().seconds() + 25)
            )?,
            Some(third_public_key())
        );
        Ok(())
    }

    #[test]
    fn is_key_revoked_at_default_only_after_created_at() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let actor = fresh_genesis().actor_id();
        store.put_actor(&fresh_genesis())?;

        let key_id = public_key().key_id();
        let revocation = signed_key_revocation(
            actor.clone(),
            key_id.clone(),
            false,
            Timestamp::from_seconds(timestamp().seconds() + 100),
        )?;
        store.put_actor_key_revocation(&revocation)?;

        assert!(!ActorResolver::is_key_revoked_at(
            &store,
            &actor,
            &key_id,
            timestamp()
        )?);
        assert!(ActorResolver::is_key_revoked_at(
            &store,
            &actor,
            &key_id,
            Timestamp::from_seconds(timestamp().seconds() + 200)
        )?);
        Ok(())
    }

    #[test]
    fn is_key_revoked_at_retroactive_invalidates_history() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let actor = fresh_genesis().actor_id();
        store.put_actor(&fresh_genesis())?;

        let key_id = public_key().key_id();
        let revocation = signed_key_revocation(
            actor.clone(),
            key_id.clone(),
            true,
            Timestamp::from_seconds(timestamp().seconds() + 100),
        )?;
        store.put_actor_key_revocation(&revocation)?;

        // Even before created_at, retroactive=true treats the key as
        // revoked.
        assert!(ActorResolver::is_key_revoked_at(
            &store,
            &actor,
            &key_id,
            timestamp()
        )?);
        Ok(())
    }

    #[test]
    fn most_restrictive_revocation_wins_in_filesystem_index() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let actor = fresh_genesis().actor_id();
        store.put_actor(&fresh_genesis())?;

        let key_id = public_key().key_id();
        // First a non-retroactive revocation at t+100.
        let r_default = signed_key_revocation(
            actor.clone(),
            key_id.clone(),
            false,
            Timestamp::from_seconds(timestamp().seconds() + 100),
        )?;
        store.put_actor_key_revocation(&r_default)?;
        // Then a retroactive revocation at t+200.
        let r_retro = signed_key_revocation(
            actor.clone(),
            key_id.clone(),
            true,
            Timestamp::from_seconds(timestamp().seconds() + 200),
        )?;
        store.put_actor_key_revocation(&r_retro)?;

        // Querying at t (before either revocation): the retroactive
        // one applies and wins.
        assert!(ActorResolver::is_key_revoked_at(
            &store,
            &actor,
            &key_id,
            timestamp()
        )?);
        Ok(())
    }

    // ---- ActorAttestationKeyAdd / ActorAttestationKeyRevocation ----

    fn signed_attestation_key_add(
        actor: ActorId,
        new_key: PublicKey,
        created_at: Timestamp,
    ) -> Result<MultiSignedStatement<ActorAttestationKeyAddBody>, Box<dyn std::error::Error>> {
        let body = ActorAttestationKeyAddBody::new(new_key);
        let subject: KairoRef = format!("actor:{actor}").parse()?;
        let unsigned = UnsignedStatement::new(actor.clone(), subject, created_at, body);
        // The store does not verify signatures; sign with the
        // operational key purely to satisfy the wire shape.
        let signature_bytes = signing_key().sign(&unsigned.canonical_bytes()).to_bytes();
        let signature = Signature::new(
            actor,
            public_key().key_id().to_string(),
            "ed25519",
            signature_bytes.to_vec(),
        );
        Ok(MultiSignedStatement::single(unsigned, signature))
    }

    fn signed_attestation_key_revocation(
        actor: ActorId,
        revoked_key: kairo_identity::KeyId,
        reason: Option<String>,
        created_at: Timestamp,
    ) -> Result<MultiSignedStatement<ActorAttestationKeyRevocationBody>, Box<dyn std::error::Error>>
    {
        let body = ActorAttestationKeyRevocationBody::new(revoked_key, reason);
        let subject: KairoRef = format!("actor:{actor}").parse()?;
        let unsigned = UnsignedStatement::new(actor.clone(), subject, created_at, body);
        let signature_bytes = signing_key().sign(&unsigned.canonical_bytes()).to_bytes();
        let signature = Signature::new(
            actor,
            public_key().key_id().to_string(),
            "ed25519",
            signature_bytes.to_vec(),
        );
        Ok(MultiSignedStatement::single(unsigned, signature))
    }

    #[test]
    fn round_trips_actor_attestation_key_revocation() -> TestResult {
        // Genesis declares one attestation key, an add appends a
        // second, and we revoke the original. The revocation lands
        // because the resulting set is non-empty.
        let (_dir, store) = open_temp_store()?;
        let actor = fresh_genesis().actor_id();
        store.put_actor(&fresh_genesis())?;

        let appended = third_public_key();
        let add = signed_attestation_key_add(
            actor.clone(),
            appended.clone(),
            Timestamp::from_seconds(timestamp().seconds() + 100),
        )?;
        store.put_actor_attestation_key_add(&add)?;

        let rev = signed_attestation_key_revocation(
            actor.clone(),
            attestation_key().key_id(),
            Some("yubikey lost".to_owned()),
            Timestamp::from_seconds(timestamp().seconds() + 200),
        )?;
        let id = store.put_actor_attestation_key_revocation(&rev)?;

        let loaded = store.get_actor_attestation_key_revocation(&id)?;
        assert_eq!(loaded, rev);

        // Resolver composition: at t+300 only the appended key
        // should remain; the genesis attestation key is gone.
        let set = ActorResolver::attestation_keys_at(
            &store,
            &actor,
            Timestamp::from_seconds(timestamp().seconds() + 300),
        )?;
        assert_eq!(set.len(), 1);
        assert!(set.contains_key(&appended.key_id()));
        assert!(!set.contains_key(&attestation_key().key_id()));
        Ok(())
    }

    #[test]
    fn attestation_key_revocation_that_would_empty_set_is_rejected() -> TestResult {
        // The actor only has the genesis-declared attestation key.
        // Revoking it with no replacement would empty the set —
        // store must refuse and write nothing.
        let (_dir, store) = open_temp_store()?;
        let actor = fresh_genesis().actor_id();
        store.put_actor(&fresh_genesis())?;

        let rev = signed_attestation_key_revocation(
            actor.clone(),
            attestation_key().key_id(),
            None,
            Timestamp::from_seconds(timestamp().seconds() + 100),
        )?;
        let result = store.put_actor_attestation_key_revocation(&rev);
        assert!(matches!(result, Err(StoreError::Rejected { .. })));

        // Resolver still sees the genesis attestation key — nothing
        // was persisted.
        let set = ActorResolver::attestation_keys_at(
            &store,
            &actor,
            Timestamp::from_seconds(timestamp().seconds() + 200),
        )?;
        assert_eq!(set.len(), 1);
        assert!(set.contains_key(&attestation_key().key_id()));
        Ok(())
    }

    #[test]
    fn attestation_key_revocation_of_unknown_key_is_redundant_no_op() -> TestResult {
        // A revocation of a key never in the attestation set is a
        // redundant no-op — it persists but doesn't shrink the set
        // (and therefore can't empty it).
        let (_dir, store) = open_temp_store()?;
        let actor = fresh_genesis().actor_id();
        store.put_actor(&fresh_genesis())?;

        let unknown = third_public_key().key_id();
        let rev = signed_attestation_key_revocation(
            actor.clone(),
            unknown,
            None,
            Timestamp::from_seconds(timestamp().seconds() + 100),
        )?;
        let id = store.put_actor_attestation_key_revocation(&rev)?;
        let _ = store.get_actor_attestation_key_revocation(&id)?;

        // Genesis attestation key still in the set.
        let set = ActorResolver::attestation_keys_at(
            &store,
            &actor,
            Timestamp::from_seconds(timestamp().seconds() + 200),
        )?;
        assert_eq!(set.len(), 1);
        assert!(set.contains_key(&attestation_key().key_id()));
        Ok(())
    }

    fn signed_attestation_threshold_change(
        actor: ActorId,
        new_threshold: u8,
        signers: &[u8],
        created_at: Timestamp,
    ) -> Result<MultiSignedStatement<ActorAttestationThresholdChangeBody>, Box<dyn std::error::Error>>
    {
        let body = ActorAttestationThresholdChangeBody::new(new_threshold)?;
        let subject: KairoRef = format!("actor:{actor}").parse()?;
        let unsigned = UnsignedStatement::new(actor.clone(), subject, created_at, body);
        let canonical = unsigned.canonical_bytes();
        let mut signatures = Vec::with_capacity(signers.len());
        for seed in signers {
            let key = ed25519_dalek::SigningKey::from_bytes(&[*seed; 32]);
            let bytes = key.sign(&canonical).to_bytes().to_vec();
            let public = PublicKey::ed25519(key.verifying_key().to_bytes());
            signatures.push(Signature::new(
                actor.clone(),
                public.key_id().to_string(),
                "ed25519",
                bytes,
            ));
        }
        Ok(MultiSignedStatement::new(unsigned, signatures)?)
    }

    #[test]
    fn round_trips_attestation_threshold_change_lower() -> TestResult {
        // Genesis declares two attestation keys with threshold 2;
        // lowering to 1 requires `current` (= 2) signatures — one
        // from each genesis key. The store accepts and indexes it.
        let (_dir, store) = open_temp_store()?;
        // Construct a genesis with two attestation keys at threshold 2.
        let genesis = ActorGenesisBody::new(
            ActorKind::person(),
            public_key(),
            vec![attestation_key(), third_public_key()],
            2,
            timestamp(),
            [9; 32],
        )?;
        let actor = genesis.actor_id();
        store.put_actor(&genesis)?;

        let stmt = signed_attestation_threshold_change(
            actor.clone(),
            1,
            &[200, 10],
            Timestamp::from_seconds(timestamp().seconds() + 50),
        )?;
        let id = store.put_actor_attestation_threshold_change(&stmt)?;
        let loaded = store.get_actor_attestation_threshold_change(&id)?;
        assert_eq!(loaded, stmt);

        // Resolver reflects the new threshold at t+100.
        let threshold = ActorResolver::attestation_threshold_at(
            &store,
            &actor,
            Timestamp::from_seconds(timestamp().seconds() + 100),
        )?;
        assert_eq!(threshold, Some(1));
        Ok(())
    }

    #[test]
    fn attestation_threshold_raise_below_authority_is_rejected() -> TestResult {
        // Genesis: two attestation keys at threshold 1. Raising to 2
        // requires `max(1, 2) = 2` distinct signatures. A single
        // signature is insufficient — store must refuse.
        let (_dir, store) = open_temp_store()?;
        let genesis = ActorGenesisBody::new(
            ActorKind::person(),
            public_key(),
            vec![attestation_key(), third_public_key()],
            1,
            timestamp(),
            [9; 32],
        )?;
        let actor = genesis.actor_id();
        store.put_actor(&genesis)?;

        let stmt = signed_attestation_threshold_change(
            actor.clone(),
            2,
            &[200],
            Timestamp::from_seconds(timestamp().seconds() + 50),
        )?;
        let result = store.put_actor_attestation_threshold_change(&stmt);
        assert!(matches!(result, Err(StoreError::Rejected { .. })));

        // Resolver still sees the genesis threshold.
        let threshold = ActorResolver::attestation_threshold_at(
            &store,
            &actor,
            Timestamp::from_seconds(timestamp().seconds() + 100),
        )?;
        assert_eq!(threshold, Some(1));
        Ok(())
    }

    #[test]
    fn attestation_threshold_change_above_set_size_is_rejected() -> TestResult {
        // Genesis: two attestation keys at threshold 1. A change to 3
        // would brick all future emergency events because the set has
        // only 2 keys; refuse.
        let (_dir, store) = open_temp_store()?;
        let genesis = ActorGenesisBody::new(
            ActorKind::person(),
            public_key(),
            vec![attestation_key(), third_public_key()],
            1,
            timestamp(),
            [9; 32],
        )?;
        let actor = genesis.actor_id();
        store.put_actor(&genesis)?;

        let stmt = signed_attestation_threshold_change(
            actor.clone(),
            3,
            &[200, 10],
            Timestamp::from_seconds(timestamp().seconds() + 50),
        )?;
        let result = store.put_actor_attestation_threshold_change(&stmt);
        assert!(matches!(result, Err(StoreError::Rejected { .. })));
        Ok(())
    }

    /// Concurrent writers to the same branch index file must all
    /// succeed and produce the union of their entries — no lost
    /// updates, no corrupt JSON. Without `with_index_lock`, two
    /// writers reading the same `BranchIndexFile::default()` would
    /// each upsert against their private copy and the slower writer
    /// would overwrite the faster one's entry on `atomic_write`.
    #[test]
    fn concurrent_branch_writes_do_not_lose_updates() -> TestResult {
        const THREADS: usize = 8;
        let (_dir, store) = open_temp_store()?;
        let actor = fresh_genesis().actor_id();
        let object = ObjectId::new(OBJECT_ID)?;
        store.put_actor(&fresh_genesis())?;

        std::thread::scope(|scope| {
            for index in 0..THREADS {
                let store = store.clone();
                let actor = actor.clone();
                let object = object.clone();
                scope.spawn(move || {
                    let name = format!("branch-{index}");
                    let revision = StatementId::from_sha256_digest([index as u8; 32]);
                    let signed = signed_branch(actor, object, &name, revision, None, timestamp())
                        .expect("branch signed");
                    store.put_object_branch(&signed).expect("put_object_branch");
                });
            }
        });

        let tips = store.list_branches(&object)?;
        assert_eq!(tips.len(), THREADS, "all branch entries should survive");
        let mut names: Vec<_> = tips.iter().map(|tip| tip.name.clone()).collect();
        names.sort();
        for index in 0..THREADS {
            assert!(
                names.contains(&format!("branch-{index}")),
                "branch-{index} should be in index"
            );
        }
        Ok(())
    }

    #[test]
    fn list_statements_by_actor_unknown_actor_is_empty() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let actor = fresh_genesis().actor_id();
        assert!(store.list_statements_by_actor(&actor)?.is_empty());
        Ok(())
    }

    #[test]
    fn list_statements_by_actor_aggregates_kinds_in_chronological_order() -> TestResult {
        // One actor signs three different envelope kinds at different
        // timestamps. The per-actor index should pick all three up and
        // return them sorted by (created_at, statement_id) ascending.
        let (_dir, store) = open_temp_store()?;
        let actor = fresh_genesis().actor_id();
        let object = ObjectId::new(OBJECT_ID)?;

        let revision = signed_revision(actor.clone())?;
        let revision_id = store.put_object_revision(&revision)?;

        let branch = signed_branch(
            actor.clone(),
            object,
            "head",
            StatementId::from_sha256_digest([0xAA; 32]),
            None,
            Timestamp::from_seconds(timestamp().seconds() + 1),
        )?;
        let branch_id = store.put_object_branch(&branch)?;

        let trusted = trusted_actor_id();
        let trust = signed_actor_trust(
            actor.clone(),
            trusted,
            Some(TrustDecision::Trusted),
            None,
            None,
            Timestamp::from_seconds(timestamp().seconds() + 2),
        )?;
        let trust_id = store.put_actor_trust(&trust)?;

        let summaries = store.list_statements_by_actor(&actor)?;
        assert_eq!(summaries.len(), 3);
        assert_eq!(summaries[0].statement_id, revision_id);
        assert_eq!(summaries[0].kind, StatementKind::ObjectRevision);
        assert_eq!(summaries[1].statement_id, branch_id);
        assert_eq!(summaries[1].kind, StatementKind::ObjectBranch);
        assert_eq!(summaries[2].statement_id, trust_id);
        assert_eq!(summaries[2].kind, StatementKind::ActorTrust);
        Ok(())
    }

    #[test]
    fn list_statements_by_actor_isolates_per_actor() -> TestResult {
        // Two distinct actors each sign one statement; each actor's
        // index returns only their own.
        let (_dir, store) = open_temp_store()?;
        let actor_a = fresh_genesis().actor_id();
        let actor_b = trusted_actor_id();
        let object = ObjectId::new(OBJECT_ID)?;

        let a_branch = signed_branch(
            actor_a.clone(),
            object.clone(),
            "head",
            StatementId::from_sha256_digest([0x01; 32]),
            None,
            timestamp(),
        )?;
        let a_branch_id = store.put_object_branch(&a_branch)?;

        let b_trust = signed_actor_trust(
            actor_b.clone(),
            actor_a.clone(),
            Some(TrustDecision::Trusted),
            None,
            None,
            timestamp(),
        )?;
        let b_trust_id = store.put_actor_trust(&b_trust)?;

        let a_list = store.list_statements_by_actor(&actor_a)?;
        assert_eq!(a_list.len(), 1);
        assert_eq!(a_list[0].statement_id, a_branch_id);

        let b_list = store.list_statements_by_actor(&actor_b)?;
        assert_eq!(b_list.len(), 1);
        assert_eq!(b_list[0].statement_id, b_trust_id);
        Ok(())
    }

    #[test]
    fn list_statements_by_actor_index_path_is_sharded_on_actor() -> TestResult {
        let (dir, store) = open_temp_store()?;
        let actor = fresh_genesis().actor_id();
        let signed = signed_revision(actor.clone())?;
        store.put_object_revision(&signed)?;

        let shard1 = &actor.as_str()[3..5];
        let shard2 = &actor.as_str()[5..7];
        let expected = dir
            .path()
            .join(STATEMENTS_BY_ACTOR_DIR)
            .join(shard1)
            .join(shard2)
            .join(format!("{actor}.json"));
        assert!(
            expected.exists(),
            "expected statements_by_actor index at {expected:?}"
        );
        Ok(())
    }

    // -----------------------------------------------------------------
    // rebuild_indexes

    #[test]
    fn rebuild_indexes_on_empty_store_is_noop() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let report = store.rebuild_indexes()?;
        assert_eq!(report.statements_scanned, 0);
        assert!(report.by_kind.is_empty());
        Ok(())
    }

    type SeededStore = (
        TempDir,
        FilesystemStore,
        ActorId,
        ObjectId,
        ResolverSnapshot,
    );

    /// Seed a store with one statement of every signed-envelope
    /// kind we currently exercise from the test helpers (revision,
    /// branch, version-tag, trust, capability grant). Returns the
    /// store + the actor + a snapshot of the resolver outputs that
    /// callers can compare against post-rebuild.
    fn seed_mixed_store() -> Result<SeededStore, Box<dyn std::error::Error>> {
        let (dir, store) = open_temp_store()?;
        let actor = fresh_genesis().actor_id();
        let object = ObjectId::new(OBJECT_ID)?;
        let trusted = trusted_actor_id();
        let grantee = grantee_actor();

        let revision = signed_revision(actor.clone())?;
        store.put_object_revision(&revision)?;

        let branch = signed_branch(
            actor.clone(),
            object.clone(),
            "head",
            StatementId::from_sha256_digest([0xAA; 32]),
            None,
            timestamp(),
        )?;
        store.put_object_branch(&branch)?;

        let tag_body = ObjectVersionTagBody::new(
            object.clone(),
            SemverVersion::parse("1.0.0").expect("semver"),
            Some(StatementId::from_sha256_digest([0xBB; 32])),
            None,
        )
        .expect("tag body");
        let tag_subject: KairoRef = format!("object:{object}").parse()?;
        let tag_unsigned =
            UnsignedStatement::new(actor.clone(), tag_subject, timestamp(), tag_body);
        let tag_sig_bytes = signing_key()
            .sign(&tag_unsigned.canonical_bytes())
            .to_bytes();
        let tag_sig = Signature::new(
            actor.clone(),
            public_key().key_id().to_string(),
            "ed25519",
            tag_sig_bytes.to_vec(),
        );
        let tag = SignedStatement::new(tag_unsigned, tag_sig);
        store.put_object_version_tag(&tag)?;

        let trust = signed_actor_trust(
            actor.clone(),
            trusted,
            Some(TrustDecision::Trusted),
            None,
            None,
            timestamp(),
        )?;
        store.put_actor_trust(&trust)?;

        let grant = signed_capability_grant(
            actor.clone(),
            grantee,
            CapabilityScope::Object(object.clone()),
            None,
            timestamp(),
        )?;
        store.put_actor_capability_grant(&grant)?;

        let snapshot = ResolverSnapshot::capture(&store, &actor, &object)?;
        Ok((dir, store, actor, object, snapshot))
    }

    /// Snapshot of every resolver answer we want rebuild to
    /// reproduce. Equality on this struct is the test assertion.
    #[derive(Debug, PartialEq, Eq)]
    struct ResolverSnapshot {
        branches: Vec<BranchTip>,
        version_tags: Vec<VersionTagHead>,
        trust_about: Vec<TrustHead>,
        caps_from: Vec<CapabilityHead>,
        caps_for_object: Vec<CapabilityByObjectHead>,
        statements_by_actor: Vec<StatementByActor>,
    }

    impl ResolverSnapshot {
        fn capture(
            store: &FilesystemStore,
            actor: &ActorId,
            object: &ObjectId,
        ) -> Result<Self, StoreError> {
            let mut branches = store.list_branches(object)?;
            branches.sort_by(|a, b| a.statement_id.cmp(&b.statement_id));
            let mut version_tags = store.list_version_tags(object)?;
            version_tags.sort_by(|a, b| a.statement_id.cmp(&b.statement_id));
            let mut trust_about = store.list_trust(actor)?;
            trust_about.sort_by(|a, b| a.statement_id.cmp(&b.statement_id));
            let mut caps_from = store.list_capabilities_from(actor)?;
            caps_from.sort_by(|a, b| a.statement_id.cmp(&b.statement_id));
            let mut caps_for_object = store.list_capabilities_for_object(object)?;
            caps_for_object.sort_by(|a, b| a.statement_id.cmp(&b.statement_id));
            let mut statements_by_actor = store.list_statements_by_actor(actor)?;
            statements_by_actor.sort_by(|a, b| a.statement_id.cmp(&b.statement_id));
            Ok(Self {
                branches,
                version_tags,
                trust_about,
                caps_from,
                caps_for_object,
                statements_by_actor,
            })
        }
    }

    #[test]
    fn rebuild_indexes_reproduces_resolver_output() -> TestResult {
        let (_dir, store, actor, object, before) = seed_mixed_store()?;

        let report = store.rebuild_indexes()?;
        assert_eq!(report.statements_scanned, 5);
        assert_eq!(report.by_kind.get(&StatementKind::ObjectRevision), Some(&1));
        assert_eq!(report.by_kind.get(&StatementKind::ObjectBranch), Some(&1));
        assert_eq!(
            report.by_kind.get(&StatementKind::ObjectVersionTag),
            Some(&1)
        );
        assert_eq!(report.by_kind.get(&StatementKind::ActorTrust), Some(&1));
        assert_eq!(
            report.by_kind.get(&StatementKind::ActorCapabilityGrant),
            Some(&1)
        );

        let after = ResolverSnapshot::capture(&store, &actor, &object)?;
        assert_eq!(after, before);
        Ok(())
    }

    #[test]
    fn rebuild_indexes_recovers_from_blown_away_index_dirs() -> TestResult {
        let (dir, store, actor, object, before) = seed_mixed_store()?;

        // Blow away every materialized-index directory, leaving
        // statements/ as the source of truth. Resolvers should now
        // return empty (no index), then rebuild restores them.
        for index_dir in [
            BRANCHES_DIR,
            VERSION_TAGS_DIR,
            TRUST_DIR,
            ACTOR_CAPABILITY_DIR,
            ACTOR_CAPABILITY_BY_OBJECT_DIR,
            STATEMENTS_BY_ACTOR_DIR,
        ] {
            let path = dir.path().join(index_dir);
            if path.exists() {
                fs::remove_dir_all(&path)?;
            }
        }
        // Sanity: at least one resolver returns nothing now.
        assert!(store.list_branches(&object)?.is_empty());

        let report = store.rebuild_indexes()?;
        assert_eq!(report.statements_scanned, 5);

        let after = ResolverSnapshot::capture(&store, &actor, &object)?;
        assert_eq!(after, before);
        Ok(())
    }

    #[test]
    fn rebuild_indexes_recovers_from_corrupt_index_file() -> TestResult {
        let (dir, store, actor, object, before) = seed_mixed_store()?;

        // Replace one branch index file with garbage. Reads through
        // the resolver should now error out, then rebuild restores
        // a consistent answer.
        let branch_path = shard::shard_path(dir.path(), BRANCHES_DIR, object.as_str(), JSON_SUFFIX)
            .map_err(|e| e.to_string())?;
        fs::write(&branch_path, b"{ this is not json }")?;
        assert!(matches!(
            store.list_branches(&object),
            Err(StoreError::Corrupt {
                reason: CorruptReason::Parse(_),
                ..
            })
        ));

        store.rebuild_indexes()?;

        let after = ResolverSnapshot::capture(&store, &actor, &object)?;
        assert_eq!(after, before);
        Ok(())
    }

    #[test]
    fn rebuild_indexes_rejects_object_genesis_under_statements_dir() -> TestResult {
        let (dir, store) = open_temp_store()?;
        let actor = fresh_genesis().actor_id();

        // Drop a (well-formed-but-misplaced) ObjectGenesis JSON
        // into statements/. The rebuild's whole-tree dispatch
        // should refuse to silently absorb it.
        let genesis = signed_object_genesis(actor);
        let object_id = genesis.object_id();
        let json = ObjectGenesisStatementJson::from_statement(&genesis);
        let bytes = serde_json::to_vec_pretty(&json)?;
        let stmt_path =
            shard::shard_path(dir.path(), STATEMENTS_DIR, object_id.as_str(), JSON_SUFFIX)
                .map_err(|e| e.to_string())?;
        atomic_write(&stmt_path, &bytes)?;

        let err = store.rebuild_indexes().expect_err("should reject");
        assert!(
            matches!(
                &err,
                StoreError::Corrupt { reason: CorruptReason::Parse(msg), .. }
                    if msg.contains("ObjectGenesis must not appear in statements/")
            ),
            "unexpected error: {err:?}",
        );
        Ok(())
    }
}
