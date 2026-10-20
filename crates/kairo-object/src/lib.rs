//! Object manifest and metadata types.

use std::error::Error;
use std::fmt;

use kairo_core::canonical::{
    encode_list, encode_option, encode_str, encode_u32, encode_u8, CanonicalEncode,
};
use kairo_core::{BlobId, ObjectId, SnapshotId, StatementId};
use kairo_statement::{ObjectGenesisStatement, ObjectRevisionBody, RevisionId, SignedStatement};
use serde::Deserialize;

/// Domain separator for `BlobId`s derived from canonical
/// `ObjectManifest` bytes. Exposed so other crates (e.g.
/// `kairo-bundle`) can re-derive a manifest blob's id during fixity
/// checks. Canonical ObjectManifest v1 encoding is documented at
/// `schemas/canonical/object-manifest-v1.md`.
pub const OBJECT_MANIFEST_DOMAIN: &[u8] = b"kairo.object.manifest.v1";

/// Canonical Snapshot v1 encoding is documented at
/// `schemas/canonical/snapshot-v1.md`.
const SNAPSHOT_DOMAIN: &[u8] = b"kairo.snapshot.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectManifest {
    kairo: KairoManifestSection,
    content: Option<ContentSection>,
    provides: Vec<ProvideDeclaration>,
    dependencies: Vec<DependencyDeclaration>,
    build_targets: Vec<BuildTarget>,
}

impl ObjectManifest {
    pub fn parse_toml(input: &str) -> Result<Self, ManifestError> {
        let raw: RawObjectManifest = toml::from_str(input).map_err(ManifestError::Toml)?;
        raw.validate()
    }

    pub fn kairo(&self) -> &KairoManifestSection {
        &self.kairo
    }

    pub fn content(&self) -> Option<&ContentSection> {
        self.content.as_ref()
    }

    pub fn provides(&self) -> &[ProvideDeclaration] {
        &self.provides
    }

    pub fn dependencies(&self) -> &[DependencyDeclaration] {
        &self.dependencies
    }

    /// The `[[build.targets]]` (`specs/DECISIONS.md` §33 sub-decision 1).
    pub fn build_targets(&self) -> &[BuildTarget] {
        &self.build_targets
    }

    pub fn build_target(&self, name: &str) -> Option<&BuildTarget> {
        self.build_targets.iter().find(|target| target.name == name)
    }

    pub fn manifest_hash(&self) -> BlobId {
        BlobId::from_bytes(OBJECT_MANIFEST_DOMAIN, &self.canonical_bytes())
    }
}

pub fn validate_revision_manifest(
    revision: &ObjectRevisionBody,
    manifest: &ObjectManifest,
) -> Result<(), RevisionManifestError> {
    let actual_manifest_hash = manifest.manifest_hash();
    if revision.manifest_hash() != &actual_manifest_hash {
        return Err(RevisionManifestError::ManifestHashMismatch {
            expected: revision.manifest_hash().clone(),
            actual: actual_manifest_hash,
        });
    }

    if let Some(declared_object) = manifest.kairo().object() {
        if revision.object() != declared_object {
            return Err(RevisionManifestError::DeclaredObjectMismatch {
                expected: revision.object().clone(),
                actual: declared_object.clone(),
            });
        }
    }

    Ok(())
}

/// Structured outcome of validating a signed `ObjectRevision` against any
/// companion data that has already been fetched.
///
/// Each dimension is reported independently — a manifest mismatch does not
/// invalidate the object-id check, etc. Companion data that the caller did
/// not (or could not) supply is reported as `*NotProvided` /
/// `Indeterminate`, which callers should treat as "not yet evaluated."
///
/// This validator is pure: it does no I/O. The caller (today the CLI;
/// the `kairo verify object` command and any future verifier API)
/// decides how to resolve the `ObjectGenesis`, read the manifest, and
/// look up the storage commit in a Git repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectRevisionValidationReport {
    pub statement_id: StatementId,
    pub object_consistency: ObjectConsistencyCheck,
    pub manifest_binding: ManifestBindingCheck,
    pub parents: ParentReferenceCheck,
    pub content: ContentLayerCheck,
}

impl ObjectRevisionValidationReport {
    /// True when both checks the statement layer can answer succeeded:
    /// object id matches the resolved genesis, and the manifest hash binds
    /// to the parsed manifest. Returns false when either check is
    /// indeterminate; callers that want to treat indeterminate as "ok unless
    /// proven otherwise" should inspect the fields directly.
    pub fn is_statement_layer_consistent(&self) -> bool {
        matches!(self.object_consistency, ObjectConsistencyCheck::Consistent)
            && matches!(self.manifest_binding, ManifestBindingCheck::Bound)
    }
}

/// Whether the revision's `object` field matches the supplied `ObjectGenesis`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectConsistencyCheck {
    /// Genesis derives the same `ObjectId` as the revision binds to.
    Consistent,
    /// Genesis derives a different `ObjectId` than the revision claims.
    /// Indicates either a wrong genesis was supplied or one of the records
    /// is corrupt.
    Mismatch {
        expected: ObjectId,
        actual: ObjectId,
    },
    /// No genesis was supplied; consistency cannot be evaluated.
    GenesisNotProvided,
}

/// Whether the revision's `manifest_hash` matches the supplied manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestBindingCheck {
    /// `manifest_hash` matches and any declared `[kairo].object` agrees with
    /// the revision's object.
    Bound,
    /// Manifest's canonical hash differs from `revision.manifest_hash`.
    HashMismatch { expected: BlobId, actual: BlobId },
    /// Manifest's `[kairo].object` declares a different object than the
    /// revision binds to. (The hash matched.)
    DeclaredObjectMismatch {
        expected: ObjectId,
        actual: ObjectId,
    },
    /// No manifest was supplied; binding cannot be evaluated.
    ManifestNotProvided,
}

/// Statement-layer report on declared parent revisions.
///
/// At the statement layer there is nothing to verify about parents beyond
/// their presence. Confirming that each parent revision actually exists
/// requires the content layer (git) and is part of TODO §11.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParentReferenceCheck {
    /// Revision declares no parents — an initial revision.
    NoParents,
    /// Revision declares one or more parents. Their existence is not
    /// proven at the statement layer.
    Declared { count: usize },
}

/// Content-layer (Git) check.
///
/// Populated by `validate_object_revision` when the caller supplies a
/// `CommitLookup` from a Git repository (TODO §11). When no lookup is
/// provided, or when the revision's storage form is not a well-formed
/// `git:<format>:` reference, the check stays `Indeterminate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentLayerCheck {
    /// Commit was found and its parents agree with the parents
    /// declared on the `ObjectRevision` statement (set-equality —
    /// parent ordering is not enforced).
    Verified,
    /// Commit was found but its parents disagree with the declared
    /// parents. Lists are normalized to git oids (without the
    /// `git:<format>:` prefix).
    ParentMismatch {
        expected: Vec<String>,
        actual: Vec<String>,
    },
    /// Commit was not present in the supplied repository.
    CommitNotFound,
    /// No repository lookup was supplied (e.g. caller did not provide
    /// a Git repo, or the revision's storage scheme is not Git).
    Indeterminate,
}

/// Result of looking up the revision's storage commit in a Git
/// repository. Constructed by the caller (typically the CLI via
/// `kairo-git`) and passed into `validate_object_revision`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitLookup {
    /// Commit exists; here are its parent oids as plain hex strings
    /// (without the `git:<format>:` prefix).
    Found { parent_oids: Vec<String> },
    /// Commit was not present in the repository.
    NotFound,
}

/// Validate a signed `ObjectRevision` statement against optional companion
/// data and return a structured report.
///
/// Pass `Some(genesis)` when the caller has resolved the `ObjectGenesis` for
/// `revision.object()`; otherwise pass `None`. Likewise for the manifest.
/// `commit_lookup` carries the result of asking a Git repository about the
/// revision's storage commit; when `None`, the content layer stays
/// `Indeterminate`. The function never fails: missing inputs are reported
/// as `*NotProvided` / `Indeterminate`.
pub fn validate_object_revision(
    statement: &SignedStatement<ObjectRevisionBody>,
    object_genesis: Option<&ObjectGenesisStatement>,
    manifest: Option<&ObjectManifest>,
    commit_lookup: Option<&CommitLookup>,
) -> ObjectRevisionValidationReport {
    let revision = statement.unsigned().body();
    let statement_id = statement.statement_id();

    let object_consistency = match object_genesis {
        Some(genesis) => {
            let derived = genesis.object_id();
            if &derived == revision.object() {
                ObjectConsistencyCheck::Consistent
            } else {
                ObjectConsistencyCheck::Mismatch {
                    expected: revision.object().clone(),
                    actual: derived,
                }
            }
        }
        None => ObjectConsistencyCheck::GenesisNotProvided,
    };

    let manifest_binding = match manifest {
        Some(manifest) => match validate_revision_manifest(revision, manifest) {
            Ok(()) => ManifestBindingCheck::Bound,
            Err(RevisionManifestError::ManifestHashMismatch { expected, actual }) => {
                ManifestBindingCheck::HashMismatch { expected, actual }
            }
            Err(RevisionManifestError::DeclaredObjectMismatch { expected, actual }) => {
                ManifestBindingCheck::DeclaredObjectMismatch { expected, actual }
            }
        },
        None => ManifestBindingCheck::ManifestNotProvided,
    };

    let parents = if revision.parents().is_empty() {
        ParentReferenceCheck::NoParents
    } else {
        ParentReferenceCheck::Declared {
            count: revision.parents().len(),
        }
    };

    let content = evaluate_content_layer(revision, commit_lookup);

    ObjectRevisionValidationReport {
        statement_id,
        object_consistency,
        manifest_binding,
        parents,
        content,
    }
}

fn evaluate_content_layer(
    revision: &ObjectRevisionBody,
    commit_lookup: Option<&CommitLookup>,
) -> ContentLayerCheck {
    let lookup = match commit_lookup {
        Some(lookup) => lookup,
        None => return ContentLayerCheck::Indeterminate,
    };
    let actual_parents = match lookup {
        CommitLookup::Found { parent_oids } => parent_oids,
        CommitLookup::NotFound => return ContentLayerCheck::CommitNotFound,
    };

    // Declared parents as Git object ids in the revision's format.
    // A parent that isn't a well-formed `git:` revision in that format
    // can't be compared against Git — treat the layer as Indeterminate
    // so we don't false-positive (`specs/DECISIONS.md` §19).
    let Ok(Some(own)) = revision.revision().git() else {
        return ContentLayerCheck::Indeterminate;
    };
    let mut expected = Vec::with_capacity(revision.parents().len());
    for parent in revision.parents() {
        match parent.git() {
            Ok(Some(parent)) if parent.format() == own.format() => {
                expected.push(parent.oid().to_owned());
            }
            _ => return ContentLayerCheck::Indeterminate,
        }
    }

    let mut expected_sorted = expected.clone();
    expected_sorted.sort();
    let mut actual_sorted = actual_parents.clone();
    actual_sorted.sort();

    if expected_sorted == actual_sorted {
        ContentLayerCheck::Verified
    } else {
        ContentLayerCheck::ParentMismatch {
            expected,
            actual: actual_parents.clone(),
        }
    }
}

/// A deterministic, content-addressed picture of an object's effective
/// state at a chosen statement frontier.
///
/// In the MVP the only contributing statement type is `ObjectRevision`, so
/// the frontier is a single `StatementId` and the effective state is the
/// `(revision, manifest_hash)` carried by that statement. As more statement
/// types land (Builds, Provides, Observations, ...) they join the frontier
/// alongside, and the canonical encoding extends without breaking this
/// shape.
///
/// Identity inputs (anything in here changes the `SnapshotId`):
///
/// - `object` — the lineage this snapshot pictures.
/// - `frontier` — sorted `StatementId`s that contribute. Sorting makes the
///   id independent of the order the caller gathered them in.
/// - `revision` — the storage `RevisionId` derived from the frontier.
/// - `manifest_hash` — the canonical manifest hash derived from the
///   frontier.
///
/// Excluded by design (per `OBJECT.md` §2.3):
///
/// - build artifacts
/// - federation metadata
/// - availability or trust information
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    object: ObjectId,
    frontier: Vec<StatementId>,
    revision: RevisionId,
    manifest_hash: BlobId,
}

impl Snapshot {
    /// Build a snapshot whose frontier is a single `ObjectRevision`
    /// statement. The revision must bind to the supplied object id; if it
    /// does not, returns `SnapshotError::ObjectMismatch` rather than
    /// silently snapshotting the wrong lineage.
    pub fn from_object_revision(
        object: &ObjectId,
        revision_statement: &SignedStatement<ObjectRevisionBody>,
    ) -> Result<Self, SnapshotError> {
        let body = revision_statement.unsigned().body();
        if body.object() != object {
            return Err(SnapshotError::ObjectMismatch {
                requested: object.clone(),
                revision_object: body.object().clone(),
            });
        }
        Ok(Self {
            object: object.clone(),
            frontier: vec![revision_statement.statement_id()],
            revision: body.revision().clone(),
            manifest_hash: body.manifest_hash().clone(),
        })
    }

    pub fn object(&self) -> &ObjectId {
        &self.object
    }

    pub fn frontier(&self) -> &[StatementId] {
        &self.frontier
    }

    pub fn revision(&self) -> &RevisionId {
        &self.revision
    }

    pub fn manifest_hash(&self) -> &BlobId {
        &self.manifest_hash
    }

    /// `SnapshotId = sha256(domain || canonical_bytes)`, base58btc
    /// multihash. Two snapshots with identical canonical bytes derive the
    /// same id; two with any difference derive different ids.
    pub fn snapshot_id(&self) -> SnapshotId {
        SnapshotId::from_bytes(SNAPSHOT_DOMAIN, &self.canonical_bytes())
    }
}

impl CanonicalEncode for Snapshot {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_str(out, "Snapshot");
        encode_u8(out, 1);
        encode_str(out, self.object.as_str());

        // Sort the frontier so the snapshot id is independent of how the
        // caller assembled it. The MVP only ever has one entry, but sorting
        // future-proofs.
        let mut sorted: Vec<&StatementId> = self.frontier.iter().collect();
        sorted.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        encode_list(out, &sorted, |out, statement_id| {
            encode_str(out, statement_id.as_str());
        });

        encode_str(out, self.revision.as_str());
        encode_str(out, self.manifest_hash.as_str());
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotError {
    /// The supplied revision binds to a different object than the snapshot
    /// is being computed for.
    ObjectMismatch {
        requested: ObjectId,
        revision_object: ObjectId,
    },
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ObjectMismatch {
                requested,
                revision_object,
            } => write!(
                f,
                "snapshot requested for object {requested} but revision binds to {revision_object}"
            ),
        }
    }
}

impl Error for SnapshotError {}

impl CanonicalEncode for ObjectManifest {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_str(out, "ObjectManifest");
        encode_u8(out, 1);
        self.kairo.encode_canonical(out);
        encode_option(out, self.content.as_ref(), |out, content| {
            content.encode_canonical(out);
        });
        encode_list(out, &self.provides, |out, provides| {
            provides.encode_canonical(out);
        });
        encode_list(out, &self.dependencies, |out, dependency| {
            dependency.encode_canonical(out);
        });
        // Present only when there are targets, so a manifest without
        // them hashes as it did before build targets existed.
        if !self.build_targets.is_empty() {
            encode_str(out, "build");
            encode_list(out, &self.build_targets, |out, target| {
                target.encode_canonical(out);
            });
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KairoManifestSection {
    schema: u32,
    object: Option<ObjectId>,
    kind: String,
    name: String,
    summary: Option<String>,
}

impl KairoManifestSection {
    pub fn schema(&self) -> u32 {
        self.schema
    }

    pub fn object(&self) -> Option<&ObjectId> {
        self.object.as_ref()
    }

    pub fn kind(&self) -> &str {
        &self.kind
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn summary(&self) -> Option<&str> {
        self.summary.as_deref()
    }
}

impl CanonicalEncode for KairoManifestSection {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_u32(out, self.schema);
        encode_option(out, self.object.as_ref(), |out, object| {
            encode_str(out, object.as_str());
        });
        encode_str(out, &self.kind);
        encode_str(out, &self.name);
        encode_option(out, self.summary.as_ref(), |out, summary| {
            encode_str(out, summary);
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentSection {
    kind: String,
}

impl ContentSection {
    pub fn kind(&self) -> &str {
        &self.kind
    }
}

impl CanonicalEncode for ContentSection {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_str(out, &self.kind);
    }
}

/// One `[[provides]]` entry: a provides label this object offers,
/// either through a build target's output (an artifact provider) or
/// as its own tree (a content provider) — `specs/DECISIONS.md` §33
/// sub-decision 2, §34 sub-decision 5.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvideDeclaration {
    provides: String,
    version: Option<String>,
    artifact: Option<ArtifactOutput>,
}

/// Which build output provides a label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactOutput {
    target: String,
    output: String,
}

impl ArtifactOutput {
    pub fn target(&self) -> &str {
        &self.target
    }

    pub fn output(&self) -> &str {
        &self.output
    }
}

impl ProvideDeclaration {
    pub fn provides(&self) -> &str {
        &self.provides
    }

    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    /// The build output that provides the label; `None` means the
    /// object's own tree does.
    pub fn artifact(&self) -> Option<&ArtifactOutput> {
        self.artifact.as_ref()
    }
}

impl CanonicalEncode for ProvideDeclaration {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_str(out, &self.provides);
        encode_option(out, self.version.as_ref(), |out, version| {
            encode_str(out, version);
        });
        // Tagged rather than an option, so a content provider hashes
        // as it did before artifact providers existed.
        if let Some(artifact) = &self.artifact {
            encode_str(out, "artifact");
            encode_str(out, &artifact.target);
            encode_str(out, &artifact.output);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DependencyDeclaration {
    Provides(ProvidesDependency),
    Object(ObjectDependency),
    /// A source object at an exact upstream commit (`DECISIONS.md` §37
    /// sub-decision 6).
    Source(SourceDependency),
}

/// A dependency on a source object, pinned by upstream commit: the
/// same pin on every node, and changed only by editing it. Optionally
/// placed at a path inside the source tree (sub-decision 7), and
/// optionally marked as a seed (sub-decision 8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceDependency {
    url: kairo_statement::SourceUrl,
    revision: kairo_statement::GitRevision,
    path: Option<String>,
    seed: bool,
}

impl SourceDependency {
    /// The source's canonical URL; its object id derives from it.
    pub fn url(&self) -> &kairo_statement::SourceUrl {
        &self.url
    }

    pub fn source(&self) -> ObjectId {
        kairo_statement::SourceGenesis::new(self.url.clone()).object_id()
    }

    pub fn revision(&self) -> &kairo_statement::GitRevision {
        &self.revision
    }

    /// Where in the source tree it is placed, if anywhere.
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    pub fn seed(&self) -> bool {
        self.seed
    }
}

impl CanonicalEncode for SourceDependency {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_str(out, self.url.as_str());
        encode_str(out, &self.revision.to_string());
        encode_option(out, self.path.as_ref(), |out, path| encode_str(out, path));
        encode_u8(out, u8::from(self.seed));
    }
}

impl CanonicalEncode for DependencyDeclaration {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        match self {
            Self::Provides(dependency) => {
                encode_str(out, "provides");
                dependency.encode_canonical(out);
            }
            Self::Object(dependency) => {
                encode_str(out, "object");
                dependency.encode_canonical(out);
            }
            Self::Source(dependency) => {
                encode_str(out, "source");
                dependency.encode_canonical(out);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvidesDependency {
    provides: String,
    version: Option<String>,
}

impl ProvidesDependency {
    pub fn provides(&self) -> &str {
        &self.provides
    }

    /// The version requirement, in Cargo's syntax.
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }
}

impl CanonicalEncode for ProvidesDependency {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_str(out, &self.provides);
        // Tagged rather than an option, so a requirement without a
        // version hashes as it did before versions were kept.
        if let Some(version) = &self.version {
            encode_str(out, "version");
            encode_str(out, version);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectDependency {
    object: ObjectId,
    selector: ObjectDependencySelector,
}

impl ObjectDependency {
    pub fn new(object: ObjectId, selector: ObjectDependencySelector) -> Self {
        Self { object, selector }
    }

    pub fn object(&self) -> &ObjectId {
        &self.object
    }

    pub fn selector(&self) -> &ObjectDependencySelector {
        &self.selector
    }
}

impl CanonicalEncode for ObjectDependency {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_str(out, self.object.as_str());
        self.selector.encode_canonical(out);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectDependencySelector {
    Version(String),
    Snapshot(SnapshotId),
}

impl CanonicalEncode for ObjectDependencySelector {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        match self {
            Self::Version(version) => {
                encode_str(out, "version");
                encode_str(out, version);
            }
            Self::Snapshot(snapshot) => {
                encode_str(out, "snapshot");
                encode_str(out, snapshot.as_str());
            }
        }
    }
}

/// One `[[build.targets]]` entry (`specs/BUILD.md` §2,
/// `specs/DECISIONS.md` §33 sub-decision 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildTarget {
    name: String,
    command: Vec<String>,
    workdir: String,
    dependencies: Vec<DependencyDeclaration>,
    outputs: Vec<BuildOutput>,
}

impl BuildTarget {
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The program and its arguments, run without a shell.
    pub fn command(&self) -> &[String] {
        &self.command
    }

    /// Relative to the tree root; `"."` when not given.
    pub fn workdir(&self) -> &str {
        &self.workdir
    }

    /// The target's own requirements, beyond the manifest's top-level
    /// `[[dependencies]]`.
    pub fn dependencies(&self) -> &[DependencyDeclaration] {
        &self.dependencies
    }

    pub fn outputs(&self) -> &[BuildOutput] {
        &self.outputs
    }
}

impl CanonicalEncode for BuildTarget {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_str(out, &self.name);
        encode_list(out, &self.command, |out, argument| {
            encode_str(out, argument);
        });
        encode_str(out, &self.workdir);
        encode_list(out, &self.dependencies, |out, dependency| {
            dependency.encode_canonical(out);
        });
        encode_list(out, &self.outputs, |out, output| {
            output.encode_canonical(out);
        });
    }
}

/// A declared output: `path` in the build workspace, collected to
/// `artifact_path` in the artifact (`specs/BUILD.md` §5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildOutput {
    name: String,
    path: String,
    artifact_path: String,
    kind: OutputKind,
}

impl BuildOutput {
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Relative to the target's workdir.
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn artifact_path(&self) -> &str {
        &self.artifact_path
    }

    pub fn kind(&self) -> OutputKind {
        self.kind
    }
}

impl CanonicalEncode for BuildOutput {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_str(out, &self.name);
        encode_str(out, &self.path);
        encode_str(out, &self.artifact_path);
        encode_str(out, self.kind.as_str());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputKind {
    File,
    Directory,
}

impl OutputKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Directory => "directory",
        }
    }
}

#[derive(Debug)]
pub enum ManifestError {
    Toml(toml::de::Error),
    InvalidObjectId(kairo_core::IdError),
    InvalidSnapshotId(kairo_core::IdError),
    UnsupportedSchema(u32),
    EmptyField(&'static str),
    UnknownDependencyKind(String),
    MissingObjectDependencyObject,
    MissingObjectDependencySelector,
    ConflictingObjectDependencySelectors,
    MissingProvidesDependency,
    /// A version requirement that isn't in Cargo's syntax.
    InvalidVersionRequirement {
        field: &'static str,
        version: String,
    },
    InvalidTargetName(String),
    DuplicateTarget(String),
    EmptyCommand(String),
    /// A workdir, output path, or artifact path that isn't a plain
    /// relative path inside the tree.
    UnsafePath {
        field: &'static str,
        path: String,
    },
    NoOutputs(String),
    DuplicateOutput {
        target: String,
        name: String,
    },
    DuplicateArtifactPath {
        target: String,
        path: String,
    },
    UnknownOutputKind(String),
    /// A `[[provides]]` entry named a target or output without the
    /// other.
    PartialArtifactProvider(String),
    /// A `[[provides]]` entry named a target or output the manifest
    /// doesn't declare.
    UnknownArtifactProvider {
        provides: String,
        target: String,
        output: String,
    },
    /// `source`, `revision`, `path`, or `seed` on a `provides` or
    /// `object` dependency.
    SourceFieldsOnOtherKind(String),
    InvalidSourceDependency(String),
    /// Two dependencies of one target placed at the same path, or one
    /// inside another.
    PlacementConflict {
        target: String,
        path: String,
    },
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Toml(error) => write!(f, "invalid kairo.toml: {error}"),
            Self::InvalidObjectId(error) => write!(f, "invalid object id: {error}"),
            Self::InvalidSnapshotId(error) => write!(f, "invalid snapshot id: {error}"),
            Self::UnsupportedSchema(schema) => write!(f, "unsupported kairo.toml schema {schema}"),
            Self::EmptyField(field) => write!(f, "empty manifest field {field}"),
            Self::UnknownDependencyKind(kind) => write!(f, "unknown dependency kind {kind}"),
            Self::MissingObjectDependencyObject => {
                f.write_str("object dependency is missing object")
            }
            Self::MissingObjectDependencySelector => {
                f.write_str("object dependency must specify version or snapshot")
            }
            Self::ConflictingObjectDependencySelectors => {
                f.write_str("object dependency cannot specify both version and snapshot")
            }
            Self::MissingProvidesDependency => {
                f.write_str("provides dependency is missing provides token")
            }
            Self::InvalidVersionRequirement { field, version } => write!(
                f,
                "{field} {version:?} is not a version requirement (Cargo syntax, e.g. \">=4, <5\")"
            ),
            Self::InvalidTargetName(name) => {
                write!(f, "build target name {name:?} must match [a-z][a-z0-9-]*")
            }
            Self::DuplicateTarget(name) => write!(f, "build target {name} is declared twice"),
            Self::EmptyCommand(target) => {
                write!(f, "build target {target} has an empty command")
            }
            Self::UnsafePath { field, path } => write!(
                f,
                "{field} {path:?} must be a relative path without '..' or empty components"
            ),
            Self::NoOutputs(target) => write!(f, "build target {target} declares no outputs"),
            Self::DuplicateOutput { target, name } => {
                write!(f, "build target {target} declares output {name} twice")
            }
            Self::DuplicateArtifactPath { target, path } => {
                write!(f, "build target {target} maps two outputs to {path}")
            }
            Self::UnknownOutputKind(kind) => {
                write!(f, "unknown output kind {kind} (expected file or directory)")
            }
            Self::PartialArtifactProvider(provides) => write!(
                f,
                "provides {provides} must name both target and output, or neither"
            ),
            Self::SourceFieldsOnOtherKind(kind) => write!(
                f,
                "a {kind} dependency can't take source, revision, path, or seed (those are for kind = \"source\")"
            ),
            Self::InvalidSourceDependency(message) => write!(f, "source dependency {message}"),
            Self::PlacementConflict { target, path } => write!(
                f,
                "build target {target} places two dependencies at or inside {path}"
            ),
            Self::UnknownArtifactProvider {
                provides,
                target,
                output,
            } => write!(
                f,
                "provides {provides} names output {output} of build target {target}, which the manifest doesn't declare"
            ),
        }
    }
}

impl Error for ManifestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Toml(error) => Some(error),
            Self::InvalidObjectId(error) | Self::InvalidSnapshotId(error) => Some(error),
            Self::UnsupportedSchema(_)
            | Self::EmptyField(_)
            | Self::UnknownDependencyKind(_)
            | Self::MissingObjectDependencyObject
            | Self::MissingObjectDependencySelector
            | Self::ConflictingObjectDependencySelectors
            | Self::MissingProvidesDependency
            | Self::InvalidVersionRequirement { .. }
            | Self::InvalidTargetName(_)
            | Self::DuplicateTarget(_)
            | Self::EmptyCommand(_)
            | Self::UnsafePath { .. }
            | Self::NoOutputs(_)
            | Self::DuplicateOutput { .. }
            | Self::DuplicateArtifactPath { .. }
            | Self::UnknownOutputKind(_)
            | Self::PartialArtifactProvider(_)
            | Self::UnknownArtifactProvider { .. }
            | Self::SourceFieldsOnOtherKind(_)
            | Self::InvalidSourceDependency(_)
            | Self::PlacementConflict { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevisionManifestError {
    ManifestHashMismatch {
        expected: BlobId,
        actual: BlobId,
    },
    DeclaredObjectMismatch {
        expected: ObjectId,
        actual: ObjectId,
    },
}

impl fmt::Display for RevisionManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ManifestHashMismatch { expected, actual } => {
                write!(f, "revision manifest hash {expected} does not match parsed manifest hash {actual}")
            }
            Self::DeclaredObjectMismatch { expected, actual } => {
                write!(
                    f,
                    "revision object {expected} does not match manifest-declared object {actual}"
                )
            }
        }
    }
}

impl Error for RevisionManifestError {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawObjectManifest {
    kairo: RawKairoSection,
    content: Option<RawContentSection>,
    #[serde(default)]
    provides: Vec<RawProvideDeclaration>,
    #[serde(default)]
    dependencies: Vec<RawDependencyDeclaration>,
    build: Option<RawBuildSection>,
}

impl RawObjectManifest {
    fn validate(self) -> Result<ObjectManifest, ManifestError> {
        let build_targets = self
            .build
            .map(RawBuildSection::validate)
            .transpose()?
            .unwrap_or_default();
        let provides = self
            .provides
            .into_iter()
            .map(RawProvideDeclaration::validate)
            .collect::<Result<Vec<_>, _>>()?;
        for declaration in &provides {
            if let Some(artifact) = &declaration.artifact {
                let declared = build_targets
                    .iter()
                    .find(|target| target.name == artifact.target)
                    .is_some_and(|target| {
                        target
                            .outputs
                            .iter()
                            .any(|output| output.name == artifact.output)
                    });
                if !declared {
                    return Err(ManifestError::UnknownArtifactProvider {
                        provides: declaration.provides.clone(),
                        target: artifact.target.clone(),
                        output: artifact.output.clone(),
                    });
                }
            }
        }
        let dependencies = self
            .dependencies
            .into_iter()
            .map(RawDependencyDeclaration::validate)
            .collect::<Result<Vec<_>, _>>()?;
        // A target's placed dependencies (its own and the manifest's)
        // must not overlap.
        for target in &build_targets {
            let mut placed: Vec<&str> = Vec::new();
            for dependency in dependencies.iter().chain(&target.dependencies) {
                let DependencyDeclaration::Source(source) = dependency else {
                    continue;
                };
                let Some(path) = source.path() else { continue };
                let overlaps = placed.iter().any(|other| {
                    other == &path
                        || path.starts_with(&format!("{other}/"))
                        || other.starts_with(&format!("{path}/"))
                });
                if overlaps {
                    return Err(ManifestError::PlacementConflict {
                        target: target.name.clone(),
                        path: path.to_owned(),
                    });
                }
                placed.push(path);
            }
        }
        Ok(ObjectManifest {
            kairo: self.kairo.validate()?,
            content: self.content.map(RawContentSection::validate).transpose()?,
            provides,
            dependencies,
            build_targets,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBuildSection {
    #[serde(default)]
    targets: Vec<RawBuildTarget>,
}

impl RawBuildSection {
    fn validate(self) -> Result<Vec<BuildTarget>, ManifestError> {
        let mut targets: Vec<BuildTarget> = Vec::with_capacity(self.targets.len());
        for raw in self.targets {
            let target = raw.validate()?;
            if targets.iter().any(|other| other.name == target.name) {
                return Err(ManifestError::DuplicateTarget(target.name));
            }
            targets.push(target);
        }
        Ok(targets)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBuildTarget {
    name: String,
    command: Vec<String>,
    workdir: Option<String>,
    #[serde(default)]
    dependencies: Vec<RawDependencyDeclaration>,
    #[serde(default)]
    outputs: Vec<RawBuildOutput>,
}

impl RawBuildTarget {
    fn validate(self) -> Result<BuildTarget, ManifestError> {
        if !is_target_name(&self.name) {
            return Err(ManifestError::InvalidTargetName(self.name));
        }
        if self.command.first().is_none_or(String::is_empty) {
            return Err(ManifestError::EmptyCommand(self.name));
        }
        let workdir = match self.workdir {
            None => ".".to_owned(),
            Some(workdir) if workdir == "." => workdir,
            Some(workdir) => ensure_relative_path("build.targets.workdir", workdir)?,
        };
        let dependencies = self
            .dependencies
            .into_iter()
            .map(RawDependencyDeclaration::validate)
            .collect::<Result<Vec<_>, _>>()?;
        if self.outputs.is_empty() {
            return Err(ManifestError::NoOutputs(self.name));
        }
        let mut outputs: Vec<BuildOutput> = Vec::with_capacity(self.outputs.len());
        for raw in self.outputs {
            let output = raw.validate()?;
            if outputs.iter().any(|other| other.name == output.name) {
                return Err(ManifestError::DuplicateOutput {
                    target: self.name,
                    name: output.name,
                });
            }
            if outputs
                .iter()
                .any(|other| other.artifact_path == output.artifact_path)
            {
                return Err(ManifestError::DuplicateArtifactPath {
                    target: self.name,
                    path: output.artifact_path,
                });
            }
            outputs.push(output);
        }
        Ok(BuildTarget {
            name: self.name,
            command: self.command,
            workdir,
            dependencies,
            outputs,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBuildOutput {
    name: String,
    path: String,
    artifact_path: String,
    kind: String,
}

impl RawBuildOutput {
    fn validate(self) -> Result<BuildOutput, ManifestError> {
        ensure_non_empty("build.targets.outputs.name", &self.name)?;
        let kind = match self.kind.as_str() {
            "file" => OutputKind::File,
            "directory" => OutputKind::Directory,
            _ => return Err(ManifestError::UnknownOutputKind(self.kind)),
        };
        Ok(BuildOutput {
            name: self.name,
            path: ensure_relative_path("build.targets.outputs.path", self.path)?,
            artifact_path: ensure_relative_path(
                "build.targets.outputs.artifact_path",
                self.artifact_path,
            )?,
            kind,
        })
    }
}

/// `[a-z][a-z0-9-]*`.
fn is_target_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|first| first.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// A `/`-separated path of plain names: not absolute, no `.` or `..`,
/// no empty components, no backslashes.
fn ensure_relative_path(field: &'static str, path: String) -> Result<String, ManifestError> {
    let plain = !path.is_empty()
        && !path.contains('\\')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..");
    if plain {
        Ok(path)
    } else {
        Err(ManifestError::UnsafePath { field, path })
    }
}

fn ensure_version_requirement(field: &'static str, version: &str) -> Result<(), ManifestError> {
    ensure_non_empty(field, version)?;
    semver::VersionReq::parse(version).map(drop).map_err(|_| {
        ManifestError::InvalidVersionRequirement {
            field,
            version: version.to_owned(),
        }
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawKairoSection {
    schema: u32,
    object: Option<String>,
    kind: String,
    name: String,
    summary: Option<String>,
}

impl RawKairoSection {
    fn validate(self) -> Result<KairoManifestSection, ManifestError> {
        if self.schema != 1 {
            return Err(ManifestError::UnsupportedSchema(self.schema));
        }

        ensure_non_empty("kairo.kind", &self.kind)?;
        ensure_non_empty("kairo.name", &self.name)?;

        Ok(KairoManifestSection {
            schema: self.schema,
            object: self
                .object
                .map(ObjectId::new)
                .transpose()
                .map_err(ManifestError::InvalidObjectId)?,
            kind: self.kind,
            name: self.name,
            summary: self.summary,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawContentSection {
    kind: String,
}

impl RawContentSection {
    fn validate(self) -> Result<ContentSection, ManifestError> {
        ensure_non_empty("content.kind", &self.kind)?;

        Ok(ContentSection { kind: self.kind })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProvideDeclaration {
    provides: String,
    version: Option<String>,
    target: Option<String>,
    output: Option<String>,
}

impl RawProvideDeclaration {
    fn validate(self) -> Result<ProvideDeclaration, ManifestError> {
        ensure_non_empty("provides.provides", &self.provides)?;
        let artifact = match (self.target, self.output) {
            (None, None) => None,
            (Some(target), Some(output)) => Some(ArtifactOutput { target, output }),
            _ => return Err(ManifestError::PartialArtifactProvider(self.provides)),
        };

        Ok(ProvideDeclaration {
            provides: self.provides,
            version: self.version,
            artifact,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDependencyDeclaration {
    kind: String,
    provides: Option<String>,
    object: Option<String>,
    version: Option<String>,
    snapshot: Option<String>,
    source: Option<String>,
    revision: Option<String>,
    path: Option<String>,
    seed: Option<bool>,
}

impl RawDependencyDeclaration {
    fn validate(self) -> Result<DependencyDeclaration, ManifestError> {
        let source_fields = self.source.is_some()
            || self.revision.is_some()
            || self.path.is_some()
            || self.seed.is_some();
        match self.kind.as_str() {
            "provides" | "object" if source_fields => {
                Err(ManifestError::SourceFieldsOnOtherKind(self.kind.clone()))
            }
            "provides" => self.validate_provides(),
            "object" => self.validate_object(),
            "source" => self.validate_source(),
            kind => Err(ManifestError::UnknownDependencyKind(kind.to_owned())),
        }
    }

    fn validate_source(self) -> Result<DependencyDeclaration, ManifestError> {
        if self.provides.is_some()
            || self.object.is_some()
            || self.version.is_some()
            || self.snapshot.is_some()
        {
            return Err(ManifestError::InvalidSourceDependency(
                "takes source, revision, path, and seed only; sources are pinned by revision, \
                 never by version"
                    .to_owned(),
            ));
        }
        let url = self.source.ok_or_else(|| {
            ManifestError::InvalidSourceDependency("needs source, the repository URL".to_owned())
        })?;
        let url = kairo_statement::SourceUrl::parse(&url)
            .map_err(|error| ManifestError::InvalidSourceDependency(error.to_string()))?;
        let revision = self.revision.ok_or_else(|| {
            ManifestError::InvalidSourceDependency(
                "needs revision, the upstream commit (git:sha1:<oid> or git:sha256:<oid>)"
                    .to_owned(),
            )
        })?;
        let revision = kairo_statement::GitRevision::parse(&revision)
            .map_err(|error| ManifestError::InvalidSourceDependency(error.to_string()))?
            .ok_or_else(|| {
                ManifestError::InvalidSourceDependency(format!(
                    "revision {revision:?} is not a git: revision"
                ))
            })?;
        let path = self
            .path
            .map(|path| ensure_relative_path("dependencies.path", path))
            .transpose()?;
        Ok(DependencyDeclaration::Source(SourceDependency {
            url,
            revision,
            path,
            seed: self.seed.unwrap_or(false),
        }))
    }

    fn validate_provides(self) -> Result<DependencyDeclaration, ManifestError> {
        let Some(provides) = self.provides else {
            return Err(ManifestError::MissingProvidesDependency);
        };

        ensure_non_empty("dependencies.provides", &provides)?;
        if let Some(version) = &self.version {
            ensure_version_requirement("dependencies.version", version)?;
        }

        Ok(DependencyDeclaration::Provides(ProvidesDependency {
            provides,
            version: self.version,
        }))
    }

    fn validate_object(self) -> Result<DependencyDeclaration, ManifestError> {
        let Some(object) = self.object else {
            return Err(ManifestError::MissingObjectDependencyObject);
        };

        let object = ObjectId::new(object).map_err(ManifestError::InvalidObjectId)?;
        let selector = match (self.version, self.snapshot) {
            (Some(version), None) => {
                ensure_version_requirement("dependencies.version", &version)?;
                ObjectDependencySelector::Version(version)
            }
            (None, Some(snapshot)) => ObjectDependencySelector::Snapshot(
                SnapshotId::new(snapshot).map_err(ManifestError::InvalidSnapshotId)?,
            ),
            (None, None) => return Err(ManifestError::MissingObjectDependencySelector),
            (Some(_), Some(_)) => return Err(ManifestError::ConflictingObjectDependencySelectors),
        };

        Ok(DependencyDeclaration::Object(ObjectDependency::new(
            object, selector,
        )))
    }
}

fn ensure_non_empty(field: &'static str, value: &str) -> Result<(), ManifestError> {
    if value.is_empty() {
        Err(ManifestError::EmptyField(field))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairo_statement::RevisionId;

    const OBJECT_ID: &str = "zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk";
    const SNAPSHOT_ID: &str = "zQmPrz1SBNXD1XAPCaWrTtpBbEZHGevtUoWY8ibihamaApZ";

    #[test]
    fn parses_minimal_manifest() {
        let manifest = ObjectManifest::parse_toml(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "Example"

            [content]
            kind = "tree"
            "#,
        );

        assert!(
            matches!(manifest, Ok(manifest) if manifest.kairo().kind() == "software"
                && manifest.content().map(ContentSection::kind) == Some("tree"))
        );
    }

    #[test]
    fn parses_declared_object_id() {
        let manifest = ObjectManifest::parse_toml(&format!(
            r#"
            [kairo]
            schema = 1
            object = "{OBJECT_ID}"
            kind = "software"
            name = "Example"
            "#
        ));

        assert!(
            matches!(manifest, Ok(manifest) if manifest.kairo().object().map(ObjectId::as_str) == Some(OBJECT_ID))
        );
    }

    #[test]
    fn parses_provides_declaration() {
        let manifest = ObjectManifest::parse_toml(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "Example"

            [[provides]]
            provides = "tool:make"
            version = "3.81"
            "#,
        );

        assert!(
            matches!(manifest, Ok(manifest) if manifest.provides().first().map(ProvideDeclaration::provides) == Some("tool:make"))
        );
    }

    #[test]
    fn parses_provides_dependency() {
        let manifest = ObjectManifest::parse_toml(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "Example"

            [[dependencies]]
            kind = "provides"
            provides = "lib:zlib:static"
            "#,
        );

        assert!(matches!(manifest, Ok(manifest) if matches!(
            manifest.dependencies().first(),
            Some(DependencyDeclaration::Provides(dependency)) if dependency.provides() == "lib:zlib:static"
        )));
    }

    #[test]
    fn parses_object_dependency_with_version() {
        let manifest = ObjectManifest::parse_toml(&format!(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "Example"

            [[dependencies]]
            kind = "object"
            object = "{OBJECT_ID}"
            version = "^4.1.0"
            "#
        ));

        assert!(matches!(manifest, Ok(manifest) if matches!(
            manifest.dependencies().first(),
            Some(DependencyDeclaration::Object(dependency))
                if dependency.object().as_str() == OBJECT_ID
                    && dependency.selector() == &ObjectDependencySelector::Version("^4.1.0".to_owned())
        )));
    }

    #[test]
    fn parses_object_dependency_with_snapshot() {
        let expected_snapshot = SnapshotId::new(SNAPSHOT_ID);
        let manifest = ObjectManifest::parse_toml(&format!(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "Example"

            [[dependencies]]
            kind = "object"
            object = "{OBJECT_ID}"
            snapshot = "{SNAPSHOT_ID}"
            "#
        ));

        assert!(
            matches!((manifest, expected_snapshot), (Ok(manifest), Ok(_expected_snapshot)) if matches!(
                manifest.dependencies().first(),
                Some(DependencyDeclaration::Object(dependency))
                    if dependency.object().as_str() == OBJECT_ID
                        && matches!(dependency.selector(), ObjectDependencySelector::Snapshot(snapshot) if snapshot.as_str() == SNAPSHOT_ID)
            ))
        );
    }

    #[test]
    fn rejects_invalid_object_id() {
        let manifest = ObjectManifest::parse_toml(
            r#"
            [kairo]
            schema = 1
            object = "obj_invalid"
            kind = "software"
            name = "Example"
            "#,
        );

        assert!(matches!(manifest, Err(ManifestError::InvalidObjectId(_))));
    }

    #[test]
    fn rejects_object_dependency_without_selector() {
        let manifest = ObjectManifest::parse_toml(&format!(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "Example"

            [[dependencies]]
            kind = "object"
            object = "{OBJECT_ID}"
            "#
        ));

        assert!(matches!(
            manifest,
            Err(ManifestError::MissingObjectDependencySelector)
        ));
    }

    #[test]
    fn rejects_object_dependency_with_conflicting_selectors() {
        let manifest = ObjectManifest::parse_toml(&format!(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "Example"

            [[dependencies]]
            kind = "object"
            object = "{OBJECT_ID}"
            version = "^4.1.0"
            snapshot = "{SNAPSHOT_ID}"
            "#
        ));

        assert!(matches!(
            manifest,
            Err(ManifestError::ConflictingObjectDependencySelectors)
        ));
    }

    #[test]
    fn rejects_unsupported_schema() {
        let manifest = ObjectManifest::parse_toml(
            r#"
            [kairo]
            schema = 2
            kind = "software"
            name = "Example"
            "#,
        );

        assert!(matches!(manifest, Err(ManifestError::UnsupportedSchema(2))));
    }

    #[test]
    fn toml_key_order_does_not_affect_manifest_hash() {
        let first = ObjectManifest::parse_toml(&format!(
            r#"
            [kairo]
            schema = 1
            object = "{OBJECT_ID}"
            kind = "software"
            name = "Example"
            summary = "Example object."

            [content]
            kind = "tree"

            [[provides]]
            provides = "tool:make"
            version = "3.81"

            [[dependencies]]
            kind = "object"
            object = "{OBJECT_ID}"
            version = "^4.1.0"
            "#
        ));
        let second = ObjectManifest::parse_toml(&format!(
            r#"
            [[dependencies]]
            version = "^4.1.0"
            object = "{OBJECT_ID}"
            kind = "object"

            [[provides]]
            version = "3.81"
            provides = "tool:make"

            [content]
            kind = "tree"

            [kairo]
            summary = "Example object."
            name = "Example"
            kind = "software"
            object = "{OBJECT_ID}"
            schema = 1
            "#
        ));

        assert!(
            matches!((first, second), (Ok(first), Ok(second)) if first.manifest_hash() == second.manifest_hash())
        );
    }

    #[test]
    fn manifest_hash_changes_when_dependency_changes() {
        let first = ObjectManifest::parse_toml(&format!(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "Example"

            [[dependencies]]
            kind = "object"
            object = "{OBJECT_ID}"
            version = "^4.1.0"
            "#
        ));
        let second = ObjectManifest::parse_toml(&format!(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "Example"

            [[dependencies]]
            kind = "object"
            object = "{OBJECT_ID}"
            version = "^4.2.0"
            "#
        ));

        assert!(
            matches!((first, second), (Ok(first), Ok(second)) if first.manifest_hash() != second.manifest_hash())
        );
    }

    #[test]
    fn manifest_hash_is_valid_blob_id() {
        let manifest = ObjectManifest::parse_toml(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "Example"
            "#,
        );

        assert!(
            matches!(manifest.map(|manifest| manifest.manifest_hash()), Ok(hash) if BlobId::new(hash.to_string()) == Ok(hash.clone()))
        );
    }

    #[test]
    fn validates_revision_manifest_hash_and_declared_object(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let manifest = ObjectManifest::parse_toml(&manifest_toml(Some(OBJECT_ID)))?;
        let revision = object_revision(ObjectId::new(OBJECT_ID)?, manifest.manifest_hash());

        assert_eq!(validate_revision_manifest(&revision, &manifest), Ok(()));
        Ok(())
    }

    #[test]
    fn validates_revision_manifest_without_declared_object(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let manifest = ObjectManifest::parse_toml(&manifest_toml(None))?;
        let revision = object_revision(ObjectId::new(OBJECT_ID)?, manifest.manifest_hash());

        assert_eq!(validate_revision_manifest(&revision, &manifest), Ok(()));
        Ok(())
    }

    #[test]
    fn rejects_revision_manifest_hash_mismatch() -> Result<(), Box<dyn std::error::Error>> {
        let manifest = ObjectManifest::parse_toml(&manifest_toml(Some(OBJECT_ID)))?;
        let revision = object_revision(
            ObjectId::new(OBJECT_ID)?,
            BlobId::from_sha256_digest([3; 32]),
        );

        assert!(matches!(
            validate_revision_manifest(&revision, &manifest),
            Err(RevisionManifestError::ManifestHashMismatch { .. })
        ));
        Ok(())
    }

    #[test]
    fn rejects_revision_manifest_declared_object_mismatch() -> Result<(), Box<dyn std::error::Error>>
    {
        let manifest = ObjectManifest::parse_toml(&manifest_toml(Some(OBJECT_ID)))?;
        let revision = object_revision(
            ObjectId::from_sha256_digest([4; 32]),
            manifest.manifest_hash(),
        );

        assert!(matches!(
            validate_revision_manifest(&revision, &manifest),
            Err(RevisionManifestError::DeclaredObjectMismatch { .. })
        ));
        Ok(())
    }

    fn object_revision(object: ObjectId, manifest_hash: BlobId) -> ObjectRevisionBody {
        ObjectRevisionBody::new(
            object,
            RevisionId::new("git:sha1:eae79d6e0cff76fc7a3bb78723690b0b2367f849"),
            vec![RevisionId::new(
                "git:sha1:d8fd39d0bbdd2dcf322d8b11390a4c5825b11495",
            )],
            manifest_hash,
            true,
        )
    }

    fn manifest_toml(object: Option<&str>) -> String {
        let object_line = object
            .map(|object| format!("object = \"{object}\""))
            .unwrap_or_default();

        format!(
            r#"
            [kairo]
            schema = 1
            {object_line}
            kind = "software"
            name = "Example"

            [content]
            kind = "tree"
            "#
        )
    }

    /// Pinned from before build targets and provides-dependency versions
    /// existed: a manifest using neither must keep its hash.
    #[test]
    fn manifest_without_build_targets_keeps_its_hash() -> Result<(), Box<dyn std::error::Error>> {
        let manifest = ObjectManifest::parse_toml(&format!(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "Example"
            [content]
            kind = "tree"
            [[provides]]
            provides = "tool:make"
            version = "3.81"
            [[dependencies]]
            kind = "provides"
            provides = "lib:zlib:static"
            [[dependencies]]
            kind = "object"
            object = "{OBJECT_ID}"
            version = "^4.1.0"
            "#
        ))?;

        assert_eq!(
            manifest.manifest_hash().as_str(),
            "zQmQow72niX2CSUiMkabe6K3SYRxMnubGytuKYd7TVdfsts"
        );
        Ok(())
    }

    const BUILD_MANIFEST: &str = r#"
        [kairo]
        schema = 1
        kind = "software"
        name = "Example"

        [[build.targets]]
        name = "release"
        command = ["make", "release"]

        [[build.targets.dependencies]]
        kind = "provides"
        provides = "tool:make"
        version = ">=4, <5"

        [[build.targets.outputs]]
        name = "cli"
        path = "dist/app"
        artifact_path = "bin/app"
        kind = "file"
        "#;

    #[test]
    fn parses_build_target() -> Result<(), Box<dyn std::error::Error>> {
        let manifest = ObjectManifest::parse_toml(BUILD_MANIFEST)?;

        let target = manifest
            .build_target("release")
            .ok_or("no release target")?;
        assert_eq!(target.command(), ["make", "release"]);
        assert_eq!(target.workdir(), ".");
        assert!(matches!(
            target.dependencies(),
            [DependencyDeclaration::Provides(dependency)]
                if dependency.provides() == "tool:make" && dependency.version() == Some(">=4, <5")
        ));
        assert!(matches!(
            target.outputs(),
            [output] if output.name() == "cli"
                && output.path() == "dist/app"
                && output.artifact_path() == "bin/app"
                && output.kind() == OutputKind::File
        ));
        Ok(())
    }

    #[test]
    fn build_targets_are_pinned_by_the_manifest_hash() -> Result<(), Box<dyn std::error::Error>> {
        let base = ObjectManifest::parse_toml(BUILD_MANIFEST)?.manifest_hash();
        let without = ObjectManifest::parse_toml(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "Example"
            "#,
        )?
        .manifest_hash();
        assert_ne!(base, without);

        for (from, to) in [
            (r#"["make", "release"]"#, r#"["make", "all"]"#),
            (r#"version = ">=4, <5""#, r#"version = ">=4, <6""#),
            (r#"path = "dist/app""#, r#"path = "dist/app2""#),
            (r#"kind = "file""#, r#"kind = "directory""#),
        ] {
            let changed = ObjectManifest::parse_toml(&BUILD_MANIFEST.replace(from, to))?;
            assert_ne!(changed.manifest_hash(), base, "{from} -> {to}");
        }
        Ok(())
    }

    #[test]
    fn provides_dependency_version_is_kept_and_hashed() -> Result<(), Box<dyn std::error::Error>> {
        let manifest = |version: &str| {
            ObjectManifest::parse_toml(&format!(
                r#"
                [kairo]
                schema = 1
                kind = "software"
                name = "Example"

                [[dependencies]]
                kind = "provides"
                provides = "tool:make"
                {version}
                "#
            ))
        };
        let unversioned = manifest("")?;
        let versioned = manifest(r#"version = ">=4""#)?;

        assert!(matches!(
            versioned.dependencies(),
            [DependencyDeclaration::Provides(dependency)] if dependency.version() == Some(">=4")
        ));
        assert_ne!(unversioned.manifest_hash(), versioned.manifest_hash());
        Ok(())
    }

    #[test]
    fn rejects_invalid_version_requirements() {
        for dependency in [
            r#"kind = "provides"
            provides = "tool:make"
            version = "four""#,
            &format!(
                r#"kind = "object"
            object = "{OBJECT_ID}"
            version = ">=4 <5""#
            ),
        ] {
            let manifest = ObjectManifest::parse_toml(&format!(
                r#"
                [kairo]
                schema = 1
                kind = "software"
                name = "Example"

                [[dependencies]]
                {dependency}
                "#
            ));
            assert!(
                matches!(
                    manifest,
                    Err(ManifestError::InvalidVersionRequirement { .. })
                ),
                "{manifest:?}"
            );
        }
    }

    #[test]
    fn rejects_invalid_build_targets() {
        type Expected = fn(&ManifestError) -> bool;
        let cases: &[(&str, &str, Expected)] = &[
            (r#"name = "release""#, r#"name = "Release""#, |e| {
                matches!(e, ManifestError::InvalidTargetName(_))
            }),
            (r#"["make", "release"]"#, "[]", |e| {
                matches!(e, ManifestError::EmptyCommand(_))
            }),
            (r#"["make", "release"]"#, r#"["", "release"]"#, |e| {
                matches!(e, ManifestError::EmptyCommand(_))
            }),
            (r#"path = "dist/app""#, r#"path = "../app""#, |e| {
                matches!(e, ManifestError::UnsafePath { .. })
            }),
            (r#"path = "dist/app""#, r#"path = "/dist/app""#, |e| {
                matches!(e, ManifestError::UnsafePath { .. })
            }),
            (
                r#"artifact_path = "bin/app""#,
                r#"artifact_path = "bin//app""#,
                |e| matches!(e, ManifestError::UnsafePath { .. }),
            ),
            (
                r#"artifact_path = "bin/app""#,
                r#"artifact_path = "./bin/app""#,
                |e| matches!(e, ManifestError::UnsafePath { .. }),
            ),
            (
                r#"command = ["make", "release"]"#,
                r#"command = ["make", "release"]
                workdir = "src/../.."
                "#,
                |e| matches!(e, ManifestError::UnsafePath { .. }),
            ),
            (r#"kind = "file""#, r#"kind = "symlink""#, |e| {
                matches!(e, ManifestError::UnknownOutputKind(_))
            }),
            (
                r#"kind = "file""#,
                r#"kind = "file"
                sha256 = "x""#,
                |e| matches!(e, ManifestError::Toml(_)),
            ),
        ];
        for (from, to, expected) in cases {
            let manifest = ObjectManifest::parse_toml(&BUILD_MANIFEST.replace(from, to));
            assert!(
                matches!(&manifest, Err(error) if expected(error)),
                "{from} -> {to}: {manifest:?}"
            );
        }
    }

    #[test]
    fn rejects_targets_and_outputs_that_collide() {
        let output = r#"
        [[build.targets.outputs]]
        name = "cli"
        path = "dist/other"
        artifact_path = "bin/other"
        kind = "file"
        "#;
        let duplicate_output = format!("{BUILD_MANIFEST}{output}");
        assert!(matches!(
            ObjectManifest::parse_toml(&duplicate_output),
            Err(ManifestError::DuplicateOutput { .. })
        ));

        let same_path = format!(
            "{BUILD_MANIFEST}{}",
            output
                .replace(r#"name = "cli""#, r#"name = "other""#)
                .replace("bin/other", "bin/app")
        );
        assert!(matches!(
            ObjectManifest::parse_toml(&same_path),
            Err(ManifestError::DuplicateArtifactPath { .. })
        ));

        let target = r#"
        [[build.targets]]
        name = "release"
        command = ["true"]
        [[build.targets.outputs]]
        name = "x"
        path = "x"
        artifact_path = "x"
        kind = "file"
        "#;
        assert!(matches!(
            ObjectManifest::parse_toml(&format!("{BUILD_MANIFEST}{target}")),
            Err(ManifestError::DuplicateTarget(name)) if name == "release"
        ));

        let no_outputs = r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "Example"

            [[build.targets]]
            name = "check"
            command = ["make", "check"]
            "#;
        assert!(matches!(
            ObjectManifest::parse_toml(no_outputs),
            Err(ManifestError::NoOutputs(name)) if name == "check"
        ));
    }

    #[test]
    fn parses_an_artifact_provider() -> Result<(), Box<dyn std::error::Error>> {
        let provider = format!(
            r#"{BUILD_MANIFEST}
            [[provides]]
            provides = "tool:app"
            version = "1.2"
            target = "release"
            output = "cli"
            "#
        );
        let manifest = ObjectManifest::parse_toml(&provider)?;
        assert!(matches!(
            manifest.provides(),
            [declaration] if declaration.artifact().map(|a| (a.target(), a.output())) == Some(("release", "cli"))
        ));
        assert_ne!(
            manifest.manifest_hash(),
            ObjectManifest::parse_toml(&provider.replace("output = \"cli\"", "output = \"cli\"\n[[build.targets.outputs]]\nname = \"x\"\npath = \"x\"\nartifact_path = \"x\"\nkind = \"file\""))
                .map(|m| m.manifest_hash())?
        );

        for (from, to, partial) in [
            ("output = \"cli\"", "", true),
            ("target = \"release\"", "", true),
            ("output = \"cli\"", "output = \"other\"", false),
            ("target = \"release\"", "target = \"debug\"", false),
        ] {
            let manifest = ObjectManifest::parse_toml(&provider.replace(from, to));
            let expected = |error: &ManifestError| {
                if partial {
                    matches!(error, ManifestError::PartialArtifactProvider(_))
                } else {
                    matches!(error, ManifestError::UnknownArtifactProvider { .. })
                }
            };
            assert!(
                matches!(&manifest, Err(error) if expected(error)),
                "{from} -> {to:?}: {manifest:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn parses_source_dependencies_pinned_by_revision() -> Result<(), Box<dyn std::error::Error>> {
        let oid = "a".repeat(40);
        let manifest = |extra: &str| {
            format!(
                r#"
                [kairo]
                schema = 1
                kind = "software"
                name = "wrapper"
                [[build.targets]]
                name = "amd64"
                command = ["./run"]
                [[build.targets.dependencies]]
                kind = "source"
                source = "https://GitHub.com/oriansj/M2libc.git"
                revision = "git:sha1:{oid}"
                {extra}
                [[build.targets.outputs]]
                name = "bin"
                path = "out"
                artifact_path = "bin"
                kind = "directory"
                "#
            )
        };
        let parsed = ObjectManifest::parse_toml(&manifest("path = \"M2libc\"\nseed = true"))?;
        let target = parsed.build_target("amd64").ok_or("target")?;
        let DependencyDeclaration::Source(source) = &target.dependencies()[0] else {
            return Err("not a source dependency".into());
        };
        assert_eq!(source.url().as_str(), "https://github.com/oriansj/M2libc");
        assert_eq!(source.revision().oid(), oid);
        assert_eq!(source.path(), Some("M2libc"));
        assert!(source.seed());

        // The URL is hashed in canonical form: spellings don't matter.
        let respelled = manifest("path = \"M2libc\"\nseed = true").replace(
            "https://GitHub.com/oriansj/M2libc.git",
            "git@github.com:oriansj/M2libc",
        );
        assert_eq!(
            ObjectManifest::parse_toml(&respelled)?.manifest_hash(),
            parsed.manifest_hash()
        );
        // Every field is hashed.
        for changed in ["path = \"other\"\nseed = true", "path = \"M2libc\"", ""] {
            assert_ne!(
                ObjectManifest::parse_toml(&manifest(changed))?.manifest_hash(),
                parsed.manifest_hash(),
                "{changed}"
            );
        }

        for (extra, expected) in [
            ("version = \"^1\"", "never by version"),
            ("path = \"../up\"", "relative path"),
        ] {
            let result = ObjectManifest::parse_toml(&manifest(extra));
            assert!(
                matches!(&result, Err(error) if error.to_string().contains(expected)),
                "{extra}: {result:?}"
            );
        }
        let unpinned = manifest("").replace(&format!("revision = \"git:sha1:{oid}\""), "");
        assert!(matches!(
            ObjectManifest::parse_toml(&unpinned),
            Err(error) if error.to_string().contains("needs revision")
        ));
        let local = manifest("").replace(
            "https://GitHub.com/oriansj/M2libc.git",
            "file:///tmp/M2libc",
        );
        assert!(matches!(
            ObjectManifest::parse_toml(&local),
            Err(error) if error.to_string().contains("local path")
        ));
        let mixed = manifest("").replace("kind = \"source\"", "kind = \"object\"");
        assert!(matches!(
            ObjectManifest::parse_toml(&mixed),
            Err(ManifestError::SourceFieldsOnOtherKind(_))
        ));

        let overlapping = manifest("path = \"lib\"").replace(
            "[[build.targets.outputs]]",
            &format!(
                "[[build.targets.dependencies]]\nkind = \"source\"\nsource = \"https://example.org/x\"\nrevision = \"git:sha1:{oid}\"\npath = \"lib/inner\"\n[[build.targets.outputs]]"
            ),
        );
        assert!(matches!(
            ObjectManifest::parse_toml(&overlapping),
            Err(ManifestError::PlacementConflict { .. })
        ));
        Ok(())
    }

    #[test]
    fn run_targets_are_still_rejected() {
        let manifest = ObjectManifest::parse_toml(
            r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "Example"

            [[run.targets]]
            name = "main"
            command = ["PROGRAM.EXE"]
            "#,
        );
        assert!(matches!(manifest, Err(ManifestError::Toml(_))));
    }

    mod revision_validation {
        use super::*;
        use ed25519_dalek::{Signer, SigningKey};
        use kairo_core::{ActorId, KairoRef, Timestamp};
        use kairo_identity::PublicKey;
        use kairo_statement::{
            ObjectGenesisBody, ObjectGenesisStatement, ObjectKind, Signature, SignedStatement,
            UnsignedStatement,
        };

        const ACTOR_ID: &str = "zQmTn1mdQDA1ryQZsiqYfRbqj5DGcG8TNvYcRmBrBLAuk5t";

        fn timestamp() -> Timestamp {
            Timestamp::from_seconds(1_700_000_000)
        }

        fn signing_key() -> SigningKey {
            SigningKey::from_bytes(&[7; 32])
        }

        fn actor_id() -> Result<ActorId, kairo_core::IdError> {
            ActorId::new(ACTOR_ID)
        }

        fn genesis_for_object(
            nonce: [u8; 32],
        ) -> Result<ObjectGenesisStatement, kairo_core::IdError> {
            let body = ObjectGenesisBody::new(
                ObjectKind::software(),
                actor_id()?,
                timestamp(),
                nonce,
                None,
            );
            let signature_bytes = signing_key().sign(&body.canonical_bytes()).to_bytes();
            let signature = Signature::new(
                actor_id()?,
                PublicKey::ed25519(signing_key().verifying_key().to_bytes())
                    .key_id()
                    .to_string(),
                "ed25519",
                signature_bytes.to_vec(),
            );
            Ok(ObjectGenesisStatement::new(body, signature))
        }

        fn signed_revision(
            object: ObjectId,
            parents: Vec<RevisionId>,
            manifest_hash: BlobId,
        ) -> Result<SignedStatement<ObjectRevisionBody>, Box<dyn std::error::Error>> {
            let body = ObjectRevisionBody::new(
                object.clone(),
                RevisionId::new("git:sha1:eae79d6e0cff76fc7a3bb78723690b0b2367f849"),
                parents,
                manifest_hash,
                true,
            );
            let subject: KairoRef = format!("object:{object}").parse()?;
            let unsigned = UnsignedStatement::new(actor_id()?, subject, timestamp(), body);
            let signature_bytes = signing_key().sign(&unsigned.canonical_bytes()).to_bytes();
            let signature = Signature::new(
                actor_id()?,
                PublicKey::ed25519(signing_key().verifying_key().to_bytes())
                    .key_id()
                    .to_string(),
                "ed25519",
                signature_bytes.to_vec(),
            );
            Ok(SignedStatement::new(unsigned, signature))
        }

        fn manifest_with_object(object: Option<&str>) -> Result<ObjectManifest, ManifestError> {
            ObjectManifest::parse_toml(&manifest_toml(object))
        }

        #[test]
        fn report_is_consistent_with_matching_genesis_and_manifest(
        ) -> Result<(), Box<dyn std::error::Error>> {
            let genesis = genesis_for_object([42; 32])?;
            let object_id = genesis.object_id();
            let manifest = manifest_with_object(Some(object_id.as_str()))?;
            let signed = signed_revision(
                object_id.clone(),
                vec![RevisionId::new(
                    "git:sha1:d8fd39d0bbdd2dcf322d8b11390a4c5825b11495",
                )],
                manifest.manifest_hash(),
            )?;

            let report = validate_object_revision(&signed, Some(&genesis), Some(&manifest), None);

            assert_eq!(report.statement_id, signed.statement_id());
            assert_eq!(
                report.object_consistency,
                ObjectConsistencyCheck::Consistent
            );
            assert_eq!(report.manifest_binding, ManifestBindingCheck::Bound);
            assert_eq!(report.parents, ParentReferenceCheck::Declared { count: 1 });
            assert_eq!(report.content, ContentLayerCheck::Indeterminate);
            assert!(report.is_statement_layer_consistent());
            Ok(())
        }

        #[test]
        fn no_parents_reports_initial_revision() -> Result<(), Box<dyn std::error::Error>> {
            let genesis = genesis_for_object([42; 32])?;
            let object_id = genesis.object_id();
            let manifest = manifest_with_object(None)?;
            let signed = signed_revision(object_id, vec![], manifest.manifest_hash())?;

            let report = validate_object_revision(&signed, Some(&genesis), Some(&manifest), None);

            assert_eq!(report.parents, ParentReferenceCheck::NoParents);
            Ok(())
        }

        #[test]
        fn missing_genesis_reports_indeterminate() -> Result<(), Box<dyn std::error::Error>> {
            let genesis = genesis_for_object([42; 32])?;
            let object_id = genesis.object_id();
            let manifest = manifest_with_object(None)?;
            let signed = signed_revision(
                object_id,
                vec![RevisionId::new(
                    "git:sha1:d8fd39d0bbdd2dcf322d8b11390a4c5825b11495",
                )],
                manifest.manifest_hash(),
            )?;

            let report = validate_object_revision(&signed, None, Some(&manifest), None);

            assert_eq!(
                report.object_consistency,
                ObjectConsistencyCheck::GenesisNotProvided
            );
            assert_eq!(report.manifest_binding, ManifestBindingCheck::Bound);
            assert!(!report.is_statement_layer_consistent());
            Ok(())
        }

        #[test]
        fn missing_manifest_reports_indeterminate() -> Result<(), Box<dyn std::error::Error>> {
            let genesis = genesis_for_object([42; 32])?;
            let object_id = genesis.object_id();
            let signed = signed_revision(
                object_id,
                vec![RevisionId::new(
                    "git:sha1:d8fd39d0bbdd2dcf322d8b11390a4c5825b11495",
                )],
                BlobId::from_sha256_digest([1; 32]),
            )?;

            let report = validate_object_revision(&signed, Some(&genesis), None, None);

            assert_eq!(
                report.object_consistency,
                ObjectConsistencyCheck::Consistent
            );
            assert_eq!(
                report.manifest_binding,
                ManifestBindingCheck::ManifestNotProvided
            );
            assert!(!report.is_statement_layer_consistent());
            Ok(())
        }

        #[test]
        fn wrong_genesis_reports_object_mismatch() -> Result<(), Box<dyn std::error::Error>> {
            // Build a revision bound to genesis A's object, then validate
            // against genesis B (different nonce → different object id).
            let genesis_a = genesis_for_object([42; 32])?;
            let object_a = genesis_a.object_id();
            let genesis_b = genesis_for_object([99; 32])?;
            let object_b = genesis_b.object_id();
            assert_ne!(object_a, object_b);

            let signed = signed_revision(
                object_a.clone(),
                vec![],
                BlobId::from_sha256_digest([1; 32]),
            )?;

            let report = validate_object_revision(&signed, Some(&genesis_b), None, None);

            assert!(matches!(
                report.object_consistency,
                ObjectConsistencyCheck::Mismatch { ref expected, ref actual }
                    if expected == &object_a && actual == &object_b
            ));
            assert!(!report.is_statement_layer_consistent());
            Ok(())
        }

        #[test]
        fn manifest_hash_mismatch_reported() -> Result<(), Box<dyn std::error::Error>> {
            let genesis = genesis_for_object([42; 32])?;
            let object_id = genesis.object_id();
            let manifest = manifest_with_object(None)?;
            let signed = signed_revision(object_id, vec![], BlobId::from_sha256_digest([1; 32]))?;

            let report = validate_object_revision(&signed, Some(&genesis), Some(&manifest), None);

            assert!(matches!(
                report.manifest_binding,
                ManifestBindingCheck::HashMismatch { .. }
            ));
            assert_eq!(
                report.object_consistency,
                ObjectConsistencyCheck::Consistent
            );
            Ok(())
        }

        #[test]
        fn declared_object_mismatch_reported() -> Result<(), Box<dyn std::error::Error>> {
            // Manifest declares object A; revision binds to a fabricated
            // object id (which then fails the manifest-declared object check
            // even though we don't have a genesis for it).
            let revision_object = ObjectId::from_sha256_digest([4; 32]);
            let manifest_object = ObjectId::new(OBJECT_ID)?;
            assert_ne!(revision_object, manifest_object);

            let manifest = manifest_with_object(Some(manifest_object.as_str()))?;
            let signed =
                signed_revision(revision_object.clone(), vec![], manifest.manifest_hash())?;

            let report = validate_object_revision(&signed, None, Some(&manifest), None);

            assert!(matches!(
                report.manifest_binding,
                ManifestBindingCheck::DeclaredObjectMismatch { ref expected, ref actual }
                    if expected == &revision_object && actual == &manifest_object
            ));
            // No genesis was supplied, so object consistency is indeterminate.
            assert_eq!(
                report.object_consistency,
                ObjectConsistencyCheck::GenesisNotProvided
            );
            Ok(())
        }

        #[test]
        fn dimensions_are_independent() -> Result<(), Box<dyn std::error::Error>> {
            // Wrong genesis AND wrong manifest hash: each should report its
            // own failure rather than masking the other.
            let genesis_a = genesis_for_object([42; 32])?;
            let genesis_b = genesis_for_object([99; 32])?;
            let manifest = manifest_with_object(None)?;
            let signed = signed_revision(
                genesis_a.object_id(),
                vec![],
                BlobId::from_sha256_digest([2; 32]),
            )?;

            let report = validate_object_revision(&signed, Some(&genesis_b), Some(&manifest), None);

            assert!(matches!(
                report.object_consistency,
                ObjectConsistencyCheck::Mismatch { .. }
            ));
            assert!(matches!(
                report.manifest_binding,
                ManifestBindingCheck::HashMismatch { .. }
            ));
            Ok(())
        }

        #[test]
        fn content_layer_verified_when_lookup_parents_match(
        ) -> Result<(), Box<dyn std::error::Error>> {
            let genesis = genesis_for_object([42; 32])?;
            let manifest = manifest_with_object(None)?;
            let signed = signed_revision(
                genesis.object_id(),
                vec![RevisionId::new(
                    "git:sha1:7e240de74fb1ed08fa08d38063f6a6a91462a815",
                )],
                manifest.manifest_hash(),
            )?;
            let lookup = CommitLookup::Found {
                parent_oids: vec!["7e240de74fb1ed08fa08d38063f6a6a91462a815".to_owned()],
            };
            let report =
                validate_object_revision(&signed, Some(&genesis), Some(&manifest), Some(&lookup));
            assert_eq!(report.content, ContentLayerCheck::Verified);
            Ok(())
        }

        #[test]
        fn content_layer_parent_mismatch_when_oids_disagree(
        ) -> Result<(), Box<dyn std::error::Error>> {
            let genesis = genesis_for_object([42; 32])?;
            let manifest = manifest_with_object(None)?;
            let signed = signed_revision(
                genesis.object_id(),
                vec![RevisionId::new(
                    "git:sha1:7e240de74fb1ed08fa08d38063f6a6a91462a815",
                )],
                manifest.manifest_hash(),
            )?;
            let lookup = CommitLookup::Found {
                parent_oids: vec!["5cb138284d431abd6a053a56625ec088bfb88912".to_owned()],
            };
            let report =
                validate_object_revision(&signed, Some(&genesis), Some(&manifest), Some(&lookup));
            assert!(matches!(
                report.content,
                ContentLayerCheck::ParentMismatch { .. }
            ));
            Ok(())
        }

        #[test]
        fn content_layer_parent_match_is_order_independent(
        ) -> Result<(), Box<dyn std::error::Error>> {
            // Two declared parents in one order, Git reports them in the
            // opposite order. Should still verify (set equality).
            let genesis = genesis_for_object([42; 32])?;
            let manifest = manifest_with_object(None)?;
            let signed = signed_revision(
                genesis.object_id(),
                vec![
                    RevisionId::new("git:sha1:7e240de74fb1ed08fa08d38063f6a6a91462a815"),
                    RevisionId::new("git:sha1:5cb138284d431abd6a053a56625ec088bfb88912"),
                ],
                manifest.manifest_hash(),
            )?;
            let lookup = CommitLookup::Found {
                parent_oids: vec![
                    "5cb138284d431abd6a053a56625ec088bfb88912".to_owned(),
                    "7e240de74fb1ed08fa08d38063f6a6a91462a815".to_owned(),
                ],
            };
            let report =
                validate_object_revision(&signed, Some(&genesis), Some(&manifest), Some(&lookup));
            assert_eq!(report.content, ContentLayerCheck::Verified);
            Ok(())
        }

        #[test]
        fn content_layer_commit_not_found() -> Result<(), Box<dyn std::error::Error>> {
            let genesis = genesis_for_object([42; 32])?;
            let manifest = manifest_with_object(None)?;
            let signed = signed_revision(genesis.object_id(), vec![], manifest.manifest_hash())?;
            let lookup = CommitLookup::NotFound;
            let report =
                validate_object_revision(&signed, Some(&genesis), Some(&manifest), Some(&lookup));
            assert_eq!(report.content, ContentLayerCheck::CommitNotFound);
            Ok(())
        }

        #[test]
        fn content_layer_indeterminate_when_parent_lacks_git_prefix(
        ) -> Result<(), Box<dyn std::error::Error>> {
            // A non-git parent reference can't be compared against Git's
            // view; we don't false-positive — we report Indeterminate.
            let genesis = genesis_for_object([42; 32])?;
            let manifest = manifest_with_object(None)?;
            let signed = signed_revision(
                genesis.object_id(),
                vec![RevisionId::new("opaque:abc")],
                manifest.manifest_hash(),
            )?;
            let lookup = CommitLookup::Found {
                parent_oids: vec!["a9993e364706816aba3e25717850c26c9cd0d89d".to_owned()],
            };
            let report =
                validate_object_revision(&signed, Some(&genesis), Some(&manifest), Some(&lookup));
            assert_eq!(report.content, ContentLayerCheck::Indeterminate);
            Ok(())
        }
    }

    mod snapshot {
        use super::*;
        use ed25519_dalek::{Signer, SigningKey};
        use kairo_core::{ActorId, KairoRef, Timestamp};
        use kairo_identity::PublicKey;
        use kairo_statement::{
            ObjectGenesisBody, ObjectKind, Signature, SignedStatement, UnsignedStatement,
        };

        const ACTOR_ID: &str = "zQmTn1mdQDA1ryQZsiqYfRbqj5DGcG8TNvYcRmBrBLAuk5t";

        fn timestamp() -> Timestamp {
            Timestamp::from_seconds(1_700_000_000)
        }

        fn signing_key() -> SigningKey {
            SigningKey::from_bytes(&[7; 32])
        }

        fn actor_id() -> Result<ActorId, kairo_core::IdError> {
            ActorId::new(ACTOR_ID)
        }

        fn signed_revision(
            object: ObjectId,
            revision: &str,
            manifest_hash: BlobId,
        ) -> Result<SignedStatement<ObjectRevisionBody>, Box<dyn std::error::Error>> {
            let body = ObjectRevisionBody::new(
                object.clone(),
                RevisionId::new(revision),
                vec![],
                manifest_hash,
                true,
            );
            let subject: KairoRef = format!("object:{object}").parse()?;
            let unsigned = UnsignedStatement::new(actor_id()?, subject, timestamp(), body);
            let signature_bytes = signing_key().sign(&unsigned.canonical_bytes()).to_bytes();
            let signature = Signature::new(
                actor_id()?,
                PublicKey::ed25519(signing_key().verifying_key().to_bytes())
                    .key_id()
                    .to_string(),
                "ed25519",
                signature_bytes.to_vec(),
            );
            Ok(SignedStatement::new(unsigned, signature))
        }

        fn object_id_for(nonce: [u8; 32]) -> Result<ObjectId, kairo_core::IdError> {
            let body = ObjectGenesisBody::new(
                ObjectKind::software(),
                actor_id()?,
                timestamp(),
                nonce,
                None,
            );
            Ok(body.object_id())
        }

        #[test]
        fn same_revision_produces_same_snapshot_id() -> Result<(), Box<dyn std::error::Error>> {
            let object = object_id_for([42; 32])?;
            let manifest_hash = BlobId::from_sha256_digest([1; 32]);
            let signed = signed_revision(
                object.clone(),
                "git:sha1:03e404fd168941cfed98f46654680130dd85968b",
                manifest_hash,
            )?;

            let first = Snapshot::from_object_revision(&object, &signed)?;
            let second = Snapshot::from_object_revision(&object, &signed)?;

            assert_eq!(first.snapshot_id(), second.snapshot_id());
            Ok(())
        }

        #[test]
        fn different_revision_changes_snapshot_id() -> Result<(), Box<dyn std::error::Error>> {
            let object = object_id_for([42; 32])?;
            let manifest_hash = BlobId::from_sha256_digest([1; 32]);
            let first_rev = signed_revision(
                object.clone(),
                "git:sha1:5573e39b6600496d40f493d00ec7658479a19607",
                manifest_hash.clone(),
            )?;
            let second_rev = signed_revision(
                object.clone(),
                "git:sha1:a50126cc2d6c726de0ca203c3b659f658d356173",
                manifest_hash,
            )?;

            let first = Snapshot::from_object_revision(&object, &first_rev)?.snapshot_id();
            let second = Snapshot::from_object_revision(&object, &second_rev)?.snapshot_id();

            assert_ne!(first, second);
            Ok(())
        }

        #[test]
        fn different_manifest_hash_changes_snapshot_id() -> Result<(), Box<dyn std::error::Error>> {
            let object = object_id_for([42; 32])?;
            let first_rev = signed_revision(
                object.clone(),
                "git:sha1:03e404fd168941cfed98f46654680130dd85968b",
                BlobId::from_sha256_digest([1; 32]),
            )?;
            let second_rev = signed_revision(
                object.clone(),
                "git:sha1:03e404fd168941cfed98f46654680130dd85968b",
                BlobId::from_sha256_digest([2; 32]),
            )?;

            let first = Snapshot::from_object_revision(&object, &first_rev)?.snapshot_id();
            let second = Snapshot::from_object_revision(&object, &second_rev)?.snapshot_id();

            assert_ne!(first, second);
            Ok(())
        }

        #[test]
        fn different_object_changes_snapshot_id() -> Result<(), Box<dyn std::error::Error>> {
            // Two distinct lineages, otherwise-identical revision content. The
            // statement_ids differ (different object) so snapshot ids differ.
            let object_a = object_id_for([42; 32])?;
            let object_b = object_id_for([99; 32])?;
            let manifest_hash = BlobId::from_sha256_digest([1; 32]);

            let rev_a = signed_revision(
                object_a.clone(),
                "git:sha1:03e404fd168941cfed98f46654680130dd85968b",
                manifest_hash.clone(),
            )?;
            let rev_b = signed_revision(
                object_b.clone(),
                "git:sha1:03e404fd168941cfed98f46654680130dd85968b",
                manifest_hash,
            )?;

            let snapshot_a = Snapshot::from_object_revision(&object_a, &rev_a)?.snapshot_id();
            let snapshot_b = Snapshot::from_object_revision(&object_b, &rev_b)?.snapshot_id();

            assert_ne!(snapshot_a, snapshot_b);
            Ok(())
        }

        #[test]
        fn rejects_revision_for_wrong_object() -> Result<(), Box<dyn std::error::Error>> {
            let object_a = object_id_for([42; 32])?;
            let object_b = object_id_for([99; 32])?;
            let manifest_hash = BlobId::from_sha256_digest([1; 32]);
            let rev_for_b = signed_revision(
                object_b.clone(),
                "git:sha1:03e404fd168941cfed98f46654680130dd85968b",
                manifest_hash,
            )?;

            let result = Snapshot::from_object_revision(&object_a, &rev_for_b);

            assert!(matches!(result, Err(SnapshotError::ObjectMismatch { .. })));
            Ok(())
        }

        #[test]
        fn snapshot_id_is_a_valid_snapshot_id_string() -> Result<(), Box<dyn std::error::Error>> {
            let object = object_id_for([42; 32])?;
            let manifest_hash = BlobId::from_sha256_digest([1; 32]);
            let signed = signed_revision(
                object.clone(),
                "git:sha1:03e404fd168941cfed98f46654680130dd85968b",
                manifest_hash,
            )?;
            let snapshot = Snapshot::from_object_revision(&object, &signed)?;
            let id = snapshot.snapshot_id();
            // Round-tripping through SnapshotId::new should not change the id.
            assert_eq!(SnapshotId::new(id.to_string())?, id);
            Ok(())
        }

        #[test]
        fn frontier_carries_the_revision_statement_id() -> Result<(), Box<dyn std::error::Error>> {
            let object = object_id_for([42; 32])?;
            let manifest_hash = BlobId::from_sha256_digest([1; 32]);
            let signed = signed_revision(
                object.clone(),
                "git:sha1:03e404fd168941cfed98f46654680130dd85968b",
                manifest_hash,
            )?;
            let snapshot = Snapshot::from_object_revision(&object, &signed)?;

            assert_eq!(snapshot.frontier(), &[signed.statement_id()]);
            Ok(())
        }
    }
}
