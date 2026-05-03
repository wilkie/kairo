//! Bundle importer.
//!
//! Reads a directory bundle, fixity-checks every record against its
//! filename / declared id, and ingests each into the destination
//! `FilesystemStore` via the existing `put_*` methods (which themselves
//! re-derive the canonical id and surface mismatches as
//! `StoreError::Corrupt`). Import is idempotent — a record already
//! present at the same id is rewritten with the same content; a record
//! at the same id with *different* bytes is a fixity error in the
//! store layer.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use kairo_core::{ActorId, BlobId, ObjectId, StatementId};
use kairo_identity::json::ActorGenesisJson;
use kairo_statement::json::{
    ObjectBranchStatementJson, ObjectGenesisStatementJson, ObjectRevisionStatementJson,
    ObjectVersionTagStatementJson,
};
use kairo_store::{ActorStore, BlobStore, FilesystemStore, ObjectStore, StatementStore};
use serde::Deserialize;

use crate::dirs;
use crate::error::BundleError;
use crate::export::MVP_BLOB_DOMAIN;
use crate::manifest::BundleManifest;
use crate::{BUNDLE_SCHEMA, MANIFEST_FILENAME};

/// Outcome counts for an import. The same counts apply to a re-import
/// (idempotency); the store is a sink, not an authority on whether
/// the record was "new."
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportSummary {
    pub actors: usize,
    pub objects: usize,
    pub statements: usize,
    pub blobs: usize,
}

/// Read the bundle at `src` and ingest its contents into `store`.
pub fn import_bundle(src: &Path, store: &FilesystemStore) -> Result<ImportSummary, BundleError> {
    let manifest = read_manifest(src)?;
    if manifest.schema != BUNDLE_SCHEMA {
        return Err(BundleError::UnsupportedSchema {
            found: manifest.schema,
            expected: BUNDLE_SCHEMA,
        });
    }

    // Build the set of actor ids the bundle promises so we can flag
    // any statement whose signer the bundle didn't ship.
    let manifest_actor_set: std::collections::BTreeSet<&str> =
        manifest.contents.actors.iter().map(|s| s.as_str()).collect();

    let mut summary = ImportSummary::default();

    // 1. Ingest blobs first. Statements may reference these via
    //    manifest_hash; depending on later validation, the importer
    //    may want the blob present so consumers can immediately rerun
    //    `verify object`.
    for blob_id_str in &manifest.contents.blobs {
        let blob_id = BlobId::new(blob_id_str.clone()).map_err(|source| {
            BundleError::BadIdFilename {
                path: src.join(dirs::BLOBS).join(blob_id_str),
                kind: "blob",
                source,
            }
        })?;
        let path = src.join(dirs::BLOBS).join(blob_id.as_str());
        let bytes = fs::read(&path).map_err(|source| match source.kind() {
            io::ErrorKind::NotFound => BundleError::MissingRecord {
                kind: "blob",
                id: blob_id_str.clone(),
            },
            _ => BundleError::Io {
                path: path.clone(),
                source,
            },
        })?;
        let derived = BlobId::from_bytes(MVP_BLOB_DOMAIN, &bytes);
        if derived != blob_id {
            return Err(BundleError::BlobHashMismatch {
                path,
                expected: blob_id.to_string(),
                actual: derived.to_string(),
            });
        }
        store.put_blob(&blob_id, &bytes).map_err(BundleError::Store)?;
        summary.blobs += 1;
    }

    // 2. Actors next so subsequent statement ingestion can verify
    //    signatures via the in-store ActorResolver if a caller chains
    //    a verify pass.
    for actor_id_str in &manifest.contents.actors {
        let actor_id = ActorId::new(actor_id_str.clone()).map_err(|source| {
            BundleError::BadIdFilename {
                path: src.join(dirs::ACTORS).join(format!("{actor_id_str}.json")),
                kind: "actor",
                source,
            }
        })?;
        let path = src.join(dirs::ACTORS).join(format!("{actor_id}.json"));
        let bytes = fs::read(&path).map_err(|source| match source.kind() {
            io::ErrorKind::NotFound => BundleError::MissingRecord {
                kind: "actor",
                id: actor_id_str.clone(),
            },
            _ => BundleError::Io {
                path: path.clone(),
                source,
            },
        })?;
        let dto: ActorGenesisJson =
            serde_json::from_slice(&bytes).map_err(|source| BundleError::RecordParse {
                path: path.clone(),
                source,
            })?;
        let body = dto.to_body().map_err(|error| BundleError::StatementShape {
            path: path.clone(),
            message: error.to_string(),
        })?;
        let derived = body.actor_id();
        if derived != actor_id {
            return Err(BundleError::FixityMismatch {
                path,
                expected: actor_id.to_string(),
                actual: derived.to_string(),
            });
        }
        store.put_actor(&body).map_err(BundleError::Store)?;
        summary.actors += 1;
    }

    // 3. Object genesis records.
    for object_id_str in &manifest.contents.objects {
        let object_id = ObjectId::new(object_id_str.clone()).map_err(|source| {
            BundleError::BadIdFilename {
                path: src.join(dirs::OBJECTS).join(format!("{object_id_str}.json")),
                kind: "object",
                source,
            }
        })?;
        let path = src.join(dirs::OBJECTS).join(format!("{object_id}.json"));
        let bytes = fs::read(&path).map_err(|source| match source.kind() {
            io::ErrorKind::NotFound => BundleError::MissingRecord {
                kind: "object",
                id: object_id_str.clone(),
            },
            _ => BundleError::Io {
                path: path.clone(),
                source,
            },
        })?;
        let dto: ObjectGenesisStatementJson =
            serde_json::from_slice(&bytes).map_err(|source| BundleError::RecordParse {
                path: path.clone(),
                source,
            })?;
        let signed = dto.to_statement().map_err(|error| BundleError::StatementShape {
            path: path.clone(),
            message: error.to_string(),
        })?;
        let derived = signed.object_id();
        if derived != object_id {
            return Err(BundleError::FixityMismatch {
                path,
                expected: object_id.to_string(),
                actual: derived.to_string(),
            });
        }
        store
            .put_object_genesis(&signed)
            .map_err(BundleError::Store)?;
        summary.objects += 1;
    }

    // 4. Statements. Dispatch on the JSON `type` field, ingest into
    //    the right typed store method.
    #[derive(Debug, Deserialize)]
    struct EnvelopePeek {
        #[serde(rename = "type")]
        statement_type: String,
        actor: String,
    }

    for statement_id_str in &manifest.contents.statements {
        let statement_id = StatementId::new(statement_id_str.clone()).map_err(|source| {
            BundleError::BadIdFilename {
                path: src
                    .join(dirs::STATEMENTS)
                    .join(format!("{statement_id_str}.json")),
                kind: "statement",
                source,
            }
        })?;
        let path = src
            .join(dirs::STATEMENTS)
            .join(format!("{statement_id}.json"));
        let bytes = fs::read(&path).map_err(|source| match source.kind() {
            io::ErrorKind::NotFound => BundleError::MissingRecord {
                kind: "statement",
                id: statement_id_str.clone(),
            },
            _ => BundleError::Io {
                path: path.clone(),
                source,
            },
        })?;
        let peek: EnvelopePeek =
            serde_json::from_slice(&bytes).map_err(|source| BundleError::RecordParse {
                path: path.clone(),
                source,
            })?;
        if !manifest_actor_set.contains(peek.actor.as_str()) {
            return Err(BundleError::DanglingActor {
                statement: statement_id.to_string(),
                actor: peek.actor.clone(),
            });
        }
        match peek.statement_type.as_str() {
            "ObjectRevision" => {
                let dto: ObjectRevisionStatementJson = serde_json::from_slice(&bytes)
                    .map_err(|source| BundleError::RecordParse {
                        path: path.clone(),
                        source,
                    })?;
                let signed = dto.to_statement().map_err(|error| BundleError::StatementShape {
                    path: path.clone(),
                    message: error.to_string(),
                })?;
                fixity_check_statement(&path, &statement_id, &signed.statement_id())?;
                store
                    .put_object_revision(&signed)
                    .map_err(BundleError::Store)?;
            }
            "ObjectBranch" => {
                let dto: ObjectBranchStatementJson = serde_json::from_slice(&bytes)
                    .map_err(|source| BundleError::RecordParse {
                        path: path.clone(),
                        source,
                    })?;
                let signed = dto.to_statement().map_err(|error| BundleError::StatementShape {
                    path: path.clone(),
                    message: error.to_string(),
                })?;
                fixity_check_statement(&path, &statement_id, &signed.statement_id())?;
                store
                    .put_object_branch(&signed)
                    .map_err(BundleError::Store)?;
            }
            "ObjectVersionTag" => {
                let dto: ObjectVersionTagStatementJson = serde_json::from_slice(&bytes)
                    .map_err(|source| BundleError::RecordParse {
                        path: path.clone(),
                        source,
                    })?;
                let signed = dto.to_statement().map_err(|error| BundleError::StatementShape {
                    path: path.clone(),
                    message: error.to_string(),
                })?;
                fixity_check_statement(&path, &statement_id, &signed.statement_id())?;
                store
                    .put_object_version_tag(&signed)
                    .map_err(BundleError::Store)?;
            }
            other => {
                return Err(BundleError::UnsupportedStatementType {
                    path,
                    statement_type: other.to_owned(),
                });
            }
        }
        summary.statements += 1;
    }

    Ok(summary)
}

fn read_manifest(src: &Path) -> Result<BundleManifest, BundleError> {
    let path: PathBuf = src.join(MANIFEST_FILENAME);
    let bytes = fs::read(&path).map_err(|source| BundleError::Io {
        path: path.clone(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(BundleError::ManifestParse)
}

fn fixity_check_statement(
    path: &Path,
    expected: &StatementId,
    derived: &StatementId,
) -> Result<(), BundleError> {
    if expected == derived {
        Ok(())
    } else {
        Err(BundleError::FixityMismatch {
            path: path.to_path_buf(),
            expected: expected.to_string(),
            actual: derived.to_string(),
        })
    }
}
