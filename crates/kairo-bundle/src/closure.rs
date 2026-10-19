//! The closure bundle (`specs/DECISIONS.md` §26 sub-decision 8): the
//! transfer form of a published closure, which a push uploads.
//!
//! Unlike an object bundle it carries authority and every actor's key
//! events, because it stands in for a pull from a node that may not be
//! reachable. It may also be partial: the files are what the receiver
//! asked for, while the manifest's inventory names the whole closure.
//!
//! ```text
//! manifest.json                 kairo.closure-bundle.v1
//! genesis.json                  the object genesis, if included
//! actors/<actor-id>.json        actor geneses included
//! statements/<statement-id>.json
//! blobs/<blob-id>
//! git/pack                      a (thin) Git pack, if any
//! ```
//!
//! Records are stored byte-for-byte as the sending store holds them.
//! Nothing here verifies them: the receiver ingests a bundle through
//! the pull verifier, which treats the inventory as the object listing
//! and each file as the record it claims to be.
//!
//! On the wire the directory is an uncompressed POSIX tar
//! ([`write_tar`], [`unpack_tar`]). Unpacking accepts only regular
//! files at exactly the paths above, named by valid ids.

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use kairo_core::{ActorId, BlobId, ObjectId, StatementId};
use kairo_federation_core::ClosureInventory;
use serde::{Deserialize, Serialize};

pub const CLOSURE_BUNDLE_SCHEMA: &str = "kairo.closure-bundle.v1";

const MANIFEST: &str = "manifest.json";
const GENESIS: &str = "genesis.json";
const ACTORS: &str = "actors";
const STATEMENTS: &str = "statements";
const BLOBS: &str = "blobs";
const GIT: &str = "git";
const PACK: &str = "pack";

/// `manifest.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClosureManifest {
    pub schema: String,
    pub object: String,
    /// RFC 3339; informational.
    pub created_at: String,
    /// The whole closure as the sender holds it.
    pub inventory: ClosureInventory,
    pub git: ClosureGit,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClosureGit {
    /// The inventory commits' object format (`sha1` / `sha256`), if any.
    pub format: Option<String>,
    /// `git/pack` is present.
    pub pack: bool,
}

#[derive(Debug)]
pub enum ClosureBundleError {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    Manifest(String),
    /// A tar entry that isn't one of the bundle's files.
    BadEntry {
        path: String,
        reason: String,
    },
}

impl fmt::Display for ClosureBundleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Manifest(message) => write!(f, "closure bundle manifest: {message}"),
            Self::BadEntry { path, reason } => write!(f, "closure bundle entry {path:?}: {reason}"),
        }
    }
}

impl std::error::Error for ClosureBundleError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> ClosureBundleError + '_ {
    move |source| ClosureBundleError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Builds a closure bundle directory. Write the records, optionally a
/// pack at [`ClosureBundleWriter::pack_path`], then [`finish`].
///
/// [`finish`]: ClosureBundleWriter::finish
#[derive(Debug)]
pub struct ClosureBundleWriter {
    dir: PathBuf,
}

impl ClosureBundleWriter {
    /// Start a bundle in `dir`, which must be empty or absent.
    pub fn create(dir: impl Into<PathBuf>) -> Result<Self, ClosureBundleError> {
        let dir = dir.into();
        for sub in [ACTORS, STATEMENTS, BLOBS, GIT] {
            let path = dir.join(sub);
            fs::create_dir_all(&path).map_err(io_error(&path))?;
        }
        Ok(Self { dir })
    }

    fn write(&self, relative: PathBuf, bytes: &[u8]) -> Result<(), ClosureBundleError> {
        let path = self.dir.join(relative);
        fs::write(&path, bytes).map_err(io_error(&path))
    }

    pub fn put_genesis(&self, bytes: &[u8]) -> Result<(), ClosureBundleError> {
        self.write(PathBuf::from(GENESIS), bytes)
    }

    pub fn put_actor(&self, actor: &ActorId, bytes: &[u8]) -> Result<(), ClosureBundleError> {
        self.write(Path::new(ACTORS).join(format!("{actor}.json")), bytes)
    }

    pub fn put_statement(&self, id: &StatementId, bytes: &[u8]) -> Result<(), ClosureBundleError> {
        self.write(Path::new(STATEMENTS).join(format!("{id}.json")), bytes)
    }

    pub fn put_blob(&self, id: &BlobId, bytes: &[u8]) -> Result<(), ClosureBundleError> {
        self.write(Path::new(BLOBS).join(id.as_str()), bytes)
    }

    /// Where a Git pack goes; the caller streams one there.
    pub fn pack_path(&self) -> PathBuf {
        self.dir.join(GIT).join(PACK)
    }

    /// Write the manifest. `git.pack` is set from whether a pack was
    /// written.
    pub fn finish(self, mut manifest: ClosureManifest) -> Result<PathBuf, ClosureBundleError> {
        manifest.git.pack = self.pack_path().is_file();
        let mut bytes = serde_json::to_vec_pretty(&manifest)
            .map_err(|error| ClosureBundleError::Manifest(error.to_string()))?;
        bytes.push(b'\n');
        self.write(PathBuf::from(MANIFEST), &bytes)?;
        Ok(self.dir)
    }
}

/// An unpacked closure bundle, read record by record.
#[derive(Debug, Clone)]
pub struct ClosureBundle {
    dir: PathBuf,
    manifest: ClosureManifest,
    object: ObjectId,
}

impl ClosureBundle {
    pub fn open(dir: impl Into<PathBuf>) -> Result<Self, ClosureBundleError> {
        let dir = dir.into();
        let path = dir.join(MANIFEST);
        let bytes = fs::read(&path).map_err(io_error(&path))?;
        let manifest: ClosureManifest = serde_json::from_slice(&bytes)
            .map_err(|error| ClosureBundleError::Manifest(error.to_string()))?;
        if manifest.schema != CLOSURE_BUNDLE_SCHEMA {
            return Err(ClosureBundleError::Manifest(format!(
                "schema {:?}, expected {CLOSURE_BUNDLE_SCHEMA:?}",
                manifest.schema
            )));
        }
        let object = ObjectId::new(manifest.object.clone())
            .map_err(|error| ClosureBundleError::Manifest(format!("object: {error}")))?;
        manifest
            .inventory
            .validate()
            .map_err(|error| ClosureBundleError::Manifest(error.to_string()))?;
        Ok(Self {
            dir,
            manifest,
            object,
        })
    }

    pub fn manifest(&self) -> &ClosureManifest {
        &self.manifest
    }

    pub fn object(&self) -> &ObjectId {
        &self.object
    }

    fn read(&self, relative: PathBuf) -> Result<Option<Vec<u8>>, ClosureBundleError> {
        let path = self.dir.join(relative);
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(ClosureBundleError::Io { path, source }),
        }
    }

    pub fn genesis(&self) -> Result<Option<Vec<u8>>, ClosureBundleError> {
        self.read(PathBuf::from(GENESIS))
    }

    pub fn actor(&self, actor: &ActorId) -> Result<Option<Vec<u8>>, ClosureBundleError> {
        self.read(Path::new(ACTORS).join(format!("{actor}.json")))
    }

    pub fn statement(&self, id: &StatementId) -> Result<Option<Vec<u8>>, ClosureBundleError> {
        self.read(Path::new(STATEMENTS).join(format!("{id}.json")))
    }

    pub fn blob(&self, id: &BlobId) -> Result<Option<Vec<u8>>, ClosureBundleError> {
        self.read(Path::new(BLOBS).join(id.as_str()))
    }

    /// The Git pack, if the bundle has one.
    pub fn pack(&self) -> Option<PathBuf> {
        let path = self.dir.join(GIT).join(PACK);
        path.is_file().then_some(path)
    }
}

/// Write `dir` (a finished bundle) to `sink` as an uncompressed tar, in
/// a fixed order: manifest first, then each directory sorted.
pub fn write_tar(dir: &Path, sink: impl Write) -> Result<(), ClosureBundleError> {
    let mut builder = tar::Builder::new(sink);
    builder.mode(tar::HeaderMode::Deterministic);
    let mut files = vec![PathBuf::from(MANIFEST)];
    if dir.join(GENESIS).is_file() {
        files.push(PathBuf::from(GENESIS));
    }
    for sub in [ACTORS, STATEMENTS, BLOBS, GIT] {
        let path = dir.join(sub);
        let mut names: Vec<String> = match fs::read_dir(&path) {
            Ok(entries) => entries
                .map(|entry| {
                    entry
                        .map(|entry| entry.file_name().to_string_lossy().into_owned())
                        .map_err(io_error(&path))
                })
                .collect::<Result<_, _>>()?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(source) => return Err(ClosureBundleError::Io { path, source }),
        };
        names.sort();
        files.extend(names.into_iter().map(|name| Path::new(sub).join(name)));
    }
    for relative in files {
        let path = dir.join(&relative);
        let mut file = fs::File::open(&path).map_err(io_error(&path))?;
        builder
            .append_file(&relative, &mut file)
            .map_err(io_error(&path))?;
    }
    builder.into_inner().map_err(io_error(dir))?;
    Ok(())
}

/// Unpack a closure bundle tar into `dest` (created, must not hold a
/// bundle already), refusing anything but the bundle's own files.
pub fn unpack_tar(source: impl Read, dest: &Path) -> Result<ClosureBundle, ClosureBundleError> {
    let mut archive = tar::Archive::new(source);
    let mut seen = BTreeSet::new();
    for sub in [ACTORS, STATEMENTS, BLOBS, GIT] {
        let path = dest.join(sub);
        fs::create_dir_all(&path).map_err(io_error(&path))?;
    }
    let entries = archive.entries().map_err(io_error(dest))?;
    for entry in entries {
        let mut entry = entry.map_err(io_error(dest))?;
        let raw = String::from_utf8_lossy(&entry.path_bytes()).into_owned();
        let bad = |reason: &str| ClosureBundleError::BadEntry {
            path: raw.clone(),
            reason: reason.to_owned(),
        };
        match entry.header().entry_type() {
            tar::EntryType::Regular => {}
            tar::EntryType::Directory
                if [ACTORS, STATEMENTS, BLOBS, GIT]
                    .iter()
                    .any(|sub| raw.trim_end_matches('/') == *sub) =>
            {
                continue
            }
            _ => return Err(bad("not a regular file")),
        }
        let relative = allowed_path(&raw).map_err(|reason| bad(&reason))?;
        if !seen.insert(relative.clone()) {
            return Err(bad("appears twice"));
        }
        let path = dest.join(&relative);
        let mut file = fs::File::create(&path).map_err(io_error(&path))?;
        io::copy(&mut entry, &mut file).map_err(io_error(&path))?;
    }
    if !seen.contains(Path::new(MANIFEST)) {
        return Err(ClosureBundleError::Manifest(
            "missing manifest.json".to_owned(),
        ));
    }
    ClosureBundle::open(dest)
}

/// The relative path for an archive entry, if it's a bundle file.
fn allowed_path(raw: &str) -> Result<PathBuf, String> {
    if raw == MANIFEST || raw == GENESIS {
        return Ok(PathBuf::from(raw));
    }
    if raw == format!("{GIT}/{PACK}") {
        return Ok(Path::new(GIT).join(PACK));
    }
    let (dir, name) = raw
        .split_once('/')
        .ok_or_else(|| "not a bundle path".to_owned())?;
    let id_error = |error: kairo_core::IdError| format!("not named by an id: {error}");
    match dir {
        ACTORS => {
            let id = name.strip_suffix(".json").ok_or("not a .json file")?;
            ActorId::new(id.to_owned()).map_err(id_error)?;
        }
        STATEMENTS => {
            let id = name.strip_suffix(".json").ok_or("not a .json file")?;
            StatementId::new(id.to_owned()).map_err(id_error)?;
        }
        BLOBS => {
            BlobId::new(name.to_owned()).map_err(id_error)?;
        }
        _ => return Err("not a bundle path".to_owned()),
    }
    Ok(Path::new(dir).join(name))
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn manifest(object: &ObjectId) -> ClosureManifest {
        ClosureManifest {
            schema: CLOSURE_BUNDLE_SCHEMA.to_owned(),
            object: object.to_string(),
            created_at: "2026-10-18T12:00:00Z".to_owned(),
            inventory: ClosureInventory::default(),
            git: ClosureGit::default(),
        }
    }

    #[test]
    fn a_bundle_round_trips_through_tar() {
        let object = ObjectId::from_sha256_digest([1; 32]);
        let actor = ActorId::from_sha256_digest([2; 32]);
        let statement = StatementId::from_sha256_digest([3; 32]);
        let blob = BlobId::from_sha256_digest([4; 32]);
        let source = tempfile::TempDir::new().expect("tempdir");
        let writer = ClosureBundleWriter::create(source.path()).expect("create");
        writer.put_genesis(b"genesis").expect("genesis");
        writer.put_actor(&actor, b"actor").expect("actor");
        writer
            .put_statement(&statement, b"statement")
            .expect("statement");
        writer.put_blob(&blob, b"blob").expect("blob");
        fs::write(writer.pack_path(), b"PACK").expect("pack");
        writer.finish(manifest(&object)).expect("finish");

        let mut tar_bytes = Vec::new();
        write_tar(source.path(), &mut tar_bytes).expect("tar");
        let dest = tempfile::TempDir::new().expect("tempdir");
        let bundle = unpack_tar(&tar_bytes[..], dest.path()).expect("unpack");
        assert_eq!(bundle.object(), &object);
        assert!(bundle.manifest().git.pack);
        assert_eq!(bundle.genesis().expect("read"), Some(b"genesis".to_vec()));
        assert_eq!(bundle.actor(&actor).expect("read"), Some(b"actor".to_vec()));
        assert_eq!(
            bundle.statement(&statement).expect("read"),
            Some(b"statement".to_vec())
        );
        assert_eq!(bundle.blob(&blob).expect("read"), Some(b"blob".to_vec()));
        assert!(bundle.pack().is_some());
        let other = StatementId::from_sha256_digest([9; 32]);
        assert_eq!(bundle.statement(&other).expect("read"), None);
    }

    fn tar_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (path, bytes) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_entry_type(tar::EntryType::Regular);
            // Bypass the builder's path checks, as a hostile sender would.
            let name = header.as_old_mut().name.as_mut();
            name[..path.len()].copy_from_slice(path.as_bytes());
            header.set_cksum();
            builder.append(&header, *bytes).expect("append");
        }
        builder.into_inner().expect("tar")
    }

    #[test]
    fn unpacking_refuses_anything_but_bundle_files() {
        let object = ObjectId::from_sha256_digest([1; 32]);
        let manifest_bytes = serde_json::to_vec(&manifest(&object)).expect("json");
        let ok = tar_with(&[(MANIFEST, &manifest_bytes)]);
        let dest = tempfile::TempDir::new().expect("tempdir");
        unpack_tar(&ok[..], dest.path()).expect("a manifest alone is a bundle");

        for bad in [
            "../escape.json",
            "/etc/passwd",
            "statements/../manifest.json",
            "statements/not-an-id.json",
            "blobs/zQm/nested",
            "config.toml",
            "git/other",
        ] {
            let tar_bytes = tar_with(&[(MANIFEST, &manifest_bytes), (bad, b"x")]);
            let dest = tempfile::TempDir::new().expect("tempdir");
            assert!(
                matches!(
                    unpack_tar(&tar_bytes[..], dest.path()),
                    Err(ClosureBundleError::BadEntry { .. })
                ),
                "{bad}"
            );
        }
        let twice = tar_with(&[(MANIFEST, &manifest_bytes), (MANIFEST, &manifest_bytes)]);
        let dest = tempfile::TempDir::new().expect("tempdir");
        assert!(unpack_tar(&twice[..], dest.path()).is_err());
        let none = tar_with(&[(GENESIS, b"g")]);
        let dest = tempfile::TempDir::new().expect("tempdir");
        assert!(unpack_tar(&none[..], dest.path()).is_err());
    }
}
