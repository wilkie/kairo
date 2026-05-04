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
//! `put_actor_capability_grant` / `put_actor_capability_revocation`.
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
pub mod error;
mod shard;
mod tags;
mod trust;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use kairo_core::{ActorId, BlobId, ObjectId, StatementId};
use kairo_identity::json::ActorGenesisJson;
use kairo_identity::{ActorGenesisBody, ActorResolveError, ActorResolver};
use kairo_statement::json::{
    ActorCapabilityGrantStatementJson, ActorCapabilityRevocationStatementJson,
    ActorTrustStatementJson, ObjectBranchStatementJson, ObjectGenesisStatementJson,
    ObjectRevisionStatementJson, ObjectVersionTagStatementJson,
};
use kairo_statement::{
    ActorCapabilityGrantBody, ActorCapabilityRevocationBody, ActorTrustBody, CapabilityScope,
    ObjectBranchBody, ObjectGenesisStatement, ObjectRevisionBody, ObjectVersionTagBody,
    SignedStatement,
};

pub use branches::BranchTip;
pub use capabilities::{CapabilityHead, CapabilityRevocationRecord};
pub use error::{CorruptReason, StoreError};
pub use tags::VersionTagHead;
pub use trust::TrustHead;

const STORE_VERSION: &str = "1";
const VERSION_FILE: &str = "version.txt";

const ACTORS_DIR: &str = "actors";
const OBJECTS_DIR: &str = "objects";
const STATEMENTS_DIR: &str = "statements";
const BRANCHES_DIR: &str = "branches";
const VERSION_TAGS_DIR: &str = "version_tags";
const TRUST_DIR: &str = "trust";
const ACTOR_CAPABILITY_DIR: &str = "actor_capability";
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
    fn list_opinions_about(
        &self,
        trusted_actor: &ActorId,
    ) -> Result<Vec<TrustHead>, StoreError>;
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
    fn list_capabilities_from(&self, grantor: &ActorId)
        -> Result<Vec<CapabilityHead>, StoreError>;
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

        let actor = statement.unsigned().actor();
        let object = statement.unsigned().body().object();
        let name = statement.unsigned().body().name();
        let created_at = statement.unsigned().created_at();
        self.upsert_branch_index(object, actor, name, &id, created_at)?;

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

        let actor = statement.unsigned().actor();
        let body = statement.unsigned().body();
        let object = body.object();
        let version = body.version().as_str();
        let created_at = statement.unsigned().created_at();
        let supersedes = body.supersedes();
        self.upsert_version_tag_index(object, actor, version, &id, created_at, supersedes)?;

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

        let by_actor = statement.unsigned().actor();
        let body = statement.unsigned().body();
        let trusted_actor = body.trusted_actor();
        let created_at = statement.unsigned().created_at();
        let decision = body.decision().map(|d| d.as_str());
        let supersedes = body.supersedes();
        self.upsert_trust_index(
            trusted_actor,
            by_actor,
            &id,
            created_at,
            decision,
            supersedes,
        )?;

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

        let grantor = statement.unsigned().actor();
        let body = statement.unsigned().body();
        let grantee = body.grantee();
        let scope = body.capability().scope();
        let created_at = statement.unsigned().created_at();
        let supersedes = body.supersedes();
        self.upsert_capability_grant_index(
            grantor, grantee, scope, &id, created_at, supersedes,
        )?;

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

        let grantor = statement.unsigned().actor();
        let body = statement.unsigned().body();
        let revoked_grant = body.revoked_grant();
        let created_at = statement.unsigned().created_at();
        let retroactive = body.retroactive();
        self.upsert_capability_revocation_index(
            grantor,
            revoked_grant,
            &id,
            created_at,
            retroactive,
        )?;

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
}

impl FilesystemStore {
    fn upsert_branch_index(
        &self,
        object: &ObjectId,
        actor: &ActorId,
        name: &str,
        statement_id: &StatementId,
        created_at: kairo_core::Timestamp,
    ) -> Result<(), StoreError> {
        let path = self.shard_path(BRANCHES_DIR, object.as_str(), JSON_SUFFIX)?;
        let mut index =
            match fs::read(&path) {
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

        let updated = index.upsert(actor, name, statement_id, created_at);
        if updated {
            let bytes = serde_json::to_vec_pretty(&index).map_err(json_to_corrupt(object))?;
            atomic_write(&path, &bytes)?;
        }
        Ok(())
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
        let mut index = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<trust::TrustIndexFile>(&bytes).map_err(
                |error| StoreError::Corrupt {
                    id: trusted_actor.to_string(),
                    reason: CorruptReason::Parse(format!("invalid trust index: {error}")),
                },
            )?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => trust::TrustIndexFile::default(),
            Err(error) => return Err(StoreError::Unavailable(error)),
        };

        let updated = index.upsert(by_actor, statement_id, created_at, decision, supersedes);
        if updated {
            let bytes = serde_json::to_vec_pretty(&index).map_err(json_to_corrupt(trusted_actor))?;
            atomic_write(&path, &bytes)?;
        }
        Ok(())
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

    fn upsert_capability_grant_index(
        &self,
        grantor: &ActorId,
        grantee: &ActorId,
        scope: &CapabilityScope,
        statement_id: &StatementId,
        created_at: kairo_core::Timestamp,
        supersedes: Option<&StatementId>,
    ) -> Result<(), StoreError> {
        let mut index = self.read_capability_index_or_default(grantor)?;
        let updated = index.upsert_grant(grantee, scope, statement_id, created_at, supersedes);
        if updated {
            let path = self.shard_path(ACTOR_CAPABILITY_DIR, grantor.as_str(), JSON_SUFFIX)?;
            let bytes = serde_json::to_vec_pretty(&index).map_err(json_to_corrupt(grantor))?;
            atomic_write(&path, &bytes)?;
        }
        Ok(())
    }

    fn upsert_capability_revocation_index(
        &self,
        grantor: &ActorId,
        revoked_grant: &StatementId,
        statement_id: &StatementId,
        created_at: kairo_core::Timestamp,
        retroactive: bool,
    ) -> Result<(), StoreError> {
        let mut index = self.read_capability_index_or_default(grantor)?;
        let updated =
            index.upsert_revocation(revoked_grant, statement_id, created_at, retroactive);
        if updated {
            let path = self.shard_path(ACTOR_CAPABILITY_DIR, grantor.as_str(), JSON_SUFFIX)?;
            let bytes = serde_json::to_vec_pretty(&index).map_err(json_to_corrupt(grantor))?;
            atomic_write(&path, &bytes)?;
        }
        Ok(())
    }

    fn read_capability_index_or_default(
        &self,
        grantor: &ActorId,
    ) -> Result<capabilities::CapabilityIndexFile, StoreError> {
        Ok(self
            .read_capability_index(grantor)?
            .unwrap_or_default())
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
                        reason: CorruptReason::Parse(format!(
                            "invalid capability index: {error}"
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
                    let index: trust::TrustIndexFile = serde_json::from_slice(&bytes)
                        .map_err(|error| StoreError::Corrupt {
                            id: stem.to_string(),
                            reason: CorruptReason::Parse(format!(
                                "invalid trust index: {error}"
                            )),
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
        let Some(entry) = index.lookup(actor, name) else {
            return Ok(None);
        };
        let statement_id =
            StatementId::new(entry.statement_id.clone()).map_err(|error| StoreError::Corrupt {
                id: entry.statement_id.clone(),
                reason: CorruptReason::Parse(format!(
                    "invalid statement id in branch index: {error}"
                )),
            })?;
        let signed = self.get_object_branch(&statement_id)?;

        if signed.unsigned().actor() != actor
            || signed.unsigned().body().object() != object
            || signed.unsigned().body().name() != name
        {
            return Err(StoreError::Corrupt {
                id: statement_id.to_string(),
                reason: CorruptReason::Parse(
                    "branch index points at a statement with mismatched (actor, object, name)"
                        .to_owned(),
                ),
            });
        }

        Ok(Some(signed))
    }

    fn list_branches(&self, object: &ObjectId) -> Result<Vec<BranchTip>, StoreError> {
        let Some(index) = self.read_branch_index(object)? else {
            return Ok(Vec::new());
        };
        index.into_tips(object)
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
        let statement_id =
            StatementId::new(entry.statement_id.clone()).map_err(|error| StoreError::Corrupt {
                id: entry.statement_id.clone(),
                reason: CorruptReason::Parse(format!(
                    "invalid statement id in version tag index: {error}"
                )),
            })?;
        let signed = self.get_object_version_tag(&statement_id)?;

        if signed.unsigned().actor() != actor
            || signed.unsigned().body().object() != object
            || signed.unsigned().body().version().as_str() != version
        {
            return Err(StoreError::Corrupt {
                id: statement_id.to_string(),
                reason: CorruptReason::Parse(
                    "version tag index points at a statement with mismatched (actor, object, version)"
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
        index.into_heads(object)
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

    fn list_opinions_about(
        &self,
        trusted_actor: &ActorId,
    ) -> Result<Vec<TrustHead>, StoreError> {
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

    fn list_capabilities_from(
        &self,
        grantor: &ActorId,
    ) -> Result<Vec<CapabilityHead>, StoreError> {
        let Some(index) = self.read_capability_index(grantor)? else {
            return Ok(Vec::new());
        };
        index.into_heads(grantor)
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
mod tests {
    use std::fs;

    use ed25519_dalek::{Signer, SigningKey};
    use kairo_core::canonical::CanonicalEncode;
    use kairo_core::{BlobId, KairoRef, ObjectId, Timestamp};
    use kairo_identity::{ActorGenesisBody, ActorKind, PublicKey};
    use kairo_statement::verify::{verify_envelope_statement, ActorResolution, SignatureStatus};
    use kairo_statement::{
        ActorCapabilityGrantBody, ActorCapabilityRevocationBody, ActorTrustBody, Capability,
        CapabilityScope, ObjectGenesisBody, ObjectGenesisStatement, ObjectKind,
        ObjectRevisionBody, ObjectVersionTagBody, RevisionId, SemverVersion, Signature,
        SignedStatement, StatementKind, TrustDecision, UnsignedStatement,
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

    fn timestamp() -> Timestamp {
        Timestamp::from_seconds(1_700_000_000)
    }

    fn fresh_genesis() -> ActorGenesisBody {
        ActorGenesisBody::new(ActorKind::person(), public_key(), timestamp(), [9; 32])
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
        let different =
            ActorGenesisBody::new(ActorKind::person(), public_key(), timestamp(), [10; 32]);
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
        created_at: Timestamp,
    ) -> Result<SignedStatement<ObjectBranchBody>, Box<dyn std::error::Error>> {
        let body = ObjectBranchBody::new(object.clone(), name, revision);
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
        let signed = signed_branch(actor, object, "head", revision, timestamp())?;
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
            Timestamp::from_seconds(timestamp().seconds()),
        )?;
        let later = signed_branch(
            actor.clone(),
            object.clone(),
            "head",
            StatementId::from_sha256_digest([0xBB; 32]),
            Timestamp::from_seconds(timestamp().seconds() + 1),
        )?;
        store.put_object_branch(&earlier)?;
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
        let later = signed_branch(
            actor.clone(),
            object.clone(),
            "head",
            StatementId::from_sha256_digest([0xBB; 32]),
            Timestamp::from_seconds(timestamp().seconds() + 1),
        )?;
        let earlier = signed_branch(
            actor.clone(),
            object.clone(),
            "head",
            StatementId::from_sha256_digest([0xAA; 32]),
            Timestamp::from_seconds(timestamp().seconds()),
        )?;
        // Insert later first, then an earlier write should not move the index.
        store.put_object_branch(&later)?;
        store.put_object_branch(&earlier)?;

        let resolved = store.latest_branch(&actor, &object, "head")?;
        assert_eq!(resolved, Some(later));
        // But the earlier branch is still on disk by its statement id.
        assert!(store.get_object_branch(&earlier.statement_id()).is_ok());
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
            timestamp(),
        )?;
        let release = signed_branch(
            actor.clone(),
            object.clone(),
            "release",
            StatementId::from_sha256_digest([0xCC; 32]),
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
            timestamp(),
        )?;
        let release = signed_branch(
            actor.clone(),
            object.clone(),
            "release",
            StatementId::from_sha256_digest([0xCC; 32]),
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
        assert!(expected.exists(), "expected version tag index at {expected:?}");
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
    fn cross_actor_supersedes_does_not_replace_per_actor_head() -> TestResult {
        // Actor B signs a tag whose supersedes points at actor A's
        // tag. The MVP per-actor resolver intentionally does not honor
        // cross-actor supersession (that requires the §10 capability
        // model). A's head stays A's tag; B has its own head.
        let (_dir, store) = open_temp_store()?;
        let actor_a = fresh_genesis().actor_id();
        let actor_b = ActorGenesisBody::new(
            ActorKind::person(),
            PublicKey::ed25519(SigningKey::from_bytes(&[8; 32]).verifying_key().to_bytes()),
            timestamp(),
            [11; 32],
        )
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
            timestamp(),
            [33; 32],
        )
        .actor_id()
    }

    fn other_trusted_actor_id() -> ActorId {
        ActorGenesisBody::new(
            ActorKind::person(),
            PublicKey::ed25519(SigningKey::from_bytes(&[6; 32]).verifying_key().to_bytes()),
            timestamp(),
            [44; 32],
        )
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
            timestamp(),
            [11; 32],
        )
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
            timestamp(),
            [11; 32],
        )
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
            timestamp(),
            [55; 32],
        )
        .actor_id()
    }

    fn other_grantee_actor() -> ActorId {
        ActorGenesisBody::new(
            ActorKind::person(),
            PublicKey::ed25519(SigningKey::from_bytes(&[7; 32]).verifying_key().to_bytes()),
            timestamp(),
            [66; 32],
        )
        .actor_id()
    }

    fn capability_for(scope: CapabilityScope) -> Result<Capability, Box<dyn std::error::Error>> {
        Ok(Capability::new(
            scope,
            vec![
                StatementKind::ObjectRevision,
                StatementKind::ObjectVersionTag,
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
        let body =
            ActorCapabilityRevocationBody::new(revoked_grant.clone(), retroactive, None);
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
        let grant = signed_capability_grant(
            grantor.clone(),
            grantee,
            scope,
            None,
            timestamp(),
        )?;
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
        assert_eq!(
            resolved.map(|s| s.statement_id()),
            Some(successor_id),
        );
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
        let resolved_revocation =
            store.latest_capability_revocation(&grantor, &grant_id)?;
        assert_eq!(resolved_revocation, Some(revocation));
        Ok(())
    }

    #[test]
    fn retroactive_revocation_overrides_non_retroactive_for_same_grant() -> TestResult {
        let (_dir, store) = open_temp_store()?;
        let grantor = fresh_genesis().actor_id();
        let grantee = grantee_actor();
        let scope = CapabilityScope::Object(ObjectId::new(OBJECT_ID)?);
        let grant = signed_capability_grant(
            grantor.clone(),
            grantee,
            scope,
            None,
            timestamp(),
        )?;
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
        let signed = signed_capability_grant(
            grantor.clone(),
            grantee,
            scope,
            None,
            timestamp(),
        )?;
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
        let signed = signed_capability_grant(
            grantor,
            grantee,
            scope,
            None,
            timestamp(),
        )?;
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
        let other = signed_capability_grant(
            grantor,
            grantee,
            other_scope,
            None,
            timestamp(),
        )?;
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
}
