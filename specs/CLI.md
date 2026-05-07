# CLI.md

## Status

Draft specification.

This document defines the Kairo command-line interface. The CLI is the primary
operator and developer interface for interacting with local Kairo stores, the
Kairo daemon, object snapshots, builds, runs, federation, and diagnostics.

This specification is intentionally prescriptive enough to guide implementation.

---

## 1. Purpose

The Kairo CLI provides a human- and script-friendly interface to Kairo.

The CLI is responsible for:

1. Starting and stopping the local daemon.
2. Inspecting objects and snapshots.
3. Verifying snapshots.
4. Importing and exporting object data.
5. Fetching and synchronizing objects through the daemon/federation layer.
6. Building snapshots.
7. Running snapshots.
8. Reproducing snapshots.
9. Managing pins.
10. Displaying store, daemon, federation, and runtime status.
11. Providing stable machine-readable output for automation.

The CLI is not responsible for:

1. Defining object semantics.
2. Defining statement interpretation.
3. Defining validation rules.
4. Implementing federation protocols.
5. Implementing daemon scheduling.
6. Executing unplanned object content directly.

---

## 2. Dependency Relationship

The CLI depends on:

- `CORE_LIBRARY.md`
- `STORE.md`
- `DAEMON.md`
- `FEDERATION.md`
- `OBJECT.md`
- `STATEMENTS.md`
- `BUILD.md`
- `PLANNER.md`

The CLI must use daemon/core results as the source of truth.

The CLI must not independently reinterpret:

- Statement authority
- Snapshot validity
- Effective object state
- Build plans
- Run plans
- Conflict semantics

---

## 3. Operating Modes

The CLI should support two operating modes:

1. Daemon mode
2. Direct/local mode

### 3.1 Daemon mode

Daemon mode is the default for commands that require:

- Federation
- Background tasks
- Runtime execution
- Build execution
- Long-running operations
- Shared local store coordination
- Web-client-compatible state

In daemon mode, the CLI communicates with the daemon API.

### 3.2 Direct/local mode

Direct/local mode uses the core library and store directly.

Direct mode may be used for:

- Offline validation
- CI workflows
- Store inspection
- Local archive verification
- Bootstrapping before daemon startup
- Recovery and diagnostics

Direct mode must not bypass core validation semantics.

### 3.3 Mode selection

The CLI should support:

```text
--daemon
--direct
--offline
--store <path>
```

**[v1]** behavior — locked in `DECISIONS.md` §10.6 (probe-and-
fall-back):

1. **Default (no mode flag).** The CLI probes the Unix socket at
   `<store>/daemon.sock`, sends `GET /api/v1/status`, and uses
   the daemon if it answers. Otherwise it falls back to direct
   mode silently.
2. **`--daemon`** requires the daemon: probe failure or
   `connect(2)` error returns exit 9 (`daemon unavailable`).
3. **`--direct` / `--offline`** force direct mode and never touch
   the socket.
4. **Writes always stay direct.** Signing/persisting commands
   (`actor create`, `object create`, `revision create`, `branch
   set`, `tag bind`, `trust grant`, `capability grant`, etc.)
   ignore the daemon entirely in v1 — there are no write
   endpoints. The daemon is read-only (DECISIONS.md §10.4).
5. **Federation, build, run, reproduce, task, sync, fetch.** Not
   shipped in v1 in either mode; they exit 13 (`unsupported
   feature`) until the relevant phases land.

The "fall back silently" behavior applies only to commands that
have a direct-mode implementation. Commands whose only home is
post-v1 surface return exit 13 regardless of daemon availability.

---

## 4. Global Command Shape

The binary name should be:

```text
kairo
```

Global options:

```text
kairo [GLOBAL_OPTIONS] <command> [COMMAND_OPTIONS]
```

Recommended global options:

```text
--config <path>
--store <path>
--daemon-url <url>
--daemon
--direct
--offline
--format <human|json|ndjson>
--quiet
--verbose
--debug
--no-color
--color <auto|always|never>
--yes
--no
--help
--version
```

### 4.1 Output formats

The CLI must support:

- `human`
- `json`

The CLI should support:

- `ndjson` for streaming task and event output.

Human output is intended for terminals.

JSON output is intended for scripts and must be stable.

### 4.2 Color

Color must not be required to understand output.

When `--no-color` or `--color never` is used, all semantic information must remain visible as text.

---

## 5. Object and Snapshot References

Commands should accept object references in a consistent form.

Examples:

```text
kairo inspect <object-ref>
kairo verify <snapshot-ref>
kairo build <snapshot-ref>
```

Reference kinds:

1. Object reference, such as `object:z6MkObject...`
2. Object name, if resolvable
3. Snapshot reference, such as `object:z6MkObject...:snapshot:z6MkSnapshot...`
4. Object reference plus frontier selector
5. Local path
6. External Kairo reference, such as `kairo:object:z6MkObject...`
7. Imported archive path

The CLI must resolve ambiguous references explicitly.

If a name resolves to multiple objects, the CLI must not guess unless a deterministic disambiguation flag is provided.

---

### 5.1 Early Direct Validation Commands

Before daemon-backed object and snapshot workflows exist, the CLI may expose
direct local commands for deterministic validation primitives.

```text
kairo manifest hash [path]
kairo manifest inspect [path]
kairo actor id --genesis <actor-genesis.json>
kairo actor create --kind <kind> (--attestation-key <hex> | --generate-attestation-key)...
kairo actor import --genesis <actor-genesis.json>
kairo actor rotate-key --actor <id>
kairo actor revoke-key --actor <id> --key <key-id> [--retroactive] [--reason <text>] [--brick-actor]
kairo actor key-history --actor <id> [--json]
kairo actor recover-key sign --actor <id> --attestation-key-seed <path>
kairo actor recover-key prepare --actor <id> --new-key <hex> --output <path>
kairo actor recover-key import --prepared <path> --signature <path>
kairo actor add-attestation-key sign --actor <id> --signing-attestation-key-seed <path> (--key <hex> | --generate)
kairo actor add-attestation-key prepare --actor <id> --new-key <hex> --output <path>
kairo actor add-attestation-key import --prepared <path> --signature <path>
kairo object create --actor <id> --kind <kind> [--initial-revision <ref>]
kairo object import --statement <object-genesis.json>
kairo revision create --actor <id> --object <id> --revision <ref> [--manifest <path>] [--parent <ref>]... [--no-attests-reachable-history]
kairo revision import --statement <object-revision.json>
kairo revision inspect --statement <statement-id> [--json]
kairo revision list --object <object-id>
kairo revision validate-manifest --statement <object-revision.json> [--manifest <kairo.toml>]
kairo revision verify-signature --statement <object-revision.json> (--public-key <base64>|--public-key-file <path>)
kairo revision verify-actor-genesis --statement <object-revision.json> --actor-genesis <actor-genesis.json> [--json]
kairo branch set --actor <id> --object <id> --revision <statement-id> [--name <name>]
kairo branch show --object <id> [--actor <id>] [--name <name>] [--json]
kairo branch list --object <id>
kairo tag bind --actor <id> --object <id> --version <semver> --revision <statement-id>
kairo tag revoke --actor <id> --object <id> --version <semver>
kairo tag show --object <id> [--actor <id>] --version <semver> [--json]
kairo tag list --object <id>
kairo tag history --object <id> [--actor <id>] --version <semver> [--json]
kairo trust grant --by <id> --of <id> [--reason <text>]
kairo trust block --by <id> --of <id> [--reason <text>]
kairo trust withdraw --by <id> --of <id> [--reason <text>]
kairo trust show --by <id> --of <id> [--json]
kairo trust list --by <id>
kairo trust history --by <id> --of <id> [--json]
kairo capability grant --grantor <id> --grantee <id> --object <id> --kind <kind>... [--delegable] [--expires-at <RFC3339>] [--max-delegation-depth <N>] [--key-pinned <keyid>]
kairo capability revoke --grantor <id> --grant <statement-id> [--retroactive] [--reason <text>]
kairo capability list (--grantor <id> | --object <id>)
kairo snapshot compute --object <id> [--actor <id>] [--name <name>] [--statement <id>] [--json]
kairo verify object --object <id> [--actor <id>] [--name <name>] [--statement <id>] [--as <id>|--no-as] [--repo <path>|--no-repo] [--manifest <path>] [--json]
```

`manifest hash` parses `kairo.toml` and prints the canonical `ObjectManifest`
`BlobId`.

`manifest inspect` prints parsed manifest fields and the same canonical manifest
hash.

`actor id` parses an `ActorGenesis` JSON document and prints the derived
`ActorId`.

`revision validate-manifest` parses an `ObjectRevision` JSON statement and a
`kairo.toml` file, then verifies:

1. `ObjectRevision.manifest_hash` equals the canonical manifest hash.
2. `[kairo].object`, when present, equals `ObjectRevision.object`.

`revision verify-signature` parses an `ObjectRevision` JSON statement and checks
its ed25519 signature against raw public key bytes encoded as standard base64.

`revision verify-actor-genesis` parses an `ObjectRevision` JSON statement and an
`ActorGenesis` JSON document, then verifies:

1. The derived `ActorId` equals `signature.actor`.
2. The statement signature verifies against the actor genesis initial key.

`--json` emits a stable JSON `VerificationReport`; the human form prints the
three independent dimensions (signature, actor resolution, trust).

`actor create` generates a fresh signing keypair, signs and persists the
resulting `ActorGenesis`, and stores the signing secret in the keystore.
At least one cold-storage attestation key is required (`ACTORS.md`
§5.5.2). Pass operator-presented public keys with `--attestation-key
<hex>` (recommended; the private half stays on the operator's
YubiKey/HSM/air-gapped device), or use `--generate-attestation-key` to
have Kairo generate a fresh keypair and print the seed once to stdout.
Both flags are repeatable and can be mixed. The generate-and-print
flow emits a one-time `seed = <base64>` line plus a stderr warning;
Kairo never writes the attestation seed to disk. `object create` and
`revision create` load the actor's stored signing key, sign the
corresponding body, and persist the statement to the local store.

`actor import`, `object import`, and `revision import` ingest pre-existing
JSON records into the local store. Each command re-derives the canonical
identity (`ActorId`, `ObjectId`, `StatementId`) from the parsed body and
uses that as the storage key, so import is fixity-checked: a tampered body
ends up at a different id than its filename or original location.

`actor rotate-key` generates a fresh ed25519 keypair, signs an
`ActorKeyRotation` statement with the actor's currently active key
(auto-chaining `supersedes` off any prior rotation), persists the
statement, and replaces the keystore entry so future signing uses the
new key. After rotation, every other signing command continues to
work — the keystore-vs-actor check resolves the active key via the
rotation chain, not via `ActorGenesis.initial_key`. See
`schemas/canonical/actor-key-rotation-v1.md`.

`actor revoke-key --actor <id> --key <key-id>` signs an
`ActorKeyRevocation` retracting `<key-id>`'s signing authority. The
revocation itself is signed by the actor's currently active key
(which may be `<key-id>`). `--retroactive` invalidates statements
signed by the revoked key from inception (compromise mode);
`--reason <text>` is optional audit text included in canonical bytes.
The CLI refuses to revoke the only active key (the genesis-initial
key when no rotations have happened) without `--brick-actor` —
revoking the only key without rotating first permanently retires the
actor (`ACTORS.md` §5.5.1). The error message points at `actor rotate-key`
as the safe alternative.

`actor key-history --actor <id>` is a diagnostic surface that prints
the actor's full key history: the genesis-initial signing key, every
rotation (routine + emergency, tagged with `surface`) in storage order,
every revocation, the genesis-declared attestation set, and any
`ActorAttestationKeyAdd` entries. `--json` emits a stable shape for
automation.

`actor recover-key` is the cold-storage recovery surface for a lost
or compromised active signing key (`ACTORS.md` §5.5.2). Three flows
under one command:

- `recover-key sign --actor <id> --attestation-key-seed <path>` —
  convenience: reads a base64-encoded attestation seed from a file
  the operator pulled from cold storage, generates a fresh active
  signing key, signs and persists an `ActorEmergencyKeyRotation`,
  and stores the new signing secret in the keystore. The seed file
  is read once and never persisted by Kairo.
- `recover-key prepare --actor <id> --new-key <hex> --output <path>`
  — pure two-step path for operators using a YubiKey/HSM. Emits a
  partially-filled `ActorEmergencyKeyRotation` JSON envelope at
  `<path>` plus a sibling `<path>.payload` containing the raw
  canonical bytes the operator must sign on their cold device.
  The operator generates the new active signing key externally
  (`--new-key` is its hex public key); the private half is
  operator-managed.
- `recover-key import --prepared <path> --signature <path>` —
  ingest the prepared envelope plus the operator's externally-
  produced base64 signature. Auto-detects which attestation key
  produced the signature by trying each one in the actor's
  attestation set at `created_at`; persists the rotation. The
  operator's new active signing key is not added to the keystore;
  signing future statements requires either a separate keystore
  import or another externally-signed flow.

`actor add-attestation-key` appends to the actor's append-only
attestation set (`ACTORS.md` §5.5.2). Mirrors the `recover-key`
shape with three flows:

- `add-attestation-key sign --actor <id>
  --signing-attestation-key-seed <path> (--key <hex> | --generate)`
  — convenience: reads an existing attestation seed (which signs
  the add) plus the new attestation key, either operator-presented
  (`--key <hex>`) or generated-and-printed (`--generate`). Persists
  an `ActorAttestationKeyAdd` statement.
- `add-attestation-key prepare --actor <id> --new-key <hex>
  --output <path>` — pure two-step prepare; emits the unsigned
  envelope and a sibling `.payload` for external signing.
- `add-attestation-key import --prepared <path> --signature <path>`
  — ingest the externally-signed envelope; auto-detects the
  signing attestation key.

`revision inspect --statement <id>` reads a stored revision by `StatementId`
and prints its body fields. `--json` emits a stable JSON shape suitable for
automation. `revision list --object <id>` scans the local store for
revisions whose `body.object` matches and prints one summary per match.

`branch set` signs and persists an `ObjectBranch` statement that points the
named branch at the given `ObjectRevision` `StatementId`. The branch must
match the revision's object; the command refuses to create a dangling
pointer. The CLI auto-computes the `supersedes` pointer: if the actor
already has a chain leaf for `(actor, object, name)`, the new statement
supersedes it; otherwise it is the genesis advance. Pointer movement
is **always explicit** — `revision create` is a low-level primitive and
never advances any branch on its own.

`branch show` resolves the chain leaf `ObjectBranch` for
`(actor, object, name)`. `--actor` defaults to `ObjectGenesis.created_by`;
`--name` defaults to the conventional `"head"`. `branch list --object <id>`
enumerates all known `(actor, name)` branch tips for the object.

Capability resolution rides the branch read path the same way it does
the tag read path: a cross-actor `supersedes` edge is honored when the
successor's signer holds an `ObjectBranch` capability on the object at
the successor's `created_at` (`specs/CAPABILITIES.md` §6.2). The flip
happens transparently inside `branch show`, `branch list`,
`verify object`, etc.

`tag bind --actor <id> --object <id> --version <semver> --revision <statement-id>`
signs and persists an `ObjectVersionTag` that binds the semver string to
the given `ObjectRevision`. The CLI auto-computes the `supersedes`
pointer: if the actor has a prior tag for that `(object, version)`, the
new statement supersedes it; otherwise it is the genesis tag. The
revision must bind to the same object as the tag; the command refuses
to create a dangling pointer.

`tag revoke --actor <id> --object <id> --version <semver>` signs an
`ObjectVersionTag` whose `target` is `null`, withdrawing the version for
that actor. Revocation requires a prior tag — the CLI sets `supersedes`
to the actor's current head and errors if there is none.

`tag show --object <id> [--actor <id>] --version <semver>` resolves the
current head for `(actor, object, version)` and prints whether it is a
bind or a revoke. `tag list --object <id>` enumerates known
`(actor, version)` heads. `tag history --object <id> [--actor <id>]
--version <semver>` walks the `supersedes` chain backwards from the head
(newest first); a missing predecessor is reported as indeterminate
rather than failing.

Tag pointers are mutable. Consumers that need build reproducibility
must record the resolved `StatementId` (or `SnapshotId`) — not the
version string — in their lockfile equivalent. Two resolvers can
disagree on `(actor, object, version)` if they have seen different
subsets of the actor's tag history; this is by design.

`trust grant --by <id> --of <id>` signs and persists an `ActorTrust`
that records `--by`'s opinion that `--of` is trusted. `trust block`
records the opposite (untrusted), and `trust withdraw` retracts any
prior opinion. The CLI auto-computes the `supersedes` pointer: if
`--by` already has an opinion about `--of`, the new statement
supersedes it; otherwise it is the genesis opinion. Withdrawal
requires a prior opinion to chain off of and errors if there is none.
`--reason <text>` is optional and is included in canonical bytes.

`trust show --by <id> --of <id>` resolves `--by`'s current opinion
about `--of`. A missing opinion prints `decision = unknown` and is
**not** an error — trust is informational. `trust list --by <id>`
enumerates the truster's current opinions, one per trusted actor.
`trust history --by <id> --of <id>` walks the `supersedes` chain
backwards from the head (newest first); a missing predecessor is
reported as indeterminate rather than failing.

`capability grant --grantor <id> --grantee <id> --object <id> --kind <kind>...`
signs and persists an `ActorCapabilityGrant` authorizing `--grantee` to
issue the named statement kinds against `--object`. Repeat `--kind`
once per kind (e.g. `--kind ObjectVersionTag --kind ObjectBranch`); the
canonical encoder normalizes order. The CLI auto-computes the
`supersedes` pointer: if `--grantor` has a prior chain leaf for the
`(grantor, grantee, Object(<object>))` triple, the new statement
supersedes it; otherwise it is the genesis grant. Optional flags:
`--delegable` (allow further re-grant), `--expires-at <RFC3339>`,
`--max-delegation-depth <N>`, `--key-pinned <keyid>`. See
`specs/CAPABILITIES.md` §4.

`capability revoke --grantor <id> --grant <statement-id>` signs an
`ActorCapabilityRevocation` against the grant whose `StatementId` is
`--grant`. The signing actor must equal the grant's original grantor;
cross-grantor revocation is rejected at the CLI layer (and is invalid
per `specs/CAPABILITIES.md` §5.2). `--retroactive` invalidates the
grant from inception (see §6.3); the default invalidates only
statements created strictly after the revocation.

`capability list` enumerates chain heads. Pass exactly one of
`--grantor <id>` (audit query — what has this actor delegated, one
head per `(grantee, scope)` triple — drives the §7.1 cleanup runbook)
or `--object <id>` (cross-cutting query — who holds capabilities on
this object, one head per `(grantor, grantee)` pair).

Capability resolution is on the read path: when resolving a tag head
via `latest_version_tag`, a cross-actor `supersedes` edge is honored
when the successor's signer holds an `ObjectVersionTag` capability on
the object at the successor's `created_at`
(`specs/CAPABILITIES.md` §6.2). The CLI does not need a separate
"evaluate capability" command for that — the flip happens
transparently inside `tag show`, `tag list`, `verify object`, etc.

`snapshot compute --object <id>` resolves the chosen `ObjectRevision` and
computes its `SnapshotId`. By default it follows the creator-actor's
`"head"` branch; `--actor`, `--name`, and `--statement` override that
resolution. `--statement <id>` pins the frontier directly and conflicts
with `--actor` / `--name`. There is no implicit bootstrap from
`ObjectGenesis.initial_revision` — sign at least one `ObjectRevision`
(and, for the default-resolved form, set a branch) before computing a
snapshot.

`verify object --object <id>` is the end-to-end verification entrypoint.
It loads the `ObjectGenesis` (the store re-derives the `ObjectId` on
read, so a successful load is a fixity check), resolves the chosen
`ObjectRevision` (default: creator-actor's `"head"` branch — same
resolution rules as `snapshot compute`, with `--actor`, `--name`, and
`--statement` overrides), verifies the revision's signature against
the resolved actor through the local `ActorResolver`, looks the
storage commit up in a Git repository, and validates the manifest
binding against the `kairo.toml` blob in the commit's tree. The output
aggregates these into a single `VALID`, `INDETERMINATE`, or `INVALID`
verdict per the rules below; `--json` emits a stable JSON shape.

Git lookup behavior:

- `--repo <path>` — explicit Git repository path (working tree or
  `.git`). Authoritative when given.
- No flag — walk upward from the current working directory looking
  for a `.git` (the same algorithm `git status` uses). Use the first
  one found.
- Neither — error with a hint pointing at `--repo` and `--no-repo`.
- `--no-repo` — skip the Git lookup entirely. The content layer stays
  `INDETERMINATE` and, without an explicit `--manifest`, manifest
  binding does too.

Manifest source:

- `--manifest <path>` overrides everything; useful for unusual
  layouts or for verifying a revision against a manifest the user
  pinned out-of-tree.
- Otherwise the verifier reads `kairo.toml` from the commit's tree at
  the revision being verified. The report records the source as
  `git:sha256:<oid>/kairo.toml` so the audit trail is explicit.

Trust evaluation:

- `--as <by-actor>` — explicit truster. The report folds
  `by_actor`'s active `ActorTrust` opinion about the revision's
  signing actor into one of `trusted`, `untrusted`, or `unknown`.
- No flag — auto-pick the sole local actor from the keystore. If the
  keystore has zero keys, trust stays `unevaluated`. If it has more
  than one, the command errors with the candidate list and a hint
  to pass `--as` (or `--no-as` to skip).
- `--no-as` — skip trust evaluation entirely; trust stays
  `unevaluated` regardless of keystore contents.

Trust never changes the `VALID` / `INVALID` / `INDETERMINATE`
verdict; it is reported alongside as a separate line (or JSON field)
so callers can compose it independently.

Aggregation:

- `VALID` — every check returned valid: genesis fixity, signature,
  actor resolution, object consistency, manifest binding, and content
  layer (commit found + parents agree).
- `INVALID` — at least one check actively failed (signature invalid,
  actor mismatch, object id mismatch, manifest hash mismatch, commit
  not in repo, parent disagreement). Exits non-zero.
- `INDETERMINATE` — no failures, but at least one check could not be
  evaluated (no manifest available, no Git lookup performed, non-Git
  revision scheme). Exits zero with the marker in the report.

These commands do not validate actor authority, actor key-active status,
or snapshot closure completeness. They do verify that signed Git commits
exist locally and that declared parents agree with the Git graph; the
remaining gaps are reported explicitly in the verdict.

---

## 6. Validation Status Rendering

The CLI must preserve core validation statuses.

Statuses:

```text
valid
invalid
conflicted
indeterminate
unverified
```

### 6.1 Human rendering

Recommended visual labels:

```text
VALID
INVALID
CONFLICTED
INDETERMINATE
UNVERIFIED
```

The CLI must distinguish:

- Core validation status
- Daemon policy decision
- Operational task status

Example:

```text
Validation: VALID
Policy:     REQUIRES APPROVAL
Task:       RUNNING
```

### 6.2 JSON rendering

JSON output must include structured fields:

```json
{
  "validation": {
    "status": "valid",
    "purpose": "run",
    "issues": []
  },
  "policy": {
    "decision": "allow"
  }
}
```

---

## 7. Exit Codes

The CLI must use stable exit codes.

Recommended exit codes:

```text
0   success
1   general error
2   invalid command-line usage
3   not found
4   validation invalid
5   validation conflicted
6   validation indeterminate
7   policy denied
8   user cancelled
9   daemon unavailable
10  store error
11  federation error
12  runtime error
13  unsupported feature
14  internal error
```

For commands that primarily report validation status, validation failures should map to the corresponding validation exit codes.

For commands that complete operationally but discover an invalid snapshot, the CLI should return the validation status code rather than generic failure.

---

## 8. Commands Overview

Recommended top-level commands (long-term), tagged by phase:

```text
kairo init           [v1]
kairo daemon         [v1] start/status/stop only
kairo web            [post-v1; Phase 2 §5]
kairo actor          [v1] §5.1 direct-mode subtree
kairo object         [v1] §5.1 direct-mode subtree
kairo revision       [v1] §5.1 direct-mode subtree
kairo branch         [v1] §5.1 direct-mode subtree
kairo tag            [v1] §5.1 direct-mode subtree
kairo trust          [v1] §5.1 direct-mode subtree
kairo capability     [v1] §5.1 direct-mode subtree
kairo manifest       [v1] §5.1 direct-mode subtree
kairo snapshot       [v1] §5.1 direct-mode subtree
kairo verify         [v1] direct-mode object verification
kairo bundle         [v1] direct-mode bundle export/import
kairo git            [v1] managed Git cache (Phase 2 §1)
kairo status         [v1] daemon-aware status
kairo import         [post-v1] subsumed by `bundle import`/`git fetch` in v1
kairo export         [post-v1] subsumed by `bundle export` in v1
kairo inspect        [post-v1] depends on snapshot resolution
kairo fetch          [post-v1] depends on Phase 2 §4
kairo sync           [post-v1] depends on Phase 2 §4
kairo build          [post-v1] depends on Phase 2 §7
kairo run            [post-v1] depends on Phase 2 §7
kairo reproduce      [post-v1] depends on Phase 2 §7
kairo pin            [post-v1] depends on GC design
kairo unpin          [post-v1] depends on GC design
kairo list           [post-v1] depends on indexing
kairo task           [post-v1] depends on TaskManager
kairo store          [post-v1] depends on long-running task surface
kairo federation     [post-v1] depends on Phase 2 §4
kairo policy         [post-v1] depends on PolicyService
kairo config         [post-v1] depends on ConfigService
```

The `[v1]` direct-mode subtrees (`kairo actor`, `object`,
`revision`, `branch`, `tag`, `trust`, `capability`, `manifest`,
`snapshot`, `verify`, `bundle`, `git`) are documented in §5.1
and ship today; they ignore the daemon by design (writes always
go direct — see §3.3). Sections §11–§27 below describe the
long-term shape; v1 shipping behavior is summarized inline where
the v1 surface differs.

---

## 9. `kairo init`

Initializes a local Kairo store or workspace.

Usage:

```text
kairo init [path] [--store <path>] [--bare]
```

Responsibilities:

1. Create local store structure.
2. Initialize config if needed.
3. Initialize metadata.
4. Refuse to overwrite existing non-empty data unless `--force` is supplied.

Must not fetch or execute remote data.

---

## 10. `kairo daemon`

Manages the daemon process.

Subcommands (long-term):

```text
kairo daemon start
kairo daemon stop
kairo daemon restart
kairo daemon status
kairo daemon logs
```

**[v1] surface:** v1 ships `start`, `status`, and `stop` only.
`restart` and `logs` are post-v1 — `restart` is just `stop` + a
fresh `start` for now, and `logs` depends on the rolling-log
work deferred per `DECISIONS.md` §10.7.

### 10.1 `daemon start`

Starts the daemon.

Options (long-term):

```text
--foreground
--background
--config <path>
--store <path>
```

**[v1]** start is **foreground only** (`DECISIONS.md` §10.1).
The process blocks until interrupted; users supervise via
systemd / launchd / tmux / `nohup` / `&`. `--background` is
post-v1; passing it in v1 returns exit 13. `--foreground` is
the implicit default and accepted as a no-op for forward
compatibility. The process logs to stderr in structured-text
form (DECISIONS.md §10.7).

Startup sequence is `DAEMON.md` §6.1: load config, open store,
bind socket at `<store>/daemon.sock` (mode 0600), write PID,
start axum.

### 10.2 `daemon status`

Long-term reports daemon running state, API endpoint, store
path, federation status, runtime status, active task count,
and version.

**[v1]** prints daemon-running, store path, store-schema-version,
PID, and version. Federation/runtime/task fields are omitted
because those subsystems are post-v1. `daemon status` works in
both daemon and direct mode: it probes the socket itself, so
the user does not need a daemon to ask "is there a daemon?".

### 10.3 `daemon stop`

**[v1]** sends a graceful shutdown signal to the running
daemon (DAEMON.md §6.2): drain in-flight requests, close the
socket, remove the PID file, exit 0. Because v1 has no long-
running tasks, drain is effectively immediate.

### 10.4 `kairo web` (deferred)

**[Phase 2 §5, deferred].** The companion `kairo web start |
status | stop` verb tree manages the separate `kairo-web`
process (TCP / browser-facing — see `DECISIONS.md` §11). It is
not part of the v1 CLI surface and is documented here only so
the daemon and web verb trees stay parallel.

---

## 11. `kairo import`

**[post-v1].** v1 has no top-level `kairo import`. Object data
enters the local store via `kairo bundle import` (directory
bundles, Phase 2 §1) or `kairo git fetch` (managed Git cache,
Phase 2 §1). The unified `kairo import` surface lands when the
daemon-side import endpoint (API §22) does, with task tracking
and policy.

Imports object data into the local store.

Usage:

```text
kairo import <path-or-url> [--pin] [--verify] [--format <human|json>]
```

Import may accept:

- Directory
- Archive/package
- Object file
- Snapshot closure file
- Blob bundle
- Remote URL, if daemon/federation policy allows

Import must:

1. Ingest data without executing it.
2. Preserve provenance where available.
3. Report import success separately from validation success.
4. Optionally verify after import when `--verify` is supplied.

Import success does not imply snapshot validity.

---

## 12. `kairo export`

**[post-v1].** v1 has `kairo bundle export` (directory bundles)
in direct mode. The unified `kairo export` surface — snapshot
closure shapes, task-based export, daemon-mediated — lands
post-v1.

Exports object or snapshot data.

Usage:

```text
kairo export <object-or-snapshot-ref> --output <path> [OPTIONS]
```

Options:

```text
--snapshot
--full-log
--closure <inspect|build|run|reproduce|archive-mirror>
--include-blobs
--include-dependencies
```

Export must not alter object semantics.

---

## 13. `kairo inspect`

**[post-v1].** Depends on snapshot resolution and the daemon's
inspect endpoint (API §12.3). v1 surfaces equivalent reads
through `kairo object show`, `kairo revision inspect`, `kairo
branch show`, and `kairo verify object`.

Displays object or snapshot information.

Usage:

```text
kairo inspect <object-or-snapshot-ref> [OPTIONS]
```

Options:

```text
--snapshot <snapshot-ref>
--latest
--purpose <inspect|build|run|reproduce>
--format <human|json>
--show-statements
--show-artifacts
--show-builds
--show-runs
--show-dependencies
```

Process:

1. Resolve object/snapshot reference.
2. Acquire inspect snapshot closure.
3. Call daemon/core inspect flow.
4. Display effective object metadata and validation status.

Inspect may show unverified previews, but must label them as unverified or indeterminate.

---

## 14. `kairo verify`

Validates a snapshot.

**[v1] surface:** `kairo verify object` is documented in §5.1
and is the v1 verification entrypoint. The snapshot-purpose
form below is the long-term shape and is post-v1 — it depends
on snapshot resolution + closure validation.

Usage:

```text
kairo verify <snapshot-ref> [--purpose <inspect|build|run|reproduce|archive-mirror>]
```

Process:

1. Resolve snapshot reference.
2. Acquire closure for requested purpose.
3. Call core validation through daemon or direct mode.
4. Print validation status and issues.

The CLI must return validation-specific exit codes.

---

## 15. `kairo fetch`

**[post-v1].** Depends on Phase 2 §4 federation. The Git-side
of fetch (per-object Git mirror) ships in v1 as `kairo git
fetch` (Phase 2 §1).

Fetches object data from federation or a remote source.

Usage:

```text
kairo fetch <object-ref-or-locator> [OPTIONS]
```

Options:

```text
--snapshot <snapshot-ref>
--purpose <inspect|build|run|reproduce>
--with-blobs
--with-dependencies
--pin
```

Fetch must:

1. Request remote data through daemon/federation.
2. Store fetched data locally.
3. Avoid execution.
4. Report whether the requested closure is complete, partial, or unavailable.

Fetch does not imply validity.

---

## 16. `kairo sync`

**[post-v1].** Depends on Phase 2 §4 federation.

Synchronizes local object data with federation.

Usage:

```text
kairo sync [object-ref] [OPTIONS]
```

Options:

```text
--pull
--push
--all
--statements
--blobs
--closures
--dry-run
```

Sync must obey daemon publication and ingestion policy.

Sync must not execute object content.

---

## 17. `kairo build`

**[post-v1].** Depends on Phase 2 §7 build/run + RuntimeService.

Builds a snapshot.

Usage:

```text
kairo build <snapshot-ref> [OPTIONS]
```

Options:

```text
--target <target>
--environment <environment>
--output <path>
--plan-only
--reproduce
--yes
--format <human|json|ndjson>
```

Process:

1. Resolve snapshot reference.
2. Acquire build closure.
3. Validate snapshot for `Build`.
4. Ask core for build plan.
5. Ask daemon policy whether build is allowed.
6. If `--plan-only`, print plan and stop.
7. Otherwise dispatch build to daemon runtime/build executor.
8. Track task progress.
9. Report outputs.

The CLI must not invoke arbitrary build commands outside daemon/core planning.

If validation is not `valid`, build must not execute.

---

## 18. `kairo run`

**[post-v1].** Depends on Phase 2 §7 build/run + RuntimeService
+ PolicyService.

Runs a snapshot.

Usage:

```text
kairo run <snapshot-ref> [OPTIONS]
```

Options:

```text
--entrypoint <name>
--environment <environment>
--plan-only
--cap <capability>
--deny-cap <capability>
--yes
--format <human|json|ndjson>
```

Process:

1. Resolve snapshot reference.
2. Acquire run closure.
3. Validate snapshot for `Run`.
4. Ask core for run plan.
5. Display requested runtime capabilities when interactive.
6. Ask daemon policy whether run is allowed.
7. If required, ask user approval.
8. If `--plan-only`, print plan and stop.
9. Dispatch run to daemon runtime executor.
10. Attach to output or print task ID.

If validation is not `valid`, run must not execute.

---

## 19. `kairo reproduce`

**[post-v1].** Depends on Phase 2 §7 build/run.

Reproduces a snapshot.

Usage:

```text
kairo reproduce <snapshot-ref> [OPTIONS]
```

Options:

```text
--plan-only
--strict
--output <path>
--format <human|json|ndjson>
```

Process:

1. Acquire reproduce closure.
2. Validate snapshot for `Reproduce`.
3. Produce build/run plans as required.
4. Enforce stricter reproducibility policy.
5. Execute only if validation and policy allow.

Reproduce should report:

- Input hashes
- Dependency snapshot IDs
- Environment identifiers
- Reproducibility warnings
- Output hashes

---

## 20. `kairo pin` and `kairo unpin`

**[post-v1].** Depends on GC design (DAEMON.md §20). v1 has no
GC and no pins.

Manages local retention pins.

Usage:

```text
kairo pin <object-or-snapshot-ref> [OPTIONS]
kairo unpin <object-or-snapshot-ref> [OPTIONS]
```

Options:

```text
--recursive
--with-blobs
--with-dependencies
--reason <text>
```

Pins prevent garbage collection according to `STORE.md` and `DAEMON.md`.

Pinning does not imply validation.

---

## 21. `kairo list`

**[post-v1].** Depends on indexing. v1 surfaces equivalent
listings through subtree-specific commands: `kairo revision
list`, `kairo branch list`, `kairo tag list`, `kairo trust
list`, `kairo capability list`.

Lists local objects, snapshots, tasks, or other records.

Usage:

```text
kairo list objects
kairo list snapshots <object-ref>
kairo list tasks
kairo list pins
```

List commands must support JSON output for automation.

---

## 22. `kairo status`

Displays local system status.

Usage:

```text
kairo status
```

Long-term must include daemon status, store path/status,
federation status, runtime executor status, active task summary,
and version information.

**[v1]** prints daemon status (probe `<store>/daemon.sock`),
store path + schema version, and version information.
Federation/runtime/task fields are omitted in v1 (those
subsystems are post-v1).

---

## 23. `kairo task`

**[post-v1].** Depends on TaskManager (DAEMON.md §5.6).

Inspects or controls daemon tasks.

Subcommands:

```text
kairo task list
kairo task show <task-id>
kairo task logs <task-id>
kairo task cancel <task-id>
kairo task wait <task-id>
```

Task status must be distinct from validation status.

`task wait` should return the exit code corresponding to the task result when possible.

---

## 24. `kairo store`

**[post-v1].** Store maintenance (verify, GC, rebuild-index)
depends on long-running task surface and GC design. v1 surfaces
store identity through `kairo status` only.

Store diagnostics and maintenance.

Subcommands:

```text
kairo store status
kairo store verify
kairo store gc
kairo store rebuild-index
kairo store path
```

Store maintenance commands must not alter object semantics.

Garbage collection must respect pins and active tasks.

---

## 25. `kairo federation`

**[post-v1].** Depends on Phase 2 §4 federation.

Federation diagnostics and controls.

Subcommands:

```text
kairo federation status
kairo federation peers
kairo federation search <query>
kairo federation publish <object-ref>
kairo federation unpublish <object-ref>
```

Federation commands require daemon/federation support unless a direct federation client is explicitly implemented.

Search results must be labeled as unverified until core validation occurs.

---

## 26. `kairo policy`

**[post-v1].** Depends on PolicyService (DAEMON.md §5.4).

Policy inspection and management.

Subcommands:

```text
kairo policy show
kairo policy check <snapshot-ref> --purpose <build|run|reproduce>
kairo policy approve <request-id>
kairo policy deny <request-id>
```

Policy status must be distinct from core validation status.

---

## 27. `kairo config`

**[post-v1].** Depends on ConfigService.

Configuration inspection and editing.

Subcommands:

```text
kairo config show
kairo config path
kairo config validate
```

Editing commands may be added later, but must avoid unsafe mutation without confirmation.

---

## 28. Safety Prompts

The CLI must ask for explicit confirmation before:

1. Running object content when policy requires approval.
2. Granting runtime capabilities not already approved.
3. Publishing local/private data to federation.
4. Deleting or garbage-collecting unpinned data when user-visible.
5. Overwriting export destinations unless `--force` is supplied.
6. Migrating a store without backup when migration is destructive.

`--yes` may bypass prompts only where safe and policy permits.

`--yes` must not bypass daemon policy denial.

---

## 29. Offline Behavior

`--offline` means:

1. Do not contact federation.
2. Do not fetch remote data.
3. Use local store only.
4. Return indeterminate/not-found when required data is missing.

Offline mode must not silently fall back to network access.

---

## 30. Machine-readable Output

JSON output must be stable and versioned.

Recommended envelope:

```json
{
  "schema": "kairo.cli.result.v1",
  "command": "verify",
  "ok": true,
  "result": {}
}
```

Errors should use:

```json
{
  "schema": "kairo.cli.error.v1",
  "ok": false,
  "error": {
    "code": "validation_indeterminate",
    "message": "Snapshot closure is incomplete",
    "details": {}
  }
}
```

NDJSON output should emit one JSON object per line for task progress.

---

## 31. Human Output Requirements

Human output should be:

1. Clear.
2. Stable enough for documentation.
3. Not relied upon for scripting.
4. Explicit about validation, policy, and task state.
5. Concise by default, detailed with `--verbose`.

Human output must not hide warnings that affect safety or validity.

---

## 32. Error Handling

The CLI must distinguish:

- Command-line usage error
- Daemon unavailable
- Store error
- Federation error
- Core validation status
- Policy denial
- Runtime failure
- User cancellation
- Unsupported feature

Errors should include actionable hints when possible.

Example:

```text
Validation: INDETERMINATE
Reason: Missing authority statement 2b7...
Hint: Run `kairo fetch <object> --purpose run --with-dependencies`
```

---

## 33. Scripting Compatibility

Commands intended for automation must:

1. Support `--format json`.
2. Use stable exit codes.
3. Avoid interactive prompts unless explicitly requested.
4. Fail instead of prompting when stdin is not a TTY, unless `--yes` or an explicit approval option is supplied.
5. Avoid progress spinners in non-human output.

---

## 34. Security Requirements

The CLI must:

1. Never execute fetched/imported content implicitly.
2. Clearly display runtime capabilities before run/build when interactive.
3. Not present unverified data as verified.
4. Not leak private local paths in remote/federation commands unless requested.
5. Respect daemon policy decisions.
6. Avoid shell injection when invoking local helpers.
7. Avoid using human output for security-sensitive machine parsing.

---

## 35. Implementation Checklist

The long-term CLI checklist is:

1. Global option parser.
2. Daemon connection logic.
3. Direct/local mode for basic validation.
4. JSON output envelope.
5. Stable exit codes.
6. `init`.
7. `daemon status`.
8. `inspect`.
9. `verify`.
10. `import`.
11. `fetch`.
12. `build --plan-only`.
13. `run --plan-only`.
14. `task list/show/wait`.
15. `pin` and `unpin`.
16. `status`.
17. Clear validation/policy/task rendering.
18. Non-interactive scripting behavior.

### 35.1 v1 implementation checklist

Phase 2 §2 ships:

1. Global option parser (`--store`, `--daemon`, `--direct`,
   `--offline`, `--format`, `--quiet`, `--verbose`, etc.).
2. Probe-and-fall-back daemon dispatch (§3.3) for read-only
   commands; writes always direct.
3. JSON output envelope and stable exit codes.
4. `kairo init` (already shipped).
5. `kairo daemon start | status | stop` (foreground only).
6. `kairo status` (daemon-aware probe + direct-mode fallback).
7. `kairo verify object` (already shipped).
8. The §5.1 actor / object / revision / branch / tag / trust /
   capability / manifest / snapshot / bundle / git subtrees
   (already shipped — direct mode only).
9. CLI integration tests for daemon dispatch + lifecycle.

`inspect`, `import` (top-level), `export` (top-level), `fetch`,
`build`, `run`, `reproduce`, `pin`/`unpin`, `list`, `task`,
`store`, `federation`, `policy`, `config`, and `kairo web` are
post-v1 per their respective sections.

---

End of `CLI.md`.
