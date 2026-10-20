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
use kairo_store::{BlobStore, FilesystemStore};
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactEntry {
    /// `/`-separated, relative.
    pub path: String,
    pub blob: String,
    pub executable: bool,
}

impl ArtifactTree {
    fn new(mut entries: Vec<ArtifactEntry>) -> Self {
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Self {
            schema: ARTIFACT_TREE_SCHEMA.to_owned(),
            entries,
        }
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

/// Write a held artifact's files under `dest`, which exists.
pub fn materialize_artifact(
    store: &FilesystemStore,
    artifact: &BlobId,
    dest: &Path,
) -> Result<ArtifactTree, BuildError> {
    let tree = load_artifact(store, artifact)?;
    for entry in &tree.entries {
        let path = safe_join(dest, &entry.path).ok_or_else(|| BuildError::UnsafeArtifactPath {
            artifact: artifact.clone(),
            path: entry.path.clone(),
        })?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| io_error("creating", parent, source))?;
        }
        let blob = BlobId::new(entry.blob.clone()).map_err(|_| BuildError::UnsafeArtifactPath {
            artifact: artifact.clone(),
            path: entry.path.clone(),
        })?;
        let bytes = store.get_blob(&blob).map_err(BuildError::Store)?;
        fs::write(&path, bytes).map_err(|source| io_error("writing", &path, source))?;
        set_executable(&path, entry.executable)?;
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
    /// What the executor doesn't do.
    pub isolation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostInfo {
    pub system: String,
    pub arch: String,
    /// The development-mode host tools the plan used, by path.
    pub tools: Vec<String>,
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

/// Run `plan`'s target once. The plan's `builds_first` must be empty
/// and every artifact provider's artifact held; `source_epoch` is the
/// revision's timestamp, for `SOURCE_DATE_EPOCH`.
pub fn run_plan(
    store: &FilesystemStore,
    trees: &dyn TreeSource,
    plan: &PlanDocument,
    source_epoch: i64,
    limits: &Limits,
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

    // Providers, and the environment they contribute.
    let mut env: BTreeMap<String, String> = BTreeMap::new();
    let mut path_dirs: Vec<PathBuf> = Vec::new();
    let mut host_tool_dirs: Vec<PathBuf> = Vec::new();
    let mut host_tools: Vec<String> = Vec::new();
    for requirement in &manifest.requirements {
        let label = match &requirement.requested {
            kairo_resolver::Requested::Provides { provides, .. } => provides.clone(),
            kairo_resolver::Requested::Object { object, .. } => format!("object:{object}"),
        };
        match &requirement.resolved {
            Resolved::Content {
                object: provider,
                snapshot: provider_snapshot,
            } => {
                let provider_id = ObjectId::new(provider.clone()).map_err(BuildError::Id)?;
                let provider_snapshot =
                    SnapshotId::new(provider_snapshot.clone()).map_err(BuildError::Id)?;
                let dest = deps.join(provider);
                if !dest.exists() {
                    fs::create_dir(&dest).map_err(|source| io_error("creating", &dest, source))?;
                    if !trees
                        .export(&provider_id, &provider_snapshot, &dest)
                        .map_err(|source| BuildError::TreeSource {
                            object: provider_id.clone(),
                            snapshot: provider_snapshot.clone(),
                            source,
                        })?
                    {
                        return Err(BuildError::TreeNotHeld {
                            object: provider_id,
                            snapshot: provider_snapshot,
                        });
                    }
                }
                env.insert(provider_var(&label), dest.display().to_string());
            }
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
                let artifact = BlobId::new(artifact.clone()).map_err(BuildError::Id)?;
                let dest = deps.join(provider);
                if !dest.exists() {
                    fs::create_dir(&dest).map_err(|source| io_error("creating", &dest, source))?;
                    materialize_artifact(store, &artifact, &dest)?;
                }
                let bin = dest.join("bin");
                if bin.is_dir() && !path_dirs.contains(&bin) {
                    path_dirs.push(bin);
                }
                env.insert(provider_var(&label), dest.display().to_string());
            }
            Resolved::HostTool { path, .. } => {
                let tool = PathBuf::from(path);
                if let Some(dir) = tool.parent() {
                    if !host_tool_dirs.contains(&dir.to_path_buf()) {
                        host_tool_dirs.push(dir.to_path_buf());
                    }
                }
                host_tools.push(path.clone());
                env.insert(provider_var(&label), path.clone());
            }
            Resolved::Environment { .. } => {}
        }
    }
    path_dirs.extend(host_tool_dirs);
    let path_value = std::env::join_paths(&path_dirs)
        .map_err(|error| BuildError::Environment(error.to_string()))?;
    env.insert("HOME".to_owned(), home.display().to_string());
    env.insert("LANG".to_owned(), "C".to_owned());
    env.insert("SOURCE_DATE_EPOCH".to_owned(), source_epoch.to_string());
    env.insert("KAIRO_SRC".to_owned(), src.display().to_string());
    env.insert("PATH".to_owned(), path_value.to_string_lossy().into_owned());

    // Run.
    let workdir = if target.workdir() == "." {
        src.clone()
    } else {
        src.join(target.workdir())
    };
    let stdout_path = log.join("stdout");
    let stderr_path = log.join("stderr");
    let stdout = fs::File::create(&stdout_path)
        .map_err(|source| io_error("creating", &stdout_path, source))?;
    let stderr = fs::File::create(&stderr_path)
        .map_err(|source| io_error("creating", &stderr_path, source))?;
    let started_at = Timestamp::now();
    let started = Instant::now();
    let mut diagnostics = Vec::new();
    let command = target.command();
    let mut child = Command::new(&command[0])
        .args(&command[1..])
        .current_dir(&workdir)
        .env_clear()
        .envs(&env)
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
        plan: plan.hash.to_string(),
        object: manifest.object.clone(),
        snapshot: manifest.snapshot.clone(),
        target: manifest.target.clone(),
        executor: ExecutorInfo {
            id: EXECUTOR_ID.to_owned(),
            version: EXECUTOR_VERSION,
            isolation: "none: a host process with a scrubbed environment (development mode)"
                .to_owned(),
        },
        environment: manifest.environment.clone(),
        host_dependent: true,
        host: HostInfo {
            system: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.replace('_', "-"),
            tools: host_tools,
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
        return Err(format!(
            "{} is a symbolic link; outputs must be files or directories",
            path.display()
        ));
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
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "{} is a symbolic link; outputs must be files or directories",
                child_path.display()
            ));
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
    Ok(ArtifactEntry {
        path: artifact_path.to_owned(),
        blob: blob.to_string(),
        executable,
    })
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
    Environment(String),
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
            Self::UnsafeArtifactPath { artifact, path } => {
                write!(f, "artifact {artifact} has an unsafe path {path:?}")
            }
            Self::Environment(message) => write!(f, "building the environment: {message}"),
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
            | Self::Environment(_) => None,
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests;
