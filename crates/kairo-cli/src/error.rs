//! CLI error type. Aggregates every failure mode the CLI surfaces to the
//! user. Each variant carries enough context to print a precise message
//! without requiring the caller to chase the source through `Display`.

use std::error::Error;
use std::fmt;
use std::path::PathBuf;

use kairo_bundle::BundleError;
use kairo_core::{ActorId, ObjectId};
use kairo_identity::KeyId;
use kairo_object::SnapshotError;
use kairo_statement::verify::{ActorResolution, SignatureStatus, VerificationReport};
use kairo_statement::{
    ActorTrustShapeError, CapabilityShapeError, ObjectVersionTagShapeError, SemverParseError,
    StatementKindParseError,
};

#[derive(Debug)]
pub(crate) enum CliError {
    ReadManifest {
        path: PathBuf,
        source: std::io::Error,
    },
    ParseManifest(kairo_object::ManifestError),
    ReadStatement {
        path: PathBuf,
        source: std::io::Error,
    },
    ReadPublicKey {
        path: PathBuf,
        source: std::io::Error,
    },
    ReadActorGenesis {
        path: PathBuf,
        source: std::io::Error,
    },
    ParseActorGenesisJson(serde_json::Error),
    ParseActorGenesis(kairo_identity::json::ActorGenesisJsonError),
    ParseStatementJson(serde_json::Error),
    ParseStatement(kairo_statement::json::StatementJsonError),
    ValidateRevisionManifest(kairo_object::RevisionManifestError),
    VerifyStatementSignature(kairo_statement::StatementSignatureError),
    VerificationFailed(Box<VerificationReport>),
    HomeNotSet,
    OpenStore {
        path: PathBuf,
        source: kairo_store::StoreError,
    },
    OpenKeystore {
        path: PathBuf,
        source: kairo_keystore::KeystoreError,
    },
    GenerateKey(kairo_identity::KeyGenerationError),
    ActorGenesisShape(kairo_identity::ActorGenesisShapeError),
    NoAttestationKeyProvided,
    InvalidAttestationKeyHex {
        provided: String,
    },
    WriteKey {
        actor: ActorId,
        source: kairo_keystore::KeystoreError,
    },
    WriteActor {
        actor: ActorId,
        source: kairo_store::StoreError,
    },
    ReadActor {
        actor: ActorId,
        source: kairo_store::StoreError,
    },
    ReadKey {
        actor: ActorId,
        source: kairo_keystore::KeystoreError,
    },
    KeyDoesNotMatchActor {
        actor: ActorId,
    },
    ReadActiveKey {
        actor: ActorId,
        source: kairo_identity::ActorResolveError,
    },
    ActorHasNoActiveKey {
        actor: ActorId,
    },
    WouldBrickActor {
        actor: ActorId,
        key_id: KeyId,
    },
    WriteKeyRotation {
        statement: kairo_core::StatementId,
        source: kairo_store::StoreError,
    },
    WriteKeyRevocation {
        statement: kairo_core::StatementId,
        source: kairo_store::StoreError,
    },
    WriteEmergencyKeyRotation {
        statement: kairo_core::StatementId,
        source: kairo_store::StoreError,
    },
    AttestationKeyNotInSet {
        actor: ActorId,
        key_id: KeyId,
    },
    ReadAttestationSeed {
        path: PathBuf,
        source: std::io::Error,
    },
    InvalidAttestationSeedBase64 {
        path: PathBuf,
    },
    InvalidAttestationSeedLength {
        path: PathBuf,
        actual: usize,
    },
    ReadSignatureFile {
        path: PathBuf,
        source: std::io::Error,
    },
    InvalidSignatureBase64Path {
        path: PathBuf,
    },
    InvalidSignatureLength {
        path: PathBuf,
        actual: usize,
    },
    SignatureNoAttestationMatch {
        actor: ActorId,
    },
    /// The prepared envelope JSON is missing required top-level
    /// fields (`actor`, `created_at`, `signatures`) or has the wrong
    /// shape. Reading a non-attestation-surface envelope here is a
    /// programmer error; emit a clear top-level message rather than
    /// surfacing a deserialization error.
    CosignEnvelopeShape,
    CosignActorMismatch {
        expected: ActorId,
        actual: String,
    },
    CosignKeyNotInAttestationSet {
        actor: ActorId,
        key_id: String,
    },
    CosignDuplicateKeyId {
        actor: ActorId,
        key_id: String,
    },
    ChangeThresholdSignNeedsCosign {
        actor: ActorId,
        current_threshold: u8,
        required: u8,
    },
    ChangeThresholdShape(kairo_statement::ActorAttestationThresholdChangeShapeError),
    WriteThresholdChange {
        statement: kairo_core::StatementId,
        source: kairo_store::StoreError,
    },
    WriteAttestationKeyRevocation {
        statement: kairo_core::StatementId,
        source: kairo_store::StoreError,
    },
    SerializePreparedEnvelope(serde_json::Error),
    WritePreparedEnvelope {
        path: PathBuf,
        source: std::io::Error,
    },
    ReadPreparedEnvelope {
        path: PathBuf,
        source: std::io::Error,
    },
    AddAttestationKeyMissingKeySource,
    AttestationKeyAlreadyInSet {
        actor: ActorId,
        key_id: KeyId,
    },
    AttestationKeySharesSigningKey {
        actor: ActorId,
        key_id: KeyId,
    },
    WriteAttestationKeyAdd {
        statement: kairo_core::StatementId,
        source: kairo_store::StoreError,
    },
    ParseActorId {
        actor: String,
        source: kairo_core::IdError,
    },
    ParseObjectId {
        object: String,
        source: kairo_core::IdError,
    },
    WriteObjectGenesis {
        object: ObjectId,
        source: kairo_store::StoreError,
    },
    WriteRevision {
        statement: kairo_core::StatementId,
        source: kairo_store::StoreError,
    },
    WriteBlob {
        blob: kairo_core::BlobId,
        source: kairo_store::StoreError,
    },
    ReadRevision {
        statement: kairo_core::StatementId,
        source: kairo_store::StoreError,
    },
    ParseStatementId {
        statement: String,
        source: kairo_core::IdError,
    },
    ScanStatements {
        path: PathBuf,
        source: std::io::Error,
    },
    WriteBranch {
        statement: kairo_core::StatementId,
        source: kairo_store::StoreError,
    },
    ReadBranch(kairo_store::StoreError),
    ReadObjectGenesis {
        object: ObjectId,
        source: kairo_store::StoreError,
    },
    BranchObjectMismatch {
        branch_object: ObjectId,
        revision_object: ObjectId,
    },
    BranchNotFound {
        actor: ActorId,
        object: ObjectId,
        name: String,
    },
    ParseSemver(SemverParseError),
    TagShape(ObjectVersionTagShapeError),
    WriteVersionTag {
        statement: kairo_core::StatementId,
        source: kairo_store::StoreError,
    },
    ReadVersionTag(kairo_store::StoreError),
    TagObjectMismatch {
        tag_object: ObjectId,
        revision_object: ObjectId,
    },
    TagNotFound {
        actor: ActorId,
        object: ObjectId,
        version: String,
    },
    RevokeWithoutPriorTag {
        actor: ActorId,
        object: ObjectId,
        version: String,
    },
    TrustShape(ActorTrustShapeError),
    WriteActorTrust {
        statement: kairo_core::StatementId,
        source: kairo_store::StoreError,
    },
    ReadActorTrust(kairo_store::StoreError),
    WithdrawWithoutPriorTrust {
        by_actor: ActorId,
        trusted_actor: ActorId,
    },
    BuildActorSubjectRef {
        actor: ActorId,
        source: kairo_core::IdError,
    },
    BuildStatementSubjectRef {
        statement: kairo_core::StatementId,
        source: kairo_core::IdError,
    },
    CapabilityKindsRequired,
    CapabilityListExclusive,
    ParseStatementKind {
        kind: String,
        source: StatementKindParseError,
    },
    ParseTimestamp {
        value: String,
        source: kairo_core::TimestampError,
    },
    CapabilityShape(CapabilityShapeError),
    ReadCapability(kairo_store::StoreError),
    ReadGrant {
        statement: kairo_core::StatementId,
        source: kairo_store::StoreError,
    },
    WriteCapabilityGrant {
        statement: kairo_core::StatementId,
        source: kairo_store::StoreError,
    },
    WriteCapabilityRevocation {
        statement: kairo_core::StatementId,
        source: kairo_store::StoreError,
    },
    RevokeWrongGrantor {
        grant: kairo_core::StatementId,
        expected: ActorId,
        got: ActorId,
    },
    ListKeystore(kairo_keystore::KeystoreError),
    AmbiguousLocalActor {
        candidates: Vec<ActorId>,
    },
    Bundle(BundleError),
    ComputeSnapshot(SnapshotError),
    ObjectVerificationFailed(String),
    CwdUnavailable {
        source: std::io::Error,
    },
    OpenGitRepo {
        path: PathBuf,
        source: kairo_git::GitError,
    },
    GitRepoNotDiscovered {
        searched_from: PathBuf,
    },
    GitOperation {
        source: kairo_git::GitError,
    },
    ManifestNotUtf8,
    ManifestObjectMismatch {
        manifest_object: ObjectId,
        cli_object: ObjectId,
    },
    BuildSubjectRef {
        object: ObjectId,
        source: kairo_core::IdError,
    },
    MissingPublicKey,
    ConflictingPublicKeyInputs,
    InvalidPublicKeyBase64,
    InvalidPublicKeyLength {
        expected: usize,
        actual: usize,
    },
    /// Failed to build a tokio runtime for an async daemon
    /// command (start / status / stop). Surfaces an OS-level
    /// resource issue.
    DaemonRuntime(std::io::Error),
    /// `kairo_daemon::serve` returned an error during the request
    /// loop (bind failed, store open failed, double-start, etc).
    DaemonServe {
        source: Box<dyn Error + Send + Sync>,
    },
    /// `kairo daemon status --daemon` couldn't reach the daemon.
    /// Maps to exit code 9 (`daemon_unavailable` per `CLI.md`
    /// §7).
    DaemonUnavailable {
        socket: std::path::PathBuf,
    },
    /// Could not read the daemon's PID file (`<store>/daemon.pid`).
    /// Most commonly: no daemon was ever started against this
    /// store, or the daemon shut down cleanly and removed it.
    ReadPid {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    /// PID file exists but its contents do not parse as an i32.
    /// Indicates a corrupted file — surface verbatim so the user
    /// can decide whether to remove it.
    InvalidPid {
        path: std::path::PathBuf,
        contents: String,
    },
    /// `kill(pid, SIGTERM)` failed. Reports `ESRCH` when the PID
    /// in the file is no longer a live process.
    DaemonKill {
        pid: i32,
        source: nix::Error,
    },
    /// `kairo daemon stop --wait` reached its timeout before the
    /// listening socket disappeared.
    DaemonStopTimeout {
        socket: std::path::PathBuf,
        waited: std::time::Duration,
    },
    /// A daemon-mode request through `kairo-daemon-client`
    /// failed for a reason other than a typed 404 (which the
    /// dispatch handler maps to direct-mode-equivalent
    /// `NotFound` variants). Surfaces transport, decode, and
    /// non-404 HTTP errors verbatim.
    DaemonRequestFailed(kairo_daemon_client::ClientError),
}

impl CliError {
    /// Process exit code for this error. Defaults to 1 (general
    /// error). Variants whose mapping is in `specs/CLI.md` §7
    /// override here — slice 4 introduces `daemon_unavailable`
    /// (exit 9); later slices add validation, policy, etc.
    pub(crate) fn exit_code(&self) -> u8 {
        match self {
            Self::DaemonUnavailable { .. } => 9,
            _ => 1,
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadManifest { path, source }
            | Self::ReadStatement { path, source }
            | Self::ReadPublicKey { path, source }
            | Self::ReadActorGenesis { path, source }
            | Self::ReadPid { path, source } => {
                write!(f, "failed to read {}: {source}", path.display())
            }
            Self::ParseManifest(error) => write!(f, "{error}"),
            Self::ParseActorGenesisJson(error) => {
                write!(f, "invalid actor genesis JSON: {error}")
            }
            Self::ParseActorGenesis(error) => write!(f, "{error}"),
            Self::ParseStatementJson(error) => write!(f, "invalid statement JSON: {error}"),
            Self::ParseStatement(error) => write!(f, "{error}"),
            Self::ValidateRevisionManifest(error) => write!(f, "{error}"),
            Self::VerifyStatementSignature(error) => write!(f, "{error}"),
            Self::VerificationFailed(report) => {
                write!(f, "{}", describe_verification_failure(report))
            }
            Self::HomeNotSet => f.write_str(
                "HOME environment variable is not set; use --store to specify a store root",
            ),
            Self::OpenStore { path, source } => {
                write!(f, "failed to open store at {}: {source}", path.display())
            }
            Self::OpenKeystore { path, source } => {
                write!(f, "failed to open keystore at {}: {source}", path.display())
            }
            Self::GenerateKey(error) => write!(f, "{error}"),
            Self::ActorGenesisShape(error) => write!(f, "{error}"),
            Self::NoAttestationKeyProvided => f.write_str(
                "actor create requires at least one attestation key: pass \
                 --attestation-key <hex-pubkey> (operator-presented, recommended) or \
                 --generate-attestation-key (Kairo generates and prints the seed once). \
                 See ACTORS.md §5.5.2.",
            ),
            Self::InvalidAttestationKeyHex { provided } => write!(
                f,
                "invalid attestation key hex (expected 64 lowercase hex chars for raw ed25519 pubkey, got {:?})",
                provided
            ),
            Self::WriteKey { actor, source } => {
                write!(f, "failed to write key for actor {actor}: {source}")
            }
            Self::WriteActor { actor, source } => {
                write!(f, "failed to write actor {actor}: {source}")
            }
            Self::ReadActor { actor, source } => {
                write!(f, "failed to read actor {actor}: {source}")
            }
            Self::ReadKey { actor, source } => {
                write!(f, "failed to read key for actor {actor}: {source}")
            }
            Self::KeyDoesNotMatchActor { actor } => write!(
                f,
                "stored key for actor {actor} does not match the actor's currently active public key"
            ),
            Self::ReadActiveKey { actor, source } => {
                write!(f, "failed to resolve active key for actor {actor}: {source}")
            }
            Self::ActorHasNoActiveKey { actor } => write!(
                f,
                "actor {actor} has no active signing key (rotate-key first)"
            ),
            Self::WouldBrickActor { actor, key_id } => write!(
                f,
                "refusing to revoke {key_id} because it is the only active key for actor {actor}; rotate-key first or pass --brick-actor (see ACTORS.md §5.5.1)"
            ),
            Self::WriteKeyRotation { statement, source } => write!(
                f,
                "failed to write key rotation statement {statement}: {source}"
            ),
            Self::WriteKeyRevocation { statement, source } => write!(
                f,
                "failed to write key revocation statement {statement}: {source}"
            ),
            Self::WriteEmergencyKeyRotation { statement, source } => write!(
                f,
                "failed to write emergency key rotation statement {statement}: {source}"
            ),
            Self::AttestationKeyNotInSet { actor, key_id } => write!(
                f,
                "attestation key {key_id} is not in actor {actor}'s attestation set; check the seed file or use a different attestation key (see ACTORS.md §5.5.2)"
            ),
            Self::ReadAttestationSeed { path, source } => write!(
                f,
                "failed to read attestation seed file {}: {source}",
                path.display()
            ),
            Self::InvalidAttestationSeedBase64 { path } => write!(
                f,
                "attestation seed file {} is not valid base64 (expected the same format printed by `actor create --generate-attestation-key`)",
                path.display()
            ),
            Self::InvalidAttestationSeedLength { path, actual } => write!(
                f,
                "attestation seed file {} decoded to {actual} bytes; expected exactly 32",
                path.display()
            ),
            Self::ReadSignatureFile { path, source } => write!(
                f,
                "failed to read signature file {}: {source}",
                path.display()
            ),
            Self::InvalidSignatureBase64Path { path } => write!(
                f,
                "signature file {} is not valid base64",
                path.display()
            ),
            Self::InvalidSignatureLength { path, actual } => write!(
                f,
                "signature file {} decoded to {actual} bytes; expected exactly 64",
                path.display()
            ),
            Self::SignatureNoAttestationMatch { actor } => write!(
                f,
                "signature did not verify against any of actor {actor}'s attestation keys; check the prepared envelope and signature were paired correctly"
            ),
            Self::CosignEnvelopeShape => write!(
                f,
                "prepared envelope is missing required top-level fields (actor, created_at, signatures)"
            ),
            Self::CosignActorMismatch { expected, actual } => write!(
                f,
                "envelope actor is {actual}, but --actor is {expected}"
            ),
            Self::CosignKeyNotInAttestationSet { actor, key_id } => write!(
                f,
                "key {key_id} is not in actor {actor}'s attestation set at the envelope's created_at"
            ),
            Self::CosignDuplicateKeyId { actor, key_id } => write!(
                f,
                "envelope already carries a signature from key {key_id} for actor {actor}; cosigners must use distinct keys"
            ),
            Self::ChangeThresholdSignNeedsCosign { actor, current_threshold, required } => write!(
                f,
                "this threshold change for actor {actor} requires {required} distinct signature(s) under the asymmetric authority rule (current threshold {current_threshold}). The `sign` convenience flow only works when one signature suffices. Use `prepare` + `co-sign` + `submit` instead. See ACTORS.md §5.5.3."
            ),
            Self::ChangeThresholdShape(error) => write!(f, "{error}"),
            Self::WriteThresholdChange { statement, source } => write!(
                f,
                "failed to write attestation-threshold-change {statement}: {source}"
            ),
            Self::WriteAttestationKeyRevocation { statement, source } => write!(
                f,
                "failed to write attestation-key-revocation {statement}: {source}"
            ),
            Self::SerializePreparedEnvelope(error) => write!(
                f,
                "failed to serialize prepared envelope: {error}"
            ),
            Self::WritePreparedEnvelope { path, source } => write!(
                f,
                "failed to write prepared envelope {}: {source}",
                path.display()
            ),
            Self::ReadPreparedEnvelope { path, source } => write!(
                f,
                "failed to read prepared envelope {}: {source}",
                path.display()
            ),
            Self::AddAttestationKeyMissingKeySource => f.write_str(
                "actor add-attestation-key sign requires either --key <hex> (operator-presented) or --generate (Kairo generates and prints the seed once)",
            ),
            Self::AttestationKeyAlreadyInSet { actor, key_id } => write!(
                f,
                "attestation key {key_id} is already in actor {actor}'s attestation set"
            ),
            Self::AttestationKeySharesSigningKey { actor, key_id } => write!(
                f,
                "attestation key {key_id} collides with one of actor {actor}'s signing keys; the surfaces must stay disjoint (see ACTORS.md §5.1)"
            ),
            Self::WriteAttestationKeyAdd { statement, source } => write!(
                f,
                "failed to write attestation-key-add statement {statement}: {source}"
            ),
            Self::ParseActorId { actor, source } => {
                write!(f, "invalid actor id {actor}: {source}")
            }
            Self::ParseObjectId { object, source } => {
                write!(f, "invalid object id {object}: {source}")
            }
            Self::WriteObjectGenesis { object, source } => {
                write!(f, "failed to write object genesis {object}: {source}")
            }
            Self::WriteBlob { blob, source } => {
                write!(f, "failed to write blob {blob}: {source}")
            }
            Self::WriteRevision { statement, source } => {
                write!(
                    f,
                    "failed to write revision statement {statement}: {source}"
                )
            }
            Self::ReadRevision { statement, source } => {
                write!(f, "failed to read revision statement {statement}: {source}")
            }
            Self::ParseStatementId { statement, source } => {
                write!(f, "invalid statement id {statement}: {source}")
            }
            Self::ScanStatements { path, source } => {
                write!(
                    f,
                    "failed to scan statements at {}: {source}",
                    path.display()
                )
            }
            Self::WriteBranch { statement, source } => {
                write!(f, "failed to write branch statement {statement}: {source}")
            }
            Self::ReadBranch(error) => write!(f, "failed to read branch: {error}"),
            Self::ReadObjectGenesis { object, source } => {
                write!(f, "failed to read object genesis {object}: {source}")
            }
            Self::BranchObjectMismatch {
                branch_object,
                revision_object,
            } => write!(
                f,
                "branch declares object {branch_object} but the pointed-at revision binds to {revision_object}"
            ),
            Self::BranchNotFound { actor, object, name } => write!(
                f,
                "no branch named {name} for actor {actor} on object {object}"
            ),
            Self::ParseSemver(error) => write!(f, "{error}"),
            Self::TagShape(error) => write!(f, "{error}"),
            Self::WriteVersionTag { statement, source } => {
                write!(f, "failed to write version tag statement {statement}: {source}")
            }
            Self::ReadVersionTag(error) => write!(f, "failed to read version tag: {error}"),
            Self::TagObjectMismatch {
                tag_object,
                revision_object,
            } => write!(
                f,
                "tag declares object {tag_object} but the pointed-at revision binds to {revision_object}"
            ),
            Self::TagNotFound {
                actor,
                object,
                version,
            } => write!(
                f,
                "no tag for version {version} from actor {actor} on object {object}"
            ),
            Self::RevokeWithoutPriorTag {
                actor,
                object,
                version,
            } => write!(
                f,
                "cannot revoke version {version} on object {object}: actor {actor} has no prior tag for it"
            ),
            Self::TrustShape(error) => write!(f, "{error}"),
            Self::WriteActorTrust { statement, source } => {
                write!(f, "failed to write actor trust statement {statement}: {source}")
            }
            Self::ReadActorTrust(error) => write!(f, "failed to read actor trust: {error}"),
            Self::WithdrawWithoutPriorTrust {
                by_actor,
                trusted_actor,
            } => write!(
                f,
                "cannot withdraw trust: actor {by_actor} has no prior opinion about {trusted_actor}"
            ),
            Self::BuildActorSubjectRef { actor, source } => write!(
                f,
                "could not build subject reference for actor {actor}: {source}"
            ),
            Self::BuildStatementSubjectRef { statement, source } => write!(
                f,
                "could not build subject reference for statement {statement}: {source}"
            ),
            Self::CapabilityKindsRequired => f.write_str(
                "at least one --kind is required (e.g. --kind ObjectVersionTag)",
            ),
            Self::CapabilityListExclusive => f.write_str(
                "kairo capability list takes exactly one of --grantor <id> or --object <id>",
            ),
            Self::ParseStatementKind { kind, source } => {
                write!(f, "invalid statement kind {kind:?}: {source}")
            }
            Self::ParseTimestamp { value, source } => {
                write!(f, "invalid timestamp {value:?}: {source}")
            }
            Self::CapabilityShape(error) => write!(f, "{error}"),
            Self::ReadCapability(error) => write!(f, "failed to read capability: {error}"),
            Self::ReadGrant { statement, source } => {
                write!(f, "failed to read capability grant {statement}: {source}")
            }
            Self::WriteCapabilityGrant { statement, source } => write!(
                f,
                "failed to write capability grant statement {statement}: {source}"
            ),
            Self::WriteCapabilityRevocation { statement, source } => write!(
                f,
                "failed to write capability revocation statement {statement}: {source}"
            ),
            Self::RevokeWrongGrantor { grant, expected, got } => write!(
                f,
                "cannot revoke grant {grant}: signer {got} does not match the original grantor {expected}"
            ),
            Self::ListKeystore(error) => write!(f, "failed to list keystore: {error}"),
            Self::Bundle(error) => write!(f, "{error}"),
            Self::AmbiguousLocalActor { candidates } => {
                f.write_str(
                    "multiple local actors found in keystore; pass --as <actor-id> to choose, or --no-as to skip trust evaluation. candidates:\n",
                )?;
                for actor in candidates {
                    writeln!(f, "  {actor}")?;
                }
                Ok(())
            }
            Self::ComputeSnapshot(error) => write!(f, "{error}"),
            Self::ObjectVerificationFailed(report) => {
                f.write_str(report)?;
                f.write_str("object verification reported INVALID")
            }
            Self::CwdUnavailable { source } => {
                write!(f, "could not read current working directory: {source}")
            }
            Self::OpenGitRepo { path, source } => {
                write!(f, "failed to open Git repository at {}: {source}", path.display())
            }
            Self::GitRepoNotDiscovered { searched_from } => write!(
                f,
                "no Git repository discovered from {}; pass --repo to specify one or --no-repo to skip Git lookup",
                searched_from.display()
            ),
            Self::GitOperation { source } => write!(f, "{source}"),
            Self::ManifestNotUtf8 => f.write_str("kairo.toml in commit tree is not valid UTF-8"),
            Self::ManifestObjectMismatch {
                manifest_object,
                cli_object,
            } => write!(
                f,
                "manifest declares object {manifest_object} but --object is {cli_object}"
            ),
            Self::BuildSubjectRef { object, source } => {
                write!(
                    f,
                    "could not build subject reference for object {object}: {source}"
                )
            }
            Self::MissingPublicKey => f.write_str("missing --public-key or --public-key-file"),
            Self::ConflictingPublicKeyInputs => {
                f.write_str("use only one of --public-key or --public-key-file")
            }
            Self::InvalidPublicKeyBase64 => f.write_str("invalid public key base64"),
            Self::InvalidPublicKeyLength { expected, actual } => {
                write!(f, "invalid public key length {actual}; expected {expected}")
            }
            Self::DaemonRuntime(error) => {
                write!(f, "failed to build async runtime: {error}")
            }
            Self::DaemonServe { source } => write!(f, "daemon serve loop failed: {source}"),
            Self::DaemonUnavailable { socket } => write!(
                f,
                "daemon is not running at {} (--daemon was set)",
                socket.display()
            ),
            Self::InvalidPid { path, contents } => write!(
                f,
                "PID file {} does not contain a valid i32 (got {contents:?})",
                path.display()
            ),
            Self::DaemonKill { pid, source } => {
                write!(f, "kill(pid={pid}, SIGTERM) failed: {source}")
            }
            Self::DaemonStopTimeout { socket, waited } => write!(
                f,
                "daemon did not stop within {:.0}s (socket {} still present)",
                waited.as_secs_f64(),
                socket.display()
            ),
            Self::DaemonRequestFailed(error) => write!(f, "daemon request failed: {error}"),
        }
    }
}

impl Error for CliError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ReadManifest { source, .. }
            | Self::ReadStatement { source, .. }
            | Self::ReadPublicKey { source, .. }
            | Self::ReadActorGenesis { source, .. }
            | Self::ScanStatements { source, .. }
            | Self::CwdUnavailable { source }
            | Self::ReadAttestationSeed { source, .. }
            | Self::ReadSignatureFile { source, .. }
            | Self::WritePreparedEnvelope { source, .. }
            | Self::ReadPreparedEnvelope { source, .. }
            | Self::ReadPid { source, .. } => Some(source),
            Self::ParseManifest(error) => Some(error),
            Self::ParseActorGenesisJson(error)
            | Self::ParseStatementJson(error)
            | Self::SerializePreparedEnvelope(error) => Some(error),
            Self::ParseActorGenesis(error) => Some(error),
            Self::ParseStatement(error) => Some(error),
            Self::ValidateRevisionManifest(error) => Some(error),
            Self::VerifyStatementSignature(error) => Some(error),
            Self::OpenStore { source, .. }
            | Self::WriteActor { source, .. }
            | Self::ReadActor { source, .. }
            | Self::WriteObjectGenesis { source, .. }
            | Self::WriteRevision { source, .. }
            | Self::WriteBlob { source, .. }
            | Self::ReadRevision { source, .. }
            | Self::WriteBranch { source, .. }
            | Self::WriteVersionTag { source, .. }
            | Self::WriteActorTrust { source, .. }
            | Self::WriteCapabilityGrant { source, .. }
            | Self::WriteCapabilityRevocation { source, .. }
            | Self::WriteKeyRotation { source, .. }
            | Self::WriteKeyRevocation { source, .. }
            | Self::WriteEmergencyKeyRotation { source, .. }
            | Self::WriteAttestationKeyAdd { source, .. }
            | Self::WriteThresholdChange { source, .. }
            | Self::WriteAttestationKeyRevocation { source, .. }
            | Self::ReadGrant { source, .. }
            | Self::ReadObjectGenesis { source, .. } => Some(source),
            Self::ReadActiveKey { source, .. } => Some(source),
            Self::ReadBranch(error)
            | Self::ReadVersionTag(error)
            | Self::ReadActorTrust(error)
            | Self::ReadCapability(error) => Some(error),
            Self::ParseSemver(error) => Some(error),
            Self::TagShape(error) => Some(error),
            Self::TrustShape(error) => Some(error),
            Self::CapabilityShape(error) => Some(error),
            Self::ComputeSnapshot(error) => Some(error),
            Self::OpenKeystore { source, .. }
            | Self::WriteKey { source, .. }
            | Self::ReadKey { source, .. } => Some(source),
            Self::ListKeystore(error) => Some(error),
            Self::Bundle(error) => Some(error),
            Self::ParseActorId { source, .. }
            | Self::ParseObjectId { source, .. }
            | Self::ParseStatementId { source, .. }
            | Self::BuildSubjectRef { source, .. }
            | Self::BuildActorSubjectRef { source, .. }
            | Self::BuildStatementSubjectRef { source, .. } => Some(source),
            Self::ParseStatementKind { source, .. } => Some(source),
            Self::ParseTimestamp { source, .. } => Some(source),
            Self::GenerateKey(error) => Some(error),
            Self::ActorGenesisShape(error) => Some(error),
            Self::OpenGitRepo { source, .. } | Self::GitOperation { source } => Some(source),
            Self::VerificationFailed(_)
            | Self::ObjectVerificationFailed(_)
            | Self::HomeNotSet
            | Self::KeyDoesNotMatchActor { .. }
            | Self::ManifestObjectMismatch { .. }
            | Self::BranchObjectMismatch { .. }
            | Self::BranchNotFound { .. }
            | Self::TagObjectMismatch { .. }
            | Self::TagNotFound { .. }
            | Self::RevokeWithoutPriorTag { .. }
            | Self::WithdrawWithoutPriorTrust { .. }
            | Self::CapabilityKindsRequired
            | Self::CapabilityListExclusive
            | Self::RevokeWrongGrantor { .. }
            | Self::AmbiguousLocalActor { .. }
            | Self::GitRepoNotDiscovered { .. }
            | Self::ManifestNotUtf8
            | Self::MissingPublicKey
            | Self::ConflictingPublicKeyInputs
            | Self::InvalidPublicKeyBase64
            | Self::InvalidPublicKeyLength { .. }
            | Self::ActorHasNoActiveKey { .. }
            | Self::WouldBrickActor { .. }
            | Self::NoAttestationKeyProvided
            | Self::InvalidAttestationKeyHex { .. }
            | Self::AttestationKeyNotInSet { .. }
            | Self::InvalidAttestationSeedBase64 { .. }
            | Self::InvalidAttestationSeedLength { .. }
            | Self::InvalidSignatureBase64Path { .. }
            | Self::InvalidSignatureLength { .. }
            | Self::SignatureNoAttestationMatch { .. }
            | Self::CosignEnvelopeShape
            | Self::CosignActorMismatch { .. }
            | Self::CosignKeyNotInAttestationSet { .. }
            | Self::CosignDuplicateKeyId { .. }
            | Self::ChangeThresholdSignNeedsCosign { .. }
            | Self::AddAttestationKeyMissingKeySource
            | Self::AttestationKeyAlreadyInSet { .. }
            | Self::AttestationKeySharesSigningKey { .. }
            | Self::DaemonUnavailable { .. }
            | Self::InvalidPid { .. }
            | Self::DaemonStopTimeout { .. } => None,
            Self::ChangeThresholdShape(error) => Some(error),
            Self::DaemonRuntime(error) => Some(error),
            Self::DaemonServe { source } => Some(source.as_ref()),
            Self::DaemonKill { source, .. } => Some(source),
            Self::DaemonRequestFailed(error) => Some(error),
        }
    }
}

fn describe_verification_failure(report: &VerificationReport) -> String {
    let mut parts = Vec::new();
    match &report.actor {
        ActorResolution::Resolved => {}
        ActorResolution::NotFound => parts.push(format!(
            "actor {} could not be resolved",
            report.envelope_actor
        )),
        ActorResolution::ResolverUnavailable(reason) => {
            parts.push(format!("actor resolver unavailable: {reason}"));
        }
        ActorResolution::SignatureActorMismatch => parts.push(format!(
            "signature actor {} does not match envelope actor {}",
            report.signature_actor, report.envelope_actor
        )),
    }
    match &report.signature {
        SignatureStatus::Valid | SignatureStatus::NotEvaluated => {}
        SignatureStatus::Invalid => parts.push("signature did not verify".to_owned()),
        SignatureStatus::UnsupportedAlgorithm(algorithm) => {
            parts.push(format!("unsupported signature algorithm {algorithm}"));
        }
        SignatureStatus::Malformed {
            expected_len,
            actual_len,
        } => parts.push(format!(
            "malformed signature length {actual_len}; expected {expected_len}"
        )),
        SignatureStatus::AlgorithmMismatch => {
            parts.push("signature algorithm does not match resolved key".to_owned());
        }
        SignatureStatus::KeyMismatch {
            signature_key_id,
            active_key_id,
        } => parts.push(format!(
            "signature key {signature_key_id} is not the actor's active key at this causal position (active key is {active_key_id})"
        )),
        SignatureStatus::KeyRevoked => {
            parts.push("signing key is revoked at this causal position".to_owned());
        }
        SignatureStatus::NoActiveKey => {
            parts.push("actor has no active signing key at this causal position".to_owned());
        }
        SignatureStatus::NotInAttestationSet { signature_key_id } => {
            parts.push(format!(
                "signature key {signature_key_id} is not in the actor's attestation set at this causal position"
            ));
        }
        SignatureStatus::BelowThreshold { provided, required } => {
            parts.push(format!(
                "multi-sig envelope has {provided} signature(s); required {required} (M-of-N attestation threshold)"
            ));
        }
    }
    if parts.is_empty() {
        "verification failed".to_owned()
    } else {
        parts.join("; ")
    }
}
