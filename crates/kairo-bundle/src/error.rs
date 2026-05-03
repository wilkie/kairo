use std::fmt;
use std::io;
use std::path::PathBuf;

#[derive(Debug)]
pub enum BundleError {
    /// I/O failure reading or writing a bundle file.
    Io { path: PathBuf, source: io::Error },
    /// `manifest.json` could not be parsed.
    ManifestParse(serde_json::Error),
    /// `manifest.schema` is not the supported version.
    UnsupportedSchema { found: String, expected: &'static str },
    /// A record file could not be JSON-parsed.
    RecordParse {
        path: PathBuf,
        source: serde_json::Error,
    },
    /// A statement's `type` field is not one we ship in MVP bundles
    /// (e.g. an `ActorTrust` accidentally placed in `statements/`).
    UnsupportedStatementType {
        path: PathBuf,
        statement_type: String,
    },
    /// A typed payload could not be reconstructed from JSON.
    StatementShape {
        path: PathBuf,
        message: String,
    },
    /// A record's filename did not parse as the expected id type.
    BadIdFilename {
        path: PathBuf,
        kind: &'static str,
        source: kairo_core::IdError,
    },
    /// A record's content-addressed id did not match its filename or
    /// its declared id field. Bundles are fixity-only — a mismatch
    /// here is a hard import failure, never silently repaired.
    FixityMismatch {
        path: PathBuf,
        expected: String,
        actual: String,
    },
    /// A blob in `blobs/` does not match its filename hash.
    BlobHashMismatch {
        path: PathBuf,
        expected: String,
        actual: String,
    },
    /// The manifest's `contents` listed an id that no file in the
    /// bundle actually backs.
    MissingRecord {
        kind: &'static str,
        id: String,
    },
    /// A statement in the bundle references an actor that the bundle
    /// itself does not include. Bundles must be self-contained for the
    /// signing actors of the statements they carry.
    DanglingActor { statement: String, actor: String },
    /// Underlying store call failed.
    Store(kairo_store::StoreError),
    /// Underlying object-genesis read failed.
    ObjectGenesisLookup(kairo_store::StoreError),
    /// Bundle export was asked for an object the store has no genesis
    /// for.
    RootObjectNotFound { object: String },
    /// The destination passed to `write_bundle` already contains files
    /// that are not part of this bundle. Refuse rather than overwrite.
    DestinationNotEmpty { path: PathBuf },
}

impl fmt::Display for BundleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "bundle I/O error at {}: {source}", path.display())
            }
            Self::ManifestParse(error) => write!(f, "invalid bundle manifest: {error}"),
            Self::UnsupportedSchema { found, expected } => write!(
                f,
                "unsupported bundle schema {found:?}; this build supports {expected:?}"
            ),
            Self::RecordParse { path, source } => {
                write!(f, "failed to parse {}: {source}", path.display())
            }
            Self::UnsupportedStatementType {
                path,
                statement_type,
            } => write!(
                f,
                "{} declares unsupported statement type {statement_type:?} for an MVP bundle",
                path.display()
            ),
            Self::StatementShape { path, message } => {
                write!(f, "{} could not be reconstructed: {message}", path.display())
            }
            Self::BadIdFilename { path, kind, source } => write!(
                f,
                "{} has an invalid {kind} id in its filename: {source}",
                path.display()
            ),
            Self::FixityMismatch {
                path,
                expected,
                actual,
            } => write!(
                f,
                "fixity check failed for {}: expected {expected}, derived {actual}",
                path.display()
            ),
            Self::BlobHashMismatch {
                path,
                expected,
                actual,
            } => write!(
                f,
                "blob hash mismatch for {}: expected {expected}, computed {actual}",
                path.display()
            ),
            Self::MissingRecord { kind, id } => {
                write!(f, "manifest lists {kind} {id} but no record file is present")
            }
            Self::DanglingActor { statement, actor } => write!(
                f,
                "statement {statement} is signed by {actor}, but the bundle does not include that actor"
            ),
            Self::Store(error) => write!(f, "store error: {error}"),
            Self::ObjectGenesisLookup(error) => {
                write!(f, "could not load ObjectGenesis: {error}")
            }
            Self::RootObjectNotFound { object } => {
                write!(f, "no ObjectGenesis found for root object {object}")
            }
            Self::DestinationNotEmpty { path } => write!(
                f,
                "refusing to write bundle into non-empty destination {}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for BundleError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::ManifestParse(error) | Self::RecordParse { source: error, .. } => Some(error),
            Self::BadIdFilename { source, .. } => Some(source),
            Self::Store(error) | Self::ObjectGenesisLookup(error) => Some(error),
            _ => None,
        }
    }
}

impl From<kairo_store::StoreError> for BundleError {
    fn from(error: kairo_store::StoreError) -> Self {
        Self::Store(error)
    }
}
