//! `kairo-resolver` — local build planning (`specs/DECISIONS.md` §33
//! sub-decisions 2–3, §34 sub-decision 5).
//!
//! Given an object, a snapshot and a build target, the resolver pins
//! every requirement the target has — the manifest's top-level
//! `[[dependencies]]` plus the target's own — to something this node
//! holds, recursively, and writes the result as a
//! `kairo.resolved-manifest.v1` document whose blob id is the plan
//! hash. A provides requirement resolves to one of:
//!
//! - an **artifact provider**: a held object whose `[[provides]]`
//!   entry names a build target and output, backed by that output's
//!   artifact when one is held, or scheduled as a build the plan needs
//!   first;
//! - a **content provider**: a held object whose `[[provides]]` entry
//!   names no target, meaning its own tree;
//! - an **environment contract**: an `environment:*` label an
//!   allowlisted engine implements, or — in development mode, when the
//!   label is the host's own — the host, which marks the plan
//!   host-dependent;
//! - a **development-mode host tool** from the node's config, which
//!   also marks the plan host-dependent.
//!
//! An object requirement resolves to a pinned snapshot, or to the
//! newest of the creator's version tags that meets its requirement.
//! Labels match exactly. Among several candidates the resolver prefers
//! artifact providers, then content providers, then host tools, and
//! breaks ties by object id, listing the losers as diagnostics.
//!
//! The resolver reads manifests through [`ManifestSource`] and build
//! results through [`ArtifactSource`], so the store stays the only
//! thing it walks: the CLI supplies the Git cache for the former, and
//! nothing yet for the latter (`BuildResult` is the next slice).
//! The plan hash covers the whole closure and never the engine (§34
//! sub-decision 4).

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::path::PathBuf;

use kairo_core::{BlobId, ObjectId, SnapshotId};
use kairo_object::{
    DependencyDeclaration, ObjectDependencySelector, ObjectManifest, Snapshot, SnapshotError,
};
use kairo_statement::{GitRevision, RevisionId};
use kairo_store::{
    BranchResolver, FilesystemStore, ObjectStore, SourceStore, StatementStore, StoreError,
    VersionTagResolver,
};
use serde::{Deserialize, Serialize};

/// Domain separator for the plan hash: `BlobId` of a resolved
/// manifest's bytes (`schemas/canonical/resolved-manifest-v1.md`).
pub const RESOLVED_MANIFEST_DOMAIN: &[u8] = b"kairo.resolved-manifest.v1";

/// The `schema` field of a resolved manifest.
pub const RESOLVED_MANIFEST_SCHEMA: &str = "kairo.resolved-manifest.v1";

const ENVIRONMENT_PREFIX: &str = "environment:";

/// Where the resolver reads a snapshot's `kairo.toml` from. The
/// manifest's canonical hash must equal the revision's
/// `manifest_hash`; the resolver checks.
pub trait ManifestSource {
    /// The manifest at `revision`, or `None` when this node doesn't
    /// hold the revision's tree.
    fn manifest(
        &self,
        object: &ObjectId,
        revision: &RevisionId,
    ) -> Result<Option<ObjectManifest>, SourceError>;
}

/// Where the resolver asks whether a build output's artifact is held.
pub trait ArtifactSource {
    /// The artifact id of `target`'s successful build at `snapshot`,
    /// if this node holds one.
    fn artifact(
        &self,
        object: &ObjectId,
        snapshot: &SnapshotId,
        target: &str,
    ) -> Result<Option<BlobId>, SourceError>;
}

/// Whether this node holds a source's commit (§37): the source object,
/// and the commit with its closure in the source's Git cache.
pub trait SourceHolder {
    fn holds(&self, source: &ObjectId, revision: &GitRevision) -> Result<bool, SourceError>;
}

/// A [`SourceHolder`] that holds nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoSources;

impl SourceHolder for NoSources {
    fn holds(&self, _source: &ObjectId, _revision: &GitRevision) -> Result<bool, SourceError> {
        Ok(false)
    }
}

/// An accepted seed: a source at an exact upstream commit (§37
/// sub-decision 8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSeed {
    pub source: ObjectId,
    pub revision: GitRevision,
}

/// An [`ArtifactSource`] that holds nothing: every artifact provider
/// becomes a build the plan needs first.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoArtifacts;

impl ArtifactSource for NoArtifacts {
    fn artifact(
        &self,
        _object: &ObjectId,
        _snapshot: &SnapshotId,
        _target: &str,
    ) -> Result<Option<BlobId>, SourceError> {
        Ok(None)
    }
}

/// A source's own failure, as text; the resolver wraps it with what
/// it was asking for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceError(pub String);

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for SourceError {}

/// Host facts (`DECISIONS.md` §34 sub-decision 2): detected, never
/// configured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Host {
    /// `std::env::consts::OS`, e.g. `linux`.
    pub system: String,
    /// `std::env::consts::ARCH` in label spelling (`x86-64`,
    /// `aarch64`).
    pub arch: String,
    /// The environment contracts the host's allowlisted engines
    /// implement. Empty until the first engine lands.
    pub engines: Vec<String>,
}

impl Host {
    /// This machine, with no engines.
    pub fn detect() -> Self {
        Self {
            system: std::env::consts::OS.to_owned(),
            arch: label_arch(std::env::consts::ARCH),
            engines: Vec::new(),
        }
    }

    /// The contract label the host itself would satisfy in
    /// development mode: `environment:<system>/<arch>`.
    pub fn contract(&self) -> String {
        format!("{ENVIRONMENT_PREFIX}{}/{}", self.system, self.arch)
    }
}

/// `x86_64` is spelled `x86-64` in labels (`DECISIONS.md` §33).
fn label_arch(arch: &str) -> String {
    arch.replace('_', "-")
}

/// A development-mode host tool (`[[runtime.host.tools]]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostTool {
    pub provides: String,
    pub version: Option<String>,
    pub path: PathBuf,
}

/// Which snapshot of an object to plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotSelector {
    /// The creator's `head` branch.
    Current,
    /// Exactly this snapshot, which must be the current one or one of
    /// the creator's tagged versions.
    Pinned(SnapshotId),
}

// --- the resolved manifest -------------------------------------------

/// A `kairo.resolved-manifest.v1` document (`DECISIONS.md` §33
/// sub-decision 3). Serialized compactly with the fields in this
/// order; its bytes' `BlobId` under [`RESOLVED_MANIFEST_DOMAIN`] is the
/// plan hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedManifest {
    pub schema: String,
    pub object: String,
    pub snapshot: String,
    pub target: String,
    /// The environment contract an engine will provide, or `none`.
    pub environment: String,
    /// The plan leans on this host: a host tool, or the host standing
    /// in for a contract in development mode (§34 sub-decision 10).
    pub host_dependent: bool,
    pub requirements: Vec<Requirement>,
    /// The builds this plan needs first, dependencies before
    /// dependents, each by its own plan hash.
    pub builds_first: Vec<BuildFirst>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Requirement {
    pub requested: Requested,
    pub resolved: Resolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Requested {
    Provides {
        provides: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        version: Option<String>,
        /// Where the provider is copied in the source tree (§38).
        #[serde(skip_serializing_if = "Option::is_none", default)]
        path: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        from: Option<String>,
        #[serde(skip_serializing_if = "std::ops::Not::not", default)]
        overlay: bool,
    },
    Object {
        object: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        version: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        snapshot: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        path: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        from: Option<String>,
        #[serde(skip_serializing_if = "std::ops::Not::not", default)]
        overlay: bool,
    },
    /// A source object at an exact upstream commit (§37).
    Source {
        /// The canonical URL.
        source: String,
        revision: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(skip_serializing_if = "std::ops::Not::not", default)]
        seed: bool,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        from: Option<String>,
        #[serde(skip_serializing_if = "std::ops::Not::not", default)]
        overlay: bool,
    },
    /// A file source by its hash, placed at `path` (§38).
    File {
        sha256: String,
        path: String,
        #[serde(skip_serializing_if = "std::ops::Not::not", default)]
        overlay: bool,
    },
}

/// Where a requirement is placed in the source tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlacedAt<'a> {
    pub path: &'a str,
    pub from: Option<&'a str>,
    pub overlay: bool,
}

impl Requested {
    /// Where the requirement is placed in the source tree, if it is.
    pub fn placement(&self) -> Option<PlacedAt<'_>> {
        match self {
            Self::Provides {
                path,
                from,
                overlay,
                ..
            }
            | Self::Object {
                path,
                from,
                overlay,
                ..
            }
            | Self::Source {
                path,
                from,
                overlay,
                ..
            } => path.as_deref().map(|path| PlacedAt {
                path,
                from: from.as_deref(),
                overlay: *overlay,
            }),
            Self::File { path, overlay, .. } => Some(PlacedAt {
                path,
                from: None,
                overlay: *overlay,
            }),
        }
    }
}

/// A manifest placement's fields as the plan spells them.
fn placement_fields(
    placement: Option<&kairo_object::Placement>,
) -> (Option<String>, Option<String>, bool) {
    placement.map_or((None, None, false), |placement| {
        (
            Some(placement.path().to_owned()),
            placement.from().map(str::to_owned),
            placement.overlay(),
        )
    })
}

impl fmt::Display for Requested {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Provides {
                provides, version, ..
            } => match version {
                Some(version) => write!(f, "{provides} {version}"),
                None => f.write_str(provides),
            },
            Self::Source {
                source,
                revision,
                path,
                seed,
                ..
            } => {
                write!(f, "{source} at {revision}")?;
                if let Some(path) = path {
                    write!(f, " in {path}")?;
                }
                if *seed {
                    f.write_str(" (seed)")?;
                }
                Ok(())
            }
            Self::Object {
                object,
                version,
                snapshot,
                ..
            } => match (version, snapshot) {
                (Some(version), _) => write!(f, "object:{object} {version}"),
                (_, Some(snapshot)) => write!(f, "object:{object}:snapshot:{snapshot}"),
                (None, None) => write!(f, "object:{object}"),
            },
            Self::File { sha256, path, .. } => write!(f, "file sha256:{sha256} at {path}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Resolved {
    /// A build output of a held object. `artifact` is its id when the
    /// build result is held; otherwise the build is in `builds_first`.
    Artifact {
        object: String,
        snapshot: String,
        target: String,
        output: String,
        artifact: Option<String>,
        /// Where the artifact lands in the guest: its target's install
        /// root, or `None` for `/kairo/artifact/<id>` (`DECISIONS.md`
        /// §41 sub-decision 3).
        #[serde(default)]
        install: Option<String>,
        /// For a child object (§41 sub-decision 4): its part of the
        /// building object's artifact, a relative path within it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        root: Option<String>,
    },
    /// A held object's own tree.
    Content { object: String, snapshot: String },
    /// An environment contract; `engine` names the engine that
    /// implements it, or is `None` for the host in development mode.
    Environment {
        contract: String,
        engine: Option<String>,
    },
    /// A development-mode host tool.
    HostTool {
        provides: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        version: Option<String>,
        path: String,
    },
    /// A source's tree at an upstream commit, placed at `path` in the
    /// source tree when given.
    Source {
        source: String,
        url: String,
        revision: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(skip_serializing_if = "std::ops::Not::not", default)]
        seed: bool,
    },
    /// A held file source's bytes, placed at `path` in the source tree.
    File {
        source: String,
        sha256: String,
        path: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildFirst {
    pub object: String,
    pub snapshot: String,
    pub target: String,
    pub plan: String,
}

/// A resolved manifest with its bytes and plan hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanDocument {
    pub manifest: ResolvedManifest,
    pub bytes: Vec<u8>,
    pub hash: BlobId,
}

impl PlanDocument {
    fn new(manifest: ResolvedManifest) -> Result<Self, ResolveError> {
        let bytes = serde_json::to_vec(&manifest).map_err(ResolveError::Encode)?;
        let hash = BlobId::from_bytes(RESOLVED_MANIFEST_DOMAIN, &bytes);
        Ok(Self {
            manifest,
            bytes,
            hash,
        })
    }

    /// Parse stored bytes back; the hash is re-derived from them.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, ResolveError> {
        let manifest: ResolvedManifest =
            serde_json::from_slice(&bytes).map_err(ResolveError::Encode)?;
        if manifest.schema != RESOLVED_MANIFEST_SCHEMA {
            return Err(ResolveError::UnsupportedSchema(manifest.schema));
        }
        let hash = BlobId::from_bytes(RESOLVED_MANIFEST_DOMAIN, &bytes);
        Ok(Self {
            manifest,
            bytes,
            hash,
        })
    }
}

/// What a resolution produced: the plan, the plans it needs first
/// (dependencies before dependents), and what the resolver passed
/// over on the way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub plan: PlanDocument,
    pub prerequisites: Vec<PlanDocument>,
    pub diagnostics: Diagnostics,
}

/// Not part of the plan hash: held objects the resolver couldn't
/// consider, and the candidates it didn't choose.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diagnostics {
    pub skipped: Vec<Skipped>,
    pub alternatives: Vec<Alternative>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    pub object: ObjectId,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alternative {
    pub requested: Requested,
    pub chosen: String,
    pub others: Vec<String>,
}

// --- candidates --------------------------------------------------------

/// A held object at its current snapshot, with its manifest.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub object: ObjectId,
    pub snapshot: SnapshotId,
    pub revision: RevisionId,
    pub manifest: ObjectManifest,
}

/// One way a candidate satisfies a provides label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub object: ObjectId,
    pub snapshot: SnapshotId,
    pub provides: String,
    pub version: Option<String>,
    /// `Some((target, output))` for an artifact provider.
    pub artifact: Option<(String, String)>,
    /// The environment contract the artifact's build target declares,
    /// if any (§38 sub-decision 8).
    pub environment: Option<String>,
    /// The artifact's install root, from its build target (§41).
    pub install: Option<String>,
    /// For a child object's offer: the object whose build makes it
    /// (its parent) and the child's root in that artifact.
    pub built_by: Option<(ObjectId, String)>,
}

/// The environment contract a target declares, among its own and its
/// manifest's dependencies.
pub fn declared_environment(
    manifest: &ObjectManifest,
    target: &kairo_object::BuildTarget,
) -> Option<String> {
    manifest
        .dependencies()
        .iter()
        .chain(target.dependencies())
        .find_map(|dependency| match dependency {
            DependencyDeclaration::Provides(dependency)
                if dependency.provides().starts_with(ENVIRONMENT_PREFIX) =>
            {
                Some(dependency.provides().to_owned())
            }
            _ => None,
        })
}

/// The object whose build makes the child `id` (`DECISIONS.md` §44
/// sub-decision 2): found in the current manifests `index` holds, or
/// else in the manifests at each held object's version-tagged
/// snapshots. `None` when no manifest this node holds declares it.
pub fn parent_of_child(
    store: &FilesystemStore,
    manifests: &dyn ManifestSource,
    index: &Index,
    id: &ObjectId,
) -> Result<Option<ObjectId>, ResolveError> {
    if let Some((candidate, _, _)) = index.child(id) {
        return Ok(Some(candidate.object.clone()));
    }
    for candidate in &index.candidates {
        for (_, snapshot) in tagged_snapshots(store, &candidate.object)? {
            let Some(manifest) = load_manifest(manifests, &candidate.object, &snapshot)? else {
                continue;
            };
            let declares = manifest.build_targets().iter().any(|target| {
                target.children().iter().any(|child| {
                    &kairo_object::ChildGenesis::new(
                        candidate.object.clone(),
                        target.name(),
                        child.name(),
                    )
                    .object_id()
                        == id
                })
            });
            if declares {
                return Ok(Some(candidate.object.clone()));
            }
        }
    }
    Ok(None)
}

/// An offer of a child object, built by `candidate`'s `target`.
fn child_offer(
    candidate: &Candidate,
    target: &kairo_object::BuildTarget,
    child: &kairo_object::ChildDeclaration,
    version: Option<&str>,
) -> Offer {
    Offer {
        object: kairo_object::ChildGenesis::new(
            candidate.object.clone(),
            target.name(),
            child.name(),
        )
        .object_id(),
        snapshot: candidate.snapshot.clone(),
        provides: String::new(),
        version: version.map(str::to_owned),
        artifact: Some((target.name().to_owned(), String::new())),
        environment: declared_environment(&candidate.manifest, target),
        install: child.install().map(str::to_owned),
        built_by: Some((candidate.object.clone(), child.root().to_owned())),
    }
}

/// A contract label's architecture: what follows its `/`.
fn contract_arch(label: &str) -> &str {
    label.rsplit_once('/').map_or(label, |(_, arch)| arch)
}

impl Offer {
    fn rank(&self) -> u8 {
        if self.artifact.is_some() {
            0
        } else {
            1
        }
    }

    fn describe(&self) -> String {
        match &self.artifact {
            Some((target, output)) => {
                format!("object:{} target {target} output {output}", self.object)
            }
            None => format!("object:{} (content)", self.object),
        }
    }
}

/// Every held object the resolver can consider, built once per
/// resolution.
#[derive(Debug)]
pub struct Index {
    pub candidates: Vec<Candidate>,
    pub skipped: Vec<Skipped>,
}

impl Index {
    /// Walk the store: each object's current snapshot and manifest.
    /// Objects without a creator `head` branch or whose tree this node
    /// lacks are skipped, not errors.
    pub fn build(
        store: &FilesystemStore,
        manifests: &dyn ManifestSource,
    ) -> Result<Self, ResolveError> {
        let mut candidates = Vec::new();
        let mut skipped = Vec::new();
        for object in store.list_objects().map_err(ResolveError::Store)? {
            match current_snapshot(store, &object)? {
                None => skipped.push(Skipped {
                    object,
                    reason: "no head branch by its creator".to_owned(),
                }),
                Some(snapshot) => match load_manifest(manifests, &object, &snapshot)? {
                    None => skipped.push(Skipped {
                        object,
                        reason: format!(
                            "tree for revision {} is not held (kairo git fetch)",
                            snapshot.revision().as_str()
                        ),
                    }),
                    Some(manifest) => candidates.push(Candidate {
                        object,
                        snapshot: snapshot.snapshot_id(),
                        revision: snapshot.revision().clone(),
                        manifest,
                    }),
                },
            }
        }
        Ok(Self {
            candidates,
            skipped,
        })
    }

    /// Candidates offering `label`, in rank order (artifact providers
    /// first, then by object id).
    pub fn offers(&self, label: &str) -> Vec<Offer> {
        let mut offers: Vec<Offer> =
            self.candidates
                .iter()
                .flat_map(|candidate| {
                    candidate
                        .manifest
                        .provides()
                        .iter()
                        .filter(|declaration| declaration.provides() == label)
                        .map(move |declaration| Offer {
                            object: candidate.object.clone(),
                            snapshot: candidate.snapshot.clone(),
                            provides: declaration.provides().to_owned(),
                            version: declaration.version().map(str::to_owned),
                            artifact: declaration.artifact().map(|artifact| {
                                (artifact.target().to_owned(), artifact.output().to_owned())
                            }),
                            environment: declaration.artifact().and_then(|artifact| {
                                candidate.manifest.build_target(artifact.target()).and_then(
                                    |target| declared_environment(&candidate.manifest, target),
                                )
                            }),
                            install: declaration.artifact().and_then(|artifact| {
                                candidate
                                    .manifest
                                    .build_target(artifact.target())
                                    .and_then(|target| target.install().map(str::to_owned))
                            }),
                            built_by: None,
                        })
                })
                .collect();
        // Child objects' labels (§41 sub-decision 4).
        for candidate in &self.candidates {
            for target in candidate.manifest.build_targets() {
                for child in target.children() {
                    for provide in child.provides().iter().filter(|p| p.provides() == label) {
                        offers.push(child_offer(candidate, target, child, provide.version()));
                    }
                }
            }
        }
        offers.sort_by(|a, b| {
            a.rank()
                .cmp(&b.rank())
                .then_with(|| a.object.cmp(&b.object))
        });
        offers
    }

    /// The child object `id` among the candidates' current manifests: its
    /// parent, the target that makes it, and its declaration.
    pub fn child(
        &self,
        id: &ObjectId,
    ) -> Option<(
        &Candidate,
        &kairo_object::BuildTarget,
        &kairo_object::ChildDeclaration,
    )> {
        self.candidates.iter().find_map(|candidate| {
            candidate
                .manifest
                .build_targets()
                .iter()
                .find_map(|target| {
                    target
                        .children()
                        .iter()
                        .find(|child| {
                            &kairo_object::ChildGenesis::new(
                                candidate.object.clone(),
                                target.name(),
                                child.name(),
                            )
                            .object_id()
                                == id
                        })
                        .map(|child| (candidate, target, child))
                })
        })
    }

    /// Every provides label offered by a held object, with its offers.
    pub fn all_offers(&self) -> BTreeMap<String, Vec<Offer>> {
        let mut labels: Vec<&str> = self
            .candidates
            .iter()
            .flat_map(|candidate| candidate.manifest.provides().iter())
            .map(|declaration| declaration.provides())
            .chain(self.candidates.iter().flat_map(|candidate| {
                candidate
                    .manifest
                    .build_targets()
                    .iter()
                    .flat_map(|target| target.children())
                    .flat_map(|child| child.provides())
                    .map(|provide| provide.provides())
            }))
            .collect();
        labels.sort_unstable();
        labels.dedup();
        labels
            .into_iter()
            .map(|label| (label.to_owned(), self.offers(label)))
            .collect()
    }
}

/// The creator's `head` snapshot of `object`, if it has one.
pub fn current_snapshot(
    store: &FilesystemStore,
    object: &ObjectId,
) -> Result<Option<Snapshot>, ResolveError> {
    let genesis = store
        .get_object_genesis(object)
        .map_err(ResolveError::Store)?;
    let creator = genesis.body().created_by().clone();
    let Some(branch) = store
        .latest_branch(&creator, object, "head")
        .map_err(ResolveError::Store)?
    else {
        return Ok(None);
    };
    snapshot_at(store, object, branch.unsigned().body().revision()).map(Some)
}

/// The held `Snapshot` with id `snapshot`: the creator's head or one
/// of the creator's tagged versions, as the resolver would pin it.
pub fn find_snapshot(
    store: &FilesystemStore,
    object: &ObjectId,
    snapshot: &SnapshotId,
) -> Result<Option<Snapshot>, ResolveError> {
    if let Some(current) = current_snapshot(store, object)? {
        if &current.snapshot_id() == snapshot {
            return Ok(Some(current));
        }
    }
    Ok(tagged_snapshots(store, object)?
        .into_iter()
        .map(|(_, tagged)| tagged)
        .find(|tagged| &tagged.snapshot_id() == snapshot))
}

fn snapshot_at(
    store: &FilesystemStore,
    object: &ObjectId,
    revision_statement: &kairo_core::StatementId,
) -> Result<Snapshot, ResolveError> {
    let revision = store
        .get_object_revision(revision_statement)
        .map_err(ResolveError::Store)?;
    Snapshot::from_object_revision(object, &revision).map_err(ResolveError::Snapshot)
}

/// The creator's tagged snapshots of `object`, newest version first.
fn tagged_snapshots(
    store: &FilesystemStore,
    object: &ObjectId,
) -> Result<Vec<(semver::Version, Snapshot)>, ResolveError> {
    let genesis = store
        .get_object_genesis(object)
        .map_err(ResolveError::Store)?;
    let creator = genesis.body().created_by();
    let mut tagged = Vec::new();
    for head in store
        .list_version_tags(object)
        .map_err(ResolveError::Store)?
    {
        if &head.actor != creator {
            continue;
        }
        let Ok(version) = semver::Version::parse(&head.version) else {
            continue;
        };
        let Some(tag) = store
            .latest_version_tag(creator, object, &head.version)
            .map_err(ResolveError::Store)?
        else {
            continue;
        };
        let Some(target) = tag.unsigned().body().target() else {
            continue; // revoked
        };
        tagged.push((version, snapshot_at(store, object, target)?));
    }
    tagged.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(tagged)
}

fn load_manifest(
    manifests: &dyn ManifestSource,
    object: &ObjectId,
    snapshot: &Snapshot,
) -> Result<Option<ObjectManifest>, ResolveError> {
    let Some(manifest) = manifests
        .manifest(object, snapshot.revision())
        .map_err(|source| ResolveError::ManifestSource {
            object: object.clone(),
            revision: snapshot.revision().clone(),
            source,
        })?
    else {
        return Ok(None);
    };
    let actual = manifest.manifest_hash();
    if &actual != snapshot.manifest_hash() {
        return Err(ResolveError::ManifestMismatch {
            object: object.clone(),
            revision: snapshot.revision().clone(),
            expected: snapshot.manifest_hash().clone(),
            actual,
        });
    }
    Ok(Some(manifest))
}

/// A declared version like `3.81`, read leniently: missing minor or
/// patch parts are zero. `None` when it still isn't a version.
pub fn lenient_version(value: &str) -> Option<semver::Version> {
    if let Ok(version) = semver::Version::parse(value) {
        return Some(version);
    }
    let parts: Vec<&str> = value.split('.').collect();
    if parts.is_empty() || parts.len() > 3 || parts.iter().any(|part| part.is_empty()) {
        return None;
    }
    let mut padded = parts;
    while padded.len() < 3 {
        padded.push("0");
    }
    semver::Version::parse(&padded.join(".")).ok()
}

/// Whether a declared version (read leniently) satisfies a Cargo-syntax
/// requirement; no requirement is satisfied by anything.
pub fn version_matches(requirement: Option<&str>, declared: Option<&str>) -> bool {
    let Some(requirement) = requirement else {
        return true;
    };
    let (Ok(requirement), Some(declared)) = (
        semver::VersionReq::parse(requirement),
        declared.and_then(lenient_version),
    ) else {
        return false;
    };
    requirement.matches(&declared)
}

// --- the resolver --------------------------------------------------------

/// Plans builds against one store, host, and set of sources.
pub struct Resolver<'a> {
    store: &'a FilesystemStore,
    manifests: &'a dyn ManifestSource,
    artifacts: &'a dyn ArtifactSource,
    host: &'a Host,
    host_tools: &'a [HostTool],
    sources: &'a dyn SourceHolder,
    source_seeds: &'a [SourceSeed],
}

impl fmt::Debug for Resolver<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Resolver")
            .field("host", self.host)
            .field("host_tools", &self.host_tools)
            .finish_non_exhaustive()
    }
}

impl<'a> Resolver<'a> {
    pub fn new(
        store: &'a FilesystemStore,
        manifests: &'a dyn ManifestSource,
        artifacts: &'a dyn ArtifactSource,
        host: &'a Host,
        host_tools: &'a [HostTool],
    ) -> Self {
        Self {
            store,
            manifests,
            artifacts,
            host,
            host_tools,
            sources: &NoSources,
            source_seeds: &[],
        }
    }

    /// Where source dependencies are checked for.
    pub fn with_sources(mut self, sources: &'a dyn SourceHolder) -> Self {
        self.sources = sources;
        self
    }

    /// Accept these `(source, revision)` pairs as seeds: the only ones
    /// a `seed = true` dependency resolves to.
    pub fn with_source_seeds(mut self, seeds: &'a [SourceSeed]) -> Self {
        self.source_seeds = seeds;
        self
    }

    /// Index this store's objects for this resolver's manifest source.
    pub fn index(&self) -> Result<Index, ResolveError> {
        Index::build(self.store, self.manifests)
    }

    /// Plan `target` of `object` at `selector`.
    pub fn resolve(
        &self,
        object: &ObjectId,
        selector: &SnapshotSelector,
        target: &str,
    ) -> Result<Resolution, ResolveError> {
        let index = self.index()?;
        let mut diagnostics = Diagnostics {
            skipped: index.skipped.clone(),
            alternatives: Vec::new(),
        };
        let mut prerequisites: Vec<PlanDocument> = Vec::new();
        let mut stack = Vec::new();
        let plan = self.resolve_target(
            &index,
            object,
            selector,
            target,
            &mut stack,
            &mut prerequisites,
            &mut diagnostics,
        )?;
        Ok(Resolution {
            plan,
            prerequisites,
            diagnostics,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn resolve_target(
        &self,
        index: &Index,
        object: &ObjectId,
        selector: &SnapshotSelector,
        target_name: &str,
        stack: &mut Vec<(ObjectId, String)>,
        prerequisites: &mut Vec<PlanDocument>,
        diagnostics: &mut Diagnostics,
    ) -> Result<PlanDocument, ResolveError> {
        let key = (object.clone(), target_name.to_owned());
        if let Some(start) = stack.iter().position(|entry| entry == &key) {
            let mut chain: Vec<String> = stack[start..]
                .iter()
                .map(|(object, target)| format!("object:{object} target {target}"))
                .collect();
            chain.push(format!("object:{object} target {target_name}"));
            return Err(ResolveError::Cycle { chain });
        }
        stack.push(key);

        let snapshot = self.select_snapshot(object, selector)?;
        let manifest = load_manifest(self.manifests, object, &snapshot)?.ok_or_else(|| {
            ResolveError::ManifestUnavailable {
                object: object.clone(),
                revision: snapshot.revision().clone(),
            }
        })?;
        let target = manifest
            .build_target(target_name)
            .ok_or_else(|| ResolveError::NoTarget {
                object: object.clone(),
                target: target_name.to_owned(),
                declared: manifest
                    .build_targets()
                    .iter()
                    .map(|target| target.name().to_owned())
                    .collect(),
            })?;

        let mut requirements = Vec::new();
        let mut environment = "none".to_owned();
        let mut host_dependent = false;
        let mut needed_first: Vec<BuildFirst> = Vec::new();
        // Under an engine, host tools are not candidates (§35
        // sub-decision 4), so the environment is settled first.
        let consumer_environment = declared_environment(&manifest, target);
        let isolated = manifest
            .dependencies()
            .iter()
            .chain(target.dependencies())
            .any(|dependency| match dependency {
                DependencyDeclaration::Provides(dependency) => {
                    dependency.provides().starts_with(ENVIRONMENT_PREFIX)
                        && self
                            .host
                            .engines
                            .iter()
                            .any(|contract| contract == dependency.provides())
                }
                DependencyDeclaration::Object(_)
                | DependencyDeclaration::Source(_)
                | DependencyDeclaration::File(_) => false,
            });
        for dependency in manifest.dependencies().iter().chain(target.dependencies()) {
            let (requested, resolved) = match dependency {
                DependencyDeclaration::Provides(dependency) => {
                    let (path, from, overlay) = placement_fields(dependency.placement());
                    let requested = Requested::Provides {
                        provides: dependency.provides().to_owned(),
                        version: dependency.version().map(str::to_owned),
                        path,
                        from,
                        overlay,
                    };
                    let resolved = if dependency.provides().starts_with(ENVIRONMENT_PREFIX) {
                        self.resolve_environment(
                            object,
                            target_name,
                            &requested,
                            dependency.provides(),
                        )?
                    } else {
                        // Every version this target requires of the label.
                        let together: Vec<&str> = manifest
                            .dependencies()
                            .iter()
                            .chain(target.dependencies())
                            .filter_map(|other| match other {
                                DependencyDeclaration::Provides(other)
                                    if other.provides() == dependency.provides() =>
                                {
                                    other.version()
                                }
                                _ => None,
                            })
                            .collect();
                        self.resolve_provides(
                            index,
                            object,
                            target_name,
                            &requested,
                            dependency.provides(),
                            dependency.version(),
                            &together,
                            isolated,
                            consumer_environment.as_deref(),
                            stack,
                            prerequisites,
                            &mut needed_first,
                            diagnostics,
                        )?
                    };
                    (requested, resolved)
                }
                DependencyDeclaration::Object(dependency) => {
                    let (version, pinned) = match dependency.selector() {
                        ObjectDependencySelector::Version(version) => (Some(version.clone()), None),
                        ObjectDependencySelector::Snapshot(snapshot) => {
                            (None, Some(snapshot.clone()))
                        }
                        ObjectDependencySelector::Highest => (None, None),
                    };
                    let (path, from, overlay) = placement_fields(dependency.placement());
                    let requested = Requested::Object {
                        object: dependency.object().to_string(),
                        version: version.clone(),
                        snapshot: pinned.as_ref().map(ToString::to_string),
                        path,
                        from,
                        overlay,
                    };
                    // Not an object of its own: a child (§44).
                    let resolved = if self.store.get_object_genesis(dependency.object()).is_err() {
                        self.resolve_child(
                            index,
                            object,
                            target_name,
                            &requested,
                            dependency.object(),
                            version.as_deref(),
                            pinned.as_ref(),
                            stack,
                            prerequisites,
                            &mut needed_first,
                            diagnostics,
                        )?
                    } else {
                        self.resolve_object(
                            object,
                            target_name,
                            &requested,
                            dependency.object(),
                            version.as_deref(),
                            pinned.as_ref(),
                        )?
                    };
                    (requested, resolved)
                }
                DependencyDeclaration::Source(dependency) => {
                    let (path, from, overlay) = placement_fields(dependency.placement());
                    let requested = Requested::Source {
                        source: dependency.url().to_string(),
                        revision: dependency.revision().to_string(),
                        path,
                        seed: dependency.seed(),
                        from,
                        overlay,
                    };
                    let resolved =
                        self.resolve_source(object, target_name, &requested, dependency)?;
                    (requested, resolved)
                }
                DependencyDeclaration::File(dependency) => {
                    let requested = Requested::File {
                        sha256: dependency.sha256().to_string(),
                        path: dependency.path().to_owned(),
                        overlay: dependency.overlay(),
                    };
                    let source = dependency.source();
                    if !self
                        .store
                        .holds_file_source(&source)
                        .map_err(ResolveError::Store)?
                    {
                        return Err(ResolveError::Unsatisfied(Box::new(Unsatisfied {
                            object: object.clone(),
                            target: target_name.to_owned(),
                            requested,
                            considered: vec![format!(
                                "file sha256:{} is not held (kairo source fetch-file <url> --sha256 {})",
                                dependency.sha256(),
                                dependency.sha256()
                            )],
                        })));
                    }
                    let resolved = Resolved::File {
                        source: source.to_string(),
                        sha256: dependency.sha256().to_string(),
                        path: dependency.path().to_owned(),
                    };
                    (requested, resolved)
                }
            };
            match &resolved {
                Resolved::Environment {
                    contract,
                    engine: Some(_),
                } => environment = contract.clone(),
                Resolved::Environment { engine: None, .. } | Resolved::HostTool { .. } => {
                    host_dependent = true;
                }
                Resolved::Artifact { .. }
                | Resolved::Content { .. }
                | Resolved::Source { .. }
                | Resolved::File { .. } => {}
            }
            requirements.push(Requirement {
                requested,
                resolved,
            });
        }

        stack.pop();
        PlanDocument::new(ResolvedManifest {
            schema: RESOLVED_MANIFEST_SCHEMA.to_owned(),
            object: object.to_string(),
            snapshot: snapshot.snapshot_id().to_string(),
            target: target_name.to_owned(),
            environment,
            host_dependent,
            requirements,
            builds_first: needed_first,
        })
    }

    fn select_snapshot(
        &self,
        object: &ObjectId,
        selector: &SnapshotSelector,
    ) -> Result<Snapshot, ResolveError> {
        let current = current_snapshot(self.store, object)?;
        match selector {
            SnapshotSelector::Current => current.ok_or_else(|| ResolveError::NoHead {
                object: object.clone(),
            }),
            SnapshotSelector::Pinned(pinned) => {
                if let Some(current) = current {
                    if &current.snapshot_id() == pinned {
                        return Ok(current);
                    }
                }
                tagged_snapshots(self.store, object)?
                    .into_iter()
                    .map(|(_, snapshot)| snapshot)
                    .find(|snapshot| &snapshot.snapshot_id() == pinned)
                    .ok_or_else(|| ResolveError::SnapshotNotHeld {
                        object: object.clone(),
                        snapshot: pinned.clone(),
                    })
            }
        }
    }

    fn resolve_environment(
        &self,
        object: &ObjectId,
        target: &str,
        requested: &Requested,
        label: &str,
    ) -> Result<Resolved, ResolveError> {
        if self.host.engines.iter().any(|contract| contract == label) {
            return Ok(Resolved::Environment {
                contract: label.to_owned(),
                engine: Some(label.to_owned()),
            });
        }
        if label == self.host.contract() {
            return Ok(Resolved::Environment {
                contract: label.to_owned(),
                engine: None,
            });
        }
        Err(ResolveError::Unsatisfied(Box::new(Unsatisfied {
            object: object.clone(),
            target: target.to_owned(),
            requested: requested.clone(),
            considered: vec![format!(
                "host {} (development mode only)",
                self.host.contract()
            )],
        })))
    }

    #[allow(clippy::too_many_arguments)]
    fn resolve_provides(
        &self,
        index: &Index,
        object: &ObjectId,
        target: &str,
        requested: &Requested,
        label: &str,
        version: Option<&str>,
        together: &[&str],
        isolated: bool,
        consumer_environment: Option<&str>,
        stack: &mut Vec<(ObjectId, String)>,
        prerequisites: &mut Vec<PlanDocument>,
        needed_first: &mut Vec<BuildFirst>,
        diagnostics: &mut Diagnostics,
    ) -> Result<Resolved, ResolveError> {
        let mut considered = Vec::new();
        let mut matching = Vec::new();
        for offer in index.offers(label) {
            // Binaries are for one architecture (§38 sub-decision 8).
            if let (Some(wanted), Some(built)) = (consumer_environment, &offer.environment) {
                if contract_arch(wanted) != contract_arch(built) {
                    considered.push(format!(
                        "{} is built for {built}, not {}",
                        offer.describe(),
                        contract_arch(wanted)
                    ));
                    continue;
                }
            }
            if version_matches(version, offer.version.as_deref()) {
                matching.push(offer);
            } else {
                considered.push(format!(
                    "{} version {} doesn't satisfy {}",
                    offer.describe(),
                    offer.version.as_deref().unwrap_or("unknown"),
                    version.unwrap_or_default()
                ));
            }
        }
        let host_tools: Vec<&HostTool> = self
            .host_tools
            .iter()
            .filter(|tool| tool.provides == label)
            .filter(|tool| {
                if isolated {
                    considered.push(format!(
                        "host tool {} (not usable under an isolation engine)",
                        tool.path.display()
                    ));
                    false
                } else if version_matches(version, tool.version.as_deref()) {
                    true
                } else {
                    considered.push(format!(
                        "host tool {} version {} doesn't satisfy {}",
                        tool.path.display(),
                        tool.version.as_deref().unwrap_or("unknown"),
                        version.unwrap_or_default()
                    ));
                    false
                }
            })
            .collect();

        // The highest version that satisfies the requirement, and before
        // it any that satisfy every requirement this target makes of the
        // label (§43); offers without a version come last, and ties keep
        // the index's order.
        matching.sort_by_cached_key(|offer| {
            let declared = offer.version.as_deref();
            let all = together
                .iter()
                .all(|requirement| version_matches(Some(requirement), declared));
            (!all, std::cmp::Reverse(declared.and_then(lenient_version)))
        });
        let mut others: Vec<String> = matching.iter().skip(1).map(Offer::describe).collect();
        others.extend(
            host_tools
                .iter()
                .skip(usize::from(matching.is_empty()))
                .map(|tool| format!("host tool {}", tool.path.display())),
        );

        let resolved = if let Some(offer) = matching.first() {
            match &offer.artifact {
                None => Resolved::Content {
                    object: offer.object.to_string(),
                    snapshot: offer.snapshot.to_string(),
                },
                Some((provider_target, output)) => self.artifact_for(
                    index,
                    offer,
                    provider_target,
                    output,
                    stack,
                    prerequisites,
                    needed_first,
                    diagnostics,
                )?,
            }
        } else if let Some(tool) = host_tools.first() {
            Resolved::HostTool {
                provides: tool.provides.clone(),
                version: tool.version.clone(),
                path: tool.path.display().to_string(),
            }
        } else {
            return Err(ResolveError::Unsatisfied(Box::new(Unsatisfied {
                object: object.clone(),
                target: target.to_owned(),
                requested: requested.clone(),
                considered,
            })));
        };

        if !others.is_empty() {
            diagnostics.alternatives.push(Alternative {
                requested: requested.clone(),
                chosen: describe_resolved(&resolved),
                others,
            });
        }
        Ok(resolved)
    }

    /// The artifact `offer` names, built by its object or, for a child,
    /// its parent; planned first when not yet built.
    #[allow(clippy::too_many_arguments)]
    fn artifact_for(
        &self,
        index: &Index,
        offer: &Offer,
        provider_target: &str,
        output: &str,
        stack: &mut Vec<(ObjectId, String)>,
        prerequisites: &mut Vec<PlanDocument>,
        needed_first: &mut Vec<BuildFirst>,
        diagnostics: &mut Diagnostics,
    ) -> Result<Resolved, ResolveError> {
        let builder = offer
            .built_by
            .as_ref()
            .map_or(&offer.object, |(parent, _)| parent);
        let artifact = self
            .artifacts
            .artifact(builder, &offer.snapshot, provider_target)
            .map_err(|source| ResolveError::ArtifactSource {
                object: builder.clone(),
                target: provider_target.to_owned(),
                source,
            })?;
        if artifact.is_none() {
            let plan = self.resolve_target(
                index,
                builder,
                &SnapshotSelector::Pinned(offer.snapshot.clone()),
                provider_target,
                stack,
                prerequisites,
                diagnostics,
            )?;
            let own = BuildFirst {
                object: builder.to_string(),
                snapshot: offer.snapshot.to_string(),
                target: provider_target.to_owned(),
                plan: plan.hash.to_string(),
            };
            for first in plan.manifest.builds_first.iter().chain([&own]) {
                if !needed_first.contains(first) {
                    needed_first.push(first.clone());
                }
            }
            if !prerequisites
                .iter()
                .any(|existing| existing.hash == plan.hash)
            {
                prerequisites.push(plan);
            }
        }
        Ok(Resolved::Artifact {
            object: offer.object.to_string(),
            snapshot: offer.snapshot.to_string(),
            target: provider_target.to_owned(),
            output: output.to_owned(),
            artifact: artifact.map(|id| id.to_string()),
            install: offer.install.clone(),
            root: offer.built_by.as_ref().map(|(_, root)| root.clone()),
        })
    }

    /// A child object depended on by id (§41 sub-decision 4): its
    /// parent's snapshot chosen by the dependency's version or snapshot,
    /// then the child's part of the parent's artifact there.
    #[allow(clippy::too_many_arguments)]
    fn resolve_child(
        &self,
        index: &Index,
        object: &ObjectId,
        target: &str,
        requested: &Requested,
        wanted: &ObjectId,
        version: Option<&str>,
        pinned: Option<&SnapshotId>,
        stack: &mut Vec<(ObjectId, String)>,
        prerequisites: &mut Vec<PlanDocument>,
        needed_first: &mut Vec<BuildFirst>,
        diagnostics: &mut Diagnostics,
    ) -> Result<Resolved, ResolveError> {
        let unsatisfied = |considered: Vec<String>| {
            ResolveError::Unsatisfied(Box::new(Unsatisfied {
                object: object.clone(),
                target: target.to_owned(),
                requested: requested.clone(),
                considered,
            }))
        };
        // The parent: from the current manifests, or else whichever held
        // object declares the child at one of its snapshots (§44
        // sub-decision 2).
        let parents: Vec<ObjectId> = match index.child(wanted) {
            Some((candidate, _, _)) => vec![candidate.object.clone()],
            None => index
                .candidates
                .iter()
                .map(|candidate| candidate.object.clone())
                .collect(),
        };
        // Each declaration of the child, the current snapshot's first,
        // then the tagged ones', highest tag first.
        let mut considered = Vec::new();
        let mut declared: Vec<(Option<semver::Version>, Candidate, String, String)> = Vec::new();
        for parent in &parents {
            let mut snapshots: Vec<Snapshot> =
                current_snapshot(self.store, parent)?.into_iter().collect();
            for (_, tagged) in tagged_snapshots(self.store, parent)? {
                if !snapshots
                    .iter()
                    .any(|held| held.snapshot_id() == tagged.snapshot_id())
                {
                    snapshots.push(tagged);
                }
            }
            for snapshot in snapshots {
                if pinned.is_some_and(|pinned| pinned != &snapshot.snapshot_id()) {
                    continue;
                }
                let Some(manifest) = load_manifest(self.manifests, parent, &snapshot)? else {
                    continue;
                };
                let found = manifest.build_targets().iter().find_map(|target| {
                    target
                        .children()
                        .iter()
                        .find(|child| {
                            &kairo_object::ChildGenesis::new(
                                parent.clone(),
                                target.name(),
                                child.name(),
                            )
                            .object_id()
                                == wanted
                        })
                        .map(|child| {
                            (
                                target.name().to_owned(),
                                child.name().to_owned(),
                                child.version().map(str::to_owned),
                            )
                        })
                });
                let Some((target_name, child_name, child_version)) = found else {
                    continue;
                };
                if !version_matches(version, child_version.as_deref()) {
                    considered.push(format!(
                        "object:{wanted} at snapshot {} is version {}",
                        snapshot.snapshot_id(),
                        child_version.as_deref().unwrap_or("unknown")
                    ));
                    continue;
                }
                declared.push((
                    child_version.as_deref().and_then(lenient_version),
                    Candidate {
                        object: parent.clone(),
                        snapshot: snapshot.snapshot_id(),
                        revision: snapshot.revision().clone(),
                        manifest,
                    },
                    target_name,
                    child_name,
                ));
            }
        }
        // The highest version; the order above breaks ties.
        declared.sort_by(|a, b| b.0.cmp(&a.0));
        let Some((_, at, target_name, child_name)) = declared.into_iter().next() else {
            if considered.is_empty() {
                considered.push(match pinned {
                    Some(pinned) => {
                        format!("no held object declares object:{wanted} at snapshot {pinned}")
                    }
                    None => format!("object:{wanted} is not held"),
                });
            }
            return Err(unsatisfied(considered));
        };
        let (at_target, at_child) = at
            .manifest
            .build_target(&target_name)
            .and_then(|target| {
                target
                    .children()
                    .iter()
                    .find(|child| child.name() == child_name)
                    .map(|child| (target, child))
            })
            .ok_or_else(|| unsatisfied(vec![format!("object:{wanted} is not held")]))?;
        let offer = child_offer(&at, at_target, at_child, None);
        self.artifact_for(
            index,
            &offer,
            at_target.name(),
            "",
            stack,
            prerequisites,
            needed_first,
            diagnostics,
        )
    }

    fn resolve_source(
        &self,
        object: &ObjectId,
        target: &str,
        requested: &Requested,
        dependency: &kairo_object::SourceDependency,
    ) -> Result<Resolved, ResolveError> {
        let source = dependency.source();
        let url = dependency.url();
        let revision = dependency.revision();
        let unsatisfied = |reason: String| {
            ResolveError::Unsatisfied(Box::new(Unsatisfied {
                object: object.clone(),
                target: target.to_owned(),
                requested: requested.clone(),
                considered: vec![reason],
            }))
        };
        match self.store.get_source(&source) {
            Ok(_) => {}
            Err(kairo_store::StoreError::Missing) => {
                return Err(unsatisfied(format!(
                    "source {url} is not held (kairo source fetch {url})"
                )))
            }
            Err(error) => return Err(ResolveError::Store(error)),
        }
        let held = self
            .sources
            .holds(&source, revision)
            .map_err(|source_error| ResolveError::SourceHolder {
                source: source.clone(),
                revision: revision.clone(),
                error: source_error,
            })?;
        if !held {
            return Err(unsatisfied(format!(
                "{revision} of {url} is not held (kairo source fetch {url})"
            )));
        }
        if dependency.seed()
            && !self
                .source_seeds
                .iter()
                .any(|seed| seed.source == source && &seed.revision == revision)
        {
            return Err(unsatisfied(format!(
                "{url} at {revision} is not an accepted seed ([[runtime.seeds]] source and revision)"
            )));
        }
        Ok(Resolved::Source {
            source: source.to_string(),
            url: url.to_string(),
            revision: revision.to_string(),
            path: dependency.path().map(str::to_owned),
            seed: dependency.seed(),
        })
    }

    /// The snapshot of `wanted` an object dependency's version or
    /// snapshot selects.
    fn select_object_snapshot(
        &self,
        wanted: &ObjectId,
        version: Option<&str>,
        pinned: Option<&SnapshotId>,
        unsatisfied: &dyn Fn(Vec<String>) -> ResolveError,
    ) -> Result<Snapshot, ResolveError> {
        Ok(match (version, pinned) {
            (_, Some(pinned)) => self
                .select_snapshot(wanted, &SnapshotSelector::Pinned(pinned.clone()))
                .map_err(|error| match error {
                    ResolveError::SnapshotNotHeld { .. } => unsatisfied(vec![format!(
                        "snapshot {pinned} of object:{wanted} is not held"
                    )]),
                    other => other,
                })?,
            (Some(version), None) => {
                let requirement = semver::VersionReq::parse(version).map_err(|_| {
                    unsatisfied(vec![format!("{version} is not a version requirement")])
                })?;
                let tagged = tagged_snapshots(self.store, wanted)?;
                let considered: Vec<String> = tagged
                    .iter()
                    .map(|(tagged, _)| format!("object:{wanted} version {tagged}"))
                    .collect();
                tagged
                    .into_iter()
                    .find(|(tagged, _)| requirement.matches(tagged))
                    .map(|(_, snapshot)| snapshot)
                    .ok_or_else(|| unsatisfied(considered))?
            }
            // The highest version (§43); the tags come highest first.
            (None, None) => tagged_snapshots(self.store, wanted)?
                .into_iter()
                .next()
                .map(|(_, snapshot)| snapshot)
                .ok_or_else(|| {
                    unsatisfied(vec![format!(
                        "object:{wanted} has no version tags; name a snapshot"
                    )])
                })?,
        })
    }

    fn resolve_object(
        &self,
        object: &ObjectId,
        target: &str,
        requested: &Requested,
        wanted: &ObjectId,
        version: Option<&str>,
        pinned: Option<&SnapshotId>,
    ) -> Result<Resolved, ResolveError> {
        let unsatisfied = |considered: Vec<String>| {
            ResolveError::Unsatisfied(Box::new(Unsatisfied {
                object: object.clone(),
                target: target.to_owned(),
                requested: requested.clone(),
                considered,
            }))
        };
        if self
            .store
            .get_object_genesis(wanted)
            .map_err(ResolveError::Store)
            .is_err()
        {
            return Err(unsatisfied(vec![format!("object:{wanted} is not held")]));
        }
        let snapshot = self.select_object_snapshot(wanted, version, pinned, &unsatisfied)?;
        if load_manifest(self.manifests, wanted, &snapshot)?.is_none() {
            return Err(unsatisfied(vec![format!(
                "tree for revision {} of object:{wanted} is not held (kairo git fetch)",
                snapshot.revision().as_str()
            )]));
        }
        Ok(Resolved::Content {
            object: wanted.to_string(),
            snapshot: snapshot.snapshot_id().to_string(),
        })
    }
}

/// One line naming what a requirement resolved to.
pub fn describe_resolved(resolved: &Resolved) -> String {
    match resolved {
        Resolved::Artifact {
            object,
            target,
            output,
            artifact,
            root,
            ..
        } => {
            let part = match root {
                Some(root) => format!("root {root}"),
                None => format!("output {output}"),
            };
            match artifact {
                Some(artifact) => {
                    format!("object:{object} target {target} {part} (artifact {artifact})")
                }
                None => format!("object:{object} target {target} {part} (build first)"),
            }
        }
        Resolved::Content { object, .. } => format!("object:{object} (content)"),
        Resolved::Environment { contract, engine } => match engine {
            Some(engine) => format!("{contract} via engine {engine}"),
            None => format!("{contract} on this host (development mode)"),
        },
        Resolved::Source {
            url,
            revision,
            path,
            seed,
            ..
        } => {
            let mut text = format!("{url} at {revision}");
            if let Some(path) = path {
                text.push_str(&format!(" in {path}"));
            }
            if *seed {
                text.push_str(" (accepted seed)");
            }
            text
        }
        Resolved::HostTool { path, version, .. } => match version {
            Some(version) => format!("host tool {path} version {version}"),
            None => format!("host tool {path}"),
        },
        Resolved::File { sha256, path, .. } => format!("file sha256:{sha256} at {path}"),
    }
}

// --- errors ------------------------------------------------------------

/// A requirement nothing held satisfies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsatisfied {
    pub object: ObjectId,
    pub target: String,
    pub requested: Requested,
    /// What was looked at and why it didn't do.
    pub considered: Vec<String>,
}

#[derive(Debug)]
pub enum ResolveError {
    Store(StoreError),
    Snapshot(SnapshotError),
    ManifestSource {
        object: ObjectId,
        revision: RevisionId,
        source: SourceError,
    },
    /// The planned object's own tree isn't held.
    ManifestUnavailable {
        object: ObjectId,
        revision: RevisionId,
    },
    /// The tree's `kairo.toml` doesn't hash to what the revision
    /// signed.
    ManifestMismatch {
        object: ObjectId,
        revision: RevisionId,
        expected: BlobId,
        actual: BlobId,
    },
    ArtifactSource {
        object: ObjectId,
        target: String,
        source: SourceError,
    },
    SourceHolder {
        source: ObjectId,
        revision: GitRevision,
        error: SourceError,
    },
    NoHead {
        object: ObjectId,
    },
    SnapshotNotHeld {
        object: ObjectId,
        snapshot: SnapshotId,
    },
    NoTarget {
        object: ObjectId,
        target: String,
        declared: Vec<String>,
    },
    Unsatisfied(Box<Unsatisfied>),
    Cycle {
        chain: Vec<String>,
    },
    Encode(serde_json::Error),
    UnsupportedSchema(String),
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(f, "store: {error}"),
            Self::Snapshot(error) => write!(f, "snapshot: {error}"),
            Self::ManifestSource {
                object,
                revision,
                source,
            } => write!(
                f,
                "reading kairo.toml of object:{object} at {}: {source}",
                revision.as_str()
            ),
            Self::ManifestUnavailable { object, revision } => write!(
                f,
                "the tree of object:{object} at {} is not held; fetch it with kairo git fetch",
                revision.as_str()
            ),
            Self::ManifestMismatch {
                object,
                revision,
                expected,
                actual,
            } => write!(
                f,
                "kairo.toml of object:{object} at {} hashes to {actual}, but its revision signed {expected}",
                revision.as_str()
            ),
            Self::ArtifactSource {
                object,
                target,
                source,
            } => write!(
                f,
                "looking up the artifact of object:{object} target {target}: {source}"
            ),
            Self::SourceHolder {
                source,
                revision,
                error,
            } => write!(f, "checking {revision} of source {source}: {error}"),
            Self::NoHead { object } => {
                write!(f, "object:{object} has no head branch by its creator")
            }
            Self::SnapshotNotHeld { object, snapshot } => write!(
                f,
                "snapshot {snapshot} of object:{object} is neither its head nor one of its creator's tagged versions"
            ),
            Self::NoTarget {
                object,
                target,
                declared,
            } => {
                write!(f, "object:{object} declares no build target {target}")?;
                if declared.is_empty() {
                    f.write_str(" (it declares none)")
                } else {
                    write!(f, " (it declares {})", declared.join(", "))
                }
            }
            Self::Unsatisfied(unsatisfied) => {
                let Unsatisfied {
                    object,
                    target,
                    requested,
                    considered,
                } = unsatisfied.as_ref();
                write!(
                    f,
                    "object:{object} target {target} requires {requested}, which nothing held satisfies"
                )?;
                for line in considered {
                    write!(f, "\n  considered: {line}")?;
                }
                Ok(())
            }
            Self::Cycle { chain } => write!(f, "build dependencies form a cycle: {}", chain.join(" -> ")),
            Self::Encode(error) => write!(f, "encoding the resolved manifest: {error}"),
            Self::UnsupportedSchema(schema) => {
                write!(f, "unsupported resolved manifest schema {schema}")
            }
        }
    }
}

impl Error for ResolveError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Store(error) => Some(error),
            Self::Snapshot(error) => Some(error),
            Self::ManifestSource { source, .. } | Self::ArtifactSource { source, .. } => {
                Some(source)
            }
            Self::SourceHolder { error, .. } => Some(error),
            Self::Encode(error) => Some(error),
            Self::ManifestUnavailable { .. }
            | Self::ManifestMismatch { .. }
            | Self::NoHead { .. }
            | Self::SnapshotNotHeld { .. }
            | Self::NoTarget { .. }
            | Self::Unsatisfied(_)
            | Self::Cycle { .. }
            | Self::UnsupportedSchema(_) => None,
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests;
