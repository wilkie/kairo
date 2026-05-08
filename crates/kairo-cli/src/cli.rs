//! Top-level clap definitions for the `kairo` binary. Every subcommand
//! enum and the root `Cli` struct lives here; the dispatch and the
//! command runners live elsewhere (`commands/`, `error`, etc.). This
//! split keeps the user-facing CLI surface in one file rather than
//! interleaved with implementation logic.

use std::path::PathBuf;

use clap::{ArgAction, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "kairo", version)]
pub(crate) struct Cli {
    /// Override the store root (default ~/.kairo).
    #[arg(long, env = "KAIRO_STORE", global = true)]
    pub(crate) store: Option<PathBuf>,

    /// Override the keystore directory (default <store>/keys).
    #[arg(long, env = "KAIRO_KEYS", global = true)]
    pub(crate) keys: Option<PathBuf>,

    /// Require the daemon for read commands; exit 9 when the
    /// daemon is unreachable. Without this flag, read commands
    /// silently fall back to direct mode (probe-and-fall-back —
    /// `specs/CLI.md` §3.3). Slice 4 only consumes this flag in
    /// `kairo daemon status`; slice 8 wires the rest of the
    /// dispatch.
    #[arg(long, global = true)]
    pub(crate) daemon: bool,

    /// Force direct/local mode (skip the daemon even if it is
    /// reachable). Mutually exclusive with `--daemon`. Wired into
    /// dispatch in slice 8.
    #[arg(long, global = true, conflicts_with = "daemon")]
    pub(crate) direct: bool,

    /// Like `--direct`, plus refuse network / federation
    /// operations. Wired into dispatch in slice 8 / phase 4.
    #[arg(long, global = true, conflicts_with = "daemon")]
    pub(crate) offline: bool,

    #[command(subcommand)]
    pub(crate) command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Work with actors.
    Actor {
        #[command(subcommand)]
        command: ActorCommand,
    },
    /// Work with kairo.toml manifests.
    Manifest {
        #[command(subcommand)]
        command: ManifestCommand,
    },
    /// Work with Objects.
    Object {
        #[command(subcommand)]
        command: ObjectSubcommand,
    },
    /// Work with Object revisions.
    Revision {
        #[command(subcommand)]
        command: RevisionCommand,
    },
    /// Work with named, mutable revision pointers (ObjectBranch).
    Branch {
        #[command(subcommand)]
        command: BranchCommand,
    },
    /// Work with semver-named release pointers (ObjectVersionTag).
    Tag {
        #[command(subcommand)]
        command: TagCommand,
    },
    /// Work with first-person trust opinions (ActorTrust).
    Trust {
        #[command(subcommand)]
        command: TrustCommand,
    },
    /// Work with cross-actor capability grants (ActorCapabilityGrant /
    /// ActorCapabilityRevocation). See specs/CAPABILITIES.md.
    Capability {
        #[command(subcommand)]
        command: CapabilityCommand,
    },
    /// Export and import portable directory bundles for an object.
    Bundle {
        #[command(subcommand)]
        command: BundleCommand,
    },
    /// Compute a SnapshotId for an object's effective state.
    Snapshot {
        #[command(subcommand)]
        command: SnapshotCommand,
    },
    /// Verify objects, statements, and the bindings between them.
    Verify {
        #[command(subcommand)]
        command: VerifyCommand,
    },
    /// Manage the local Git cache (`<store>/git/`). Fetch commits an
    /// object's `ObjectRevision` statements name into the cache, or
    /// inspect cache state. See `specs/DECISIONS.md` §7–§9.
    Git {
        #[command(subcommand)]
        command: GitCommand,
    },
    /// Manage the local Kairo daemon process. The daemon serves
    /// the read-only HTTP+JSON API on `<store>/daemon.sock`
    /// (`specs/DAEMON.md`, `specs/PHASE_2_DAEMON.md`).
    Daemon {
        #[command(subcommand)]
        command: DaemonCommand,
    },
    /// Manage the local Kairo web server (`kairo-web`). The web
    /// server is the browser-facing TCP front-end that proxies
    /// `/api/v1/*` to the daemon's Unix socket and serves the
    /// SPA bundle (`specs/DECISIONS.md` §12).
    Web {
        #[command(subcommand)]
        command: WebCommand,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum DaemonCommand {
    /// Run the daemon in the foreground until SIGTERM/SIGINT.
    /// Foreground only in v1 (`specs/DECISIONS.md` §10.1); users
    /// supervise via systemd / launchd / tmux / `&` / `nohup`.
    Start,
    /// Probe the daemon and print its status, or "not running"
    /// when unreachable. With the global `--daemon` flag, exits
    /// 9 (`daemon_unavailable`) instead of 0 when not running.
    Status,
    /// Send SIGTERM to the daemon's PID and (optionally) wait
    /// for it to exit. Errors when the PID file is missing or
    /// the recorded process is not running.
    Stop {
        /// Block until the daemon's listening socket disappears
        /// or the timeout is reached (default 10s).
        #[arg(long)]
        wait: bool,
        /// How long `--wait` will poll before giving up.
        #[arg(long, default_value = "10", value_name = "SECONDS")]
        wait_timeout: u64,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum WebCommand {
    /// Run the web server in the foreground until SIGTERM/SIGINT.
    /// Foreground only in v1 (`specs/DECISIONS.md` §12.7); users
    /// supervise via systemd / launchd / tmux / `&` / `nohup`.
    Start {
        /// Filesystem path of the built SPA bundle. Required —
        /// no convention default for v1.
        #[arg(long, value_name = "PATH")]
        spa_dir: std::path::PathBuf,
        /// TCP address to listen on. Must be a loopback address;
        /// non-loopback values are rejected at startup. Default
        /// `127.0.0.1:7878`.
        #[arg(long, value_name = "ADDR")]
        bind: Option<String>,
    },
    /// Probe the web server and print its status, or "not running"
    /// when unreachable. Default probe address `127.0.0.1:7878`;
    /// override with `--bind`.
    Status {
        /// TCP address to probe. Default `127.0.0.1:7878`.
        #[arg(long, value_name = "ADDR")]
        bind: Option<String>,
    },
    /// Send SIGTERM to the web server's PID and (optionally) wait
    /// for it to exit. Errors when the PID file is missing or the
    /// recorded process is not running.
    Stop {
        /// Block until the web server's PID file disappears or
        /// the timeout is reached (default 10s).
        #[arg(long)]
        wait: bool,
        /// How long `--wait` will poll before giving up.
        #[arg(long, default_value = "10", value_name = "SECONDS")]
        wait_timeout: u64,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum GitCommand {
    /// Fetch a branch from a remote Git URL into the cache.
    ///
    /// Lands the commits in the shared object pool and writes
    /// `refs/heads/<branch>` in the per-object cache repo. Subsequent
    /// `kairo verify object --object <id>` calls (with the default
    /// cache-first precedence) can resolve the storage commit
    /// without a cwd repo.
    Fetch {
        /// Object id whose cache repo the fetched ref will land in.
        #[arg(long)]
        object: String,
        /// Remote Git URL (`https://`, `ssh://`, `file://`, `git://`).
        #[arg(long)]
        remote: String,
        /// Branch name to fetch (e.g. `main`). Stripped of any
        /// leading `refs/heads/` prefix. Defaults to `main`.
        #[arg(long, default_value = "main")]
        branch: String,
    },
    /// Inspect cache state.
    Cache {
        #[command(subcommand)]
        command: GitCacheCommand,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum GitCacheCommand {
    /// Print the cache layout: pool initialization state and every
    /// per-object cache repo with its head refs.
    Status,
}

#[derive(Debug, Subcommand)]
pub(crate) enum VerifyCommand {
    /// Verify an object end-to-end through the local store.
    ///
    /// Loads the `ObjectGenesis`, resolves the chosen `ObjectRevision`
    /// (default: creator-actor's `head` branch; override with `--actor`,
    /// `--name`, or `--statement`), verifies the revision's signature
    /// against the resolved actor, looks the storage commit up in a
    /// Git repository (default: discovered upward from the current
    /// directory; override with `--repo`), and validates the manifest
    /// binding by reading `kairo.toml` from the commit's tree. Pass
    /// `--manifest <path>` to override the tree-derived manifest, or
    /// `--no-repo` to skip the Git lookup entirely.
    Object {
        /// Object whose verification report to compute.
        #[arg(long)]
        object: String,
        /// Pin the frontier to a specific ObjectRevision statement,
        /// bypassing branch resolution. Conflicts with --actor / --name.
        #[arg(long, conflicts_with_all = ["actor", "name"])]
        statement: Option<String>,
        /// Actor whose branch tip to follow. Defaults to ObjectGenesis.created_by.
        #[arg(long)]
        actor: Option<String>,
        /// Branch name (defaults to "head").
        #[arg(long, default_value = "head")]
        name: String,
        /// Truster whose perspective to evaluate trust from. Defaults
        /// to the sole local actor (the only key in the keystore); if
        /// the keystore has multiple keys, you must pass --as.
        /// `--no-as` skips trust evaluation entirely (report says
        /// `unevaluated`).
        #[arg(long, conflicts_with = "no_as")]
        r#as: Option<String>,
        /// Skip trust evaluation. Trust stays `unevaluated` regardless
        /// of what is in the keystore. Conflicts with --as.
        #[arg(long)]
        no_as: bool,
        /// Path to a Git repository (working tree or .git directory).
        /// When set, this path is the only Git source consulted —
        /// neither the managed Git cache nor cwd discovery is tried.
        /// Conflicts with --no-repo.
        #[arg(long, conflicts_with = "no_repo")]
        repo: Option<PathBuf>,
        /// Skip Git lookup entirely. Content-layer check stays
        /// INDETERMINATE; without `--manifest`, manifest binding does
        /// too. Conflicts with --repo.
        #[arg(long)]
        no_repo: bool,
        /// Skip the managed Git cache (`<store>/git/`) when
        /// resolving the storage commit. By default verify-object
        /// consults the cache first (for `git:sha256:` revisions
        /// where the per-object cache repo exists and contains the
        /// commit), then falls back to the cwd-discovered repo.
        /// Pass this to force a cwd-only lookup.
        #[arg(long)]
        no_cache: bool,
        /// Skip cwd-upward Git discovery. Useful for hermetic
        /// verification against the managed cache only (no
        /// dependence on whatever working tree happens to be
        /// checked out). When set together with a cache miss, the
        /// content layer reports INDETERMINATE rather than erroring.
        #[arg(long)]
        no_cwd_repo: bool,
        /// Override the kairo.toml manifest (otherwise read from the
        /// commit's tree at `kairo.toml`). Useful when verifying a
        /// revision that named a non-default manifest path.
        #[arg(long)]
        manifest: Option<PathBuf>,
        /// Emit a stable JSON representation of the report.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum TagCommand {
    /// Sign a new ObjectVersionTag binding version to revision. If the
    /// actor has previously published a tag for (object, version), the
    /// new statement supersedes it; otherwise it is the genesis tag.
    Bind {
        /// Actor whose key signs the tag.
        #[arg(long)]
        actor: String,
        /// Object whose lineage the tag belongs to.
        #[arg(long)]
        object: String,
        /// Strict semver 2.0.0 version string (e.g. 1.2.3, 1.2.3-rc.1).
        #[arg(long)]
        version: String,
        /// StatementId of the ObjectRevision the tag points at.
        #[arg(long)]
        revision: String,
    },
    /// Sign a new ObjectVersionTag that withdraws (actor, object, version).
    /// Requires a prior tag to revoke; the supersedes pointer is auto-set
    /// to the actor's current head for that version.
    Revoke {
        #[arg(long)]
        actor: String,
        #[arg(long)]
        object: String,
        #[arg(long)]
        version: String,
    },
    /// Resolve and print the current tag head for (actor, object, version).
    Show {
        #[arg(long)]
        object: String,
        /// Actor whose tag to resolve. Defaults to ObjectGenesis.created_by.
        #[arg(long)]
        actor: Option<String>,
        #[arg(long)]
        version: String,
        /// Emit a stable JSON representation.
        #[arg(long)]
        json: bool,
    },
    /// List all known (actor, version) tag heads for an object.
    List {
        #[arg(long)]
        object: String,
    },
    /// Walk the supersedes chain backwards from the current head, newest
    /// first. Missing chain links are reported as indeterminate.
    History {
        #[arg(long)]
        object: String,
        #[arg(long)]
        actor: Option<String>,
        #[arg(long)]
        version: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum TrustCommand {
    /// Sign a new ActorTrust granting trust from --by to --of. If the
    /// truster has previously published an opinion about --of, the new
    /// statement supersedes it; otherwise it is the genesis opinion.
    Grant {
        /// Truster: the local actor whose key signs this opinion.
        #[arg(long)]
        by: String,
        /// Trusted actor: the actor being judged.
        #[arg(long)]
        of: String,
        /// Optional human-readable reason. Included in canonical bytes.
        #[arg(long)]
        reason: Option<String>,
    },
    /// Sign a new ActorTrust marking --of as untrusted from --by's
    /// perspective. Auto-supersedes any prior opinion.
    Block {
        #[arg(long)]
        by: String,
        #[arg(long)]
        of: String,
        #[arg(long)]
        reason: Option<String>,
    },
    /// Sign a new ActorTrust withdrawing --by's prior opinion about
    /// --of. Requires a prior opinion to chain off of; the supersedes
    /// pointer is auto-set to the truster's current head.
    Withdraw {
        #[arg(long)]
        by: String,
        #[arg(long)]
        of: String,
        #[arg(long)]
        reason: Option<String>,
    },
    /// Resolve and print --by's current opinion about --of. A missing
    /// opinion is reported as Unknown rather than an error.
    Show {
        #[arg(long)]
        by: String,
        #[arg(long)]
        of: String,
        /// Emit a stable JSON representation.
        #[arg(long)]
        json: bool,
    },
    /// List all current opinions signed by --by, one per trusted actor.
    List {
        #[arg(long)]
        by: String,
    },
    /// Walk the supersedes chain backwards from --by's current opinion
    /// about --of, newest first. Missing chain links are reported as
    /// indeterminate.
    History {
        #[arg(long)]
        by: String,
        #[arg(long)]
        of: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum CapabilityCommand {
    /// Sign a new ActorCapabilityGrant from --grantor to --grantee on
    /// --object. If a chain head already exists for the (grantor,
    /// grantee, object) triple, the new statement supersedes it;
    /// otherwise it is the genesis grant. Pass --kind once per
    /// statement kind to authorize.
    Grant {
        /// Grantor: the local actor whose key signs this delegation.
        #[arg(long)]
        grantor: String,
        /// Grantee: the actor being authorized.
        #[arg(long)]
        grantee: String,
        /// Object whose surface this grant covers.
        #[arg(long)]
        object: String,
        /// Statement kind the grantee may issue. Repeat for multiple
        /// (e.g. `--kind ObjectVersionTag --kind ObjectBranch`).
        #[arg(long = "kind")]
        kinds: Vec<String>,
        /// Allow the grantee to further re-grant this capability.
        #[arg(long)]
        delegable: bool,
        /// RFC 3339 UTC seconds. Grant invalid for statements created
        /// strictly after this timestamp.
        #[arg(long)]
        expires_at: Option<String>,
        /// Maximum re-grant chain depth (0..=255).
        #[arg(long)]
        max_delegation_depth: Option<u8>,
        /// Bind the grant to a specific grantor signing key. Revoking
        /// that key auto-invalidates the grant. See
        /// specs/CAPABILITIES.md §7.2.
        #[arg(long)]
        key_pinned: Option<String>,
    },
    /// Sign an ActorCapabilityRevocation against --grant. The local
    /// signer must be the grant's original grantor (cross-grantor
    /// revocation is invalid in v1).
    Revoke {
        /// Grantor: the actor whose key signs the revocation. Must
        /// equal the grant's signer.
        #[arg(long)]
        grantor: String,
        /// StatementId of the ActorCapabilityGrant being revoked.
        #[arg(long)]
        grant: String,
        /// Invalidate the grant from inception (every statement
        /// issued under it is re-evaluated). Default revocation only
        /// invalidates statements created strictly after the
        /// revocation. See specs/CAPABILITIES.md §6.3.
        #[arg(long)]
        retroactive: bool,
        /// Optional human-readable reason. Included in canonical bytes.
        #[arg(long)]
        reason: Option<String>,
    },
    /// List capability chain heads. Either `--grantor <id>` (audit
    /// what an actor has delegated) or `--object <id>` (cross-cutting
    /// view of who holds capabilities on an object). Exactly one of
    /// the two flags is required.
    List {
        /// List heads of grants signed by this grantor.
        #[arg(long, conflicts_with = "object")]
        grantor: Option<String>,
        /// List heads of grants on this object across grantors.
        #[arg(long)]
        object: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum BundleCommand {
    /// Write a portable directory bundle for an object: its
    /// `ObjectGenesis`, every known `ObjectRevision` / `ObjectBranch`
    /// / `ObjectVersionTag` for it, every signing actor, and every
    /// referenced blob. `ActorTrust` statements are intentionally
    /// excluded; trust is first-person and does not transport with
    /// object data. The destination directory must be empty (or not
    /// exist).
    Export {
        /// Object whose bundle to write.
        #[arg(long)]
        object: String,
        /// Destination directory for the bundle. Created if missing.
        #[arg(long)]
        output: PathBuf,
        /// Pack the object's commits from the managed Git cache
        /// into `<output>/git/<object-id>.pack` and set
        /// `git_history.included = true` in the manifest. The
        /// per-object cache repo must already contain the commits
        /// (typically via `kairo git fetch` or a prior bundle
        /// import). Default off keeps bundle exports cheap and
        /// predictable; opt in when shipping self-contained
        /// federation/archival packages. See `DECISIONS.md` §9.
        #[arg(long)]
        include_git: bool,
    },
    /// Read a directory bundle and ingest its contents into the local
    /// store. Every record is fixity-checked: ids are re-derived from
    /// the canonical bytes and rejected on mismatch. Idempotent —
    /// re-importing the same bundle is a no-op.
    Import {
        /// Bundle directory to read.
        #[arg(long)]
        input: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum SnapshotCommand {
    /// Resolve a snapshot for an object and print its SnapshotId.
    ///
    /// By default, follows the creator-actor's "head" branch. Override
    /// resolution with --actor, --name, or pin the frontier directly with
    /// --statement.
    Compute {
        /// Object whose snapshot to compute.
        #[arg(long)]
        object: String,
        /// Pin the frontier to a specific ObjectRevision statement,
        /// bypassing branch resolution. Conflicts with --actor and --name.
        #[arg(long, conflicts_with_all = ["actor", "name"])]
        statement: Option<String>,
        /// Actor whose branch tip to follow. Defaults to ObjectGenesis.created_by.
        #[arg(long)]
        actor: Option<String>,
        /// Branch name (defaults to "head").
        #[arg(long, default_value = "head")]
        name: String,
        /// Emit a stable JSON representation.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum BranchCommand {
    /// Sign a new ObjectBranch statement that points name at revision and
    /// supersedes any earlier branch with the same (actor, object, name).
    Set {
        /// Actor whose key signs the branch update.
        #[arg(long)]
        actor: String,
        /// Object whose lineage the branch belongs to.
        #[arg(long)]
        object: String,
        /// StatementId of the ObjectRevision the branch points at.
        #[arg(long)]
        revision: String,
        /// Branch name (defaults to "head").
        #[arg(long, default_value = "head")]
        name: String,
    },
    /// Resolve and print the current branch tip for (actor, object, name).
    Show {
        #[arg(long)]
        object: String,
        /// Actor whose branch tip to resolve. Defaults to ObjectGenesis.created_by.
        #[arg(long)]
        actor: Option<String>,
        /// Branch name (defaults to "head").
        #[arg(long, default_value = "head")]
        name: String,
        /// Emit a stable JSON representation.
        #[arg(long)]
        json: bool,
    },
    /// List all known (actor, name) branch tips for an object.
    List {
        #[arg(long)]
        object: String,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum ActorCommand {
    /// Derive an ActorId from an ActorGenesis JSON document.
    Id {
        #[arg(long)]
        genesis: PathBuf,
    },
    /// Generate a fresh actor (keypair + ActorGenesis) and persist it.
    ///
    /// Every actor needs at least one cold-storage attestation key
    /// declared at genesis (`ACTORS.md` §5.5.2). Pass an
    /// operator-presented public key with `--attestation-key`
    /// (recommended; the private half stays in your hardware wallet /
    /// air-gapped device / safe), or use `--generate-attestation-key`
    /// to have Kairo generate one and print the seed once. Both flags
    /// are repeatable and can be mixed.
    ///
    /// `--attestation-threshold <N>` sets the M-of-N quorum required
    /// for any attestation-surface emergency event (`ACTORS.md`
    /// §5.5.3). Defaults to 1 for solo operators; raise it after
    /// you have multiple distinct attestation keys to protect
    /// against single-key compromise. Use M-of-N with N > M for
    /// resilience to lost keys (e.g. 3-of-5, not 3-of-3).
    Create {
        /// Actor kind, e.g. person, project, organization, service.
        #[arg(long)]
        kind: String,
        /// Operator-presented attestation public key (hex-encoded raw
        /// ed25519 bytes; 64 hex chars). Repeatable. Kairo never sees
        /// the private half — this is the recommended path.
        #[arg(long = "attestation-key")]
        attestation_keys: Vec<String>,
        /// Generate a fresh attestation keypair, print the seed once
        /// to stdout, and embed only the public key in the genesis.
        /// Repeatable. The seed is not saved by Kairo — record it
        /// externally before continuing.
        #[arg(long = "generate-attestation-key", action = ArgAction::Count)]
        generate_attestation_keys: u8,
        /// M of the M-of-N quorum required for emergency events.
        /// Defaults to 1. Must satisfy 1 ≤ N ≤ total attestation
        /// keys. See `ACTORS.md` §5.5.3.
        #[arg(long = "attestation-threshold", default_value_t = 1)]
        attestation_threshold: u8,
    },
    /// Import an ActorGenesis JSON document into the local store.
    Import {
        #[arg(long)]
        genesis: PathBuf,
    },
    /// Rotate the actor's active signing key. Generates a fresh
    /// keypair, signs an `ActorKeyRotation` with the prior active
    /// key, persists it, and replaces the keystore entry with the
    /// new secret.
    RotateKey {
        #[arg(long)]
        actor: String,
    },
    /// Revoke a key the actor previously held. Default revocation
    /// invalidates statements signed by the key after this point;
    /// `--retroactive` invalidates them from inception. Refuses to
    /// revoke the only active key without `--brick-actor` (rotate
    /// first; see `ACTORS.md` §5.5.1).
    RevokeKey {
        #[arg(long)]
        actor: String,
        #[arg(long = "key")]
        key_id: String,
        #[arg(long)]
        retroactive: bool,
        #[arg(long)]
        reason: Option<String>,
        #[arg(long = "brick-actor")]
        brick_actor: bool,
    },
    /// Print the actor's key history: genesis-initial key, every
    /// rotation, every revocation, and the attestation set. Useful
    /// for diagnostic checks.
    KeyHistory {
        #[arg(long)]
        actor: String,
        #[arg(long)]
        json: bool,
    },
    /// Recover from a lost or compromised active signing key by
    /// signing an `ActorEmergencyKeyRotation` with a cold-storage
    /// attestation key (`ACTORS.md` §5.5.2). Two flows:
    /// `sign` reads the attestation seed in-process; `prepare` /
    /// `import` lets the operator sign externally on a YubiKey/HSM.
    RecoverKey {
        #[command(subcommand)]
        command: RecoverKeyCommand,
    },
    /// Append a new attestation key to the actor's append-only
    /// attestation set (`ACTORS.md` §5.5.2). Signed by an existing
    /// attestation key the operator pulls from cold storage. Same
    /// `sign` / `prepare` / `submit` flows as `recover-key`.
    AddAttestationKey {
        #[command(subcommand)]
        command: AddAttestationKeyCommand,
    },
    /// Retract the recovery authority of an attestation key the
    /// actor previously held — either declared in
    /// `ActorGenesis.attestation_keys` or appended via
    /// `ActorAttestationKeyAdd`. Self-revocation is permitted; the
    /// store refuses revocations that would leave fewer attestation
    /// keys than the live threshold (`ACTORS.md` §5.5.2 / §5.5.3).
    RevokeAttestationKey {
        #[command(subcommand)]
        command: RevokeAttestationKeyCommand,
    },
    /// Mutate the M of the M-of-N attestation threshold
    /// (`ACTORS.md` §5.5.3). The asymmetric authority rule is
    /// enforced at submit time: raises require
    /// `max(current, new)` distinct attestation-set signatures;
    /// lowers/equal require `current`. `sign` is the threshold = 1
    /// convenience flow; for thresholds above 1 use
    /// `prepare` + `co-sign` + `submit`.
    ChangeAttestationThreshold {
        #[command(subcommand)]
        command: ChangeAttestationThresholdCommand,
    },
    /// Append a single attestation signature to a partially-signed
    /// envelope produced by any `prepare` flow. Refuses duplicate
    /// `key_id`s and refuses keys that aren't in the actor's
    /// attestation set at the envelope's `created_at`. Reports the
    /// running `(have, need)` count against the actor's threshold.
    /// See `ACTORS.md` §5.5.3.
    CoSign {
        /// Path to the partial envelope JSON written by `prepare`
        /// (with optional signatures already accumulated). The
        /// sibling `<prepared>.payload` provides the canonical bytes
        /// the seed signs over.
        #[arg(long)]
        prepared: PathBuf,
        /// Actor whose attestation set the seed must belong to.
        /// Must match the envelope's `actor` field.
        #[arg(long)]
        actor: String,
        /// File containing the cosigner's attestation key seed
        /// (base64; 32 raw bytes when decoded). Pulled from cold
        /// storage by this cosigner only; not persisted by Kairo.
        #[arg(long)]
        attestation_key_seed: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum RecoverKeyCommand {
    /// Convenience: read the attestation seed from a file the
    /// operator pulled from cold storage, generate a fresh active
    /// signing key, sign and persist an `ActorEmergencyKeyRotation`,
    /// and store the new active signing key in the keystore. The
    /// seed file is read once and never persisted by Kairo.
    Sign {
        #[arg(long)]
        actor: String,
        /// File containing the attestation key seed as base64
        /// (single line; trailing newline tolerated). 32 raw bytes
        /// when decoded.
        #[arg(long)]
        attestation_key_seed: PathBuf,
    },
    /// Pure two-step prepare: emit an unsigned
    /// `ActorEmergencyKeyRotation` JSON envelope plus the canonical
    /// bytes the operator must sign externally. Kairo never sees
    /// the attestation seed or the new active signing key's secret
    /// — both are operator-managed externally (e.g. on a YubiKey).
    Prepare {
        #[arg(long)]
        actor: String,
        /// Hex-encoded raw ed25519 public key for the new active
        /// signing key. The operator holds the private half
        /// externally.
        #[arg(long)]
        new_key: String,
        /// Output path for the partially-filled JSON envelope. A
        /// sibling `<output>.payload` is written with the raw
        /// canonical bytes the operator must sign.
        #[arg(long)]
        output: PathBuf,
    },
    /// Submit a prepared envelope. Verifies the envelope meets the
    /// actor's attestation threshold at `created_at`, validates each
    /// signature against the attestation set, and dispatches to the
    /// store's `put_actor_emergency_key_rotation`. The optional
    /// `--signature` flag attaches one external signature inline
    /// before submission (1-of-1 backward-compat path); for
    /// multi-signature envelopes use `kairo actor co-sign` first.
    Submit {
        /// Path to the JSON envelope written by `prepare` (with any
        /// signatures already appended via `co-sign`).
        #[arg(long)]
        prepared: PathBuf,
        /// Optional path to a base64 ed25519 signature of the
        /// prepared payload to append before submitting. Convenience
        /// for the single-signer (threshold = 1) flow.
        #[arg(long)]
        signature: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum ChangeAttestationThresholdCommand {
    /// Convenience for the threshold = 1 case: read a single
    /// attestation seed from a file, sign and persist a
    /// `ActorAttestationThresholdChange` directly. Refuses if the
    /// actor's current threshold is > 1 (use `prepare`/`co-sign`/
    /// `submit` instead).
    Sign {
        #[arg(long)]
        actor: String,
        /// File containing the attestation seed (base64; 32 raw
        /// bytes when decoded). Not persisted by Kairo.
        #[arg(long)]
        attestation_key_seed: PathBuf,
        /// New threshold value. Must satisfy
        /// `1 ≤ to ≤ |attestation set at created_at|`.
        #[arg(long)]
        to: u8,
    },
    /// Emit a zero-signature envelope plus the canonical bytes
    /// the cosigners must sign over.
    Prepare {
        #[arg(long)]
        actor: String,
        #[arg(long)]
        to: u8,
        #[arg(long)]
        output: PathBuf,
    },
    /// Finalize a partial envelope: validate against the
    /// attestation threshold + asymmetric authority rule, dispatch
    /// to `put_actor_attestation_threshold_change`.
    Submit {
        #[arg(long)]
        prepared: PathBuf,
        /// Optional base64 ed25519 signature of the prepared
        /// payload to append before submitting (single-signer
        /// convenience).
        #[arg(long)]
        signature: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum AddAttestationKeyCommand {
    /// Convenience: read an existing attestation seed from a file,
    /// sign and persist an `ActorAttestationKeyAdd`. The new key is
    /// either operator-presented (`--key <hex>`) or generated and
    /// printed once (`--generate`); the latter mirrors the
    /// generate-and-forget UX of `actor create
    /// --generate-attestation-key`.
    Sign {
        #[arg(long)]
        actor: String,
        /// File containing an existing attestation seed (base64).
        /// The seed signs the add and is not persisted by Kairo.
        #[arg(long)]
        signing_attestation_key_seed: PathBuf,
        /// Operator-presented hex public key for the new attestation
        /// key. Mutually exclusive with `--generate`.
        #[arg(long, conflicts_with = "generate")]
        key: Option<String>,
        /// Generate a fresh attestation keypair and print the seed
        /// once. Mutually exclusive with `--key`.
        #[arg(long, conflicts_with = "key")]
        generate: bool,
    },
    /// Pure two-step prepare: emit an unsigned
    /// `ActorAttestationKeyAdd` JSON envelope plus the canonical
    /// bytes the operator must sign externally.
    Prepare {
        #[arg(long)]
        actor: String,
        /// Hex public key of the new attestation key (operator-
        /// presented; the operator must hold the private half).
        #[arg(long)]
        new_key: String,
        /// Output path for the partially-filled JSON envelope. A
        /// sibling `<output>.payload` is written with the raw
        /// canonical bytes the operator must sign.
        #[arg(long)]
        output: PathBuf,
    },
    /// Submit a prepared envelope. Mirrors `recover-key submit`:
    /// validates the envelope against the actor's attestation
    /// threshold and dispatches to
    /// `put_actor_attestation_key_add`. Use `--signature` for the
    /// single-signer convenience path; for multi-signature envelopes
    /// use `kairo actor co-sign` first.
    Submit {
        #[arg(long)]
        prepared: PathBuf,
        /// Optional base64 ed25519 signature of the prepared payload
        /// to append before submitting. Single-signer convenience.
        #[arg(long)]
        signature: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum RevokeAttestationKeyCommand {
    /// Convenience: read an attestation seed, sign and persist an
    /// `ActorAttestationKeyRevocation`. The signing key may be the
    /// same as the revoked key (self-revocation is permitted) or a
    /// different attestation key. Refuses upstream if the resulting
    /// set would fall below the live threshold.
    Sign {
        #[arg(long)]
        actor: String,
        /// File containing the signing attestation seed (base64;
        /// 32 raw bytes when decoded). Not persisted.
        #[arg(long)]
        signing_attestation_key_seed: PathBuf,
        /// `KeyId` of the attestation key being revoked.
        #[arg(long = "revoke-key")]
        revoke_key: String,
        /// Optional human-readable reason recorded on the
        /// statement (e.g., "yubikey lost").
        #[arg(long)]
        reason: Option<String>,
    },
    /// Pure two-step prepare: emit an unsigned
    /// `ActorAttestationKeyRevocation` JSON envelope plus the
    /// canonical bytes the cosigners must sign externally.
    Prepare {
        #[arg(long)]
        actor: String,
        #[arg(long = "revoke-key")]
        revoke_key: String,
        #[arg(long)]
        reason: Option<String>,
        #[arg(long)]
        output: PathBuf,
    },
    /// Submit a prepared envelope. Validates the envelope against
    /// the actor's attestation threshold and dispatches to
    /// `put_actor_attestation_key_revocation`. Use `--signature`
    /// for the single-signer convenience path; for multi-signature
    /// envelopes use `kairo actor co-sign` first.
    Submit {
        #[arg(long)]
        prepared: PathBuf,
        #[arg(long)]
        signature: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum ObjectSubcommand {
    /// Create a new ObjectGenesis statement signed by the given actor.
    Create {
        /// Actor whose key signs the genesis statement.
        #[arg(long)]
        actor: String,
        /// Object kind, e.g. software, dataset, image.
        #[arg(long)]
        kind: String,
        /// Optional initial storage revision (e.g. git:sha256:<commit>).
        #[arg(long)]
        initial_revision: Option<String>,
    },
    /// Import a signed ObjectGenesis statement JSON into the local store.
    Import {
        #[arg(long)]
        statement: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum ManifestCommand {
    /// Print the canonical manifest BlobId.
    Hash {
        #[arg(default_value = "kairo.toml")]
        path: PathBuf,
    },
    /// Print parsed manifest details.
    Inspect {
        #[arg(default_value = "kairo.toml")]
        path: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum RevisionCommand {
    /// Validate an ObjectRevision JSON statement against a kairo.toml manifest.
    ValidateManifest {
        #[arg(long)]
        statement: PathBuf,
        #[arg(long, default_value = "kairo.toml")]
        manifest: PathBuf,
    },
    /// Verify an ObjectRevision JSON statement signature with a raw ed25519 public key.
    VerifySignature {
        #[arg(long)]
        statement: PathBuf,
        #[arg(long, conflicts_with = "public_key_file")]
        public_key: Option<String>,
        #[arg(long, conflicts_with = "public_key")]
        public_key_file: Option<PathBuf>,
    },
    /// Verify an ObjectRevision signature against an ActorGenesis initial key.
    VerifyActorGenesis {
        #[arg(long)]
        statement: PathBuf,
        #[arg(long)]
        actor_genesis: PathBuf,
        /// Emit a stable JSON representation of the verification report.
        #[arg(long)]
        json: bool,
    },
    /// Create a signed ObjectRevision statement and persist it to the store.
    Create {
        #[arg(long)]
        actor: String,
        #[arg(long)]
        object: String,
        #[arg(long)]
        revision: String,
        #[arg(long, default_value = "kairo.toml")]
        manifest: PathBuf,
        /// Storage parent revision (may be repeated for multi-parent statements).
        #[arg(long = "parent")]
        parents: Vec<String>,
        /// Suppress the default `attests_reachable_history = true` claim.
        #[arg(long)]
        no_attests_reachable_history: bool,
    },
    /// Import a signed ObjectRevision statement JSON into the local store.
    Import {
        #[arg(long)]
        statement: PathBuf,
    },
    /// Print the body fields of a stored ObjectRevision statement.
    Inspect {
        /// StatementId to inspect.
        #[arg(long)]
        statement: String,
        /// Emit a stable JSON representation.
        #[arg(long)]
        json: bool,
    },
    /// List ObjectRevision statements stored locally for an object.
    List {
        /// Filter to revisions whose body.object matches this id.
        #[arg(long)]
        object: String,
    },
}
