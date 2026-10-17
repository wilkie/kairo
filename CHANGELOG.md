# Changelog

All notable changes to Kairo are recorded here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), with one
addition: a **`Wire format`** section calls out changes to canonical
bytes (which determine `ActorId` / `ObjectId` / `StatementId`). See
[`VERSIONING.md`](./VERSIONING.md) for the policy.

This project adheres to pre-1.0 [Cargo semver](https://doc.rust-lang.org/cargo/reference/semver.html):
`0.x.0` for breaking changes (Rust API or wire format), `0.x.y` for
additions and fixes.

This file was created mid-flight — entries before its introduction are
reconstructed from git history and grouped at coarse-grained slice level
rather than per-commit.

## [Unreleased]

### Added

- **Browser write path (`PHASE_2.md` §16,
  `PHASE_2_WEB_WRITES.md`, `DECISIONS.md §15`).** `POST
  /api/v1/statements` accepts an unsigned `{ body_kind, body }`
  under a `kairo_session` cookie; the daemon signs as the session
  actor and runs the same verification and capability checks as
  signed mode (`ObjectGenesis` excluded for now; both
  credentials at once is `400 ambiguous_auth`). The web inspector
  gains trust-opinion and branch create/update dialogs on top of
  it.

- **Version-tag dialog in the web inspector (`PHASE_2_WEB_WRITES.md`
  slice 5).** A "Tag version" action on the object page's Tags
  panel writes `ObjectVersionTag` via the session; one form covers
  creating a tag and retargeting the signed-in actor's existing
  one (`supersedes` is derived automatically). Revocation is not
  exposed yet.

- **Daemon write-path Phase B — signed-request auth +
  capability authz (`PHASE_2.md` §15.1,
  `PHASE_2_DAEMON_AUTH.md`, `DECISIONS.md §14`).** The daemon's
  `POST /api/v1/statements` write surface now requires an
  `Authorization: KairoActor1 …` header carrying an ed25519
  signature over a domain-prefixed canonical string covering
  method, path-and-query, body SHA-256, timestamp, and a
  per-request nonce. The daemon resolves the request actor
  through the same `ActorResolver` as the rest of the system,
  enforces a ±60s timestamp window (no server-side nonce cache
  in v1; the nonce is in the signed input so a stateful cache
  can be added later without a wire-format change), and
  verifies the signature against the actor's active signing
  key at `request.timestamp`. Read endpoints stay
  unauthenticated; their trust model is the Unix socket's
  filesystem permissions, unchanged. Seven new 401 error codes
  (`missing_authorization`, `malformed_authorization`,
  `unsupported_auth_scheme`, `timestamp_out_of_window`,
  `bad_request_signature`, `actor_not_found_for_request`,
  `request_key_id_unknown`) surface each failure mode of the
  pipeline. The CLI signs the request envelope with the same
  actor key it already uses for the statement body; the
  command shape and output text are unchanged.
- **Per-kind authorization on the daemon write surface.**
  After authentication, the daemon runs a request-time
  capability check that mirrors `CAPABILITIES.md §6.1`:
  - `ActorTrust`, `ObjectGenesis` — identity-only (signer is
    the authority by construction).
  - `ObjectRevision`, `ObjectBranch`, `ObjectVersionTag` —
    root-authority short-circuit, otherwise
    `evaluate_capability(grantee=signer, scope=Object(...),
    kind, at=request.timestamp)` must return `Held`.
  - `ActorCapabilityGrant` — signer must be root authority OR
    hold every granted kind with `delegable=true` via the new
    `evaluate_capability_for_regrant`.
  - `ActorCapabilityRevocation` — signer must equal the
    original grant's `actor()`.

  Any other outcome is a 403 `not_authorized` carrying the
  underlying `CapabilityEvaluation` variant in the message.
  Direct-mode writes still bypass authz entirely (the
  operator's recovery path); the daemon is the policy
  enforcement layer.
- **`kairo daemon status` reports the live auth posture.**
  `StatusInfo` grew an `auth_mode: String` field, defaulting
  to `"filesystem-perms"` for forward-compat with pre-Phase-B
  clients. Phase B daemons emit `"signed-request"`; the CLI
  prints it as `auth    = signed-request` so operators and the
  inspector can tell at a glance whether the daemon has Phase B
  enforcement active.
- **`DECISIONS.md §14` — Phase B daemon write-path auth shape
  locked.** Eight sub-decisions paralleling §13's pattern:
  two-mechanism split (authn + authz); `Authorization:
  KairoActor1` header with structured params; domain-prefixed
  canonical signing string; SHA-256 body hash always present;
  ±60s timestamp-only replay defense (forward-compatible with a
  future stateful nonce cache); per-kind authz matrix;
  `kairo-web` POST-block remains in place during Phase B (web
  inspector login is its own decision); out-of-scope list
  covering web auth, federation peer auth, mTLS, rate limiting,
  attestation-surface request signing.

- **Daemon write-path Phase A (`PHASE_2.md` §15,
  `PHASE_2_DAEMON_WRITES.md`).** The Phase 2 §2 read-only daemon
  gains a single generic write endpoint, `POST /api/v1/statements`,
  that accepts a `SignedStatement<*>` JSON envelope and dispatches
  on the `type` discriminator. The Phase A allowlist covers seven
  kinds: `ActorTrust`, `ObjectRevision`, `ObjectBranch`,
  `ObjectVersionTag`, `ObjectGenesis`, `ActorCapabilityGrant`,
  `ActorCapabilityRevocation`. Other kinds (attestation-key
  surfaces, key rotation/revocation) stay direct-only in Phase A.
  Verification re-runs the same `verify_envelope_statement`
  (and `verify_signature` for genesis) the direct-mode CLI uses;
  the store-side `put_*` paths are unchanged.
- **CLI dispatch for write commands (`CLI.md` §3.3).** `kairo
  trust grant/block/withdraw`, `revision create`, `branch set`,
  `tag bind/revoke`, `object create`, `capability grant/revoke`
  now route through `cli::dispatch::resolve` — probe the
  `<store>/daemon.sock` socket and post the locally-signed
  envelope through the daemon if reachable, else fall through to
  the existing direct path. Output text is byte-identical across
  modes. The keystore stays the local signing surface either
  way; signed envelopes go over the wire, never private keys.
  `kairo actor create` remains direct-only by design (Phase
  A-permanent exception — keystore-generation is not a daemon
  concern).
- **`kairo daemon status` reports the live write surface.** The
  `StatusInfo` DTO grows a `write_surface: Vec<String>` field
  (single-sourced from the daemon's dispatcher allowlist); the
  CLI prints `writes  = <kinds joined by ,>` so operators and
  the inspector can tell which kinds the daemon currently
  accepts POSTs for. `#[serde(default)]` keeps older clients
  forward-compatible.
- **`kairo-web` proxy POST-block during Phase A
  (`DECISIONS.md §13` sub-decision 5).** The TCP front-end
  filters request methods to `[GET, HEAD]` and returns `405
  Method Not Allowed` with `Allow: GET, HEAD` for everything
  else, before opening a connection to the daemon. Prevents
  browser-side XSS or DNS-rebinding from tunnelling a write
  through to the daemon's unauthenticated write surface during
  Phase A; lifted when Phase B's signed-request envelope lands.
- **`DECISIONS.md §13` — daemon write-path MVP shape locked.**
  Eight sub-decisions, mirroring §10's daemon-MVP pattern:
  two-phase split (writes-first under filesystem-perms, signed-
  request auth later), single generic POST endpoint, signing
  stays client-side, data-model-only validation in Phase A,
  kairo-web POST-block, dispatch parity with reads, no wire-
  format pre-commits for Phase B, out-of-scope list.
- **Cold-storage attestation keys (`ACTORS.md` §5.5).** New per-actor
  attestation key set declared in `ActorGenesis.attestation_keys`,
  used to sign emergency key events without touching the operational
  signing key. New body types: `ActorEmergencyKeyRotation`,
  `ActorEmergencyKeyRevocation`, `ActorAttestationKeyAdd`,
  `ActorAttestationKeyRevocation`. Recovery flow lets an operator
  rotate to a fresh active signing key after compromise of the
  operational key.
- **M-of-N attestation thresholds (`ACTORS.md` §5.5.3).** New
  `attestation_threshold: u8` on `ActorGenesis`. Attestation-surface
  envelopes carry `signatures: Vec<Signature>` (≥ threshold distinct
  attestation-set signers). New body type
  `ActorAttestationThresholdChange` with the asymmetric authority
  rule (raises require `max(current, new)` distinct sigs, lowers
  require `current`).
- **`MultiSignedStatement<B>` envelope** in `kairo-statement`, used by
  every attestation-surface body. Sorted, distinct-`key_id`
  signatures; `verify_envelope_multi_statement` checks threshold +
  per-signature membership in the attestation set.
- **CLI verb tree for attestation keys.**
  - `kairo actor recover-key {sign,prepare,submit}` — emergency
    key rotation via cold-storage attestation key.
  - `kairo actor add-attestation-key {sign,prepare,submit}` —
    append a new attestation key to the actor's set.
  - `kairo actor revoke-attestation-key {sign,prepare,submit}` —
    retract a previously-held attestation key.
  - `kairo actor change-attestation-threshold {sign,prepare,submit}` —
    mutate the M-of-N quorum.
  - `kairo actor co-sign --prepared <path> --actor <id>
    --attestation-key-seed <path>` — append a single signature to a
    partial multi-sig envelope.
  - `kairo actor key-history` extended with attestation adds,
    revocations, and the threshold trajectory (with per-event
    `quorum_at_event`).
- **Capability / delegation model (`CAPABILITIES.md`).** Body types
  `ActorCapabilityGrant` and `ActorCapabilityRevocation`, with scope
  (Object / Actor), statement-kind allowlist, delegation depth, and
  expiration constraints. `evaluate_capability` resolves chained
  grants with cycle detection. New CLI verb tree:
  `kairo capability {grant,revoke,list}`.
- **First-person trust (`STATEMENTS.md` §4.2e).** Body type
  `ActorTrust` with `Trusted` / `Untrusted` / `withdraw`
  decisions, parameterized by the asking truster. `verify object`
  surfaces trust as `trusted` / `untrusted` / `unknown` /
  `unevaluated`. New CLI verb tree:
  `kairo trust {grant,block,withdraw,show,list,history}`.
- **Object branches and version tags.** Body types `ObjectBranch`
  and `ObjectVersionTag`, with cross-actor `supersedes` chains.
  Snapshot identity over the resolved frontier. New CLI verbs:
  `kairo branch {set,show,list}`, `kairo tag
  {bind,revoke,show,list,history}`, `kairo snapshot compute`.
- **Object bundles (`PACKAGE.md`).** Directory-format export/import
  carrying actors, objects, statements, blobs, and a manifest with
  expected Git commit references. New CLI verbs:
  `kairo bundle {export,import}`. Git history is _not_ in the bundle
  in v1; the manifest declares which commits the receiver needs.
- **End-to-end object verification.** `kairo verify object` rolls up
  six independent dimensions (genesis fixity, frontier resolution,
  signature, actor consistency, manifest binding, content layer)
  into a single `VALID` / `INDETERMINATE` / `INVALID` verdict.
  Optional `--as <id>` to evaluate trust from a specific truster.
- **Git integration (`kairo-git`).** Read-only `gix`-backed
  repository handle used by `verify object` to look up the storage
  commit and validate the manifest against the commit's tree.
- **Property tests** for canonical-encoding determinism + JSON DTO
  round-trip across every body type
  (`kairo-statement/tests/property_tests.rs`,
  `kairo-identity/tests/property_tests.rs`).
- **README walkthrough integration test**
  (`examples_readme_walkthrough_round_trip` in `kairo-cli/src/tests.rs`)
  mirrors `examples/README.md` step-for-step so the walkthrough doesn't
  bit-rot when CLI verbs change.
- **Threat model** (`specs/THREAT_MODEL.md`) consolidating the
  security argument: assets, adversaries, defended attacks with
  mechanism cross-references, residual risk, explicit non-goals.
- **MSRV: Rust 1.95.** Pinned in `[workspace.package].rust-version`.

### Changed

- **`kairo-web` lifts its POST-block for an exact allowlist
  (`DECISIONS.md §15`).** The proxy now forwards `POST
  /api/v1/sessions`, `DELETE /api/v1/sessions/current`, and
  `POST /api/v1/statements` in addition to `GET`/`HEAD`, so
  sign-in and browser writes reach a real daemon (previously they
  worked only against MSW mocks). Every request must carry a
  loopback `Host` (`403 web_forbidden_host` otherwise — DNS-rebind
  defense); writes must carry `Origin: http://<Host>` (`403
  web_cross_origin`). Off-allowlist methods are `405
  web_method_not_allowed` with a per-path `Allow` header and a
  JSON error envelope instead of plain text. `kairo web status`
  now sends the dialed loopback address as `Host`.

- **Mock version tags use strict semver.** The MSW registry seeded
  `v1.0.0` / `v1.1.0`, which the daemon's strict-semver parser
  rejects; seeds are now `1.0.0` / `1.1.0`, and the
  `GET /api/v1/version-tags/{object}/{version}` OpenAPI example
  matches.

- Renamed `kairo actor recover-key import` and `kairo actor
  add-attestation-key import` to `submit` for symmetry with
  `prepare` / `co-sign`. The same verb finalizes an envelope
  whether it was assembled inline (`--signature <path>`) or through
  the cosign flow.
- `kairo actor recover-key prepare` and `add-attestation-key
  prepare` now emit a partial envelope with `signatures: []`
  (previously a placeholder-signature hack). `co-sign` and `submit`
  append signatures into the empty array; `submit` constructs the
  `MultiSignedStatement` and verifies the threshold.
- `kairo-cli`'s `main.rs` split into focused modules: `cli` (clap
  definitions), `error` (`CliError`), `format` (verification
  reports), `store_paths` (path resolution), `commands/` (per-verb
  runners), `tests` (the integration test block). 10458-line
  single file → 4640-line dispatcher with sibling modules.

### Wire format

Items in this section change the canonical bytes a body hashes to,
or otherwise alter the on-the-wire shape. Each is a `0.x.0` bump.

- **`ActorGenesis` v1 schema gained
  `attestation_keys: Vec<PublicKey>` and `attestation_threshold:
  u8`.** Both participate in canonical bytes, so the resulting
  `ActorId` differs from any pre-attestation-keys actor body. Every
  existing actor in pre-introduction stores must be re-derived;
  there is no compatibility shim because nothing was deployed yet.
- **Attestation-surface envelopes now carry `signatures` (array)
  instead of `signature` (single).** Affects the JSON shape for
  `ActorEmergencyKeyRotation`, `ActorEmergencyKeyRevocation`,
  `ActorAttestationKeyAdd`, `ActorAttestationKeyRevocation`. The
  multi-sig envelope's canonical bytes exclude signatures (same as
  the single-sig envelope), so the body StatementId is unchanged.
- **New body type
  `ActorAttestationThresholdChange`** added to the canonical
  schema set; carries `new_threshold: u8`. See
  `schemas/canonical/actor-attestation-threshold-change-v1.md`.
- **`ObjectBranch` v1 gained an optional `supersedes`
  field.** Pre-supersedes branch records remain
  parseable (the field is `None`); new records that set it derive
  a different StatementId.

### Fixed

- **`@kairo/ui` Vitest suite.** jest-dom matchers registered on
  the workspace's vitest 2.x `expect` rather than `@kairo/ui`'s
  4.x, so 24 of the package's existing tests failed with
  `Invalid Chai property`. The setup file now extends its own
  `expect`.

[Unreleased]: https://example.invalid/kairo
