//! Local filesystem store for Kairo statements, actors, and blobs.
//!
//! The MVP is a direct-mode (single-process) store rooted at a path on
//! disk. Records are sharded under each record-type directory using two
//! levels of two base58 characters — see [`shard`] for details.
//!
//! Three independent traits split the responsibilities:
//!
//! - [`ActorStore`] — actor genesis bodies, indexed by [`ActorId`].
//! - [`ObjectStore`] — signed `ObjectGenesis` statements, indexed by
//!   [`ObjectId`].
//! - [`StatementStore`] — signed envelope statements (`ObjectRevision`,
//!   `ObjectBranch`), indexed by [`StatementId`].
//! - [`BlobStore`] — raw bytes addressed by a caller-supplied [`BlobId`].
//!
//! [`BranchResolver`] resolves the current `(actor, object, name)`
//! `ObjectBranch` tip via a per-object materialized index that
//! `put_object_branch` keeps in sync.
//!
//! [`FilesystemStore`] implements all three. It also implements
//! [`ActorResolver`] so `kairo-statement::verify` consumes a store directly.
//!
//! Future work (revisit at the moments noted):
//! - Multi-process safety (file locks): land soon after MVP structure is
//!   stable; the trait boundary already isolates callers from this change.
//! - SQLite or other index backends: defer until query patterns demand it.
//! - Canonical-binary sidecars (`.canonical` files) preserving exact signed
//!   bytes for federation/package exchange: revisit at TODO §9.

mod branches;
pub mod error;
mod shard;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use kairo_core::{ActorId, BlobId, ObjectId, StatementId};
use kairo_identity::json::ActorGenesisJson;
use kairo_identity::{ActorGenesisBody, ActorResolveError, ActorResolver};
use kairo_statement::json::{
    ObjectBranchStatementJson, ObjectGenesisStatementJson, ObjectRevisionStatementJson,
};
use kairo_statement::{
    ObjectBranchBody, ObjectGenesisStatement, ObjectRevisionBody, SignedStatement,
};

pub use branches::BranchTip;
pub use error::{CorruptReason, StoreError};

const STORE_VERSION: &str = "1";
const VERSION_FILE: &str = "version.txt";

const ACTORS_DIR: &str = "actors";
const OBJECTS_DIR: &str = "objects";
const STATEMENTS_DIR: &str = "statements";
const BRANCHES_DIR: &str = "branches";
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
/// Today `ObjectRevision` and `ObjectBranch` are supported. New statement
/// types add new methods alongside; do not collapse them into a single
/// generic until the shape of more statement types is known.
///
/// `put_object_branch` also updates the per-object branch tip index, so a
/// later `BranchResolver::latest_branch` call returns the new tip without
/// scanning all branch statements.
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
        ObjectGenesisBody, ObjectGenesisStatement, ObjectKind, ObjectRevisionBody, RevisionId,
        Signature, SignedStatement, UnsignedStatement,
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
}
