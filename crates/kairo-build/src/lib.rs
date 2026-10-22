//! `kairo-build` — the native, development-mode executor
//! (`specs/DECISIONS.md` §33 sub-decisions 5–7, §34 sub-decision 10)
//! and the artifact trees it produces.
//!
//! [`run_plan`] takes a resolved manifest whose prerequisites are all
//! built, materializes the snapshot's tree and each provider into a
//! fresh workspace, runs the target's command there with no shell and
//! a scrubbed environment, collects the declared outputs into an
//! artifact tree of blobs, and writes an unsigned execution record.
//! It implements no environment contract: there is no filesystem or
//! network isolation and no canonical mount path, and the record says
//! so. Signing a `BuildResult` is the caller's decision.
//!
//! The workspace:
//!
//! ```text
//! <store>/build/<random>/
//!   src/                 the snapshot's tree; the command runs in
//!                        src/<workdir>
//!   deps/<object>/       each provider's tree or artifact
//!   home/                $HOME
//!   log/stdout, stderr
//! ```
//!
//! The environment is `HOME`, `LANG=C`, `SOURCE_DATE_EPOCH` (the
//! revision's timestamp), `KAIRO_SRC`, one `KAIRO_PROVIDER_<LABEL>`
//! per requirement (the provider's path, or the host tool's), and a
//! `PATH` of the artifact providers' `bin/` directories followed by
//! the declared host tools' directories — never the host's own.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use kairo_core::{BlobId, ObjectId, SnapshotId, Timestamp};
use kairo_object::{ObjectManifest, OutputKind};
use kairo_resolver::{PlanDocument, Resolved, SourceError};
use kairo_statement::GitRevision;
use kairo_store::{BlobStore, FilesystemStore, SourceStore};
use serde::{Deserialize, Serialize};

/// Blob id domain for an artifact's files.
pub const ARTIFACT_BLOB_DOMAIN: &[u8] = b"kairo.artifact.blob.v1";
/// Blob id domain for an artifact tree document; its id is the
/// artifact id (`schemas/canonical/artifact-tree-v1.md`).
pub const ARTIFACT_TREE_DOMAIN: &[u8] = b"kairo.artifact-tree.v1";
pub const ARTIFACT_TREE_SCHEMA: &str = "kairo.artifact-tree.v1";
/// Blob id domain for a build's captured output streams.
pub const LOG_BLOB_DOMAIN: &[u8] = b"kairo.build.log.v1";
/// Id domain for execution records.
pub const EXECUTION_RECORD_DOMAIN: &[u8] = b"kairo.execution-record.v1";
pub const EXECUTION_RECORD_SCHEMA: &str = "kairo.execution-record.v1";
/// Where records live under the store root.
pub const EXECUTIONS_DIR: &str = "executions";
const WORKSPACES_DIR: &str = "build";

/// The executor's identity in records and results.
pub const EXECUTOR_ID: &str = "native";
pub const EXECUTOR_VERSION: u32 = 1;
pub const BUBBLEWRAP_EXECUTOR_ID: &str = "bubblewrap";
pub const BUBBLEWRAP_EXECUTOR_VERSION: u32 = 1;

fn executor_info(engine: &Engine, rooted: bool) -> ExecutorInfo {
    match engine {
        Engine::Native => ExecutorInfo {
            id: EXECUTOR_ID.to_owned(),
            version: EXECUTOR_VERSION,
            isolation: "none: a host process with a scrubbed environment (development mode)"
                .to_owned(),
            engine: None,
            known_nondeterminism: vec!["host filesystem, network, and clock".to_owned()],
        },
        Engine::Bubblewrap(bwrap) => ExecutorInfo {
            id: BUBBLEWRAP_EXECUTOR_ID.to_owned(),
            version: BUBBLEWRAP_EXECUTOR_VERSION,
            isolation: if rooted {
                "linux namespaces (bubblewrap): the build tree as /, with /proc, /dev and /tmp \
                 over it; no network; hostname kairo; uid and gid 0 with every capability \
                 inside its own namespaces"
            } else {
                "linux namespaces (bubblewrap): only /kairo, /proc, /dev and /tmp visible; \
                 no network; hostname kairo; uid and gid 1000"
            }
            .to_owned(),
            engine: Some(EngineInfo {
                path: bwrap.path.display().to_string(),
                version: bwrap.version.clone(),
            }),
            known_nondeterminism: vec!["wall clock not frozen".to_owned()],
        },
    }
}

/// Where the executor gets a snapshot's tree.
pub trait TreeSource {
    /// Write the tree of `object` at `snapshot` under `dest` (which
    /// exists and is empty), or `Ok(false)` when it isn't held.
    fn export(
        &self,
        object: &ObjectId,
        snapshot: &SnapshotId,
        dest: &Path,
    ) -> Result<bool, SourceError>;

    /// Write a source object's tree at `revision` under `dest` (which
    /// exists and is empty), or `Ok(false)` when it isn't held
    /// (`DECISIONS.md` §37).
    fn export_source(
        &self,
        _source: &ObjectId,
        _revision: &GitRevision,
        _dest: &Path,
    ) -> Result<bool, SourceError> {
        Ok(false)
    }
}

/// What a build may use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    /// Wall-clock limit; the process group is killed at it.
    pub timeout: Duration,
}

// --- artifact trees ----------------------------------------------------

/// A `kairo.artifact-tree.v1` document: the files of one artifact,
/// sorted by path. Serialized compactly; its bytes' id under
/// [`ARTIFACT_TREE_DOMAIN`] is the artifact id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactTree {
    pub schema: String,
    pub entries: Vec<ArtifactEntry>,
}

/// One entry of an artifact: a file, or a symbolic link
/// (`DECISIONS.md` §39).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArtifactEntry {
    File(FileEntry),
    Link(LinkEntry),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileEntry {
    /// `/`-separated, relative.
    pub path: String,
    pub blob: String,
    pub executable: bool,
}

/// A symbolic link: its target, relative and confined to the artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkEntry {
    pub path: String,
    pub link: String,
}

impl ArtifactEntry {
    pub fn path(&self) -> &str {
        match self {
            Self::File(file) => &file.path,
            Self::Link(link) => &link.path,
        }
    }

    /// The file's blob; `None` for a link.
    pub fn blob(&self) -> Option<&str> {
        match self {
            Self::File(file) => Some(&file.blob),
            Self::Link(_) => None,
        }
    }

    /// The link's target; `None` for a file.
    pub fn link(&self) -> Option<&str> {
        match self {
            Self::File(_) => None,
            Self::Link(link) => Some(&link.link),
        }
    }

    pub fn executable(&self) -> bool {
        matches!(self, Self::File(file) if file.executable)
    }
}

/// Why a link target isn't allowed (`DECISIONS.md` §39 sub-decision 2),
/// or `None` if it is. An absolute target names a path in the guest's
/// filesystem and is kept as written; Kairo never follows a link, so it
/// only needs to be more than `/`. A relative target names a path in the
/// artifact, never climbing above the artifact's root when resolved
/// lexically from the directory holding `path`; empty segments (from a
/// trailing `/` or `//`) mean nothing, as in POSIX path resolution.
pub fn link_target_problem(path: &str, target: &str) -> Option<&'static str> {
    if target.is_empty() {
        return Some("is empty");
    }
    if target.contains('\0') {
        return Some("contains NUL");
    }
    if let Some(absolute) = target.strip_prefix('/') {
        return absolute
            .trim_start_matches('/')
            .is_empty()
            .then_some("is only /");
    }
    let mut depth = path.split('/').count().saturating_sub(1);
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => match depth.checked_sub(1) {
                Some(up) => depth = up,
                None => return Some("climbs above the artifact"),
            },
            _ => depth += 1,
        }
    }
    None
}

impl ArtifactTree {
    fn new(mut entries: Vec<ArtifactEntry>) -> Self {
        entries.sort_by(|a, b| a.path().cmp(b.path()));
        Self {
            schema: ARTIFACT_TREE_SCHEMA.to_owned(),
            entries,
        }
    }

    /// The shape rules a tree must keep (`DECISIONS.md` §39): safe,
    /// unique paths in order; links' targets confined; and no entry
    /// under a link, so nothing is ever written through one.
    pub fn validate(&self) -> Result<(), String> {
        let mut links: Vec<&str> = Vec::new();
        let mut previous: Option<&str> = None;
        for entry in &self.entries {
            let path = entry.path();
            if safe_join(Path::new(""), path).is_none() {
                return Err(format!("{path:?} is not a safe relative path"));
            }
            if previous.is_some_and(|previous| previous >= path) {
                return Err(format!("{path:?} is out of order or repeated"));
            }
            previous = Some(path);
            if let Some(link) = links.iter().find(|link| {
                path.strip_prefix(**link)
                    .is_some_and(|rest| rest.starts_with('/'))
            }) {
                return Err(format!("{path:?} lies under the link {link:?}"));
            }
            if let Some(target) = entry.link() {
                if let Some(problem) = link_target_problem(path, target) {
                    return Err(format!("link {path:?} -> {target:?} {problem}"));
                }
                links.push(path);
            }
        }
        Ok(())
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    pub fn id(&self) -> Result<BlobId, serde_json::Error> {
        Ok(BlobId::from_bytes(ARTIFACT_TREE_DOMAIN, &self.to_bytes()?))
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, BuildError> {
        let tree: Self = serde_json::from_slice(bytes).map_err(BuildError::Encode)?;
        if tree.schema != ARTIFACT_TREE_SCHEMA {
            return Err(BuildError::UnsupportedSchema(tree.schema));
        }
        tree.validate().map_err(BuildError::BadArtifact)?;
        Ok(tree)
    }
}

/// Read a held artifact tree by id.
pub fn load_artifact(
    store: &FilesystemStore,
    artifact: &BlobId,
) -> Result<ArtifactTree, BuildError> {
    let bytes = store.get_blob(artifact).map_err(BuildError::Store)?;
    ArtifactTree::from_bytes(&bytes)
}

/// Write a held artifact's files and links under `dest`, which exists
/// and is empty. The tree was validated on load, so no entry lies under
/// a link and nothing here is written through one.
pub fn materialize_artifact(
    store: &FilesystemStore,
    artifact: &BlobId,
    dest: &Path,
) -> Result<ArtifactTree, BuildError> {
    let tree = load_artifact(store, artifact)?;
    for entry in &tree.entries {
        let unsafe_path = || BuildError::UnsafeArtifactPath {
            artifact: artifact.clone(),
            path: entry.path().to_owned(),
        };
        let path = safe_join(dest, entry.path()).ok_or_else(unsafe_path)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| io_error("creating", parent, source))?;
        }
        match entry {
            ArtifactEntry::File(file) => {
                let blob = BlobId::new(file.blob.clone()).map_err(|_| unsafe_path())?;
                let bytes = store.get_blob(&blob).map_err(BuildError::Store)?;
                fs::write(&path, bytes).map_err(|source| io_error("writing", &path, source))?;
                set_executable(&path, file.executable)?;
            }
            ArtifactEntry::Link(link) => {
                std::os::unix::fs::symlink(&link.link, &path)
                    .map_err(|source| io_error("linking", &path, source))?;
            }
        }
    }
    Ok(tree)
}

fn safe_join(root: &Path, relative: &str) -> Option<PathBuf> {
    if relative.is_empty()
        || relative.contains('\\')
        || relative
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return None;
    }
    Some(root.join(relative))
}

fn set_executable(path: &Path, executable: bool) -> Result<(), BuildError> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if executable { 0o755 } else { 0o644 };
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|source| io_error("setting permissions on", path, source))
}

// --- execution records -------------------------------------------------

/// An unsigned, local `kairo.execution-record.v1` document
/// (`EXECUTOR.md` §15): operational evidence, never truth.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionRecord {
    pub schema: String,
    /// 16 random bytes, hex: two runs of one plan are two records even
    /// when everything else about them agrees.
    pub run: String,
    pub plan: String,
    pub object: String,
    pub snapshot: String,
    pub target: String,
    pub executor: ExecutorInfo,
    /// The contract label, or `none`.
    pub environment: String,
    pub host_dependent: bool,
    pub host: HostInfo,
    /// RFC 3339, as `Timestamp` prints.
    pub started_at: String,
    pub completed_at: String,
    pub status: Status,
    pub exit_code: Option<i32>,
    pub outputs: Vec<OutputRecord>,
    /// The artifact id, on success.
    pub artifact: Option<String>,
    pub logs: Logs,
    /// Why a failure failed, when the executor knows.
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutorInfo {
    pub id: String,
    pub version: u32,
    /// What isolation the executor provides, or that it provides none.
    pub isolation: String,
    /// The engine binary, when there is one.
    pub engine: Option<EngineInfo>,
    /// What can make two runs of one plan differ.
    pub known_nondeterminism: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineInfo {
    pub path: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostInfo {
    pub system: String,
    pub arch: String,
    /// The development-mode host tools the plan used, by path.
    pub tools: Vec<String>,
    /// The kernel release, which the `linux` contract records rather
    /// than promises.
    pub kernel: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    Success,
    Failure,
    /// The wall-clock limit killed it: an operational outcome, never
    /// signed (§33 sub-decision 8).
    Timeout,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failure => "failure",
            Self::Timeout => "timeout",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputRecord {
    pub name: String,
    pub artifact_path: String,
    pub files: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Logs {
    pub stdout: String,
    pub stderr: String,
}

impl ExecutionRecord {
    pub fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec_pretty(self)
    }

    pub fn id(&self) -> Result<BlobId, serde_json::Error> {
        Ok(BlobId::from_bytes(
            EXECUTION_RECORD_DOMAIN,
            &self.to_bytes()?,
        ))
    }
}

/// Where a record is written.
pub fn record_path(store: &FilesystemStore, id: &BlobId) -> PathBuf {
    store
        .root()
        .join(EXECUTIONS_DIR)
        .join(format!("{}.json", id.as_str()))
}

/// Read a record back by id.
pub fn load_record(store: &FilesystemStore, id: &BlobId) -> Result<ExecutionRecord, BuildError> {
    let path = record_path(store, id);
    let bytes = fs::read(&path).map_err(|source| io_error("reading", &path, source))?;
    serde_json::from_slice(&bytes).map_err(BuildError::Encode)
}

/// What a run produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Execution {
    pub id: BlobId,
    pub record: ExecutionRecord,
    pub artifact: Option<ArtifactTree>,
}

// --- running -----------------------------------------------------------

/// How a plan runs: natively in development mode, or inside an
/// isolation engine implementing the plan's contract (§35).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Engine {
    Native,
    Bubblewrap(Bubblewrap),
}

/// A probed bubblewrap: its path and `bwrap --version`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bubblewrap {
    pub path: PathBuf,
    pub version: String,
    /// Whether i386 programs run in its sandboxes: an x86-64 kernel
    /// with IA32 emulation (`DECISIONS.md` §38 sub-decision 7).
    pub runs_x86: bool,
}

impl Bubblewrap {
    /// Check that `path` runs a trivial sandbox here: user namespaces
    /// available, the binary real. The reason when not.
    pub fn probe(path: &Path) -> Result<Self, String> {
        let version = Command::new(path)
            .arg("--version")
            .env_clear()
            .output()
            .map_err(|error| format!("cannot run {}: {error}", path.display()))?;
        if !version.status.success() {
            return Err(format!("{} --version failed", path.display()));
        }
        let version = String::from_utf8_lossy(&version.stdout).trim().to_owned();
        // The probe sandbox runs bwrap's own binary from a read-only
        // view of the host, only to show namespaces work.
        let probe = Command::new(path)
            .args(["--unshare-all", "--unshare-user", "--die-with-parent"])
            .args(["--ro-bind", "/", "/", "--proc", "/proc", "--dev", "/dev"])
            .arg("--")
            .arg(path)
            .arg("--version")
            .env_clear()
            .stdin(Stdio::null())
            .output()
            .map_err(|error| format!("cannot run {}: {error}", path.display()))?;
        if !probe.status.success() {
            return Err(format!(
                "{} cannot create a sandbox here: {}",
                path.display(),
                String::from_utf8_lossy(&probe.stderr).trim()
            ));
        }
        Ok(Self {
            path: path.to_path_buf(),
            version,
            runs_x86: std::env::consts::ARCH == "x86_64" && probe_x86(path),
        })
    }
}

/// A static i386 program of 96 bytes that exits with status 42 (the
/// `exit` system call through `int 0x80`).
fn i386_exit_42() -> Vec<u8> {
    const BASE: u32 = 0x0804_8000;
    let code: [u8; 12] = [
        0xb8, 1, 0, 0, 0, // mov eax, 1 (exit)
        0xbb, 42, 0, 0, 0, // mov ebx, 42
        0xcd, 0x80, // int 0x80
    ];
    // The ELF header (52 bytes), one program header (32), the code.
    const SIZE: u32 = 52 + 32 + 12;
    let mut elf = Vec::with_capacity(96);
    elf.extend_from_slice(b"\x7fELF\x01\x01\x01");
    elf.extend_from_slice(&[0; 9]);
    for half in [2u16, 3] {
        elf.extend_from_slice(&half.to_le_bytes()); // ET_EXEC, EM_386
    }
    for word in [1u32, BASE + 84, 52, 0, 0] {
        elf.extend_from_slice(&word.to_le_bytes()); // version, entry, phoff, shoff, flags
    }
    for half in [52u16, 32, 1, 0, 0, 0] {
        elf.extend_from_slice(&half.to_le_bytes()); // ehsize, phentsize, phnum, sh*
    }
    for word in [1u32, 0, BASE, BASE, SIZE, SIZE, 5, 0x1000] {
        elf.extend_from_slice(&word.to_le_bytes()); // PT_LOAD, r-x
    }
    elf.extend_from_slice(&code);
    elf
}

/// Whether the i386 probe program runs inside a sandbox.
fn probe_x86(bwrap: &Path) -> bool {
    let Ok(dir) = tempfile::tempdir() else {
        return false;
    };
    let program = dir.path().join("probe");
    if fs::write(&program, i386_exit_42()).is_err() || set_executable(&program, true).is_err() {
        return false;
    }
    Command::new(bwrap)
        .args(["--unshare-all", "--unshare-user", "--die-with-parent"])
        .arg("--ro-bind")
        .arg(dir.path())
        .arg("/probe")
        .args(["--proc", "/proc", "--dev", "/dev", "--", "/probe/probe"])
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.code() == Some(42))
}

/// Where `path` (relative, already validated) lands under `src`. No
/// part of it that exists may be a symlink: the snapshot's tree could
/// otherwise point a placement outside the build directory.
fn placement_target(src: &Path, path: &str) -> Result<PathBuf, BuildError> {
    let mut at = src.to_path_buf();
    for part in path.split('/') {
        at.push(part);
        match at.symlink_metadata() {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(BuildError::PlacementOccupied {
                    path: path.to_owned(),
                })
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    Ok(src.join(path))
}

/// What a requirement brings into the build.
enum Placed {
    Content {
        object: ObjectId,
        snapshot: SnapshotId,
    },
    Artifact(BlobId),
    Source {
        source: ObjectId,
        revision: GitRevision,
        url: String,
    },
    File(ObjectId),
}

/// A dependency placed at a path in the source tree.
struct Placement {
    path: String,
    from: Option<String>,
    overlay: bool,
    what: Placed,
    label: String,
}

enum PlaceError {
    /// The tree already fills the path, and the placement isn't an
    /// overlay.
    Occupied,
    Io(BuildError),
}

/// Move `tree` (a file or directory, staged in the workspace) to
/// `inside`. Without `overlay`, `inside` must be absent or an empty
/// directory (a gitlink's hole). With it, the tree fills in around
/// what is already there, which wins: entries are moved only where
/// nothing exists, and directories present on both sides are merged;
/// no symlink in the native tree is ever followed.
fn place(tree: &Path, inside: &Path, overlay: bool) -> Result<(), PlaceError> {
    let io = |action: &'static str, path: &Path| {
        let path = path.to_path_buf();
        move |source| PlaceError::Io(io_error(action, &path, source))
    };
    match inside.symlink_metadata().ok() {
        None => {}
        Some(meta)
            if meta.is_dir()
                && fs::read_dir(inside)
                    .map(|mut entries| entries.next().is_none())
                    .unwrap_or(false) =>
        {
            fs::remove_dir(inside).map_err(io("replacing", inside))?;
        }
        Some(_) if !overlay => return Err(PlaceError::Occupied),
        Some(meta) => {
            let tree_is_dir = tree
                .symlink_metadata()
                .map_err(io("reading", tree))?
                .is_dir();
            if meta.is_dir() && tree_is_dir {
                return merge(tree, inside).map_err(PlaceError::Io);
            }
            // The native tree's entry wins.
            return Ok(());
        }
    }
    if let Some(parent) = inside.parent() {
        fs::create_dir_all(parent).map_err(io("creating", parent))?;
    }
    fs::rename(tree, inside).map_err(io("placing", inside))
}

/// Move what `from` holds into the real directory `into` wherever
/// `into` has nothing; recurse where both have a directory.
fn merge(from: &Path, into: &Path) -> Result<(), BuildError> {
    let entries = fs::read_dir(from).map_err(|source| io_error("reading", from, source))?;
    for entry in entries {
        let entry = entry.map_err(|source| io_error("reading", from, source))?;
        let dest = into.join(entry.file_name());
        match dest.symlink_metadata() {
            Err(_) => fs::rename(entry.path(), &dest)
                .map_err(|source| io_error("placing", &dest, source))?,
            Ok(meta) if meta.is_dir() => {
                let ours = entry
                    .file_type()
                    .map_err(|source| io_error("reading", &entry.path(), source))?;
                if ours.is_dir() {
                    merge(&entry.path(), &dest)?;
                }
            }
            Ok(_) => {}
        }
    }
    Ok(())
}

/// Export a held object's tree under `dest`.
fn export_tree(
    trees: &dyn TreeSource,
    object: &ObjectId,
    snapshot: &SnapshotId,
    dest: &Path,
) -> Result<(), BuildError> {
    let held = trees
        .export(object, snapshot, dest)
        .map_err(|source| BuildError::TreeSource {
            object: object.clone(),
            snapshot: snapshot.clone(),
            source,
        })?;
    if held {
        Ok(())
    } else {
        Err(BuildError::TreeNotHeld {
            object: object.clone(),
            snapshot: snapshot.clone(),
        })
    }
}

fn export_source(
    trees: &dyn TreeSource,
    source: &ObjectId,
    revision: &GitRevision,
    url: &str,
    dest: &Path,
) -> Result<(), BuildError> {
    let held = trees
        .export_source(source, revision, dest)
        .map_err(|error| BuildError::SourceTree {
            url: url.to_owned(),
            revision: revision.to_string(),
            reason: error.to_string(),
        })?;
    if held {
        Ok(())
    } else {
        Err(BuildError::SourceTree {
            url: url.to_owned(),
            revision: revision.to_string(),
            reason: "not held (kairo source fetch)".to_owned(),
        })
    }
}

/// Guest paths of the `linux` contract (`schemas/environment/linux-v1.md`).
const GUEST_SRC: &str = "/kairo/src";
const GUEST_HOME: &str = "/kairo/home";
/// The `linux-root` contract's labels
/// (`schemas/environment/linux-root-v1.md`): the tree is `/`, and
/// `HOME` is the empty `/tmp`.
pub const LINUX_ROOT_PREFIX: &str = "environment:linux-root/";
const ROOTED_HOME: &str = "/tmp";

/// A provider's host directory and where the guest sees it.
struct Mount {
    host: PathBuf,
    guest: String,
}

/// Run `plan`'s target once with `engine`, which must suit the plan:
/// `Native` for a plan with environment `none`, an engine for one with
/// a contract. The plan's `builds_first` must be empty and every
/// artifact provider's artifact held; `source_epoch` is the revision's
/// timestamp, for `SOURCE_DATE_EPOCH`.
pub fn run_plan(
    store: &FilesystemStore,
    trees: &dyn TreeSource,
    plan: &PlanDocument,
    source_epoch: i64,
    limits: &Limits,
    engine: &Engine,
) -> Result<Execution, BuildError> {
    let manifest = &plan.manifest;
    if !manifest.builds_first.is_empty() {
        return Err(BuildError::PrerequisitesUnbuilt {
            builds: manifest
                .builds_first
                .iter()
                .map(|build| format!("object:{} target {}", build.object, build.target))
                .collect(),
        });
    }
    let suits = match engine {
        Engine::Native => manifest.environment == "none",
        Engine::Bubblewrap(_) => manifest.environment != "none",
    };
    if !suits {
        return Err(BuildError::EngineMismatch {
            environment: manifest.environment.clone(),
        });
    }
    let isolated = matches!(engine, Engine::Bubblewrap(_));
    // The tree is the guest's `/` (§38 sub-decision 5).
    let rooted = isolated && manifest.environment.starts_with(LINUX_ROOT_PREFIX);
    let object = ObjectId::new(manifest.object.clone()).map_err(BuildError::Id)?;
    let snapshot = SnapshotId::new(manifest.snapshot.clone()).map_err(BuildError::Id)?;

    let workspaces = store.root().join(WORKSPACES_DIR);
    fs::create_dir_all(&workspaces).map_err(|source| io_error("creating", &workspaces, source))?;
    let workspace = tempfile::Builder::new()
        .prefix("ws-")
        .tempdir_in(&workspaces)
        .map_err(|source| io_error("creating a workspace under", &workspaces, source))?;
    let src = workspace.path().join("src");
    let deps = workspace.path().join("deps");
    let home = workspace.path().join("home");
    let log = workspace.path().join("log");
    for dir in [&src, &deps, &home, &log] {
        fs::create_dir(dir).map_err(|source| io_error("creating", dir, source))?;
    }
    // What a path is called where the command runs.
    let seen = |mount: &Mount| -> String {
        if isolated {
            mount.guest.clone()
        } else {
            mount.host.display().to_string()
        }
    };

    // The snapshot's tree, and the target it declares.
    if !trees
        .export(&object, &snapshot, &src)
        .map_err(|source| BuildError::TreeSource {
            object: object.clone(),
            snapshot: snapshot.clone(),
            source,
        })?
    {
        return Err(BuildError::TreeNotHeld { object, snapshot });
    }
    let manifest_path = src.join("kairo.toml");
    let manifest_text = fs::read_to_string(&manifest_path)
        .map_err(|source| io_error("reading", &manifest_path, source))?;
    let object_manifest =
        ObjectManifest::parse_toml(&manifest_text).map_err(BuildError::Manifest)?;
    let target = object_manifest
        .build_target(&manifest.target)
        .ok_or_else(|| BuildError::NoTarget {
            object: object.clone(),
            target: manifest.target.clone(),
        })?;

    // Providers, at paths named by what they are (§35 sub-decision 1),
    // or placed in the tree (§37 sub-decision 7, §38 sub-decision 4),
    // and the environment they contribute. Under `linux-root` the tree
    // is the guest's `/`, so everything must be placed (§38
    // sub-decision 5).
    let guest_src = if rooted { "" } else { GUEST_SRC };
    let mut env: BTreeMap<String, String> = BTreeMap::new();
    let mut mounts: Vec<Mount> = Vec::new();
    let mut path_dirs: Vec<String> = Vec::new();
    let mut host_tool_dirs: Vec<String> = Vec::new();
    let mut host_tools: Vec<String> = Vec::new();
    let mut placements: Vec<Placement> = Vec::new();
    for requirement in &manifest.requirements {
        let label = match &requirement.requested {
            kairo_resolver::Requested::Provides { provides, .. } => provides.clone(),
            kairo_resolver::Requested::Object { object, .. } => format!("object:{object}"),
            kairo_resolver::Requested::Source { source, .. } => format!(
                "source:{}",
                source.rsplit('/').next().unwrap_or(source.as_str())
            ),
            kairo_resolver::Requested::File { path, .. } => format!("file:{path}"),
        };
        let placed = requirement.requested.placement();
        let unplaced = |what: String| -> Result<(), BuildError> {
            if rooted {
                return Err(BuildError::Environment(format!(
                    "{what} has no path, and under {} every dependency must be placed",
                    manifest.environment
                )));
            }
            Ok(())
        };
        let what = match &requirement.resolved {
            Resolved::Content {
                object: provider,
                snapshot: provider_snapshot,
            } => Placed::Content {
                object: ObjectId::new(provider.clone()).map_err(BuildError::Id)?,
                snapshot: SnapshotId::new(provider_snapshot.clone()).map_err(BuildError::Id)?,
            },
            Resolved::Artifact {
                object: provider,
                artifact,
                target: provider_target,
                ..
            } => {
                let Some(artifact) = artifact else {
                    return Err(BuildError::PrerequisitesUnbuilt {
                        builds: vec![format!("object:{provider} target {provider_target}")],
                    });
                };
                Placed::Artifact(BlobId::new(artifact.clone()).map_err(BuildError::Id)?)
            }
            Resolved::Source {
                source,
                url,
                revision,
                ..
            } => Placed::Source {
                source: ObjectId::new(source.clone()).map_err(BuildError::Id)?,
                revision: GitRevision::parse(revision).ok().flatten().ok_or_else(|| {
                    BuildError::Environment(format!("{revision} is not a git: revision"))
                })?,
                url: url.clone(),
            },
            Resolved::File { source, .. } => {
                Placed::File(ObjectId::new(source.clone()).map_err(BuildError::Id)?)
            }
            Resolved::HostTool { path, .. } => {
                if isolated || placed.is_some() {
                    return Err(BuildError::EngineMismatch {
                        environment: format!("{} with host tool {path}", manifest.environment),
                    });
                }
                let tool = PathBuf::from(path);
                if let Some(dir) = tool.parent() {
                    let dir = dir.display().to_string();
                    if !host_tool_dirs.contains(&dir) {
                        host_tool_dirs.push(dir);
                    }
                }
                host_tools.push(path.clone());
                env.insert(provider_var(&label), path.clone());
                continue;
            }
            Resolved::Environment { contract, .. } => {
                if placed.is_some() {
                    return Err(BuildError::Environment(format!(
                        "{contract} is an environment and can't be placed"
                    )));
                }
                continue;
            }
        };
        // Placed in the source tree: a writable copy, made once the loop
        // is done so outer placements land before the ones nested in
        // them.
        if let Some(placed) = placed {
            placements.push(Placement {
                path: placed.path.to_owned(),
                from: placed.from.map(str::to_owned),
                overlay: placed.overlay,
                what,
                label,
            });
            continue;
        }
        let mount = match what {
            Placed::Content { object, snapshot } => {
                unplaced(format!("object:{object}"))?;
                let host = deps.join("content").join(snapshot.as_str());
                if !host.exists() {
                    fs::create_dir_all(&host)
                        .map_err(|source| io_error("creating", &host, source))?;
                    export_tree(trees, &object, &snapshot, &host)?;
                }
                Mount {
                    host,
                    guest: format!("/kairo/content/{snapshot}"),
                }
            }
            Placed::Artifact(artifact) => {
                unplaced(format!("artifact {artifact}"))?;
                let host = deps.join("artifact").join(artifact.as_str());
                if !host.exists() {
                    fs::create_dir_all(&host)
                        .map_err(|source| io_error("creating", &host, source))?;
                    materialize_artifact(store, &artifact, &host)?;
                }
                Mount {
                    host,
                    guest: format!("/kairo/artifact/{artifact}"),
                }
            }
            Placed::Source {
                source,
                revision,
                url,
            } => {
                unplaced(format!("{url} at {revision}"))?;
                let staged = deps
                    .join("source")
                    .join(source.as_str())
                    .join(revision.oid());
                if !staged.exists() {
                    fs::create_dir_all(&staged)
                        .map_err(|source| io_error("creating", &staged, source))?;
                    export_source(trees, &source, &revision, &url, &staged)?;
                }
                let mount = Mount {
                    host: staged,
                    guest: format!("/kairo/source/{source}/{}", revision.oid()),
                };
                env.insert(provider_var(&label), seen(&mount));
                if !mounts.iter().any(|existing| existing.guest == mount.guest) {
                    mounts.push(mount);
                }
                continue;
            }
            // A file always has a path.
            Placed::File(_) => continue,
        };
        let bin = format!("{}/bin", seen(&mount));
        if mount.host.join("bin").is_dir() && !path_dirs.contains(&bin) {
            path_dirs.push(bin);
        }
        env.insert(provider_var(&label), seen(&mount));
        if !mounts.iter().any(|existing| existing.guest == mount.guest) {
            mounts.push(mount);
        }
    }
    // Shallowest first, so a placement nested in another lands in it.
    placements.sort_by_key(|placement| placement.path.split('/').count());
    for (index, placement) in placements.into_iter().enumerate() {
        let inside = placement_target(&src, &placement.path)?;
        let staging = deps.join("placed").join(index.to_string());
        fs::create_dir_all(&staging).map_err(|source| io_error("creating", &staging, source))?;
        let tree = match &placement.what {
            Placed::File(source) => {
                let bytes = store.get_file_source_bytes(source).map_err(|error| {
                    BuildError::FileSource {
                        path: placement.path.clone(),
                        reason: error.to_string(),
                    }
                })?;
                let file = staging.join("file");
                fs::write(&file, bytes).map_err(|source| io_error("writing", &file, source))?;
                file
            }
            Placed::Content { object, snapshot } => {
                export_tree(trees, object, snapshot, &staging)?;
                staging.clone()
            }
            Placed::Artifact(artifact) => {
                materialize_artifact(store, artifact, &staging)?;
                staging.clone()
            }
            Placed::Source {
                source,
                revision,
                url,
            } => {
                export_source(trees, source, revision, url, &staging)?;
                staging.clone()
            }
        };
        let tree = match &placement.from {
            None => tree,
            Some(from) => {
                let sub = placement_target(&tree, from)?;
                if !sub
                    .symlink_metadata()
                    .is_ok_and(|meta| meta.file_type().is_dir())
                {
                    return Err(BuildError::Environment(format!(
                        "the dependency placed at {} has no directory {from}",
                        placement.path
                    )));
                }
                sub
            }
        };
        place(&tree, &inside, placement.overlay).map_err(|error| match error {
            PlaceError::Occupied => BuildError::PlacementOccupied {
                path: placement.path.clone(),
            },
            PlaceError::Io(error) => error,
        })?;
        let mount = Mount {
            host: inside,
            guest: format!("{guest_src}/{}", placement.path),
        };
        if matches!(placement.what, Placed::Artifact(_) | Placed::Content { .. }) {
            let bin = format!("{}/bin", seen(&mount));
            if mount.host.join("bin").is_dir() && !path_dirs.contains(&bin) {
                path_dirs.push(bin);
            }
        }
        if !matches!(placement.what, Placed::File(_)) {
            env.insert(provider_var(&placement.label), seen(&mount));
        }
    }
    path_dirs.extend(host_tool_dirs);
    let src_mount = Mount {
        host: src.clone(),
        guest: if rooted { "/" } else { GUEST_SRC }.to_owned(),
    };
    let home_mount = Mount {
        host: home.clone(),
        guest: if rooted { ROOTED_HOME } else { GUEST_HOME }.to_owned(),
    };
    env.insert("HOME".to_owned(), seen(&home_mount));
    env.insert("LANG".to_owned(), "C".to_owned());
    env.insert("SOURCE_DATE_EPOCH".to_owned(), source_epoch.to_string());
    env.insert("KAIRO_SRC".to_owned(), seen(&src_mount));
    env.insert("PATH".to_owned(), path_dirs.join(":"));
    if isolated {
        env.insert("TZ".to_owned(), "UTC".to_owned());
    }

    // Run.
    let workdir = if target.workdir() == "." {
        src.clone()
    } else {
        src.join(target.workdir())
    };
    let guest_workdir = match (target.workdir(), rooted) {
        (".", true) => "/".to_owned(),
        (".", false) => GUEST_SRC.to_owned(),
        (workdir, _) => format!("{guest_src}/{workdir}"),
    };
    let stdout_path = log.join("stdout");
    let stderr_path = log.join("stderr");
    let stdout = fs::File::create(&stdout_path)
        .map_err(|source| io_error("creating", &stdout_path, source))?;
    let stderr = fs::File::create(&stderr_path)
        .map_err(|source| io_error("creating", &stderr_path, source))?;
    let command = target.command();
    let mut process = match engine {
        Engine::Native => {
            let mut process = Command::new(&command[0]);
            process
                .args(&command[1..])
                .current_dir(&workdir)
                .env_clear()
                .envs(&env);
            process
        }
        Engine::Bubblewrap(bwrap) => {
            let mut process = Command::new(&bwrap.path);
            process
                .env_clear()
                .args([
                    "--unshare-all",
                    "--unshare-user",
                    "--die-with-parent",
                    "--new-session",
                ])
                .args(["--hostname", "kairo"]);
            if rooted {
                // Root inside its own namespaces, still the invoking
                // user on the host (`DECISIONS.md` §39 sub-decision 4).
                process.args(["--uid", "0", "--gid", "0", "--cap-add", "ALL"]);
            } else {
                process.args(["--uid", "1000", "--gid", "1000"]);
            }
            if rooted {
                // The tree first, so the kernel filesystems mount over
                // it; nothing else from the host.
                process
                    .arg("--bind")
                    .arg(&src)
                    .arg("/")
                    .args(["--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp"]);
            } else {
                process
                    .args(["--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp"])
                    .args(["--dir", "/kairo"])
                    .arg("--bind")
                    .arg(&src)
                    .arg(GUEST_SRC)
                    .arg("--bind")
                    .arg(&home)
                    .arg(GUEST_HOME);
            }
            for mount in &mounts {
                process.arg("--ro-bind").arg(&mount.host).arg(&mount.guest);
            }
            process.arg("--clearenv");
            for (key, value) in &env {
                process.arg("--setenv").arg(key).arg(value);
            }
            process
                .arg("--chdir")
                .arg(&guest_workdir)
                .arg("--")
                .args(command);
            process
        }
    };
    let started_at = Timestamp::now();
    let started = Instant::now();
    let mut diagnostics = Vec::new();
    let mut child = process
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .process_group(0)
        .spawn();
    let (status, exit_code) = match &mut child {
        Err(error) => {
            diagnostics.push(format!("cannot start {:?}: {error}", command[0]));
            (Status::Failure, None)
        }
        Ok(child) => loop {
            match child.try_wait() {
                Ok(Some(exit)) => {
                    let code = exit.code();
                    if exit.success() {
                        break (Status::Success, code);
                    }
                    diagnostics.push(match code {
                        Some(code) => format!("{:?} exited with status {code}", command[0]),
                        None => format!("{:?} was killed by a signal", command[0]),
                    });
                    break (Status::Failure, code);
                }
                Ok(None) if started.elapsed() >= limits.timeout => {
                    kill_group(child.id());
                    let _ = child.wait();
                    diagnostics.push(format!(
                        "killed after {} seconds (runtime.timeout_secs)",
                        limits.timeout.as_secs()
                    ));
                    break (Status::Timeout, None);
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(25)),
                Err(error) => {
                    diagnostics.push(format!("waiting for {:?}: {error}", command[0]));
                    break (Status::Failure, None);
                }
            }
        },
    };

    // Outputs, only after a successful exit.
    let mut outputs = Vec::new();
    let mut entries = Vec::new();
    let mut status = status;
    if status == Status::Success {
        for output in target.outputs() {
            let path = workdir.join(output.path());
            match collect_output(store, &src, &path, output.artifact_path(), output.kind()) {
                Ok(collected) => {
                    outputs.push(OutputRecord {
                        name: output.name().to_owned(),
                        artifact_path: output.artifact_path().to_owned(),
                        files: collected.len(),
                    });
                    entries.extend(collected);
                }
                Err(reason) => {
                    diagnostics.push(format!("output {}: {reason}", output.name()));
                    status = Status::Failure;
                }
            }
        }
    }
    // The outputs together must make a well-formed tree: no two at one
    // path, and nothing under a link (`DECISIONS.md` §39).
    if status == Status::Success {
        if let Err(reason) = ArtifactTree::new(entries.clone()).validate() {
            diagnostics.push(format!("outputs: {reason}"));
            status = Status::Failure;
        }
    }
    let artifact = if status == Status::Success {
        let tree = ArtifactTree::new(entries);
        let bytes = tree.to_bytes().map_err(BuildError::Encode)?;
        let id = BlobId::from_bytes(ARTIFACT_TREE_DOMAIN, &bytes);
        store.put_blob(&id, &bytes).map_err(BuildError::Store)?;
        Some((id, tree))
    } else {
        None
    };

    let logs = Logs {
        stdout: store_log(store, &stdout_path)?.to_string(),
        stderr: store_log(store, &stderr_path)?.to_string(),
    };
    let record = ExecutionRecord {
        schema: EXECUTION_RECORD_SCHEMA.to_owned(),
        run: run_id()?,
        plan: plan.hash.to_string(),
        object: manifest.object.clone(),
        snapshot: manifest.snapshot.clone(),
        target: manifest.target.clone(),
        executor: executor_info(engine, rooted),
        environment: manifest.environment.clone(),
        host_dependent: !isolated || manifest.host_dependent,
        host: HostInfo {
            system: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.replace('_', "-"),
            tools: host_tools,
            kernel: fs::read_to_string("/proc/sys/kernel/osrelease")
                .map(|release| release.trim().to_owned())
                .unwrap_or_default(),
        },
        started_at: started_at.to_string(),
        completed_at: Timestamp::now().to_string(),
        status,
        exit_code,
        outputs,
        artifact: artifact.as_ref().map(|(id, _)| id.to_string()),
        logs,
        diagnostics,
    };
    let bytes = record.to_bytes().map_err(BuildError::Encode)?;
    let id = BlobId::from_bytes(EXECUTION_RECORD_DOMAIN, &bytes);
    let path = record_path(store, &id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| io_error("creating", parent, source))?;
    }
    fs::write(&path, &bytes).map_err(|source| io_error("writing", &path, source))?;
    Ok(Execution {
        id,
        record,
        artifact: artifact.map(|(_, tree)| tree),
    })
}

fn run_id() -> Result<String, BuildError> {
    use std::io::Read;
    let mut bytes = [0u8; 16];
    fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(|source| io_error("reading", Path::new("/dev/urandom"), source))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// `KAIRO_PROVIDER_<LABEL>`, the label upper-cased with every
/// non-alphanumeric character as `_`.
pub fn provider_var(label: &str) -> String {
    let mut name = String::from("KAIRO_PROVIDER_");
    name.extend(label.chars().map(|c| {
        if c.is_ascii_alphanumeric() {
            c.to_ascii_uppercase()
        } else {
            '_'
        }
    }));
    name
}

fn kill_group(pid: u32) {
    if let Ok(pid) = i32::try_from(pid) {
        let _ = nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
}

/// The files of one declared output as artifact entries. Paths are
/// confined to the source tree, so a symlink can't reach outside it.
fn collect_output(
    store: &FilesystemStore,
    src: &Path,
    path: &Path,
    artifact_path: &str,
    kind: OutputKind,
) -> Result<Vec<ArtifactEntry>, String> {
    let root = src.canonicalize().map_err(|error| error.to_string())?;
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| format!("declared path {} was not produced", path.display()))?;
    if metadata.file_type().is_symlink() {
        // A file output may be a link, recorded as one; a directory
        // output must be a directory.
        if kind == OutputKind::Directory {
            return Err(format!(
                "{} is a symbolic link; a directory output must be a directory",
                path.display()
            ));
        }
        return Ok(vec![link_entry(path, artifact_path)?]);
    }
    let actual = path.canonicalize().map_err(|error| error.to_string())?;
    if !actual.starts_with(&root) {
        return Err(format!("{} lies outside the source tree", path.display()));
    }
    let mut entries = Vec::new();
    match kind {
        OutputKind::File => {
            if !metadata.is_file() {
                return Err(format!("{} is not a regular file", path.display()));
            }
            entries.push(store_file(store, &actual, artifact_path)?);
        }
        OutputKind::Directory => {
            if !metadata.is_dir() {
                return Err(format!("{} is not a directory", path.display()));
            }
            walk_dir(store, &root, &actual, artifact_path, &mut entries)?;
        }
    }
    Ok(entries)
}

fn walk_dir(
    store: &FilesystemStore,
    root: &Path,
    dir: &Path,
    artifact_path: &str,
    entries: &mut Vec<ArtifactEntry>,
) -> Result<(), String> {
    let mut children: Vec<_> = fs::read_dir(dir)
        .map_err(|error| error.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|error| error.to_string())?;
    children.sort_by_key(|entry| entry.file_name());
    for child in children {
        let name = child
            .file_name()
            .into_string()
            .map_err(|name| format!("{name:?} is not UTF-8"))?;
        let child_path = child.path();
        let metadata = child.metadata().map_err(|error| error.to_string())?;
        let child_artifact = format!("{artifact_path}/{name}");
        // Recorded, never followed (`DECISIONS.md` §39).
        if metadata.file_type().is_symlink() {
            entries.push(link_entry(&child_path, &child_artifact)?);
            continue;
        }
        let actual = child_path
            .canonicalize()
            .map_err(|error| error.to_string())?;
        if !actual.starts_with(root) {
            return Err(format!(
                "{} lies outside the source tree",
                child_path.display()
            ));
        }
        if metadata.is_dir() {
            walk_dir(store, root, &actual, &child_artifact, entries)?;
        } else if metadata.is_file() {
            entries.push(store_file(store, &actual, &child_artifact)?);
        } else {
            return Err(format!(
                "{} is neither a file nor a directory",
                child_path.display()
            ));
        }
    }
    Ok(())
}

/// A link entry for the symbolic link at `path`, its target checked.
fn link_entry(path: &Path, artifact_path: &str) -> Result<ArtifactEntry, String> {
    let target = fs::read_link(path)
        .map_err(|error| format!("reading the link {}: {error}", path.display()))?;
    let target = target
        .into_os_string()
        .into_string()
        .map_err(|target| format!("{} links to {target:?}, which is not UTF-8", path.display()))?;
    if let Some(problem) = link_target_problem(artifact_path, &target) {
        return Err(format!(
            "{} links to {target:?}, which {problem}; links must stay inside the artifact",
            path.display()
        ));
    }
    Ok(ArtifactEntry::Link(LinkEntry {
        path: artifact_path.to_owned(),
        link: target,
    }))
}

fn store_file(
    store: &FilesystemStore,
    path: &Path,
    artifact_path: &str,
) -> Result<ArtifactEntry, String> {
    use std::os::unix::fs::PermissionsExt;
    let bytes = fs::read(path).map_err(|error| format!("reading {}: {error}", path.display()))?;
    let executable = fs::metadata(path)
        .map_err(|error| error.to_string())?
        .permissions()
        .mode()
        & 0o111
        != 0;
    let blob = BlobId::from_bytes(ARTIFACT_BLOB_DOMAIN, &bytes);
    store
        .put_blob(&blob, &bytes)
        .map_err(|error| format!("storing {}: {error}", path.display()))?;
    Ok(ArtifactEntry::File(FileEntry {
        path: artifact_path.to_owned(),
        blob: blob.to_string(),
        executable,
    }))
}

fn store_log(store: &FilesystemStore, path: &Path) -> Result<BlobId, BuildError> {
    let bytes = fs::read(path).map_err(|source| io_error("reading", path, source))?;
    let id = BlobId::from_bytes(LOG_BLOB_DOMAIN, &bytes);
    store.put_blob(&id, &bytes).map_err(BuildError::Store)?;
    Ok(id)
}

fn io_error(action: &'static str, path: &Path, source: io::Error) -> BuildError {
    BuildError::Io {
        action,
        path: path.to_path_buf(),
        source,
    }
}

// --- errors ------------------------------------------------------------

/// Why a build couldn't be attempted or recorded. A build that ran and
/// failed is a [`Status::Failure`] record, not an error.
#[derive(Debug)]
pub enum BuildError {
    Store(kairo_store::StoreError),
    Io {
        action: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    Id(kairo_core::IdError),
    Encode(serde_json::Error),
    UnsupportedSchema(String),
    Manifest(kairo_object::ManifestError),
    TreeSource {
        object: ObjectId,
        snapshot: SnapshotId,
        source: SourceError,
    },
    TreeNotHeld {
        object: ObjectId,
        snapshot: SnapshotId,
    },
    NoTarget {
        object: ObjectId,
        target: String,
    },
    /// The plan needs builds first, or an artifact provider's artifact
    /// isn't held.
    PrerequisitesUnbuilt {
        builds: Vec<String>,
    },
    UnsafeArtifactPath {
        artifact: BlobId,
        path: String,
    },
    /// An artifact tree that breaks its shape rules (`DECISIONS.md`
    /// §39): a link escaping it, or an entry under a link.
    BadArtifact(String),
    Environment(String),
    /// The engine doesn't suit the plan's environment.
    EngineMismatch {
        environment: String,
    },
    /// A source dependency's tree couldn't be materialized.
    SourceTree {
        url: String,
        revision: String,
        reason: String,
    },
    /// A source dependency's `path` already holds something in the
    /// snapshot's tree.
    PlacementOccupied {
        path: String,
    },
    /// A file source's bytes couldn't be read or failed their hash.
    FileSource {
        path: String,
        reason: String,
    },
    /// A record or blob whose id doesn't match its bytes.
    Fixity {
        what: String,
        actual: BlobId,
    },
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(f, "store: {error}"),
            Self::Io {
                action,
                path,
                source,
            } => write!(f, "{action} {}: {source}", path.display()),
            Self::Id(error) => write!(f, "invalid id in the plan: {error}"),
            Self::Encode(error) => write!(f, "encoding: {error}"),
            Self::UnsupportedSchema(schema) => write!(f, "unsupported schema {schema}"),
            Self::Manifest(error) => write!(f, "the snapshot's kairo.toml: {error}"),
            Self::TreeSource {
                object,
                snapshot,
                source,
            } => write!(
                f,
                "exporting the tree of object:{object}:snapshot:{snapshot}: {source}"
            ),
            Self::TreeNotHeld { object, snapshot } => write!(
                f,
                "the tree of object:{object}:snapshot:{snapshot} is not held; fetch it with kairo git fetch"
            ),
            Self::NoTarget { object, target } => {
                write!(f, "object:{object} declares no build target {target}")
            }
            Self::PrerequisitesUnbuilt { builds } => write!(
                f,
                "the plan needs builds first, whose results aren't held: {}",
                builds.join(", ")
            ),
            Self::BadArtifact(reason) => write!(f, "artifact tree: {reason}"),
            Self::UnsafeArtifactPath { artifact, path } => {
                write!(f, "artifact {artifact} has an unsafe path {path:?}")
            }
            Self::Environment(message) => write!(f, "building the environment: {message}"),
            Self::SourceTree {
                url,
                revision,
                reason,
            } => write!(f, "source {url} at {revision}: {reason}"),
            Self::PlacementOccupied { path } => write!(
                f,
                "a dependency is placed at {path}, which the snapshot's tree already fills (or reaches through a symlink)"
            ),
            Self::FileSource { path, reason } => {
                write!(f, "the file placed at {path}: {reason}")
            }
            Self::EngineMismatch { environment } => write!(
                f,
                "the plan's environment ({environment}) doesn't suit this executor"
            ),
            Self::Fixity { what, actual } => {
                write!(f, "{what} doesn't match its contents (they hash to {actual})")
            }
        }
    }
}

impl Error for BuildError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Store(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            Self::Id(error) => Some(error),
            Self::Encode(error) => Some(error),
            Self::Manifest(error) => Some(error),
            Self::TreeSource { source, .. } => Some(source),
            Self::UnsupportedSchema(_)
            | Self::TreeNotHeld { .. }
            | Self::NoTarget { .. }
            | Self::PrerequisitesUnbuilt { .. }
            | Self::UnsafeArtifactPath { .. }
            | Self::BadArtifact(_)
            | Self::Environment(_)
            | Self::EngineMismatch { .. }
            | Self::SourceTree { .. }
            | Self::PlacementOccupied { .. }
            | Self::FileSource { .. }
            | Self::Fixity { .. } => None,
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests;

// --- execution evidence in bundles ------------------------------------

/// The blobs an execution record names, each with the domain its id is
/// derived under, so an importer can fixity-check every one.
fn evidence_blobs(
    store: &FilesystemStore,
    record: &ExecutionRecord,
) -> Result<Vec<(BlobId, &'static [u8])>, BuildError> {
    let mut blobs = vec![
        (
            BlobId::new(record.plan.clone()).map_err(BuildError::Id)?,
            kairo_resolver::RESOLVED_MANIFEST_DOMAIN,
        ),
        (
            BlobId::new(record.logs.stdout.clone()).map_err(BuildError::Id)?,
            LOG_BLOB_DOMAIN,
        ),
        (
            BlobId::new(record.logs.stderr.clone()).map_err(BuildError::Id)?,
            LOG_BLOB_DOMAIN,
        ),
    ];
    if let Some(artifact) = &record.artifact {
        let artifact = BlobId::new(artifact.clone()).map_err(BuildError::Id)?;
        let tree = load_artifact(store, &artifact)?;
        blobs.push((artifact, ARTIFACT_TREE_DOMAIN));
        for blob in tree.entries.iter().filter_map(ArtifactEntry::blob) {
            blobs.push((
                BlobId::new(blob).map_err(BuildError::Id)?,
                ARTIFACT_BLOB_DOMAIN,
            ));
        }
    }
    blobs.sort_by(|a, b| a.0.cmp(&b.0));
    blobs.dedup_by(|a, b| a.0 == b.0);
    Ok(blobs)
}

/// Write execution `id`'s evidence under `dir/<id>/`: `record.json`
/// and every blob it names under `blobs/` (`PACKAGE.md` §14).
pub fn export_execution(
    store: &FilesystemStore,
    id: &BlobId,
    dir: &Path,
) -> Result<(), BuildError> {
    let record_file = record_path(store, id);
    let bytes =
        fs::read(&record_file).map_err(|source| io_error("reading", &record_file, source))?;
    let record: ExecutionRecord = serde_json::from_slice(&bytes).map_err(BuildError::Encode)?;
    let root = dir.join(id.as_str());
    let blobs_dir = root.join("blobs");
    fs::create_dir_all(&blobs_dir).map_err(|source| io_error("creating", &blobs_dir, source))?;
    let record_out = root.join("record.json");
    fs::write(&record_out, &bytes).map_err(|source| io_error("writing", &record_out, source))?;
    for (blob, _) in evidence_blobs(store, &record)? {
        let data = store.get_blob(&blob).map_err(BuildError::Store)?;
        let path = blobs_dir.join(blob.as_str());
        fs::write(&path, data).map_err(|source| io_error("writing", &path, source))?;
    }
    Ok(())
}

/// Read execution `id`'s evidence from `dir/<id>/`, fixity-checking
/// the record and every blob against its domain, and store it. The
/// artifact tree is stored before its files are checked, since it
/// names them.
pub fn import_execution(
    store: &FilesystemStore,
    id: &BlobId,
    dir: &Path,
) -> Result<ExecutionRecord, BuildError> {
    let root = dir.join(id.as_str());
    let record_file = root.join("record.json");
    let bytes =
        fs::read(&record_file).map_err(|source| io_error("reading", &record_file, source))?;
    let derived = BlobId::from_bytes(EXECUTION_RECORD_DOMAIN, &bytes);
    if &derived != id {
        return Err(BuildError::Fixity {
            what: format!("execution record {id}"),
            actual: derived,
        });
    }
    let record: ExecutionRecord = serde_json::from_slice(&bytes).map_err(BuildError::Encode)?;
    if record.schema != EXECUTION_RECORD_SCHEMA {
        return Err(BuildError::UnsupportedSchema(record.schema));
    }
    let read_checked = |blob: &BlobId, domain: &[u8]| -> Result<Vec<u8>, BuildError> {
        let path = root.join("blobs").join(blob.as_str());
        let data = fs::read(&path).map_err(|source| io_error("reading", &path, source))?;
        let derived = BlobId::from_bytes(domain, &data);
        if &derived != blob {
            return Err(BuildError::Fixity {
                what: format!("blob {blob} of execution {id}"),
                actual: derived,
            });
        }
        Ok(data)
    };
    if let Some(artifact) = &record.artifact {
        let artifact = BlobId::new(artifact.clone()).map_err(BuildError::Id)?;
        let tree = read_checked(&artifact, ARTIFACT_TREE_DOMAIN)?;
        ArtifactTree::from_bytes(&tree)?;
        store
            .put_blob(&artifact, &tree)
            .map_err(BuildError::Store)?;
    }
    for (blob, domain) in evidence_blobs(store, &record)? {
        let data = read_checked(&blob, domain)?;
        store.put_blob(&blob, &data).map_err(BuildError::Store)?;
    }
    let path = record_path(store, id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| io_error("creating", parent, source))?;
    }
    fs::write(&path, &bytes).map_err(|source| io_error("writing", &path, source))?;
    Ok(record)
}
