//! Bundle writer.
//!
//! Walks a `FilesystemStore`, collects every record relevant to a
//! root object, and writes a directory bundle. The root object's
//! `ObjectGenesis` must exist; missing dependent records (e.g. an
//! actor referenced by a statement but not in the store) are an
//! error rather than a silent partial bundle — bundles are required
//! to be self-contained for the records they advertise.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use kairo_core::{ActorId, BlobId, ObjectId, StatementId};
use kairo_identity::ActorResolver;
use kairo_identity::json::ActorGenesisJson;
use kairo_object::OBJECT_MANIFEST_DOMAIN;
use kairo_statement::json::{
    ObjectBranchStatementJson, ObjectGenesisStatementJson, ObjectRevisionStatementJson,
    ObjectVersionTagStatementJson,
};
use kairo_statement::SignedStatement;
use kairo_store::{BlobStore, FilesystemStore, ObjectStore};
use serde::Deserialize;

use crate::dirs;
use crate::error::BundleError;
use crate::manifest::{
    BundleContents, BundleCreator, BundleGitHistory, BundleManifest, BundleRoots,
};
use crate::{BUNDLE_SCHEMA, MANIFEST_FILENAME};

/// Kind discriminator borrowed from the JSON envelope; we only look
/// at the `type` field to decide which typed DTO to deserialize into.
#[derive(Debug, Deserialize)]
struct EnvelopePeek {
    #[serde(rename = "type")]
    statement_type: String,
}

/// Write a directory bundle for `object` under `dest`. `dest` must
/// either not exist or be empty.
///
/// `git_object_ids` declares which objects' Git packs the bundle
/// will ship. When non-empty:
///
/// - `<dest>/git/` is created (empty) so the caller can stream
///   pack files into it after `write_bundle` returns,
/// - `manifest.git_history.included` is set to `true`.
///
/// Pack bytes are NOT written by `write_bundle` itself. The caller
/// is expected to stream `<dest>/git/<object-id>.pack` for each
/// declared id (typically via `kairo_git::GitCache::
/// pack_for_object_to(id, file)`) so that arbitrarily large packs
/// flow through without buffering. Pass an empty slice for the
/// no-git-data flow; `git/` is not created and `included = false`.
///
/// The manifest's `expected_commits` field is auto-derived from
/// revision OIDs in either case.
///
/// Returns the manifest that was written, so callers can show or
/// re-emit it without re-reading from disk.
pub fn write_bundle(
    store: &FilesystemStore,
    object: &ObjectId,
    dest: &Path,
    created_at: &str,
    tool_version: &str,
    git_object_ids: &[ObjectId],
) -> Result<BundleManifest, BundleError> {
    ensure_dest_empty(dest)?;

    // 1. Object genesis. A bundle without its root object's genesis
    //    is meaningless; surface a distinct error rather than
    //    "missing record."
    let genesis = match store.get_object_genesis(object) {
        Ok(genesis) => genesis,
        Err(kairo_store::StoreError::Missing) => {
            return Err(BundleError::RootObjectNotFound {
                object: object.to_string(),
            });
        }
        Err(error) => return Err(BundleError::ObjectGenesisLookup(error)),
    };

    // 2. Walk statements/ and collect every statement about this object.
    let collected = collect_statements(store, object)?;

    // 3. Collect every actor referenced (genesis creator + every
    //    statement signer). Resolve each from the store now so we
    //    fail fast if an actor is missing.
    let mut actor_ids: BTreeSet<ActorId> = BTreeSet::new();
    actor_ids.insert(genesis.body().created_by().clone());
    for statement_id in collected.iter_signed_actor_ids() {
        actor_ids.insert(statement_id.clone());
    }

    // 4. Collect every blob referenced. Today only the per-revision
    //    manifest blob; future statement types may reference others.
    let mut blob_ids: BTreeSet<BlobId> = BTreeSet::new();
    for revision in collected.revisions.values() {
        blob_ids.insert(revision.unsigned().body().manifest_hash().clone());
    }

    // 5. Pull Git commit ids out of revision references.
    let mut expected_commits: BTreeSet<String> = BTreeSet::new();
    for revision in collected.revisions.values() {
        if let Some(oid) = revision
            .unsigned()
            .body()
            .revision()
            .as_str()
            .strip_prefix("git:sha256:")
        {
            expected_commits.insert(oid.to_owned());
        }
    }

    // 6. Materialize the bundle on disk. Order: data files first,
    //    manifest last, so a partial write is detectable (no
    //    manifest = no successful bundle).
    fs::create_dir_all(dest).map_err(|source| BundleError::Io {
        path: dest.to_path_buf(),
        source,
    })?;
    let actors_dir = dest.join(dirs::ACTORS);
    let objects_dir = dest.join(dirs::OBJECTS);
    let statements_dir = dest.join(dirs::STATEMENTS);
    let blobs_dir = dest.join(dirs::BLOBS);
    create_dir(&actors_dir)?;
    create_dir(&objects_dir)?;
    create_dir(&statements_dir)?;
    create_dir(&blobs_dir)?;

    // Actors.
    let mut actor_id_strings = Vec::new();
    for actor_id in &actor_ids {
        let body = ActorResolver::actor_genesis(store, actor_id)
            .map_err(|error| BundleError::Store(kairo_store::StoreError::Unavailable(
                io::Error::other(error.to_string()),
            )))?
            .ok_or_else(|| BundleError::DanglingActor {
                statement: "<bundle>".to_owned(),
                actor: actor_id.to_string(),
            })?;
        let json = ActorGenesisJson::from_body(&body);
        let bytes = serde_json::to_vec_pretty(&json).map_err(serde_to_io(actors_dir.clone()))?;
        write_file(&actors_dir.join(format!("{actor_id}.json")), &bytes)?;
        actor_id_strings.push(actor_id.to_string());
    }

    // Objects (always exactly one in MVP).
    let object_json = ObjectGenesisStatementJson::from_statement(&genesis);
    let object_bytes =
        serde_json::to_vec_pretty(&object_json).map_err(serde_to_io(objects_dir.clone()))?;
    write_file(
        &objects_dir.join(format!("{object}.json")),
        &object_bytes,
    )?;

    // Statements.
    let mut statement_id_strings = Vec::new();
    for (statement_id, signed) in &collected.revisions {
        let json = ObjectRevisionStatementJson::from_statement(signed);
        let bytes = serde_json::to_vec_pretty(&json).map_err(serde_to_io(statements_dir.clone()))?;
        write_file(
            &statements_dir.join(format!("{statement_id}.json")),
            &bytes,
        )?;
        statement_id_strings.push(statement_id.to_string());
    }
    for (statement_id, signed) in &collected.branches {
        let json = ObjectBranchStatementJson::from_statement(signed);
        let bytes = serde_json::to_vec_pretty(&json).map_err(serde_to_io(statements_dir.clone()))?;
        write_file(
            &statements_dir.join(format!("{statement_id}.json")),
            &bytes,
        )?;
        statement_id_strings.push(statement_id.to_string());
    }
    for (statement_id, signed) in &collected.version_tags {
        let json = ObjectVersionTagStatementJson::from_statement(signed);
        let bytes = serde_json::to_vec_pretty(&json).map_err(serde_to_io(statements_dir.clone()))?;
        write_file(
            &statements_dir.join(format!("{statement_id}.json")),
            &bytes,
        )?;
        statement_id_strings.push(statement_id.to_string());
    }

    // Blobs.
    let mut blob_id_strings = Vec::new();
    for blob_id in &blob_ids {
        let bytes = store.get_blob(blob_id).map_err(BundleError::Store)?;
        write_file(&blobs_dir.join(blob_id.as_str()), &bytes)?;
        blob_id_strings.push(blob_id.to_string());
    }

    // Reserve `<dest>/git/` for the caller to populate with pack
    // files. We create the directory (so the caller has a target
    // path) and flip `included = true`, but we don't write any
    // pack bytes ourselves — the caller streams them in afterward
    // via `kairo_git::GitCache::pack_for_object_to`.
    let git_included = !git_object_ids.is_empty();
    if git_included {
        let git_dir = dest.join("git");
        create_dir(&git_dir)?;
    }

    let manifest = BundleManifest {
        schema: BUNDLE_SCHEMA.to_owned(),
        created_at: created_at.to_owned(),
        created_by: BundleCreator {
            tool: "kairo".to_owned(),
            version: tool_version.to_owned(),
        },
        roots: BundleRoots {
            objects: vec![object.to_string()],
        },
        contents: BundleContents {
            actors: actor_id_strings,
            objects: vec![object.to_string()],
            statements: statement_id_strings,
            blobs: blob_id_strings,
        },
        git_history: BundleGitHistory {
            included: git_included,
            expected_commits: expected_commits.into_iter().collect(),
        },
    };
    let manifest_bytes =
        serde_json::to_vec_pretty(&manifest).map_err(serde_to_io(dest.to_path_buf()))?;
    write_file(&dest.join(MANIFEST_FILENAME), &manifest_bytes)?;
    Ok(manifest)
}

/// Per-statement-type collections, keyed by `StatementId` so duplicate
/// scans collapse and the writer emits one file per record.
struct CollectedStatements {
    revisions: BTreeMap<StatementId, SignedStatement<kairo_statement::ObjectRevisionBody>>,
    branches: BTreeMap<StatementId, SignedStatement<kairo_statement::ObjectBranchBody>>,
    version_tags: BTreeMap<StatementId, SignedStatement<kairo_statement::ObjectVersionTagBody>>,
}

impl CollectedStatements {
    fn iter_signed_actor_ids(&self) -> impl Iterator<Item = &ActorId> {
        let revisions = self.revisions.values().map(|s| s.unsigned().actor());
        let branches = self.branches.values().map(|s| s.unsigned().actor());
        let tags = self.version_tags.values().map(|s| s.unsigned().actor());
        revisions.chain(branches).chain(tags)
    }
}

fn collect_statements(
    store: &FilesystemStore,
    target: &ObjectId,
) -> Result<CollectedStatements, BundleError> {
    let mut out = CollectedStatements {
        revisions: BTreeMap::new(),
        branches: BTreeMap::new(),
        version_tags: BTreeMap::new(),
    };

    let statements_root = store.root().join("statements");
    let level1 = match fs::read_dir(&statements_root) {
        Ok(iter) => iter,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(out),
        Err(error) => {
            return Err(BundleError::Io {
                path: statements_root,
                source: error,
            });
        }
    };
    for shard1 in level1 {
        let shard1 = shard1.map_err(|source| BundleError::Io {
            path: statements_root.clone(),
            source,
        })?;
        if !shard1.path().is_dir() {
            continue;
        }
        for shard2 in fs::read_dir(shard1.path()).map_err(|source| BundleError::Io {
            path: shard1.path(),
            source,
        })? {
            let shard2 = shard2.map_err(|source| BundleError::Io {
                path: shard1.path(),
                source,
            })?;
            if !shard2.path().is_dir() {
                continue;
            }
            for entry in fs::read_dir(shard2.path()).map_err(|source| BundleError::Io {
                path: shard2.path(),
                source,
            })? {
                let entry = entry.map_err(|source| BundleError::Io {
                    path: shard2.path(),
                    source,
                })?;
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) != Some("json") {
                    continue;
                }
                let bytes = fs::read(&path).map_err(|source| BundleError::Io {
                    path: path.clone(),
                    source,
                })?;
                let peek: EnvelopePeek =
                    serde_json::from_slice(&bytes).map_err(|source| BundleError::RecordParse {
                        path: path.clone(),
                        source,
                    })?;
                match peek.statement_type.as_str() {
                    "ObjectRevision" => {
                        let dto: ObjectRevisionStatementJson =
                            serde_json::from_slice(&bytes).map_err(|source| {
                                BundleError::RecordParse {
                                    path: path.clone(),
                                    source,
                                }
                            })?;
                        let signed = dto.to_statement().map_err(|error| {
                            BundleError::StatementShape {
                                path: path.clone(),
                                message: error.to_string(),
                            }
                        })?;
                        if signed.unsigned().body().object() == target {
                            out.revisions.insert(signed.statement_id(), signed);
                        }
                    }
                    "ObjectBranch" => {
                        let dto: ObjectBranchStatementJson =
                            serde_json::from_slice(&bytes).map_err(|source| {
                                BundleError::RecordParse {
                                    path: path.clone(),
                                    source,
                                }
                            })?;
                        let signed = dto.to_statement().map_err(|error| {
                            BundleError::StatementShape {
                                path: path.clone(),
                                message: error.to_string(),
                            }
                        })?;
                        if signed.unsigned().body().object() == target {
                            out.branches.insert(signed.statement_id(), signed);
                        }
                    }
                    "ObjectVersionTag" => {
                        let dto: ObjectVersionTagStatementJson =
                            serde_json::from_slice(&bytes).map_err(|source| {
                                BundleError::RecordParse {
                                    path: path.clone(),
                                    source,
                                }
                            })?;
                        let signed = dto.to_statement().map_err(|error| {
                            BundleError::StatementShape {
                                path: path.clone(),
                                message: error.to_string(),
                            }
                        })?;
                        if signed.unsigned().body().object() == target {
                            out.version_tags.insert(signed.statement_id(), signed);
                        }
                    }
                    // Trust statements — and any other future
                    // non-object-bearing statement — are intentionally
                    // skipped for object bundles in the MVP.
                    _ => continue,
                }
            }
        }
    }

    Ok(out)
}

fn ensure_dest_empty(dest: &Path) -> Result<(), BundleError> {
    match fs::read_dir(dest) {
        Ok(mut entries) => {
            if entries.next().is_some() {
                Err(BundleError::DestinationNotEmpty {
                    path: dest.to_path_buf(),
                })
            } else {
                Ok(())
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(BundleError::Io {
            path: dest.to_path_buf(),
            source,
        }),
    }
}

fn create_dir(path: &Path) -> Result<(), BundleError> {
    fs::create_dir_all(path).map_err(|source| BundleError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), BundleError> {
    fs::write(path, bytes).map_err(|source| BundleError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn serde_to_io(path: PathBuf) -> impl FnOnce(serde_json::Error) -> BundleError {
    move |source| BundleError::Io {
        path,
        source: io::Error::other(source.to_string()),
    }
}

// `OBJECT_MANIFEST_DOMAIN` is the only blob domain MVP bundles emit.
// The importer re-derives each blob's `BlobId` against this domain
// and rejects mismatches. Pulling the symbol through this module
// keeps the export/import sides explicit about which domains we ship.
#[allow(dead_code)]
pub(crate) const MVP_BLOB_DOMAIN: &[u8] = OBJECT_MANIFEST_DOMAIN;
