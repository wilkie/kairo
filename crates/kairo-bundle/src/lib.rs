//! Kairo bundle: portable directory-form export of an object's Kairo
//! statements, signing actors, and referenced blobs.
//!
//! A bundle is a transport container. It is **not** an authority claim:
//! every signed statement inside still verifies on its own bytes, and
//! the importer's local trust evaluation is unchanged by ingestion.
//! Importing a bundle is fixity-only — every record's id is re-derived
//! from its bytes and must match its on-disk filename.
//!
//! # MVP scope
//!
//! - Directory layout only (no tar/zip in MVP — users may package the
//!   directory with whatever transport tool they prefer).
//! - One bundle root: a single object's full known statement history.
//! - Includes: `ObjectGenesis`, all known `ObjectRevision` /
//!   `ObjectBranch` / `ObjectVersionTag` for that object, every actor
//!   that signed any of those statements, and every blob those
//!   statements reference (today: the manifest blob hashed into each
//!   `ObjectRevision`).
//! - Excludes: `ActorTrust` statements. Trust is first-person; bundling
//!   trust opinions inside an object bundle would invite reading them
//!   as authority. A separate trust-bundle type can land later.
//! - Excludes: Git history. The manifest declares which Git commit ids
//!   the bundle's statements reference (`git_history.expected_commits`)
//!   and `git_history.included = false`; recipients must obtain those
//!   commits separately to reach `VALID` end-to-end. A future bundle
//!   version flips `included = true` and adds a `git/` subdirectory
//!   carrying the Git pack — at which point import will populate the
//!   `~/.kairo/git/` managed mirror (see TODO §11).
//!
//! # Layout
//!
//! ```text
//! manifest.json
//! actors/<actor-id>.json           # ActorGenesisJson
//! objects/<object-id>.json         # signed ObjectGenesisStatementJson
//! statements/<statement-id>.json   # ObjectRevision / Branch / VersionTag
//! blobs/<blob-id>                  # raw bytes
//! ```

mod error;
mod export;
mod import;
mod manifest;

#[cfg(test)]
mod tests;

pub use error::BundleError;
pub use export::write_bundle;
pub use import::{import_bundle, ImportSummary};
pub use manifest::{
    BundleContents, BundleCreator, BundleGitHistory, BundleManifest, BundleRoots,
};

/// Constant value of the `manifest.schema` field.
pub const BUNDLE_SCHEMA: &str = "kairo.bundle.v1";

/// Filename of the bundle manifest at the bundle root.
pub const MANIFEST_FILENAME: &str = "manifest.json";

/// Subdirectory names within a bundle.
pub mod dirs {
    pub const ACTORS: &str = "actors";
    pub const OBJECTS: &str = "objects";
    pub const STATEMENTS: &str = "statements";
    pub const BLOBS: &str = "blobs";
}
