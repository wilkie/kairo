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
//! the offers of owners this node prefers, then the highest version,
//! then artifact providers over content providers, then the newest
//! published, and only then object id (`DECISIONS.md` §48), listing
//! the losers and why as diagnostics; host tools come last.
//!
//! The resolver reads manifests through [`ManifestSource`] and build
//! results through [`ArtifactSource`], so the store stays the only
//! thing it walks: the CLI supplies the Git cache for the former, and
//! nothing yet for the latter (`BuildResult` is the next slice).
//! The plan hash covers the whole closure and never the engine (§34
//! sub-decision 4).

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::path::PathBuf;

use kairo_core::{ActorId, BlobId, ObjectId, SnapshotId, Timestamp};
use kairo_object::{
    DependencyDeclaration, ObjectDependencySelector, ObjectManifest, Snapshot, SnapshotError,
};
use kairo_statement::{GitRevision, RevisionId, TrustDecision, TrustScope};
use kairo_store::{
    BranchResolver, FilesystemStore, ObjectStore, SourceStore, StatementStore, StoreError,
    TrustResolver, VersionTagResolver,
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
/// One entry of a tree: its mode, name, and object id (§52).
pub type TreeEntry = (String, String, String);

pub trait ManifestSource {
    /// The manifest at `revision`, or `None` when this node doesn't
    /// hold the revision's tree.
    fn manifest(
        &self,
        object: &ObjectId,
        revision: &RevisionId,
    ) -> Result<Option<ObjectManifest>, SourceError>;

    /// The entries of the tree at `path` in `revision`'s tree (`"."` for
    /// the whole), each its mode, name, and object id, or `None` when
    /// this node doesn't hold the tree or `path` isn't one: what a
    /// build's tree or a content object's artifact is, for a build key
    /// (`DECISIONS.md` §52). At `"."`, `kairo.toml` among them is left
    /// out of the key, as it is out of the guest's tree.
    fn tree_entries(
        &self,
        object: &ObjectId,
        revision: &RevisionId,
        path: &str,
    ) -> Result<Option<Vec<TreeEntry>>, SourceError>;
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

    /// The plan (resolved manifest bytes) the held build of `target`
    /// at `snapshot` cites, if this node holds it (`DECISIONS.md` §46
    /// sub-decision 3). A source that answers [`Self::artifact`] but
    /// holds no plan leaves the artifact's runtime requirements unknown,
    /// and the resolver takes them as none.
    fn build_plan(
        &self,
        _object: &ObjectId,
        _snapshot: &SnapshotId,
        _target: &str,
    ) -> Result<Option<Vec<u8>>, SourceError> {
        Ok(None)
    }

    /// The artifact of a successful, accepted build of `object`'s
    /// `target` whose plan has `build_key`, at any snapshot, if held
    /// (`DECISIONS.md` §52): what any build with those inputs makes.
    fn artifact_by_key(
        &self,
        _object: &ObjectId,
        _target: &str,
        _build_key: &BlobId,
    ) -> Result<Option<BlobId>, SourceError> {
        Ok(None)
    }
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
    /// What the artifact this plan builds, and each of its children,
    /// will require at run time (`DECISIONS.md` §46 sub-decision 3):
    /// declared, or conferred by a build dependency.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<ArtifactRequirement>,
    /// The runtime closure of the build dependencies, installed in the
    /// build environment after the declared requirements (§46
    /// sub-decision 7).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtime: Vec<RuntimeRequirement>,
    /// `SOURCE_DATE_EPOCH` in the guest: the target's, or the default
    /// (`DECISIONS.md` §52). Absent from a plan made before §52, which
    /// is read as 0.
    #[serde(default)]
    pub source_date_epoch: u64,
    /// The hash of exactly what the guest sees and runs (§52): a held
    /// build with the same key is this build's artifact. Empty in a plan
    /// made before §52, which no key matches.
    #[serde(default)]
    pub build_key: String,
}

/// One provider, pinned (`DECISIONS.md` §46 sub-decision 2): an object
/// or child id at a snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    pub object: String,
    pub snapshot: String,
}

/// A runtime requirement of the artifact a plan builds (§46).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRequirement {
    /// The label; empty for a `one_of` entry.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub provides: String,
    /// For a `one_of` entry (`DECISIONS.md` §51): the alternatives, one
    /// of which a build using this artifact must request itself.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub one_of: Vec<OneOfAlternative>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pin: Option<Pin>,
    /// The child this applies to; none for the target's artifact and
    /// every child.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child: Option<String>,
    /// `declared`, or `conferred by <label> from object:<id>`.
    pub via: String,
}

/// One alternative of a `one_of` requirement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OneOfAlternative {
    pub provides: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

impl fmt::Display for OneOfAlternative {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.version {
            Some(version) => write!(f, "{} {version}", self.provides),
            None => f.write_str(&self.provides),
        }
    }
}

/// A requirement of the build environment that the runtime closure of
/// the build dependencies brought (§46 sub-decision 7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeRequirement {
    pub provides: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pin: Option<Pin>,
    /// The labels it came through, the build dependency first.
    pub via: Vec<String>,
    pub resolved: Resolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Requirement {
    pub requested: Requested,
    pub resolved: Resolved,
    /// The object that doesn't build whose dependency this is, which
    /// the build took as its own (`DECISIONS.md` §57): `object:<id>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brought_by: Option<String>,
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
        /// The objects whose offers alone may satisfy it (§59).
        #[serde(skip_serializing_if = "Vec::is_empty", default)]
        offered_by: Vec<String>,
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
        /// The build key of the plan that makes it (§52), when it could
        /// be planned here: what it is in this plan's own key.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        build_key: Option<String>,
    },
    /// A held object's own tree: its artifact, when it doesn't build
    /// (`DECISIONS.md` §50).
    Content {
        object: String,
        snapshot: String,
        /// Where it lands in the guest: its `[content] install` root, or
        /// `None` for `/kairo/content/<snapshot>`. Present only when set,
        /// so plans without it keep their hashes.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        install: Option<String>,
        /// The part of its tree that is installed, a relative path.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        root: Option<String>,
        /// What that part holds, for build keys (§52): a hash of its
        /// tree's entries, so an edit outside it changes nothing.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tree: Option<String>,
    },
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
    /// The object's creator, whose `head` branch makes the snapshot
    /// current, and so who vouches for its offers (§48).
    pub owner: ActorId,
    /// When the creator's `head` branch statement was signed.
    pub published: Timestamp,
    /// Whether the snapshot is a release, one its creator has tagged with
    /// a version (`DECISIONS.md` §61).
    pub release: bool,
}

/// Whose offers this node prefers (`DECISIONS.md` §48 sub-decision 1).
#[derive(Debug, Clone, Default)]
pub struct OwnerPreference {
    /// `--prefer`, then `[resolve] prefer`, most preferred first.
    pub preferred: Vec<ActorId>,
    /// Actors this node holds keys for.
    pub local: Vec<ActorId>,
}

impl OwnerPreference {
    /// Where `owner` ranks. An unreadable trust record leaves the owner
    /// unranked, never higher.
    pub fn tier(&self, store: &FilesystemStore, owner: &ActorId) -> OwnerTier {
        if let Some(at) = self.preferred.iter().position(|p| p == owner) {
            return OwnerTier::Preferred(at);
        }
        if self.local.contains(owner) {
            return OwnerTier::Local;
        }
        let trusted = self.local.iter().any(|by_actor| {
            store
                .latest_trust(by_actor, owner, &TrustScope::general())
                .ok()
                .flatten()
                .is_some_and(|trust| {
                    trust.unsigned().body().decision() == Some(TrustDecision::Trusted)
                })
        });
        if trusted {
            OwnerTier::Trusted
        } else {
            OwnerTier::Other
        }
    }

    /// `offers`, from [`Index::offers`], in the order the resolver
    /// tries them: the owner's tier, then the highest version, offers
    /// without one last; ties keep the index's order (§43, §48).
    pub fn rank(&self, store: &FilesystemStore, offers: &mut [Offer]) {
        offers.sort_by_cached_key(|offer| {
            (
                self.tier(store, &offer.owner),
                std::cmp::Reverse(offer.version.as_deref().and_then(lenient_version)),
            )
        });
    }
}

static NO_PREFERENCE: OwnerPreference = OwnerPreference {
    preferred: Vec::new(),
    local: Vec::new(),
};

/// Where an offer's owner ranks, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OwnerTier {
    /// Named by `--prefer` or `[resolve] prefer`, at this position.
    Preferred(usize),
    /// An actor this node holds keys for.
    Local,
    /// An actor a local one trusts at `general` scope.
    Trusted,
    Other,
}

impl fmt::Display for OwnerTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Preferred(at) => write!(f, "preferred #{}", at + 1),
            Self::Local => f.write_str("local"),
            Self::Trusted => f.write_str("trusted"),
            Self::Other => f.write_str("unranked"),
        }
    }
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
    /// A child offer's name, for its own entries in the parent's plan.
    pub child_name: Option<String>,
    /// For a content offer with a `[content] root`: the part of its tree
    /// that is its artifact (`DECISIONS.md` §50).
    pub content_root: Option<String>,
    /// What resolving this label confers on the consumer's artifact
    /// (`DECISIONS.md` §46 sub-decision 2): labels, each with a version
    /// requirement or, without one, pinned to this offer's own build.
    pub confers: Vec<(String, Option<String>)>,
    /// Whether other versions of the label can be installed beside this
    /// one at the same root (§46 sub-decision 5).
    pub coexists: bool,
    /// Its object's owner, a child's being its parent's (§48).
    pub owner: ActorId,
    /// When its owner made the snapshot current.
    pub published: Timestamp,
    /// Whether its snapshot is a release, tagged with a version by its
    /// owner: only a release satisfies a requirement that doesn't name
    /// its object (§61).
    pub release: bool,
}

impl Offer {
    /// The object whose build makes this offer's artifact: the offer's
    /// own, or a child's parent.
    fn builder(&self) -> &ObjectId {
        self.built_by
            .as_ref()
            .map_or(&self.object, |(parent, _)| parent)
    }
}

/// A declared `requires` entry as a plan records it.
fn declared_requirement(
    required: &kairo_object::Requirement,
    child: Option<&str>,
) -> ArtifactRequirement {
    let (provides, version, one_of) = match required {
        kairo_object::Requirement::Label(label) => (
            label.provides().to_owned(),
            label.version().map(str::to_owned),
            Vec::new(),
        ),
        kairo_object::Requirement::OneOf(alternatives) => (
            String::new(),
            None,
            alternatives
                .iter()
                .map(|alternative| OneOfAlternative {
                    provides: alternative.provides().to_owned(),
                    version: alternative.version().map(str::to_owned),
                })
                .collect(),
        ),
    };
    ArtifactRequirement {
        provides,
        one_of,
        version,
        pin: None,
        child: child.map(str::to_owned),
        via: "declared".to_owned(),
    }
}

/// Domain separator for a plan's build key (`DECISIONS.md` §52).
pub const BUILD_KEY_DOMAIN: &[u8] = b"kairo.build-key.v1";

/// The version of the rules a build's outputs are collected by, in its
/// key (`DECISIONS.md` §54 sub-decision 8): 2 keeps modes, directories,
/// and hard links; a key without it was collected by 1.
pub const COLLECT_RULES: u32 = 2;
/// Domain separator for what a tree holds, in a build key (§52).
pub const TREE_IDENTITY_DOMAIN: &[u8] = b"kairo.tree-identity.v1";

/// What a resolved requirement is in a build key: by what the guest
/// gets, not by which object or snapshot it came from where content
/// says that. An artifact is the build key of the plan that makes it
/// (or, when that plan couldn't be made here, its id), with the part
/// and install root used.
fn key_identity(resolved: &Resolved) -> Result<serde_json::Value, ResolveError> {
    Ok(match resolved {
        Resolved::Artifact {
            output,
            artifact,
            install,
            root,
            build_key,
            ..
        } => serde_json::json!({
            "kind": "artifact",
            "built": build_key.as_ref().or(artifact.as_ref()),
            "output": output,
            "install": install,
            "root": root,
        }),
        // By what it installs, not which snapshot it came from.
        Resolved::Content { install, tree, .. } => serde_json::json!({
            "kind": "content",
            "tree": tree,
            "install": install,
        }),
        other => serde_json::to_value(other).map_err(ResolveError::Encode)?,
    })
}

/// The hash of exactly what a build's guest sees and runs
/// (`DECISIONS.md` §52): the target's command, workdir, and what it
/// collects; its environment; each requirement as requested and what it
/// resolved to; the runtime closure; its own tree but `kairo.toml`; and
/// `SOURCE_DATE_EPOCH`. Metadata (summary, tags, versions, children,
/// what the artifact requires or confers) isn't in it.
fn build_key(
    target: &kairo_object::BuildTarget,
    environment: &str,
    host_dependent: bool,
    requirements: &[Requirement],
    runtime: &[RuntimeRequirement],
    tree: &str,
    source_date_epoch: u64,
) -> Result<BlobId, ResolveError> {
    use kairo_core::canonical::CanonicalEncode as _;
    let outputs: Vec<String> = target
        .outputs()
        .iter()
        .map(|output| {
            let mut bytes = Vec::new();
            output.encode_canonical(&mut bytes);
            bytes.iter().map(|byte| format!("{byte:02x}")).collect()
        })
        .collect();
    let requirements = requirements
        .iter()
        .map(|requirement| {
            Ok(serde_json::json!({
                "requested": requirement.requested,
                "resolved": key_identity(&requirement.resolved)?,
            }))
        })
        .collect::<Result<Vec<_>, ResolveError>>()?;
    let runtime = runtime
        .iter()
        .map(|entry| {
            Ok(serde_json::json!({
                "provides": entry.provides,
                "resolved": key_identity(&entry.resolved)?,
            }))
        })
        .collect::<Result<Vec<_>, ResolveError>>()?;
    let key = serde_json::json!({
        "command": target.command(),
        "workdir": target.workdir(),
        "artifact_root": target.artifact_root(),
        "outputs": outputs,
        "environment": environment,
        "host_dependent": host_dependent,
        "requirements": requirements,
        "runtime": runtime,
        "tree": tree,
        "source_date_epoch": source_date_epoch,
        "collect": COLLECT_RULES,
    });
    let bytes = serde_json::to_vec(&key).map_err(ResolveError::Encode)?;
    Ok(BlobId::from_bytes(BUILD_KEY_DOMAIN, &bytes))
}

/// A pin read from a plan, as the ids it names.
fn held_pin(pin: Option<Pin>) -> Result<Option<(ObjectId, SnapshotId)>, ResolveError> {
    let Some(pin) = pin else {
        return Ok(None);
    };
    Ok(Some((
        ObjectId::new(pin.object).map_err(|_| {
            ResolveError::UnsupportedSchema("a pin in a held plan isn't an object id".to_owned())
        })?,
        SnapshotId::new(pin.snapshot).map_err(|_| {
            ResolveError::UnsupportedSchema("a pin in a held plan isn't a snapshot id".to_owned())
        })?,
    )))
}

/// Where an object's own tree is installed and which part of it
/// (`DECISIONS.md` §50): its `[content] install` and `root`, each `None`
/// when not given (`/kairo/content/<snapshot>`, the whole tree).
fn content_placement(manifest: &ObjectManifest) -> (Option<String>, Option<String>) {
    match manifest.content() {
        Some(content) => (
            content.install().map(str::to_owned),
            (content.root() != ".").then(|| content.root().to_owned()),
        ),
        None => (None, None),
    }
}

/// The first environment contract among `dependencies`.
fn environment_in(dependencies: &[DependencyDeclaration]) -> Option<String> {
    dependencies.iter().find_map(|dependency| match dependency {
        DependencyDeclaration::Provides(dependency)
            if dependency.provides().starts_with(ENVIRONMENT_PREFIX) =>
        {
            Some(dependency.provides().to_owned())
        }
        _ => None,
    })
}

/// The environment a target's artifacts run in (`DECISIONS.md` §53): the
/// object's own, its top-level environment dependency, or else the
/// target's. Offers are matched to consumers by it.
pub fn artifact_environment(
    manifest: &ObjectManifest,
    target: &kairo_object::BuildTarget,
) -> Option<String> {
    environment_in(manifest.dependencies()).or_else(|| environment_in(target.dependencies()))
}

/// The environment a target is built in (`DECISIONS.md` §53): its own
/// environment dependency, which overrides the object's, or else the
/// object's. The build's engine and its dependencies' architecture
/// follow it.
pub fn build_environment(
    manifest: &ObjectManifest,
    target: &kairo_object::BuildTarget,
) -> Option<String> {
    environment_in(target.dependencies()).or_else(|| environment_in(manifest.dependencies()))
}

/// What a target's build requires: the object's dependencies and the
/// target's, but the object's environment when the target names its
/// own build environment instead (§53).
fn build_dependencies<'a>(
    manifest: &'a ObjectManifest,
    target: &'a kairo_object::BuildTarget,
) -> impl Iterator<Item = &'a DependencyDeclaration> {
    let overridden = environment_in(target.dependencies()).is_some();
    manifest
        .dependencies()
        .iter()
        .filter(move |dependency| {
            !(overridden
                && matches!(dependency, DependencyDeclaration::Provides(dependency)
                    if dependency.provides().starts_with(ENVIRONMENT_PREFIX)))
        })
        .chain(target.dependencies())
}

/// What an object that doesn't build brings to a build that depends on
/// it (`DECISIONS.md` §57): its own dependencies, and who brought them.
#[derive(Debug, Clone)]
struct Brought {
    by: String,
    dependencies: Vec<DependencyDeclaration>,
}

/// The dependency a brought list is filed under: a label at its version
/// requirement, or an object.
fn brought_key(dependency: &DependencyDeclaration) -> String {
    match dependency {
        DependencyDeclaration::Provides(provides) => format!(
            "provides {} {}",
            provides.provides(),
            provides.version().unwrap_or("")
        ),
        DependencyDeclaration::Object(object) => format!("object {}", object.object()),
        DependencyDeclaration::Source(source) => format!("source {}", source.url()),
        DependencyDeclaration::File(file) => format!("file {}", file.sha256()),
    }
}

/// What `by`, an object chosen for a build's dependency, brings it: its
/// dependencies, when it doesn't build and has any (§57). They are
/// labels and objects; an environment would change where the build
/// runs, and a file or source where its tree is, which an object that
/// doesn't build can't decide for the builds that use it.
fn brings(
    object: &ObjectId,
    target: &str,
    by: &ObjectId,
    manifest: &ObjectManifest,
) -> Result<Option<Brought>, ResolveError> {
    if !manifest.build_targets().is_empty() || manifest.dependencies().is_empty() {
        return Ok(None);
    }
    for dependency in manifest.dependencies() {
        let unsupported = match dependency {
            DependencyDeclaration::Provides(provides)
                if provides.provides().starts_with(ENVIRONMENT_PREFIX) =>
            {
                Some(provides.provides().to_owned())
            }
            DependencyDeclaration::Provides(_) | DependencyDeclaration::Object(_) => None,
            DependencyDeclaration::Source(source) => Some(format!("source {}", source.url())),
            DependencyDeclaration::File(file) => Some(format!("file sha256:{}", file.sha256())),
        };
        if let Some(dependency) = unsupported {
            return Err(ResolveError::BroughtUnsupported {
                object: object.clone(),
                target: target.to_owned(),
                by: by.clone(),
                dependency,
            });
        }
    }
    Ok(Some(Brought {
        by: format!("object:{by}"),
        dependencies: manifest.dependencies().to_vec(),
    }))
}

/// A build's dependencies with what objects that don't build brought
/// spliced in after each (§57), each brought one with who brought it.
/// A label the build declares itself replaces any brought requirement
/// on it, and an object brought twice brings its dependencies once.
fn effective_dependencies<'a>(
    declared: impl Iterator<Item = &'a DependencyDeclaration>,
    brought: &BTreeMap<String, Brought>,
) -> Vec<(DependencyDeclaration, Option<String>)> {
    fn splice(
        dependency: &DependencyDeclaration,
        brought: &BTreeMap<String, Brought>,
        own: &BTreeSet<String>,
        taken: &mut BTreeSet<String>,
        out: &mut Vec<(DependencyDeclaration, Option<String>)>,
    ) {
        let key = brought_key(dependency);
        let Some(bringing) = brought.get(&key) else {
            return;
        };
        if !taken.insert(key) {
            return;
        }
        for more in &bringing.dependencies {
            if let DependencyDeclaration::Provides(provides) = more {
                if own.contains(provides.provides()) {
                    continue;
                }
            }
            out.push((more.clone(), Some(bringing.by.clone())));
            splice(more, brought, own, taken, out);
        }
    }
    let declared: Vec<&DependencyDeclaration> = declared.collect();
    let own: BTreeSet<String> = declared
        .iter()
        .filter_map(|dependency| match dependency {
            DependencyDeclaration::Provides(provides) => Some(provides.provides().to_owned()),
            _ => None,
        })
        .collect();
    let mut taken = BTreeSet::new();
    let mut out = Vec::new();
    for dependency in declared {
        out.push((dependency.clone(), None));
        splice(dependency, brought, &own, &mut taken, &mut out);
    }
    out
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
    provide: Option<&kairo_object::ChildProvide>,
) -> Offer {
    Offer {
        object: kairo_object::ChildGenesis::new(
            candidate.object.clone(),
            target.name(),
            child.name(),
        )
        .object_id(),
        snapshot: candidate.snapshot.clone(),
        provides: provide.map(|p| p.provides().to_owned()).unwrap_or_default(),
        version: provide
            .and_then(|p| p.version())
            .or(child.version())
            .map(str::to_owned),
        artifact: Some((target.name().to_owned(), String::new())),
        environment: artifact_environment(&candidate.manifest, target),
        install: child.install().map(str::to_owned),
        built_by: Some((candidate.object.clone(), child.root().to_owned())),
        child_name: Some(child.name().to_owned()),
        content_root: None,
        confers: provide
            .map(|p| {
                p.confers()
                    .iter()
                    .map(|c| (c.provides().to_owned(), c.version().map(str::to_owned)))
                    .collect()
            })
            .unwrap_or_default(),
        coexists: provide.is_some_and(kairo_object::ChildProvide::coexists),
        owner: candidate.owner.clone(),
        published: candidate.published,
        release: candidate.release,
    }
}

/// One requirement on a label in a plan's closure (`DECISIONS.md` §46
/// sub-decision 4).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Constraint {
    version: Option<String>,
    pin: Option<(ObjectId, SnapshotId)>,
    /// The labels it came through, the build dependency first; empty
    /// for the target's own requirement.
    via: Vec<String>,
    /// The objects whose offers alone satisfy it, each an offer's own or
    /// its parent's (§59); empty for any.
    offered_by: Vec<ObjectId>,
}

impl Constraint {
    /// Whether `offer` satisfies it: a release, or else an offer of an
    /// object it names (§61), at a version and from an object it allows.
    fn satisfied_by(&self, offer: &Offer) -> bool {
        self.admits(offer) && (offer.release || self.names(offer))
    }

    /// Whether `offer` satisfies it but for being a release: as an offer
    /// the build already installs does, which the build chose itself.
    fn admits(&self, offer: &Offer) -> bool {
        self.pin
            .as_ref()
            .is_none_or(|(object, snapshot)| &offer.object == object && &offer.snapshot == snapshot)
            && version_matches(self.version.as_deref(), offer.version.as_deref())
            && (self.offered_by.is_empty() || self.names(offer))
    }

    /// Whether it names `offer`'s object, or for a child its parent.
    fn names(&self, offer: &Offer) -> bool {
        self.offered_by.contains(&offer.object) || self.offered_by.contains(offer.builder())
    }

    fn describe(&self, label: &str) -> String {
        let mut what = match (&self.pin, &self.version) {
            (Some((object, snapshot)), _) => {
                format!("{label} pinned to object:{object} at snapshot {snapshot}")
            }
            (None, Some(version)) => format!("{label} {version}"),
            (None, None) => format!("{label} (any version)"),
        };
        if !self.offered_by.is_empty() {
            let objects: Vec<String> = self
                .offered_by
                .iter()
                .map(|object| format!("object:{object}"))
                .collect();
            what = format!("{what} offered by {}", objects.join(" or "));
        }
        if self.via.is_empty() {
            format!("{what}, required by this target")
        } else {
            format!("{what}, required via {}", self.via.join(" > "))
        }
    }
}

/// Which of its own object's offers a target's build may use
/// (`DECISIONS.md` §59): another target's of the same snapshot, which is
/// a build of its own, as stage 4 builds on stage 3's children; and of
/// another snapshot, only an older generation's, a version-tagged
/// snapshot whose revision is an ancestor of the one being built. Never
/// its own target's, nor a newer or unrelated tag's, which would be built
/// with it in turn.
struct Generation {
    object: ObjectId,
    snapshot: SnapshotId,
    target: String,
    older: BTreeSet<SnapshotId>,
}

impl Generation {
    fn of(
        store: &FilesystemStore,
        index: &Index,
        snapshot: &Snapshot,
        target: &str,
    ) -> Result<Self, ResolveError> {
        let object = snapshot.object().clone();
        let ancestors: BTreeSet<String> = store
            .revision_log(&object, snapshot.frontier(), 0, usize::MAX)
            .map_err(ResolveError::Store)?
            .revisions
            .iter()
            .map(|revision| revision.unsigned().body().revision())
            .filter(|revision| *revision != snapshot.revision())
            .map(|revision| revision.as_str().to_owned())
            .collect();
        let older = index
            .candidates
            .iter()
            .chain(&index.tagged)
            .filter(|candidate| {
                candidate.object == object && ancestors.contains(candidate.revision.as_str())
            })
            .map(|candidate| candidate.snapshot.clone())
            .collect();
        Ok(Self {
            object,
            snapshot: snapshot.snapshot_id(),
            target: target.to_owned(),
            older,
        })
    }

    /// Whether `offer` is barred from this build: its own target's, or
    /// another snapshot's of its object that isn't an older generation.
    fn bars(&self, offer: &Offer) -> bool {
        if offer.builder() != &self.object {
            return false;
        }
        if offer.snapshot == self.snapshot {
            return offer
                .artifact
                .as_ref()
                .is_none_or(|(target, _)| target == &self.target);
        }
        !self.older.contains(&offer.snapshot)
    }
}

/// What a label resolved to: a held offer, or a development-mode host
/// tool.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Picked {
    Offer(Box<Offer>),
    HostTool(HostTool),
}

/// One offer (or host tool) chosen for a label, and which of the
/// label's constraints it serves.
#[derive(Debug, Clone)]
struct Choice {
    picked: Picked,
    serves: Vec<usize>,
}

/// Why a label's constraints couldn't be met by offers that co-exist.
struct Conflict {
    label: String,
    unmet: Vec<String>,
    considered: Vec<String>,
}

/// How many times the declared requirements are re-chosen under what
/// the runtime closure asks of them, and how many rounds the closure
/// gets to settle, before the resolver gives up (§46 sub-decision 4).
const CLOSURE_PASSES: usize = 8;
/// How deep objects that don't build may bring one another's
/// dependencies (§57).
const BROUGHT_DEPTH: usize = 16;
const CLOSURE_ROUNDS: usize = 32;

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
    /// Each candidate's other version-tagged snapshots, by its creator,
    /// which offer their labels too, as older generations (§59).
    pub tagged: Vec<Candidate>,
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
        let mut tagged = Vec::new();
        let mut skipped = Vec::new();
        for object in store.list_objects().map_err(ResolveError::Store)? {
            match current_head(store, &object)? {
                None => skipped.push(Skipped {
                    object,
                    reason: "no head branch by its creator".to_owned(),
                }),
                Some((snapshot, owner, published)) => {
                    match load_manifest(manifests, &object, &snapshot)? {
                        None => skipped.push(Skipped {
                            object,
                            reason: format!(
                                "tree for revision {} is not held (kairo git fetch)",
                                snapshot.revision().as_str()
                            ),
                        }),
                        Some(manifest) => {
                            // Its older generations, those of its tagged
                            // snapshots this node holds the trees of,
                            // vouched for as its head is; the head is a
                            // release when it is tagged too (§61).
                            let mut head_release = false;
                            for (_, older) in tagged_snapshots(store, &object)? {
                                if older.snapshot_id() == snapshot.snapshot_id() {
                                    head_release = true;
                                    continue;
                                }
                                if let Some(manifest) = load_manifest(manifests, &object, &older)? {
                                    tagged.push(Candidate {
                                        object: object.clone(),
                                        snapshot: older.snapshot_id(),
                                        revision: older.revision().clone(),
                                        manifest,
                                        owner: owner.clone(),
                                        published,
                                        release: true,
                                    });
                                }
                            }
                            candidates.push(Candidate {
                                object,
                                snapshot: snapshot.snapshot_id(),
                                revision: snapshot.revision().clone(),
                                manifest,
                                owner,
                                published,
                                release: head_release,
                            });
                        }
                    }
                }
            }
        }
        Ok(Self {
            candidates,
            tagged,
            skipped,
        })
    }

    /// Candidates offering `label`, in the order that breaks ties after
    /// owner and version (§48): artifact providers first, then the
    /// newest published, then by object id.
    pub fn offers(&self, label: &str) -> Vec<Offer> {
        let mut offers: Vec<Offer> =
            self.candidates
                .iter()
                .chain(&self.tagged)
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
                                    |target| artifact_environment(&candidate.manifest, target),
                                )
                            }),
                            install: match declaration.artifact() {
                                Some(artifact) => candidate
                                    .manifest
                                    .build_target(artifact.target())
                                    .and_then(|target| target.install().map(str::to_owned)),
                                None => content_placement(&candidate.manifest).0,
                            },
                            built_by: None,
                            child_name: None,
                            content_root: match declaration.artifact() {
                                Some(_) => None,
                                None => content_placement(&candidate.manifest).1,
                            },
                            confers: declaration
                                .confers()
                                .iter()
                                .map(|c| (c.provides().to_owned(), c.version().map(str::to_owned)))
                                .collect(),
                            coexists: declaration.coexists(),
                            owner: candidate.owner.clone(),
                            published: candidate.published,
                            release: candidate.release,
                        })
                })
                .collect();
        // Child objects' labels (§41 sub-decision 4).
        for candidate in self.candidates.iter().chain(&self.tagged) {
            for target in candidate.manifest.build_targets() {
                for child in target.children() {
                    for provide in child.provides().iter().filter(|p| p.provides() == label) {
                        offers.push(child_offer(candidate, target, child, Some(provide)));
                    }
                }
            }
        }
        offers.sort_by(|a, b| {
            a.rank()
                .cmp(&b.rank())
                .then_with(|| b.published.cmp(&a.published))
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
            .chain(&self.tagged)
            .flat_map(|candidate| candidate.manifest.provides().iter())
            .map(|declaration| declaration.provides())
            .chain(
                self.candidates
                    .iter()
                    .chain(&self.tagged)
                    .flat_map(|candidate| {
                        candidate
                            .manifest
                            .build_targets()
                            .iter()
                            .flat_map(|target| target.children())
                            .flat_map(|child| child.provides())
                            .map(|provide| provide.provides())
                    }),
            )
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
    Ok(current_head(store, object)?.map(|(snapshot, _, _)| snapshot))
}

/// The creator's `head` snapshot of `object`, with the creator and when
/// the branch statement making it current was signed.
pub fn current_head(
    store: &FilesystemStore,
    object: &ObjectId,
) -> Result<Option<(Snapshot, ActorId, Timestamp)>, ResolveError> {
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
    let snapshot = snapshot_at(store, object, branch.unsigned().body().revision())?;
    Ok(Some((snapshot, creator, branch.unsigned().created_at())))
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
    owners: &'a OwnerPreference,
}

/// What the runtime closure settled to, or why the declared
/// requirements must be re-chosen first.
enum Closure {
    Converged(Vec<(String, Picked, Vec<String>)>),
    Reconsider(BTreeMap<String, Vec<Constraint>>),
}

impl Conflict {
    /// For a target's own requirement: unsatisfied when nothing asked
    /// more of the label than the target did, a conflict otherwise.
    fn into_error(
        self,
        object: &ObjectId,
        target: &str,
        requested: &Requested,
        constraints: &[Constraint],
    ) -> ResolveError {
        if constraints.len() <= 1 {
            return ResolveError::Unsatisfied(Box::new(Unsatisfied {
                object: object.clone(),
                target: target.to_owned(),
                requested: requested.clone(),
                considered: self.considered,
            }));
        }
        self.into_runtime_error(object, target)
    }

    fn into_runtime_error(self, object: &ObjectId, target: &str) -> ResolveError {
        ResolveError::Conflict {
            object: object.clone(),
            target: target.to_owned(),
            label: self.label,
            unmet: self.unmet,
            considered: self.considered,
        }
    }
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
            owners: &NO_PREFERENCE,
        }
    }

    /// Rank offers by these owners first (§48); without it every owner
    /// ranks alike.
    pub fn with_owner_preference(mut self, owners: &'a OwnerPreference) -> Self {
        self.owners = owners;
        self
    }

    /// Where `owner` ranks under this resolver's preference.
    pub fn owner_tier(&self, owner: &ActorId) -> OwnerTier {
        self.owners.tier(self.store, owner)
    }

    /// `offers` in the order the resolver tries them: the owner's tier
    /// first, then the highest version, offers without one last; ties
    /// keep the index's order (§43, §48).
    fn rank_offers(&self, offers: &mut [Offer]) {
        self.owners.rank(self.store, offers);
    }

    /// Why `offer` lost to `chosen` (§48 sub-decision 4): the first rank
    /// it falls behind on.
    fn passed_over(&self, offer: &Offer, chosen: Option<&Offer>) -> String {
        let tier = self.owner_tier(&offer.owner);
        let version = offer.version.as_deref().unwrap_or("unknown");
        let described = format!(
            "{} version {version}, owner actor:{} ({tier})",
            offer.describe(),
            offer.owner
        );
        let Some(chosen) = chosen else {
            return described;
        };
        let lenient = |offer: &Offer| offer.version.as_deref().and_then(lenient_version);
        let reason = if tier != self.owner_tier(&chosen.owner) {
            "its owner ranks lower"
        } else if lenient(offer) != lenient(chosen) {
            "a lower version"
        } else if offer.rank() != chosen.rank() {
            "content, not an artifact"
        } else if offer.published != chosen.published {
            "published earlier"
        } else if offer.object != chosen.object {
            "after it by object id"
        } else {
            "another of its labels was chosen"
        };
        format!("{described}: {reason}")
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
        // Only the plans the final plan needs first, transitively: a
        // choice the runtime closure reconsidered may have planned
        // builds nothing now uses (§46).
        let mut wanted: Vec<String> = plan
            .manifest
            .builds_first
            .iter()
            .map(|first| first.plan.clone())
            .collect();
        let mut at = 0;
        while at < wanted.len() {
            if let Some(found) = prerequisites
                .iter()
                .find(|existing| existing.hash.to_string() == wanted[at])
            {
                for first in &found.manifest.builds_first {
                    if !wanted.contains(&first.plan) {
                        wanted.push(first.plan.clone());
                    }
                }
            }
            at += 1;
        }
        prerequisites.retain(|existing| wanted.contains(&existing.hash.to_string()));
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
        stack: &mut Vec<(ObjectId, SnapshotId, String)>,
        prerequisites: &mut Vec<PlanDocument>,
        diagnostics: &mut Diagnostics,
    ) -> Result<PlanDocument, ResolveError> {
        // A build is its object's target at one snapshot: another
        // snapshot's is another generation, and may be built for it (§59).
        let snapshot = self.select_snapshot(object, selector)?;
        let key = (
            object.clone(),
            snapshot.snapshot_id(),
            target_name.to_owned(),
        );
        if let Some(start) = stack.iter().position(|entry| entry == &key) {
            let mut chain: Vec<String> = stack[start..]
                .iter()
                .map(|(object, _, target)| format!("object:{object} target {target}"))
                .collect();
            chain.push(format!("object:{object} target {target_name}"));
            return Err(ResolveError::Cycle { chain });
        }
        stack.push(key);

        let manifest = load_manifest(self.manifests, object, &snapshot)?.ok_or_else(|| {
            ResolveError::ManifestUnavailable {
                object: object.clone(),
                revision: snapshot.revision().clone(),
            }
        })?;
        let generation = Generation::of(self.store, index, &snapshot, target_name)?;
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

        let alternatives_before = diagnostics.alternatives.len();
        let mut extras: BTreeMap<String, Vec<Constraint>> = BTreeMap::new();
        let mut passes = 0;
        // What each object that doesn't build, chosen for one of this
        // build's dependencies, brings: its own dependencies, which this
        // build takes as its own (§57). Found in one pass, taken in the
        // next.
        let mut brought: BTreeMap<String, Brought> = BTreeMap::new();
        let mut expansions = 0;
        let (requirements, environment, host_dependent, needed_first, requires, runtime) = loop {
            diagnostics.alternatives.truncate(alternatives_before);
            let effective = effective_dependencies(build_dependencies(&manifest, target), &brought);
            let mut found = false;
            let mut requirements = Vec::new();
            let mut environment = "none".to_owned();
            let mut host_dependent = false;
            let mut needed_first: Vec<BuildFirst> = Vec::new();
            // What this target's artifact and children require at run time
            // (§46 sub-decision 3): declared here, conferred below.
            let mut requires: Vec<ArtifactRequirement> = target
                .requires()
                .iter()
                .map(|required| declared_requirement(required, None))
                .chain(target.children().iter().flat_map(|child| {
                    child
                        .requires()
                        .iter()
                        .map(move |required| declared_requirement(required, Some(child.name())))
                }))
                .collect();
            // Every choice made for a label, and the co-existing extra ones
            // that are installed beyond the declared requirement.
            let mut chosen: BTreeMap<String, Vec<Picked>> = BTreeMap::new();
            let mut seeded: Vec<(String, Picked, Vec<String>)> = Vec::new();
            let mut seen_labels: BTreeMap<String, usize> = BTreeMap::new();
            // Under an engine, host tools are not candidates (§35
            // sub-decision 4), so the environment is settled first.
            // The build's environment, which may not be the one its
            // artifacts run in (§53).
            let consumer_environment = build_environment(&manifest, target);
            let isolated = consumer_environment
                .as_ref()
                .is_some_and(|contract| self.host.engines.contains(contract));
            for (declaration, brought_by) in &effective {
                let (requested, resolved) = match declaration {
                    DependencyDeclaration::Provides(dependency) => {
                        let (path, from, overlay) = placement_fields(dependency.placement());
                        let requested = Requested::Provides {
                            provides: dependency.provides().to_owned(),
                            version: dependency.version().map(str::to_owned),
                            path,
                            from,
                            overlay,
                            offered_by: dependency
                                .offered_by()
                                .iter()
                                .map(ToString::to_string)
                                .collect(),
                        };
                        let resolved = if dependency.provides().starts_with(ENVIRONMENT_PREFIX) {
                            self.resolve_environment(
                                object,
                                target_name,
                                &requested,
                                dependency.provides(),
                            )?
                        } else {
                            // Every requirement on the label: this target's own,
                            // then what the runtime closure asked of it (§46).
                            let label = dependency.provides();
                            let own: Vec<Constraint> = effective
                                .iter()
                                .map(|(other, _)| other)
                                .filter_map(|other| match other {
                                    DependencyDeclaration::Provides(other)
                                        if other.provides() == label =>
                                    {
                                        Some(Constraint {
                                            version: other.version().map(str::to_owned),
                                            pin: None,
                                            via: Vec::new(),
                                            offered_by: other.offered_by().to_vec(),
                                        })
                                    }
                                    _ => None,
                                })
                                .collect();
                            let mine = *seen_labels.get(label).unwrap_or(&0);
                            seen_labels.insert(label.to_owned(), mine + 1);
                            let own_count = own.len();
                            let mut constraints = own;
                            constraints.extend(extras.get(label).cloned().unwrap_or_default());
                            let (resolved, choices) = self.resolve_provides(
                                index,
                                object,
                                target_name,
                                &requested,
                                label,
                                &constraints,
                                mine,
                                isolated,
                                consumer_environment.as_deref(),
                                &generation,
                                stack,
                                prerequisites,
                                &mut needed_first,
                                diagnostics,
                            )?;
                            if let Some(offer) =
                                choices.iter().find_map(|choice| match &choice.picked {
                                    Picked::Offer(offer) if choice.serves.contains(&mine) => {
                                        Some(offer)
                                    }
                                    _ => None,
                                })
                            {
                                let key = brought_key(declaration);
                                if offer.artifact.is_none() && !brought.contains_key(&key) {
                                    if let Some(candidate) =
                                        index.candidates.iter().chain(&index.tagged).find(|c| {
                                            c.object == offer.object && c.snapshot == offer.snapshot
                                        })
                                    {
                                        if let Some(more) = brings(
                                            object,
                                            target_name,
                                            &candidate.object,
                                            &candidate.manifest,
                                        )? {
                                            brought.insert(key, more);
                                            found = true;
                                        }
                                    }
                                }
                            }
                            for choice in choices {
                                let picks = chosen.entry(label.to_owned()).or_default();
                                if !picks.contains(&choice.picked) {
                                    picks.push(choice.picked.clone());
                                }
                                let serves_own = choice.serves.iter().any(|&i| i < own_count);
                                if let (Picked::Offer(offer), true) = (&choice.picked, serves_own) {
                                    for entry in self.conferred(label, offer, dependency) {
                                        if !requires.contains(&entry) {
                                            requires.push(entry);
                                        }
                                    }
                                }
                                if !serves_own {
                                    let via = choice
                                        .serves
                                        .iter()
                                        .map(|&i| constraints[i].via.clone())
                                        .min_by_key(|via| (via.len(), via.clone()))
                                        .unwrap_or_default();
                                    let entry = (label.to_owned(), choice.picked, via);
                                    if !seeded.contains(&entry) {
                                        seeded.push(entry);
                                    }
                                }
                            }
                            resolved
                        };
                        (requested, resolved)
                    }
                    DependencyDeclaration::Object(dependency) => {
                        let (version, pinned) = match dependency.selector() {
                            ObjectDependencySelector::Version(version) => {
                                (Some(version.clone()), None)
                            }
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
                        let resolved =
                            if self.store.get_object_genesis(dependency.object()).is_err() {
                                let (resolved, offer) = self.resolve_child(
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
                                )?;
                                // Its artifact brings what it requires
                                // (§46), as one resolved by label does.
                                chosen
                                    .entry(format!("object:{}", dependency.object()))
                                    .or_default()
                                    .push(Picked::Offer(Box::new(offer)));
                                resolved
                            } else {
                                let (resolved, wanted) = self.resolve_object(
                                    object,
                                    target_name,
                                    &requested,
                                    dependency.object(),
                                    version.as_deref(),
                                    pinned.as_ref(),
                                )?;
                                if let std::collections::btree_map::Entry::Vacant(entry) =
                                    brought.entry(brought_key(declaration))
                                {
                                    if let Some(more) =
                                        brings(object, target_name, dependency.object(), &wanted)?
                                    {
                                        entry.insert(more);
                                        found = true;
                                    }
                                }
                                resolved
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
                    brought_by: brought_by.clone(),
                });
            }
            // An object that doesn't build brought dependencies not yet
            // taken: the pass is made again with them.
            if found {
                expansions += 1;
                if expansions > BROUGHT_DEPTH {
                    return Err(ResolveError::BroughtTooDeep {
                        object: object.clone(),
                        target: target_name.to_owned(),
                    });
                }
                continue;
            }
            // The runtime closure of what was chosen (§46 sub-decisions 4
            // and 7), installed after the declared requirements.
            match self.runtime_closure(
                index,
                object,
                target_name,
                &chosen,
                &seeded,
                isolated,
                consumer_environment.as_deref(),
                &generation,
                stack,
                prerequisites,
                diagnostics,
            )? {
                Closure::Converged(closure) => {
                    let mut runtime = Vec::with_capacity(closure.len());
                    for (label, picked, via) in closure {
                        let (version, pin) = match &picked {
                            Picked::Offer(offer) => (
                                offer.version.clone(),
                                Some(Pin {
                                    object: offer.object.to_string(),
                                    snapshot: offer.snapshot.to_string(),
                                }),
                            ),
                            Picked::HostTool(tool) => (tool.version.clone(), None),
                        };
                        let resolved = self.materialize(
                            index,
                            &picked,
                            stack,
                            prerequisites,
                            &mut needed_first,
                            diagnostics,
                        )?;
                        if matches!(resolved, Resolved::HostTool { .. }) {
                            host_dependent = true;
                        }
                        runtime.push(RuntimeRequirement {
                            provides: label,
                            version,
                            pin,
                            via,
                            resolved,
                        });
                    }
                    break (
                        requirements,
                        environment,
                        host_dependent,
                        needed_first,
                        requires,
                        runtime,
                    );
                }
                Closure::Reconsider(constraints) => {
                    passes += 1;
                    if passes >= CLOSURE_PASSES {
                        return Err(ResolveError::NoConvergence {
                            object: object.clone(),
                            target: target_name.to_owned(),
                        });
                    }
                    extras = constraints;
                }
            }
        };

        stack.pop();
        let source_date_epoch = target.source_date_epoch();
        let tree = self.tree_identity(object, snapshot.revision(), target.tree())?;
        let build_key = build_key(
            target,
            &environment,
            host_dependent,
            &requirements,
            &runtime,
            &tree,
            source_date_epoch,
        )?;
        PlanDocument::new(ResolvedManifest {
            schema: RESOLVED_MANIFEST_SCHEMA.to_owned(),
            object: object.to_string(),
            snapshot: snapshot.snapshot_id().to_string(),
            target: target_name.to_owned(),
            environment,
            host_dependent,
            requirements,
            builds_first: needed_first,
            requires,
            runtime,
            source_date_epoch,
            build_key: build_key.to_string(),
        })
    }

    /// What the tree at `path` in `revision` holds (§52): a hash of its
    /// entries, `kairo.toml` left out at the top. The plan can't be made
    /// without it.
    fn tree_identity(
        &self,
        object: &ObjectId,
        revision: &RevisionId,
        path: &str,
    ) -> Result<String, ResolveError> {
        let entries = self
            .manifests
            .tree_entries(object, revision, path)
            .map_err(|source| ResolveError::ManifestSource {
                object: object.clone(),
                revision: revision.clone(),
                source,
            })?
            .ok_or_else(|| ResolveError::TreeMissing {
                object: object.clone(),
                revision: revision.clone(),
                path: path.to_owned(),
            })?;
        let entries: Vec<&TreeEntry> = entries
            .iter()
            .filter(|(_, name, _)| path != "." || name != "kairo.toml")
            .collect();
        let bytes = serde_json::to_vec(&entries).map_err(ResolveError::Encode)?;
        Ok(BlobId::from_bytes(TREE_IDENTITY_DOMAIN, &bytes).to_string())
    }

    /// A content object's artifact, for build keys: the tree under its
    /// `[content] root` at `snapshot` (§50, §52).
    fn content_tree(
        &self,
        object: &ObjectId,
        snapshot: &SnapshotId,
        root: Option<&str>,
    ) -> Result<String, ResolveError> {
        let held = find_snapshot(self.store, object, snapshot)?.ok_or_else(|| {
            ResolveError::SnapshotNotHeld {
                object: object.clone(),
                snapshot: snapshot.clone(),
            }
        })?;
        self.tree_identity(object, held.revision(), root.unwrap_or("."))
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

    /// The offers of `label` that together satisfy `constraints` (§46
    /// sub-decision 4): the highest that satisfies all of them, or,
    /// when none does and every offer involved co-exists with the
    /// others, more than one (sub-decision 5); a development-mode host
    /// tool when nothing held will do and no engine isolates the build.
    fn choose_for_label(
        &self,
        index: &Index,
        label: &str,
        constraints: &[Constraint],
        isolated: bool,
        consumer_environment: Option<&str>,
        generation: &Generation,
    ) -> Result<Vec<Choice>, Conflict> {
        let mut considered = Vec::new();
        let mut offers = Vec::new();
        for offer in index.offers(label) {
            // Not built with itself, only with older generations (§59).
            if generation.bars(&offer) {
                considered.push(format!(
                    "{} is this object's, and not an older generation",
                    offer.describe()
                ));
                continue;
            }
            // Binaries are for one architecture (§38 sub-decision 8).
            if let (Some(wanted), Some(built)) = (consumer_environment, &offer.environment) {
                if contract_arch(wanted) != contract_arch(built) {
                    considered.push(format!(
                        "{} runs in {built}, not {}",
                        offer.describe(),
                        contract_arch(wanted)
                    ));
                    continue;
                }
            }
            offers.push(offer);
        }
        self.rank_offers(&mut offers);
        let mut remaining: Vec<usize> = (0..constraints.len()).collect();
        let mut choices: Vec<Choice> = Vec::new();
        while !remaining.is_empty() {
            let satisfies = |offer: &Offer, which: &[usize]| {
                which
                    .iter()
                    .filter(|&&i| constraints[i].satisfied_by(offer))
                    .copied()
                    .collect::<Vec<usize>>()
            };
            // The highest offer satisfying everything left, or else the
            // one satisfying most of it, to be joined by others.
            let best = offers
                .iter()
                .filter(|offer| {
                    !choices
                        .iter()
                        .any(|c| c.picked == Picked::Offer(Box::new((*offer).clone())))
                })
                .map(|offer| (satisfies(offer, &remaining), offer))
                .filter(|(served, _)| !served.is_empty())
                // The first of equal counts: the highest version.
                .min_by_key(|(served, _)| std::cmp::Reverse(served.len()));
            let Some((served, offer)) = best else {
                break;
            };
            remaining.retain(|i| !served.contains(i));
            choices.push(Choice {
                picked: Picked::Offer(Box::new(offer.clone())),
                serves: served,
            });
        }
        if !remaining.is_empty() {
            // Nothing held serves these; a host tool may, in development
            // mode, when it satisfies every version asked and no pin.
            let tool = self.host_tools.iter().find(|tool| {
                tool.provides == label
                    && !isolated
                    && constraints.iter().all(|c| {
                        c.pin.is_none()
                            && version_matches(c.version.as_deref(), tool.version.as_deref())
                    })
            });
            if let (Some(tool), true) = (tool, choices.is_empty()) {
                return Ok(vec![Choice {
                    picked: Picked::HostTool(tool.clone()),
                    serves: (0..constraints.len()).collect(),
                }]);
            }
            for tool in self.host_tools.iter().filter(|tool| tool.provides == label) {
                considered.push(if isolated {
                    format!(
                        "host tool {} (not usable under an isolation engine)",
                        tool.path.display()
                    )
                } else {
                    format!(
                        "host tool {} version {} doesn't satisfy every requirement",
                        tool.path.display(),
                        tool.version.as_deref().unwrap_or("unknown")
                    )
                });
            }
            for offer in &offers {
                let unmet = constraints
                    .iter()
                    .find(|constraint| !constraint.satisfied_by(offer))
                    .map(|constraint| match (&constraint.pin, &constraint.version) {
                        (Some((object, _)), _) => format!("the pin to object:{object}"),
                        (None, Some(version)) => version.clone(),
                        (None, None) => "anything".to_owned(),
                    })
                    .unwrap_or_else(|| "its co-existence".to_owned());
                considered.push(format!(
                    "{} version {} doesn't satisfy {unmet}",
                    offer.describe(),
                    offer.version.as_deref().unwrap_or("unknown")
                ));
            }
            return Err(Conflict {
                label: label.to_owned(),
                unmet: remaining
                    .iter()
                    .map(|&i| constraints[i].describe(label))
                    .collect(),
                considered,
            });
        }
        if choices.len() > 1 {
            let all_coexist = choices
                .iter()
                .all(|c| matches!(&c.picked, Picked::Offer(offer) if offer.coexists));
            if !all_coexist {
                return Err(Conflict {
                    label: label.to_owned(),
                    unmet: constraints.iter().map(|c| c.describe(label)).collect(),
                    considered: choices
                        .iter()
                        .filter_map(|c| match &c.picked {
                            Picked::Offer(offer) => Some(format!(
                                "{} version {} doesn't co-exist with other versions",
                                offer.describe(),
                                offer.version.as_deref().unwrap_or("unknown")
                            )),
                            Picked::HostTool(_) => None,
                        })
                        .collect(),
                });
            }
        }
        Ok(choices)
    }

    /// What `picked` is in a plan: its artifact (planned first when not
    /// yet built), its tree, or the host tool.
    #[allow(clippy::too_many_arguments)]
    fn materialize(
        &self,
        index: &Index,
        picked: &Picked,
        stack: &mut Vec<(ObjectId, SnapshotId, String)>,
        prerequisites: &mut Vec<PlanDocument>,
        needed_first: &mut Vec<BuildFirst>,
        diagnostics: &mut Diagnostics,
    ) -> Result<Resolved, ResolveError> {
        match picked {
            Picked::HostTool(tool) => Ok(Resolved::HostTool {
                provides: tool.provides.clone(),
                version: tool.version.clone(),
                path: tool.path.display().to_string(),
            }),
            Picked::Offer(offer) => match &offer.artifact {
                None => Ok(Resolved::Content {
                    object: offer.object.to_string(),
                    snapshot: offer.snapshot.to_string(),
                    install: offer.install.clone(),
                    root: offer.content_root.clone(),
                    tree: Some(self.content_tree(
                        &offer.object,
                        &offer.snapshot,
                        offer.content_root.as_deref(),
                    )?),
                }),
                Some((provider_target, output)) => self.artifact_for(
                    index,
                    offer,
                    provider_target,
                    output,
                    stack,
                    prerequisites,
                    needed_first,
                    diagnostics,
                ),
            },
        }
    }

    /// A target's own provides requirement, resolved under every
    /// constraint on its label: what it pins, and every choice made for
    /// the label (more than one only where they co-exist).
    #[allow(clippy::too_many_arguments)]
    fn resolve_provides(
        &self,
        index: &Index,
        object: &ObjectId,
        target: &str,
        requested: &Requested,
        label: &str,
        constraints: &[Constraint],
        mine: usize,
        isolated: bool,
        consumer_environment: Option<&str>,
        generation: &Generation,
        stack: &mut Vec<(ObjectId, SnapshotId, String)>,
        prerequisites: &mut Vec<PlanDocument>,
        needed_first: &mut Vec<BuildFirst>,
        diagnostics: &mut Diagnostics,
    ) -> Result<(Resolved, Vec<Choice>), ResolveError> {
        let choices = self
            .choose_for_label(
                index,
                label,
                constraints,
                isolated,
                consumer_environment,
                generation,
            )
            .map_err(|conflict| conflict.into_error(object, target, requested, constraints))?;
        let own = choices
            .iter()
            .find(|choice| choice.serves.contains(&mine))
            .ok_or_else(|| {
                ResolveError::Unsatisfied(Box::new(Unsatisfied {
                    object: object.clone(),
                    target: target.to_owned(),
                    requested: requested.clone(),
                    considered: vec!["no choice served this requirement".to_owned()],
                }))
            })?;
        let resolved = self.materialize(
            index,
            &own.picked,
            stack,
            prerequisites,
            needed_first,
            diagnostics,
        )?;
        // What else would have done for this requirement alone, and why
        // it lost.
        let chosen = match &own.picked {
            Picked::Offer(offer) => Some(offer.as_ref()),
            Picked::HostTool(_) => None,
        };
        let mut ranked = index.offers(label);
        self.rank_offers(&mut ranked);
        let others: Vec<String> = ranked
            .iter()
            .filter(|offer| constraints[mine].satisfied_by(offer))
            .filter(|offer| match (consumer_environment, &offer.environment) {
                (Some(wanted), Some(built)) => contract_arch(wanted) == contract_arch(built),
                _ => true,
            })
            .filter(|offer| {
                !choices
                    .iter()
                    .any(|c| c.picked == Picked::Offer(Box::new((*offer).clone())))
            })
            .map(|offer| self.passed_over(offer, chosen))
            .chain(
                self.host_tools
                    .iter()
                    .filter(|tool| {
                        tool.provides == label
                            && !matches!(&own.picked, Picked::HostTool(t) if t == *tool)
                    })
                    .map(|tool| format!("host tool {}", tool.path.display())),
            )
            .collect();
        if !others.is_empty() {
            diagnostics.alternatives.push(Alternative {
                requested: requested.clone(),
                chosen: describe_resolved(&resolved),
                others,
            });
        }
        Ok((resolved, choices))
    }

    /// What resolving `offer` confers on the consumer's artifact (§46
    /// sub-decision 2), as the plan records it: each label pinned to the
    /// provider's own build when it gives no version.
    /// What resolving `offer` for this build's own dependency on `label`
    /// confers on the artifact (`DECISIONS.md` §46, §51), as the plan
    /// records it. Only the build's direct dependencies confer. A label
    /// conferring itself without a version is pinned to the offer this
    /// build resolved; another label carries the range its provider
    /// gives. `confer = false` on the dependency passes on nothing;
    /// `confer = true` passes on the label even when its provider
    /// doesn't, at the dependency's version requirement or pinned to the
    /// offer.
    fn conferred(
        &self,
        label: &str,
        offer: &Offer,
        dependency: &kairo_object::ProvidesDependency,
    ) -> Vec<ArtifactRequirement> {
        if dependency.confer() == Some(false) {
            return Vec::new();
        }
        let resolved = || Pin {
            object: offer.object.to_string(),
            snapshot: offer.snapshot.to_string(),
        };
        let mut confers = offer.confers.clone();
        if dependency.confer() == Some(true) && !confers.iter().any(|(c, _)| c == label) {
            confers.insert(
                0,
                (label.to_owned(), dependency.version().map(str::to_owned)),
            );
        }
        confers
            .into_iter()
            .map(|(conferred, version)| {
                // Another label without a version isn't a manifest's to
                // give (§51); pinned, it is pinned to what was resolved.
                let pin = version.is_none().then(resolved);
                ArtifactRequirement {
                    provides: conferred,
                    one_of: Vec::new(),
                    version,
                    pin,
                    child: None,
                    via: format!("conferred by {label} from object:{}", offer.object),
                }
            })
            .collect()
    }

    /// What the artifact `offer` resolves to requires at run time (§46
    /// sub-decision 3), from the plan that built it or will build it.
    fn artifact_requires(
        &self,
        index: &Index,
        offer: &Offer,
        stack: &mut Vec<(ObjectId, SnapshotId, String)>,
        prerequisites: &mut Vec<PlanDocument>,
        diagnostics: &mut Diagnostics,
    ) -> Result<Vec<ArtifactRequirement>, ResolveError> {
        let Some((target, _)) = &offer.artifact else {
            return Ok(Vec::new());
        };
        let builder = offer.builder().clone();
        let snapshot = offer.snapshot.clone();
        let planned = prerequisites.iter().find(|plan| {
            plan.manifest.object == builder.to_string()
                && plan.manifest.snapshot == snapshot.to_string()
                && &plan.manifest.target == target
        });
        let requires = if let Some(plan) = planned {
            plan.manifest.requires.clone()
        } else {
            let built = self
                .artifacts
                .artifact(&builder, &snapshot, target)
                .map_err(|source| ResolveError::ArtifactSource {
                    object: builder.clone(),
                    target: target.clone(),
                    source,
                })?;
            if built.is_some() {
                match self
                    .artifacts
                    .build_plan(&builder, &snapshot, target)
                    .map_err(|source| ResolveError::ArtifactSource {
                        object: builder.clone(),
                        target: target.clone(),
                        source,
                    })? {
                    Some(bytes) => PlanDocument::from_bytes(bytes)?.manifest.requires,
                    None => Vec::new(),
                }
            } else {
                let plan = self.resolve_target(
                    index,
                    &builder,
                    &SnapshotSelector::Pinned(snapshot.clone()),
                    target,
                    stack,
                    prerequisites,
                    diagnostics,
                )?;
                let requires = plan.manifest.requires.clone();
                if !prerequisites
                    .iter()
                    .any(|existing| existing.hash == plan.hash)
                {
                    prerequisites.push(plan);
                }
                requires
            }
        };
        Ok(requires
            .into_iter()
            .filter(|entry| entry.child.is_none() || entry.child == offer.child_name)
            .collect())
    }

    /// The runtime closure of the build dependencies (§46 sub-decisions
    /// 4 and 7): rounds of gathering what every chosen artifact requires
    /// and choosing for each label until the choices stop changing.
    /// `Reconsider` says a declared requirement's label was asked
    /// something its choice doesn't satisfy, with every constraint on
    /// such labels, for the declared requirements to be re-chosen under.
    #[allow(clippy::too_many_arguments)]
    fn runtime_closure(
        &self,
        index: &Index,
        object: &ObjectId,
        target_name: &str,
        declared: &BTreeMap<String, Vec<Picked>>,
        seeded: &[(String, Picked, Vec<String>)],
        isolated: bool,
        consumer_environment: Option<&str>,
        generation: &Generation,
        stack: &mut Vec<(ObjectId, SnapshotId, String)>,
        prerequisites: &mut Vec<PlanDocument>,
        diagnostics: &mut Diagnostics,
    ) -> Result<Closure, ResolveError> {
        let mut runtime: Vec<(String, Picked, Vec<String>)> = seeded.to_vec();
        for _ in 0..CLOSURE_ROUNDS {
            // What every current choice requires, one level down.
            let mut gathered: BTreeMap<String, Vec<Constraint>> = BTreeMap::new();
            let sources = declared
                .iter()
                .flat_map(|(label, picks)| {
                    picks.iter().map(move |pick| (pick, vec![label.clone()]))
                })
                .chain(runtime.iter().map(|(_, pick, via)| (pick, via.clone())));
            for (pick, via) in sources {
                let Picked::Offer(offer) = pick else {
                    continue;
                };
                for entry in
                    self.artifact_requires(index, offer, stack, prerequisites, diagnostics)?
                {
                    // An obligation on this build, not something to install
                    // for it (§51): one of the alternatives must be among
                    // what it requests itself.
                    if !entry.one_of.is_empty() {
                        let met = entry.one_of.iter().any(|alternative| {
                            declared.get(&alternative.provides).is_some_and(|picks| {
                                picks.iter().any(|pick| match pick {
                                    Picked::Offer(offer) => version_matches(
                                        alternative.version.as_deref(),
                                        offer.version.as_deref(),
                                    ),
                                    Picked::HostTool(tool) => version_matches(
                                        alternative.version.as_deref(),
                                        tool.version.as_deref(),
                                    ),
                                })
                            })
                        });
                        if !met {
                            return Err(ResolveError::ObligationUnmet {
                                object: object.clone(),
                                target: target_name.to_owned(),
                                via: via.clone(),
                                alternatives: entry
                                    .one_of
                                    .iter()
                                    .map(ToString::to_string)
                                    .collect(),
                            });
                        }
                        continue;
                    }
                    let mut chain = via.clone();
                    chain.push(entry.provides.clone());
                    gathered
                        .entry(entry.provides)
                        .or_default()
                        .push(Constraint {
                            version: entry.version,
                            pin: held_pin(entry.pin)?,
                            via: chain,
                            offered_by: Vec::new(),
                        });
                }
            }
            // A declared label asked something its choices don't give:
            // the declared requirements are re-chosen under it all.
            let mut reconsider: BTreeMap<String, Vec<Constraint>> = BTreeMap::new();
            for (label, constraints) in &gathered {
                let Some(picks) = declared.get(label) else {
                    continue;
                };
                let unmet = constraints.iter().any(|constraint| {
                    !picks.iter().any(|pick| match pick {
                        Picked::Offer(offer) => constraint.admits(offer),
                        Picked::HostTool(tool) => {
                            constraint.pin.is_none()
                                && version_matches(
                                    constraint.version.as_deref(),
                                    tool.version.as_deref(),
                                )
                        }
                    })
                });
                if unmet {
                    reconsider.insert(label.clone(), constraints.clone());
                }
            }
            if !reconsider.is_empty() {
                return Ok(Closure::Reconsider(reconsider));
            }
            // The other labels: chosen anew from everything asked of
            // them this round.
            let mut next: Vec<(String, Picked, Vec<String>)> = seeded.to_vec();
            // What the build installs already, its declared requirements'
            // choices and the closure's so far: an object of these that
            // offers a label asked here too meets it, so the closure adds
            // no second package where one already serves (§59).
            let installed: Vec<(&ObjectId, &SnapshotId)> = declared
                .values()
                .flatten()
                .chain(runtime.iter().map(|(_, pick, _)| pick))
                .filter_map(|pick| match pick {
                    Picked::Offer(offer) => Some((&offer.object, &offer.snapshot)),
                    Picked::HostTool(_) => None,
                })
                .collect();
            for (label, constraints) in &gathered {
                if declared.contains_key(label) {
                    continue;
                }
                let already = index.offers(label).into_iter().find(|offer| {
                    installed.contains(&(&offer.object, &offer.snapshot))
                        && !generation.bars(offer)
                        && match (consumer_environment, &offer.environment) {
                            (Some(wanted), Some(built)) => {
                                contract_arch(wanted) == contract_arch(built)
                            }
                            _ => true,
                        }
                        && constraints.iter().all(|c| c.admits(offer))
                });
                let choices = match already {
                    Some(offer) => vec![Choice {
                        picked: Picked::Offer(Box::new(offer)),
                        serves: (0..constraints.len()).collect(),
                    }],
                    None => self
                        .choose_for_label(
                            index,
                            label,
                            constraints,
                            isolated,
                            consumer_environment,
                            generation,
                        )
                        .map_err(|conflict| conflict.into_runtime_error(object, target_name))?,
                };
                for choice in choices {
                    let via = choice
                        .serves
                        .iter()
                        .map(|&i| constraints[i].via.clone())
                        .min_by_key(|via| (via.len(), via.clone()))
                        .unwrap_or_default();
                    next.push((label.clone(), choice.picked, via));
                }
            }
            // Nearest the build dependencies first, then by label.
            next.sort_by(|a, b| (a.2.len(), &a.2, &a.0).cmp(&(b.2.len(), &b.2, &b.0)));
            if next == runtime {
                return Ok(Closure::Converged(runtime));
            }
            runtime = next;
        }
        Err(ResolveError::NoConvergence {
            object: object.clone(),
            target: target_name.to_owned(),
        })
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
        stack: &mut Vec<(ObjectId, SnapshotId, String)>,
        prerequisites: &mut Vec<PlanDocument>,
        needed_first: &mut Vec<BuildFirst>,
        diagnostics: &mut Diagnostics,
    ) -> Result<Resolved, ResolveError> {
        let builder = offer
            .built_by
            .as_ref()
            .map_or(&offer.object, |(parent, _)| parent);
        let source_error = |source| ResolveError::ArtifactSource {
            object: builder.clone(),
            target: provider_target.to_owned(),
            source,
        };
        // A build of this very snapshot whose plan had this plan's key,
        // as before; else one whose plan had the same build key, at any
        // snapshot (§52). Planning the provider is what says its key; a
        // provider held but not plannable here (its tree or sources not
        // held) is used as held. A build of the snapshot whose plan had
        // another key, made under other rules, isn't this plan's
        // artifact (§54 sub-decision 8); one whose plan isn't held can't
        // be told apart, and stands.
        let held = self
            .artifacts
            .artifact(builder, &offer.snapshot, provider_target)
            .map_err(source_error)?;
        let planned = match self.resolve_target(
            index,
            builder,
            &SnapshotSelector::Pinned(offer.snapshot.clone()),
            provider_target,
            stack,
            prerequisites,
            diagnostics,
        ) {
            Ok(plan) => Some(plan),
            Err(_) if held.is_some() => None,
            Err(error) => return Err(error),
        };
        let build_key = planned.as_ref().map(|plan| plan.manifest.build_key.clone());
        let held = match (held, &build_key) {
            (Some(held), Some(key)) => {
                let plan = self
                    .artifacts
                    .build_plan(builder, &offer.snapshot, provider_target)
                    .map_err(source_error)?;
                let other_key = match plan {
                    Some(bytes) => &PlanDocument::from_bytes(bytes)?.manifest.build_key != key,
                    None => false,
                };
                (!other_key).then_some(held)
            }
            (held, _) => held,
        };
        let artifact = match (held, &build_key) {
            (Some(held), _) => Some(held),
            (None, Some(key)) => self
                .artifacts
                .artifact_by_key(
                    builder,
                    provider_target,
                    &BlobId::new(key.clone()).map_err(|_| {
                        ResolveError::UnsupportedSchema(format!("{key} is not a build key"))
                    })?,
                )
                .map_err(source_error)?,
            (None, None) => None,
        };
        if let (None, Some(plan)) = (&artifact, planned) {
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
            build_key,
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
        stack: &mut Vec<(ObjectId, SnapshotId, String)>,
        prerequisites: &mut Vec<PlanDocument>,
        needed_first: &mut Vec<BuildFirst>,
        diagnostics: &mut Diagnostics,
    ) -> Result<(Resolved, Offer), ResolveError> {
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
        let parents: Vec<&Candidate> = match index.child(wanted) {
            Some((candidate, _, _)) => vec![candidate],
            None => index.candidates.iter().collect(),
        };
        // Each declaration of the child, the current snapshot's first,
        // then the tagged ones', highest tag first.
        let mut considered = Vec::new();
        let mut declared: Vec<(Option<semver::Version>, Candidate, String, String)> = Vec::new();
        for candidate in &parents {
            let parent = &candidate.object;
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
                        owner: candidate.owner.clone(),
                        // A tagged snapshot keeps its object's head time;
                        // it isn't ranked against other objects here.
                        published: candidate.published,
                        // Named by id: no release is asked for (§61).
                        release: true,
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
        let resolved = self.artifact_for(
            index,
            &offer,
            at_target.name(),
            "",
            stack,
            prerequisites,
            needed_first,
            diagnostics,
        )?;
        Ok((resolved, offer))
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
    ) -> Result<(Resolved, ObjectManifest), ResolveError> {
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
        let Some(manifest) = load_manifest(self.manifests, wanted, &snapshot)? else {
            return Err(unsatisfied(vec![format!(
                "tree for revision {} of object:{wanted} is not held (kairo git fetch)",
                snapshot.revision().as_str()
            )]));
        };
        let (install, root) = content_placement(&manifest);
        let tree = self.content_tree(wanted, &snapshot.snapshot_id(), root.as_deref())?;
        Ok((
            Resolved::Content {
                object: wanted.to_string(),
                snapshot: snapshot.snapshot_id().to_string(),
                install,
                root,
                tree: Some(tree),
            },
            manifest,
        ))
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
        Resolved::Content {
            object,
            install,
            root,
            ..
        } => {
            let mut text = format!("object:{object}");
            if let Some(root) = root {
                text.push_str(&format!(" root {root}"));
            }
            match install {
                Some(install) => text.push_str(&format!(" (content, installed at {install})")),
                None => text.push_str(" (content)"),
            }
            text
        }
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
    /// A build target's `tree` or a content object's `root` isn't a
    /// tree in the object's revision (§52).
    TreeMissing {
        object: ObjectId,
        revision: RevisionId,
        path: String,
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
    /// No set of co-existing providers satisfies every requirement on
    /// a label in the plan's closure (`DECISIONS.md` §46).
    Conflict {
        object: ObjectId,
        target: String,
        label: String,
        unmet: Vec<String>,
        considered: Vec<String>,
    },
    /// Something this build uses requires `one_of` several labels, and
    /// this build requests none of them itself (`DECISIONS.md` §51).
    /// An object that doesn't build, chosen for a dependency, has a
    /// dependency it can't bring (§57).
    BroughtUnsupported {
        object: ObjectId,
        target: String,
        by: ObjectId,
        dependency: String,
    },
    /// Objects that don't build kept bringing one another's dependencies.
    BroughtTooDeep {
        object: ObjectId,
        target: String,
    },
    ObligationUnmet {
        object: ObjectId,
        target: String,
        /// The labels it brought the obligation through, ending with the
        /// one whose artifact declares it.
        via: Vec<String>,
        /// Each alternative, with its version requirement.
        alternatives: Vec<String>,
    },
    /// The runtime closure kept changing what it asked of the declared
    /// requirements.
    NoConvergence {
        object: ObjectId,
        target: String,
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
            Self::TreeMissing {
                object,
                revision,
                path,
            } => write!(
                f,
                "object:{object} at revision {}: {path} is not a directory in its tree",
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
            Self::Conflict {
                object,
                target,
                label,
                unmet,
                considered,
            } => {
                write!(
                    f,
                    "object:{object} target {target}: no provider of {label} satisfies every requirement on it, and they don't co-exist"
                )?;
                for line in unmet {
                    write!(f, "\n  required: {line}")?;
                }
                for line in considered {
                    write!(f, "\n  considered: {line}")?;
                }
                Ok(())
            }
            Self::ObligationUnmet {
                object,
                target,
                via,
                alternatives,
            } => write!(
                f,
                "object:{object} target {target}: {} needs this build to require one of {} itself",
                via.join(" > "),
                alternatives.join(", ")
            ),
            Self::NoConvergence { object, target } => write!(
                f,
                "object:{object} target {target}: the runtime closure didn't settle after {CLOSURE_PASSES} passes"
            ),
            Self::BroughtUnsupported {
                object,
                target,
                by,
                dependency,
            } => write!(
                f,
                "object:{object} target {target}: object:{by} doesn't build, so its dependencies are this build's, but it depends on {dependency}, which only a build's own manifest can name"
            ),
            Self::BroughtTooDeep { object, target } => write!(
                f,
                "object:{object} target {target}: objects that don't build brought one another's dependencies more than {BROUGHT_DEPTH} levels deep"
            ),
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
            | Self::TreeMissing { .. }
            | Self::ManifestMismatch { .. }
            | Self::NoHead { .. }
            | Self::SnapshotNotHeld { .. }
            | Self::NoTarget { .. }
            | Self::Unsatisfied(_)
            | Self::Cycle { .. }
            | Self::Conflict { .. }
            | Self::ObligationUnmet { .. }
            | Self::BroughtUnsupported { .. }
            | Self::BroughtTooDeep { .. }
            | Self::NoConvergence { .. }
            | Self::UnsupportedSchema(_) => None,
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests;
