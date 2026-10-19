//! The files a bundle source serves the pull verifier: a closure
//! bundle (`specs/DECISIONS.md` §26 sub-decision 8, a push upload) or
//! an object bundle (`kairo bundle export`, §32). Both stand in for a
//! peer; every record they hold is checked as a pulled one would be.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use kairo_bundle::closure::{ClosureBundle, ClosureBundleError};
use kairo_bundle::{BundleManifest, BUNDLE_SCHEMA, MANIFEST_FILENAME};
use kairo_core::{ActorId, BlobId, ObjectId, StatementId};
use kairo_federation_core::{ClosureInventory, InventoryActor};

pub(crate) trait BundleFiles {
    fn object(&self) -> &ObjectId;
    /// What the bundle lists, as an object listing would.
    fn inventory(&self) -> &ClosureInventory;
    fn genesis(&self) -> Result<Option<Vec<u8>>, ClosureBundleError>;
    fn actor(&self, actor: &ActorId) -> Result<Option<Vec<u8>>, ClosureBundleError>;
    fn statement(&self, id: &StatementId) -> Result<Option<Vec<u8>>, ClosureBundleError>;
    fn blob(&self, id: &BlobId) -> Result<Option<Vec<u8>>, ClosureBundleError>;
    /// The bundle's Git pack for the object, if any.
    fn pack(&self) -> Option<PathBuf>;
}

impl BundleFiles for ClosureBundle {
    fn object(&self) -> &ObjectId {
        ClosureBundle::object(self)
    }
    fn inventory(&self) -> &ClosureInventory {
        &self.manifest().inventory
    }
    fn genesis(&self) -> Result<Option<Vec<u8>>, ClosureBundleError> {
        ClosureBundle::genesis(self)
    }
    fn actor(&self, actor: &ActorId) -> Result<Option<Vec<u8>>, ClosureBundleError> {
        ClosureBundle::actor(self, actor)
    }
    fn statement(&self, id: &StatementId) -> Result<Option<Vec<u8>>, ClosureBundleError> {
        ClosureBundle::statement(self, id)
    }
    fn blob(&self, id: &BlobId) -> Result<Option<Vec<u8>>, ClosureBundleError> {
        ClosureBundle::blob(self, id)
    }
    fn pack(&self) -> Option<PathBuf> {
        ClosureBundle::pack(self)
    }
}

/// An object bundle directory (`PACKAGE.md` MVP slice): one root
/// object, its object statements, the signers' geneses, blobs, and an
/// optional `git/<object-id>.pack`. No key events or authority: those
/// must already be held, or verification fails (§32 sub-decision 3).
#[derive(Debug)]
pub struct ObjectBundle {
    dir: PathBuf,
    object: ObjectId,
    inventory: ClosureInventory,
}

fn bad(path: &Path, message: impl Into<String>) -> ClosureBundleError {
    ClosureBundleError::Io {
        path: path.to_path_buf(),
        source: io::Error::new(io::ErrorKind::InvalidData, message.into()),
    }
}

impl ObjectBundle {
    pub fn open(dir: impl Into<PathBuf>) -> Result<Self, ClosureBundleError> {
        let dir = dir.into();
        let path = dir.join(MANIFEST_FILENAME);
        let bytes = fs::read(&path).map_err(|source| ClosureBundleError::Io {
            path: path.clone(),
            source,
        })?;
        let manifest: BundleManifest = serde_json::from_slice(&bytes)
            .map_err(|error| ClosureBundleError::Manifest(error.to_string()))?;
        if manifest.schema != BUNDLE_SCHEMA {
            return Err(ClosureBundleError::Manifest(format!(
                "schema {:?}, expected {BUNDLE_SCHEMA:?}",
                manifest.schema
            )));
        }
        let [root] = manifest.roots.objects.as_slice() else {
            return Err(ClosureBundleError::Manifest(
                "a verified import takes a bundle with exactly one root object".to_owned(),
            ));
        };
        let id_error = |kind: &str, raw: &str, error: kairo_core::IdError| {
            bad(&path, format!("{kind} {raw:?}: {error}"))
        };
        let object =
            ObjectId::new(root.clone()).map_err(|error| id_error("object", root, error))?;
        let statements = manifest
            .contents
            .statements
            .iter()
            .map(|raw| StatementId::new(raw.clone()).map_err(|e| id_error("statement", raw, e)))
            .collect::<Result<Vec<_>, _>>()?;
        let actors = manifest
            .contents
            .actors
            .iter()
            .map(|raw| {
                ActorId::new(raw.clone())
                    .map(|actor| InventoryActor {
                        actor,
                        key_events: Vec::new(),
                    })
                    .map_err(|e| id_error("actor", raw, e))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let blobs = manifest
            .contents
            .blobs
            .iter()
            .map(|raw| BlobId::new(raw.clone()).map_err(|e| id_error("blob", raw, e)))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            dir,
            object,
            inventory: ClosureInventory {
                statements,
                authority: Vec::new(),
                actors,
                blobs,
                commits: manifest.git_history.expected_commits,
            },
        })
    }

    fn read(&self, relative: PathBuf) -> Result<Option<Vec<u8>>, ClosureBundleError> {
        let path = self.dir.join(relative);
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(ClosureBundleError::Io { path, source }),
        }
    }
}

impl BundleFiles for ObjectBundle {
    fn object(&self) -> &ObjectId {
        &self.object
    }
    fn inventory(&self) -> &ClosureInventory {
        &self.inventory
    }
    fn genesis(&self) -> Result<Option<Vec<u8>>, ClosureBundleError> {
        self.read(Path::new("objects").join(format!("{}.json", self.object)))
    }
    fn actor(&self, actor: &ActorId) -> Result<Option<Vec<u8>>, ClosureBundleError> {
        self.read(Path::new("actors").join(format!("{actor}.json")))
    }
    fn statement(&self, id: &StatementId) -> Result<Option<Vec<u8>>, ClosureBundleError> {
        self.read(Path::new("statements").join(format!("{id}.json")))
    }
    fn blob(&self, id: &BlobId) -> Result<Option<Vec<u8>>, ClosureBundleError> {
        self.read(Path::new("blobs").join(id.as_str()))
    }
    fn pack(&self) -> Option<PathBuf> {
        let path = self.dir.join("git").join(format!("{}.pack", self.object));
        path.is_file().then_some(path)
    }
}
