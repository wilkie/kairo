//! Recorded plans and their reuse (`DECISIONS.md` §62).
//!
//! A plan is a function of the snapshot planned, its target, and what
//! the resolver read on the way. While a target is planned, a [`Frame`]
//! collects what it read: the labels it looked up, the objects it named,
//! the owners it ranked, what it found held, and the providers it planned.
//! The plan is recorded with a fingerprint of each, and a later
//! resolution whose fingerprints all match takes the recorded plan
//! instead of planning again. Records are cache data: one that can't be
//! read or written is planned again, never an error.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use kairo_core::{BlobId, ObjectId};
use kairo_store::FilesystemStore;
use serde::{Deserialize, Serialize};

use crate::{
    contract_arch, current_head, tagged_snapshots, Alternative, Host, HostTool, Index, Offer,
    ResolveError, SourceSeed,
};

/// Raised whenever the resolver would choose differently from the same
/// inputs: a record made by another version is planned again (§62
/// sub-decision 4).
pub const RESOLVER_VERSION: u32 = 1;

const FINGERPRINT_DOMAIN: &[u8] = b"kairo.plan.fingerprint.v1";

/// Where plans are recorded, and whether recorded ones are reused.
#[derive(Debug, Clone)]
pub struct PlanRecords {
    dir: PathBuf,
    reuse: bool,
}

impl PlanRecords {
    /// Records under `dir`, reused while they hold.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            reuse: true,
        }
    }

    /// The store's own: `cache/plans/` under its root (`STORE.md` §4).
    pub fn in_store(store: &FilesystemStore) -> Self {
        Self::new(store.root().join("cache").join("plans"))
    }

    /// Plan everything again and record it, reusing nothing
    /// (`kairo build --replan`).
    #[must_use]
    pub fn replanning(mut self) -> Self {
        self.reuse = false;
        self
    }

    pub(crate) fn reuses(&self) -> bool {
        self.reuse
    }

    fn path(&self, key: &PlanKey) -> PathBuf {
        self.dir
            .join(&key.0)
            .join(&key.1)
            .join(format!("{}.json", key.2))
    }

    pub(crate) fn read(&self, key: &PlanKey) -> Option<PlanRecord> {
        let bytes = std::fs::read(self.path(key)).ok()?;
        let record: PlanRecord = serde_json::from_slice(&bytes).ok()?;
        (record.resolver == RESOLVER_VERSION).then_some(record)
    }

    /// Written whole or not at all; a failure costs only the next
    /// resolution's time.
    pub(crate) fn write(&self, key: &PlanKey, record: &PlanRecord) {
        let path = self.path(key);
        let Some(parent) = path.parent() else {
            return;
        };
        let Ok(bytes) = serde_json::to_vec(record) else {
            return;
        };
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
        let partial = parent.join(format!(".{}.{}.partial", key.2, std::process::id()));
        if std::fs::write(&partial, bytes).is_ok() && std::fs::rename(&partial, &path).is_err() {
            let _ = std::fs::remove_file(&partial);
        }
    }

    /// The directory records are kept in.
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

/// A plan's object, snapshot, and target.
pub(crate) type PlanKey = (String, String, String);

/// A label as one target looked it up: in an architecture, or every one
/// when the target declares no environment, and with the objects and
/// snapshots its requirements named, whose unreleased offers it could
/// take too.
pub(crate) type LabelLookup = (String, Option<String>, Vec<String>);

/// What one target's planning read (§62 sub-decision 3).
#[derive(Debug, Default)]
pub(crate) struct Frame {
    pub labels: BTreeSet<LabelLookup>,
    pub objects: BTreeSet<String>,
    pub owners: BTreeMap<String, String>,
    pub bindings: BTreeSet<Binding>,
    pub providers: BTreeSet<PlanKey>,
    /// A provider's planning failed and the plan went on without it: the
    /// failure may depend on what was being planned around it, so the
    /// plan isn't recorded.
    pub unrecordable: bool,
}

/// What a plan found held: re-read on reuse, and a plan whose bindings
/// read differently is planned again (§62 sub-decision 6).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Binding {
    /// A target's held artifact at a snapshot.
    Artifact {
        object: String,
        snapshot: String,
        target: String,
        held: Option<String>,
    },
    /// The plan its held build cites, by hash.
    BuildPlan {
        object: String,
        snapshot: String,
        target: String,
        plan: Option<String>,
    },
    /// A held artifact by build key (§52).
    ByKey {
        object: String,
        target: String,
        key: String,
        held: Option<String>,
    },
    /// A source's commit held in its Git cache (§37).
    Source { source: String, revision: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct LabelSeen {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<String>,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Seen {
    pub id: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProviderSeen {
    pub object: String,
    pub snapshot: String,
    pub target: String,
}

/// One recorded plan (§62 sub-decision 7).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PlanRecord {
    pub resolver: u32,
    pub host: String,
    /// The plan's bytes, a resolved manifest's JSON.
    pub plan: String,
    pub labels: Vec<LabelSeen>,
    pub objects: Vec<Seen>,
    pub owners: Vec<Seen>,
    pub bindings: Vec<Binding>,
    pub providers: Vec<ProviderSeen>,
    /// What its planning passed over, shown again when it is reused.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternatives: Vec<Alternative>,
}

fn fingerprint(parts: &[String]) -> String {
    let mut bytes = Vec::new();
    for part in parts {
        bytes.extend_from_slice(part.as_bytes());
        bytes.push(0);
    }
    BlobId::from_bytes(FINGERPRINT_DOMAIN, &bytes).to_string()
}

/// The host's contract, engines, and development-mode host tools, and
/// the seeds accepted (§62 sub-decision 4).
pub(crate) fn host_fingerprint(host: &Host, tools: &[HostTool], seeds: &[SourceSeed]) -> String {
    let mut parts = vec![host.contract(), crate::RESOLVED_MANIFEST_SCHEMA.to_owned()];
    parts.extend(host.engines.iter().map(|engine| format!("engine {engine}")));
    parts.extend(tools.iter().map(|tool| {
        format!(
            "tool {} {} {}",
            tool.provides,
            tool.version.as_deref().unwrap_or(""),
            tool.path.display()
        )
    }));
    parts.extend(
        seeds
            .iter()
            .map(|seed| format!("seed {} {}", seed.source, seed.revision)),
    );
    fingerprint(&parts)
}

/// Whether an offer counts toward a lookup: eligible in its
/// architecture (§38 sub-decision 8), and a release or an offer of an
/// object its requirements named (§61, §62 sub-decision 2).
fn counts(offer: &Offer, arch: Option<&str>, names: &[String]) -> bool {
    let eligible = match (arch, &offer.environment) {
        (Some(wanted), Some(built)) => contract_arch(built) == wanted,
        _ => true,
    };
    let named = || {
        names.iter().any(|name| {
            name == offer.object.as_str()
                || name == offer.builder().as_str()
                || name == offer.snapshot.as_str()
        })
    };
    eligible && (offer.release || named())
}

/// A label's fingerprint for one lookup (§62 sub-decision 1): each
/// counted offer's object, snapshot, owner, and release, in the index's
/// order.
pub(crate) fn label_fingerprint(index: &Index, lookup: &LabelLookup) -> String {
    let (label, arch, names) = lookup;
    if let Some(known) = index.fingerprints.borrow().get(lookup) {
        return known.clone();
    }
    let parts: Vec<String> = index
        .offers_ref(label)
        .iter()
        .filter(|offer| counts(offer, arch.as_deref(), names))
        .map(|offer| {
            format!(
                "{} {} {} {} {}",
                offer.object,
                offer.builder(),
                offer.snapshot,
                offer.owner,
                offer.release
            )
        })
        .collect();
    let made = fingerprint(&parts);
    index
        .fingerprints
        .borrow_mut()
        .insert(lookup.clone(), made.clone());
    made
}

/// A named object's fingerprint (§62 sub-decision 2): its creator's head
/// and its version tags, a child's its parent's. A child whose parent no
/// current manifest declares is looked for among every held object, so
/// its fingerprint is all of theirs.
pub(crate) fn object_fingerprint(
    store: &FilesystemStore,
    index: &Index,
    id: &str,
) -> Result<String, ResolveError> {
    // The object planned, at its snapshot: its tags, which decide its
    // generations (§59), and whether the snapshot is still its head, but
    // not where its head has moved to since.
    if let Some((id, at)) = id.split_once('@') {
        let Ok(object) = ObjectId::new(id) else {
            return Ok(fingerprint(&[format!("unparsed {id}")]));
        };
        let mut parts = Vec::new();
        if let Some((snapshot, _, _)) = current_head(store, &object)? {
            if snapshot.snapshot_id().as_str() == at {
                parts.push("head".to_owned());
            }
        }
        for (version, snapshot) in tagged_snapshots(store, &object)? {
            parts.push(format!("tag {version} {}", snapshot.snapshot_id()));
        }
        return Ok(fingerprint(&parts));
    }
    let Ok(object) = ObjectId::new(id) else {
        return Ok(fingerprint(&[format!("unparsed {id}")]));
    };
    let held = |object: &ObjectId| -> Result<Vec<String>, ResolveError> {
        let mut parts = Vec::new();
        if let Some((snapshot, owner, _)) = current_head(store, object)? {
            parts.push(format!("head {} {owner}", snapshot.snapshot_id()));
        }
        for (version, snapshot) in tagged_snapshots(store, object)? {
            parts.push(format!("tag {version} {}", snapshot.snapshot_id()));
        }
        Ok(parts)
    };
    let mut parts = held(&object)?;
    if parts.is_empty() {
        match index.child(&object) {
            Some((parent, _, _)) => {
                parts.push(format!("child of {}", parent.object));
                parts.extend(held(&parent.object)?);
            }
            None => {
                parts.push("anywhere".to_owned());
                for candidate in index.candidates.iter().chain(&index.tagged) {
                    parts.push(format!("{} {}", candidate.object, candidate.snapshot));
                }
            }
        }
    }
    Ok(fingerprint(&parts))
}
