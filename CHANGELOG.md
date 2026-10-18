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

### Changed

- **Pulls fetch statements in batches (`DECISIONS.md` §24).**
  - `POST /federation/v1/statement-batch` returns up to 256 requested
    statements, each the stored JSON verbatim, in request order. Ids
    the node doesn't serve are left out, so missing and unpublished
    stay indistinguishable.
  - Nodes advertise `statement-batch` in `supports`. The pull uses it
    when the node does, and otherwise fetches per record. Anything a
    batch leaves out is fetched on its own, and every statement is
    still verified individually.
  - A new limit, `max_batch_bytes` (default 32 MiB), is configurable
    under `[federation.limits]`.

- **The statement envelope gains `previous` (`DECISIONS.md` §23,
  step 1 of actor chains).**
  - Every signed statement's envelope now carries `previous`, the
    signer's chain tips: strictly ascending statement ids, at most 16,
    encoded after `created_at` and required in JSON. Every
    `StatementId` changes, since the field is part of the canonical
    bytes.
  - Branches, version tags, capability grants and revocations, and key
    events are chained kinds. `ObjectRevision` and `ActorTrust` are
    unchained and refuse a non-empty `previous`.
  - Every write path signs with the current tips: CLI commands
    (including emergency key prepare; `submit` keeps the prepared
    envelope's `previous`) and daemon session signing. Unchained kinds
    are unaffected. `GET /api/v1/actors/{id}/chains` lists an actor's
    chains and tips for clients that sign their own statements.
  - Federation pulls use the chains:
    - Fetched key events and statements are ingested in chain order.
    - Each closure actor's object chain and `keys` chain are checked
      afterwards. `kairo fetch` reports `incomplete_chains` with the
      missing ids, and flags statements whose `previous` names a
      statement outside their chain. Revoked-key statements don't
      count toward gaps.
    - `--require-complete` fails the fetch (exit 11) on any gap, after
      storing what verified. The two-node smoke now fetches with it.
  - Verification uses the chains:
    - `kairo verify object` and `GET /api/v1/verify-object/{id}` check
      the chains the result depends on. Those are the object and
      `keys` chains of the creator, the revision's and branch's
      signers, and their grantors.
    - A gap or a contradiction makes the result `INDETERMINATE`. The
      CLI lists each chain under `chains:`; the API adds `chains` and
      `chain_incomplete` / `chain_contradiction` issues.
  - The store indexes every actor chain per signer
    (`actor_chains/`, rebuilt by `rebuild-indexes`). It reports each
    chain's tips, and completeness with the missing ids. It also
    reports contradictions: statements whose `previous` names a held
    statement outside the chain. A revocation chains in its grant's
    scope, and while that grant is missing, the grant is reported as
    the gap. Callers can discount statements signed with revoked keys.

- **Git history federates over Git's smart HTTP protocol v2
  (`DECISIONS.md` §22).**
  - Each published object's Git history is served at
    `/federation/v1/objects/{id}/git` (`info/refs`,
    `git-upload-pack`), and a pull runs `git fetch` against it. A
    re-pull negotiates and fetches only the new objects.
  - The server filters every request. It answers `ls-refs` from the
    published commits, refuses unpublished `want`s (`400
    fed_bad_git_request`), drops unpublished `have`s, and refuses
    other commands and protocol v0/v1. The shared pool's unpublished
    commits stay unreachable.
  - Both sides run Git with host config ignored and safety settings
    pinned. The client caps a fetch's bytes and time by watching its
    quarantine.
  - The old single-pack endpoint and the server's pack cache are
    removed, so the server writes nothing again.
    `--max-concurrent-packs` still caps concurrent fetches, and a
    30-second stall timeout replaces the size-based deadline.
  - Any Git client can `git clone --mirror` a published object's
    commits (content only; verification still needs the
    statements).

- **Git transfer follows Git-host practice (`DECISIONS.md` §21).**
  - Packing holds no lock: it snapshots the refs and packs with
    `pack-objects --revs`. Serving caps concurrent fetches
    (`--max-concurrent-packs`, `503 fed_busy` with `Retry-After` past
    it); see §22 above for the transport.
  - Incoming packs (federation pull, bundle import) go through a
    quarantine and only reach the pool when complete, with no lock
    held during the download.
  - `kairo git fetch` no longer holds the pool lock.

- **Revision history is a Git-like log (`DECISIONS.md` §20).**
  - A per-object revision index (revision id → statements) lets
    history be walked along parent links, newest first.
  - `GET /api/v1/objects/{id}/log` and `kairo revision log` page
    through it, replacing `GET /api/v1/revisions/{object}` and
    `kairo revision list`.
  - Federation closures and bundle exports carry only revisions
    reachable from branches, tags, and the genesis's initial revision.
    Bundle export no longer walks the whole store.
  - The inspector's Revisions panel pages with "Load older
    revisions".

- **Git object formats (`DECISIONS.md` §19).**
  - Revision ids are `git:sha1:<40 hex>` or `git:sha256:<64 hex>`,
    parsed strictly; the label decides the format, never the length.
    They are enforced at `revision create` / `object create`, the
    daemon write path, bundle import, and federation pull. One
    object's history has one format.
  - The Git cache keeps one pool per format (`pool-sha1/`,
    `pool-sha256/`, replacing `pool/`), and gix is upgraded 0.66 →
    0.89 with SHA-256 support, so SHA-256 repositories verify.
  - `kairo git fetch` takes `--object-format`.
  - Bundle manifests list typed revisions in `expected_commits`.
  - Fixtures that labeled SHA-1 commits `git:sha256:` are relabeled.
- **Federation fix-ups (`PHASE_2_FEDERATION.md`).**
  - Pulled and bundled Git packs are checked for every commit's full
    closure before anything is pinned, and a failed check leaves no
    repo behind.
  - The server advertises a pack only when the object's repo has
    refs.
  - Statements signed with a revoked key are stored and flagged
    instead of skipped (the report field is now `flagged`).
  - A pinned peer prints "claim matches, node not authenticated".
  - Peer URLs are compared case-insensitively.
  - DECISIONS §16 and §18 now state that withheld revocations can't
    be detected from one peer, and that discovery needs node
    authentication.

### Added

- **Two-node federation check in CI (`PHASE_2_FEDERATION.md`
  slice 6).** `scripts/federation-smoke.sh` builds an origin node
  with a real Git-backed revision, serves it, and pulls it into a
  second store through a pinned config entry. It checks that
  `verify object --no-cwd-repo` is VALID from the cache alone, that
  provenance names the origin, and that a re-fetch is incremental.
  CI runs it for SHA-1 and SHA-256 histories. The real-stack
  Playwright suite pulls an object the same way and checks the
  inspector's "Fetched from" badges.

- **Federation policy and visibility (`PHASE_2_FEDERATION.md`
  slice 5).**
  - A node config file, `<store>/config.toml`, parsed strictly by
    the new `kairo-config` crate (`DECISIONS.md` §17). It has a
    `[federation]` section: `enabled`, limit overrides, and
    `[[federation.peers]]` entries with optional `node_actor` pins.
  - `kairo fetch` refuses unlisted peers unless `--allow-peer`. It
    also refuses a pinned peer whose claimed node actor changed
    (before fetching anything), and refuses under `--offline` or
    `enabled = false`.
  - New commands: `kairo federation status` (publish list, peers,
    pins, last pull per peer) and `kairo config path | validate`.
  - The daemon serves `GET /api/v1/provenance/{record}`, and the
    inspector shows a "Fetched from <peer>" badge on statement,
    object, and actor pages.

- **Federation Git transfer (`PHASE_2_FEDERATION.md` slice 4).**
  `GET /federation/v1/objects/{id}/git` streams the object's Git pack
  from the managed cache. `kairo fetch` ingests it when a pulled
  revision's commit is missing locally, checks every commit resolves,
  and pins them under `refs/kairo/imported/`. A fetched object now
  passes `kairo verify object --no-cwd-repo` from the cache alone.
  `kairo fetch` reports a `git =` outcome.

- **Federation pull (`PHASE_2_FEDERATION.md` slice 3).** `kairo fetch
  --object <id> --from <url>` pulls a published object's closure from
  a peer: its actors and their key events, its genesis, its grants and
  revocations, its statements, and its blobs. It fetches only what
  the store lacks, so a re-pull fetches nothing new. Each record is
  checked for schema, fixity (its content matches its id), placement
  (`ActorTrust` is always refused), and signature before it is
  stored. Statements signed by a revoked key are skipped. Each record
  gets a provenance entry naming the peer, and `kairo federation
  provenance --ref <typed-ref>` shows them. The client requires
  `https://` except on loopback, follows no redirects, and enforces
  size, count, and time limits. It adds `ureq` with rustls, the
  workspace's first TLS dependency.

- **Federation serving (`PHASE_2_FEDERATION.md` slice 2).** New
  `kairo-federation-http` crate and binary: a separate, read-only,
  loopback-by-default process that serves the closures of this
  node's published objects (`DECISIONS.md` §16). It exposes the node
  document, published-object and per-object listings, object
  genesis, actor key-event listings, and single statements and
  blobs. An id outside every published closure gets the same `404`
  as one the store doesn't hold. `kairo-store` gains
  `object_closure`, `actor_key_event_ids`, and
  `get_object_genesis_bytes`. Pulling (slice 3) is not implemented
  yet.

- **Federation publish list and wire types (`PHASE_2_FEDERATION.md`
  slice 1).** New `kairo-federation-core` crate with the object-pull
  wire types (node document, published-object, object and actor
  listings, pagination, `fed_*` error envelope), which validate at
  parse time. `kairo federation publish --object <id>`,
  `unpublish`, and `published` manage the node's opt-in publish
  list at `<store>/federation/published.json` (`DECISIONS.md` §16).
  Nothing serves it yet; that is slice 2. `ActorKind::node()` names
  a federation node's own actor.

- **Create objects from the web inspector (`PHASE_2_WEB_WRITES.md`
  slice 7).** Cookie-mode `POST /api/v1/statements` now accepts
  `ObjectGenesis` with a body of just `{ object_kind,
  initial_revision? }`; the daemon fills `created_by`, `created_at`,
  and a fresh nonce and signs the body (genesis has no envelope).
  A "New object" action on the Objects page and the signed-in
  actor's own page creates one and opens it. This completes the
  browser write surface planned in `PHASE_2.md` §16.

- **Capability grant / revoke dialogs in the web inspector
  (`PHASE_2_WEB_WRITES.md` slice 6).** "Grant capability" on the
  object page's Capability heads panel writes an object-scoped
  `ActorCapabilityGrant` (kinds, delegable, optional expiry); a grant
  to someone who already holds one from the signer updates that chain
  with a derived `supersedes`. A per-row Revoke on grants the signer
  issued writes `ActorCapabilityRevocation`, forward-only unless the
  warned, reason-required retroactive option is chosen. The table now
  shows kinds, delegable, expiry, and revocation status.

- **Real-stack e2e suite (`PHASE_2_WEB_WRITES.md` slice 8).**
  `pnpm -F @kairo/web-client e2e:real` runs Playwright against the
  built SPA served by a real `kairo-web` + `kairo-daemon` over a
  store seeded by `scripts/smoke-seed.sh` (which now also writes
  `seed-ids.json`): sessions, every shipped write dialog, a daemon
  capability refusal, and the proxy guards. New CI job `e2e-real`.
  The Playwright configs force software compositing under WSL2,
  where headless Chromium otherwise never fires
  `requestAnimationFrame`.

- **Browser write path (`PHASE_2.md` §16,
  `PHASE_2_WEB_WRITES.md`, `DECISIONS.md §15`).** `POST
  /api/v1/statements` accepts an unsigned `{ body_kind, body }`
  under a `kairo_session` cookie; the daemon signs as the session
  actor and runs the same verification and capability checks as
  signed mode (both credentials at once is `400
  ambiguous_auth`). The web inspector
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

- **`CapabilityHeadDto` carries the grant's facts.** Both
  `GET /api/v1/capabilities/{grantor}` and
  `.../for-object/{object}` now return `statement_kinds`,
  `delegable`, `constraints`, and the grantor's `revocation` (if any)
  per head. Additive and `#[serde(default)]`; older clients keep
  decoding.

- **Rust toolchain pinned to 1.99.1.** `rust-toolchain.toml` and
  the CI workflow move from floating `stable` to `1.99.1`, so new
  releases can't break `fmt --check` / `clippy -D warnings` without a
  code change; bump policy in `VERSIONING.md` ("Toolchain"). The three
  hex decoders flagged by 1.99's new `chunks_exact`-with-constant lint
  now use `as_chunks::<2>()`. MSRV is unchanged (1.95).

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

- **Mock capability grants used `kinds`.** The MSW registry seeded
  grant bodies with `kinds` instead of the wire field
  `statement_kinds`.

- **`@kairo/ui` Vitest suite.** jest-dom matchers registered on
  the workspace's vitest 2.x `expect` rather than `@kairo/ui`'s
  4.x, so 24 of the package's existing tests failed with
  `Invalid Chai property`. The setup file now extends its own
  `expect`.

[Unreleased]: https://example.invalid/kairo
