//! Bundle manifest serde shape.
//!
//! `manifest.json` is the authoritative inventory of a bundle. It is
//! not signed in MVP — every record listed in `contents` is itself
//! content-addressed, and every statement's signature is preserved
//! byte-for-byte by the export. Bundle-level signing can land later
//! without breaking the v1 schema.

use serde::{Deserialize, Serialize};

/// Top-level manifest written to `manifest.json`.
///
/// Field ordering matches what the writer emits; serde does not care
/// about ordering on read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleManifest {
    /// Always `"kairo.bundle.v1"` for this implementation.
    pub schema: String,
    /// RFC 3339 UTC seconds. Informational only; not signed.
    pub created_at: String,
    pub created_by: BundleCreator,
    /// What the bundle is "about." MVP accepts a single object root.
    pub roots: BundleRoots,
    /// Inventory of every record file in the bundle, by id. Importers
    /// validate each id against the file's actual content; the manifest
    /// is a navigation aid, not an authority.
    pub contents: BundleContents,
    /// Declares which Git commits the bundle's statements reference.
    /// MVP always sets `included = false`; future bundles may include a
    /// `git/` subdirectory and flip this flag.
    pub git_history: BundleGitHistory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleCreator {
    pub tool: String,
    pub version: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleRoots {
    #[serde(default)]
    pub objects: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleContents {
    #[serde(default)]
    pub actors: Vec<String>,
    #[serde(default)]
    pub objects: Vec<String>,
    #[serde(default)]
    pub statements: Vec<String>,
    #[serde(default)]
    pub blobs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleGitHistory {
    /// `false` in MVP — the bundle does not carry Git pack data.
    pub included: bool,
    /// Hex-encoded Git commit OIDs the bundle's `ObjectRevision`
    /// statements name. Recipients need these commits in some Git repo
    /// to reach end-to-end `VALID` verification.
    #[serde(default)]
    pub expected_commits: Vec<String>,
}
