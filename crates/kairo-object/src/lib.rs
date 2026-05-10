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
/// provided, or when the revision's storage form is not a `git:sha256:`
/// reference, the check stays `Indeterminate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentLayerCheck {
    /// Commit was found and its parents agree with the parents
    /// declared on the `ObjectRevision` statement (set-equality —
    /// parent ordering is not enforced).
    Verified,
    /// Commit was found but its parents disagree with the declared
    /// parents. Lists are normalized to git oids (without the
    /// `git:sha256:` prefix).
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
    /// (without the `git:sha256:` prefix).
    Found { parent_oids: Vec<String> },
    /// Commit was not present in the repository.
    NotFound,
}

/// Storage-revision scheme prefix this validator understands.
const GIT_REVISION_PREFIX: &str = "git:sha256:";

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

    // Strip the git:sha256: prefix from declared parents. If any
    // declared parent lacks the prefix, we can't compare against Git
    // — treat the layer as Indeterminate so we don't false-positive.
    let mut expected = Vec::with_capacity(revision.parents().len());
    for parent in revision.parents() {
        match parent.as_str().strip_prefix(GIT_REVISION_PREFIX) {
            Some(oid) => expected.push(oid.to_owned()),
            None => return ContentLayerCheck::Indeterminate,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvideDeclaration {
    provides: String,
    version: Option<String>,
}

impl ProvideDeclaration {
    pub fn provides(&self) -> &str {
        &self.provides
    }

    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }
}

impl CanonicalEncode for ProvideDeclaration {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_str(out, &self.provides);
        encode_option(out, self.version.as_ref(), |out, version| {
            encode_str(out, version);
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DependencyDeclaration {
    Provides(ProvidesDependency),
    Object(ObjectDependency),
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
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvidesDependency {
    provides: String,
}

impl ProvidesDependency {
    pub fn provides(&self) -> &str {
        &self.provides
    }
}

impl CanonicalEncode for ProvidesDependency {
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        encode_str(out, &self.provides);
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
            | Self::MissingProvidesDependency => None,
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
}

impl RawObjectManifest {
    fn validate(self) -> Result<ObjectManifest, ManifestError> {
        Ok(ObjectManifest {
            kairo: self.kairo.validate()?,
            content: self.content.map(RawContentSection::validate).transpose()?,
            provides: self
                .provides
                .into_iter()
                .map(RawProvideDeclaration::validate)
                .collect::<Result<Vec<_>, _>>()?,
            dependencies: self
                .dependencies
                .into_iter()
                .map(RawDependencyDeclaration::validate)
                .collect::<Result<Vec<_>, _>>()?,
        })
    }
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
}

impl RawProvideDeclaration {
    fn validate(self) -> Result<ProvideDeclaration, ManifestError> {
        ensure_non_empty("provides.provides", &self.provides)?;

        Ok(ProvideDeclaration {
            provides: self.provides,
            version: self.version,
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
}

impl RawDependencyDeclaration {
    fn validate(self) -> Result<DependencyDeclaration, ManifestError> {
        match self.kind.as_str() {
            "provides" => self.validate_provides(),
            "object" => self.validate_object(),
            kind => Err(ManifestError::UnknownDependencyKind(kind.to_owned())),
        }
    }

    fn validate_provides(self) -> Result<DependencyDeclaration, ManifestError> {
        let Some(provides) = self.provides else {
            return Err(ManifestError::MissingProvidesDependency);
        };

        ensure_non_empty("dependencies.provides", &provides)?;

        Ok(DependencyDeclaration::Provides(ProvidesDependency {
            provides,
        }))
    }

    fn validate_object(self) -> Result<DependencyDeclaration, ManifestError> {
        let Some(object) = self.object else {
            return Err(ManifestError::MissingObjectDependencyObject);
        };

        let object = ObjectId::new(object).map_err(ManifestError::InvalidObjectId)?;
        let selector = match (self.version, self.snapshot) {
            (Some(version), None) => {
                ensure_non_empty("dependencies.version", &version)?;
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
            RevisionId::new("git:sha256:revision"),
            vec![RevisionId::new("git:sha256:parent")],
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
                RevisionId::new("git:sha256:revision"),
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
                vec![RevisionId::new("git:sha256:parent")],
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
                vec![RevisionId::new("git:sha256:parent")],
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
                vec![RevisionId::new("git:sha256:parent")],
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
                vec![RevisionId::new("git:sha256:aaa")],
                manifest.manifest_hash(),
            )?;
            let lookup = CommitLookup::Found {
                parent_oids: vec!["aaa".to_owned()],
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
                vec![RevisionId::new("git:sha256:aaa")],
                manifest.manifest_hash(),
            )?;
            let lookup = CommitLookup::Found {
                parent_oids: vec!["bbb".to_owned()],
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
                    RevisionId::new("git:sha256:aaa"),
                    RevisionId::new("git:sha256:bbb"),
                ],
                manifest.manifest_hash(),
            )?;
            let lookup = CommitLookup::Found {
                parent_oids: vec!["bbb".to_owned(), "aaa".to_owned()],
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
                parent_oids: vec!["abc".to_owned()],
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
            let signed = signed_revision(object.clone(), "git:sha256:rev", manifest_hash)?;

            let first = Snapshot::from_object_revision(&object, &signed)?;
            let second = Snapshot::from_object_revision(&object, &signed)?;

            assert_eq!(first.snapshot_id(), second.snapshot_id());
            Ok(())
        }

        #[test]
        fn different_revision_changes_snapshot_id() -> Result<(), Box<dyn std::error::Error>> {
            let object = object_id_for([42; 32])?;
            let manifest_hash = BlobId::from_sha256_digest([1; 32]);
            let first_rev =
                signed_revision(object.clone(), "git:sha256:r1", manifest_hash.clone())?;
            let second_rev = signed_revision(object.clone(), "git:sha256:r2", manifest_hash)?;

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
                "git:sha256:rev",
                BlobId::from_sha256_digest([1; 32]),
            )?;
            let second_rev = signed_revision(
                object.clone(),
                "git:sha256:rev",
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

            let rev_a = signed_revision(object_a.clone(), "git:sha256:rev", manifest_hash.clone())?;
            let rev_b = signed_revision(object_b.clone(), "git:sha256:rev", manifest_hash)?;

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
            let rev_for_b = signed_revision(object_b.clone(), "git:sha256:rev", manifest_hash)?;

            let result = Snapshot::from_object_revision(&object_a, &rev_for_b);

            assert!(matches!(result, Err(SnapshotError::ObjectMismatch { .. })));
            Ok(())
        }

        #[test]
        fn snapshot_id_is_a_valid_snapshot_id_string() -> Result<(), Box<dyn std::error::Error>> {
            let object = object_id_for([42; 32])?;
            let manifest_hash = BlobId::from_sha256_digest([1; 32]);
            let signed = signed_revision(object.clone(), "git:sha256:rev", manifest_hash)?;
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
            let signed = signed_revision(object.clone(), "git:sha256:rev", manifest_hash)?;
            let snapshot = Snapshot::from_object_revision(&object, &signed)?;

            assert_eq!(snapshot.frontier(), &[signed.statement_id()]);
            Ok(())
        }
    }
}
