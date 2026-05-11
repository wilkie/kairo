# PHASE_2_DAEMON_WRITES.md

## Status

Slice plan for Phase A of the daemon write-path — the
filesystem-perms-only sliver that adds `POST /api/v1/statements`
under the same trust model the v1 reads already use. Phase B
(actor signing-key request auth + capability enforcement +
`kairo-web` POST-pass-through) is a separate slice plan that
lands once Phase A is stable and `DECISIONS.md §13` sub-decision
7's wire format has its own lock.

References:

- `DECISIONS.md` §13 — the eight Phase A sub-decisions.
- `DECISIONS.md` §10 / §11 — daemon MVP and two-process shape
  that Phase A extends.
- `DAEMON.md` §18.1, `API.md` §9.4 — Phase B direction (signed-
  request auth + capability enforcement); not in this slice plan.
- `PHASE_2_DAEMON.md` — the read-side slice plan this parallels.
- `PHASE_2.md` §15 — top-level bullet list (this spec is the
  decomposition of those bullets).

Out of scope (deferred to Phase B or later phases): per-request
authentication, capability enforcement on writes, `kairo-web`
POST/PUT/PATCH/DELETE pass-through, mutation policy
(`DAEMON.md` §12), batch writes, transactional multi-statement
submission, federation push/pull, attestation-key rotation
flows, the keystore moving server-side.

---

## 1. Crate Layout

No new crates. Phase A extends three existing crates:

```text
crates/kairo-daemon         + POST /api/v1/statements handler
crates/kairo-daemon-client  + put_statement methods + DTOs
crates/kairo-cli            + dispatch routing for write commands
crates/kairo-web            + method-filter on the /api/v1 proxy
```

Statement DTOs already exist in `kairo-daemon-client::dto` from
the read slices; Phase A reuses them as request bodies — the
signed statement on the wire is the same JSON shape that read
endpoints return, just posted instead of fetched.

---

## 2. Cross-cutting Conventions

### 2.1 Request body shape

`POST /api/v1/statements` accepts a `SignedStatement<*>` JSON
envelope — the same polymorphic shape statements carry on disk
and that read endpoints return. The body's `type` discriminator
(`ActorTrust`, `ObjectRevision`, `ObjectBranch`,
`ObjectVersionTag`, `ObjectGenesis`, `ActorCapabilityGrant`,
`ActorCapabilityRevocation`) selects the matching store
`put_*` method.

Request `Content-Type` is `application/json`. Bodies are read
into a buffer before parsing — Phase A has no streaming write
endpoints (those would land alongside large-payload statements,
which Phase A does not introduce).

### 2.2 Response shape

The success envelope returns the persisted statement_id and the
echoed body, so callers don't need a follow-up read:

```json
{
  "ok": true,
  "schema": "kairo.api.result.v1",
  "result": {
    "statement_id": "kairo:stmt:…",
    "statement": { /* SignedStatement<*> */ }
  }
}
```

Error envelope is the `API.md` §7.2 shape unchanged.

### 2.3 Verification

The handler calls `kairo_statement::verify_envelope_statement`
against the daemon's existing `ActorResolver` before dispatching
to the store. Verification covers signature validity, envelope/
signature actor match, and (for operational kinds) active-key
resolution at `created_at`. Failures map to:

```text
signature_invalid        -> 422  bad_signature
actor_not_found          -> 422  actor_not_found
signature_actor_mismatch -> 422  signature_actor_mismatch
resolver_unavailable     -> 503  store_error
```

Body-shape errors (decode failure, missing required field) map
to `400 bad_request`. Supersedes/parent invariant violations
that the store enforces map their existing `StoreError` variants
to the codes documented in `PHASE_2_DAEMON.md §2.2`, extended
with:

```text
supersedes_not_found    -> 409  supersedes_not_found
parent_not_found        -> 409  parent_not_found
already_exists          -> 409  already_exists
```

`already_exists` is informational — Phase A treats a duplicate
`statement_id` write as idempotent success when the bodies match
byte-for-byte (re-deliver / retry); only a true byte-level
collision returns the 409.

### 2.4 Auth surface

None. Phase A's handler reads no auth headers, performs no
capability lookup, and trusts every `connect(2)` caller —
identical to the v1 read endpoints. The Unix socket's mode 0600
filesystem permission is the only trust boundary.

### 2.5 `kairo-web` POST-block

`crates/kairo-web/src/proxy.rs` filters request methods before
forwarding. Phase A allows `GET` and `HEAD` only; all other
methods return `405 Method Not Allowed` with `Allow: GET, HEAD`
and a plain-text body that explains the Phase A invariant. The
test suite asserts that POST/PUT/PATCH/DELETE all return 405
without opening a daemon connection.

### 2.6 Test harness

Reuses the `PHASE_2_DAEMON.md §2.5` harness unchanged. Phase A
adds a small helper that builds and signs each statement kind
through `kairo-keystore` against an in-memory `FilesystemStore`,
then posts the result through the in-process client. The same
helper backs daemon handler tests and `kairo-daemon-client`
round-trip tests.

---

## 3. Slice Sequence

### Slice 1 — `kairo-web` POST-block guard

**Ships:**

- `crates/kairo-web/src/proxy.rs` method-filter: accept `GET` and
  `HEAD`, return `405 Method Not Allowed` for everything else.
- `Allow: GET, HEAD` header on 405 responses.
- Plain-text body that explains the Phase A invariant and points
  callers at direct-mode CLI for writes.
- Tests cover POST / PUT / PATCH / DELETE all returning 405
  without opening a Unix-socket connection (so the daemon never
  sees the request).
- `crates/kairo-web/tests/smoke.rs` extends its passthrough test
  to include a POST → 405 assertion.

**Exit criteria:**

- Existing read paths still return 200 through the proxy.
- A POST to `/api/v1/anything` returns 405 with `Allow: GET, HEAD`.
- No new daemon endpoints yet — this slice lands the security
  invariant before Phase A's write endpoint becomes reachable.

**Deferred:** the daemon write endpoint itself; CLI dispatch.

---

### Slice 2 — Trust grant end-to-end

**Ships:**

Server-side:

- `POST /api/v1/statements` handler in `kairo-daemon`.
- Body decode into `SignedStatement<ActorTrustBody>` (Phase A's
  initial dispatcher accepts the `ActorTrust` discriminator only;
  other kinds return `415 unsupported_media_type` with
  `code: unsupported_statement_kind` until later slices open
  them up).
- `verify_envelope_statement` against the daemon's resolver.
- `FilesystemStore::put_actor_trust` on success.
- `try_into_signed_statement` helper that returns a typed
  `Result<SignedStatement<B>, ApiError>` so kind-by-kind
  expansion stays mechanical.

Client-side:

- `kairo_daemon_client::Client::put_statement(&SignedStatement<B>)
  -> Result<PutStatementResult, ClientError>` generic over the
  body type (each kind picks up support as the daemon side opens
  it).
- DTO additions: `PutStatementResult { statement_id, statement }`
  in `kairo-daemon-client::dto`.

CLI-side:

- `kairo trust grant`, `kairo trust block`, and `kairo trust
  withdraw` route through `cli::dispatch`: probe the socket; if
  reachable, call `Client::put_statement`; else fall through to
  the existing direct path. `--daemon` requires reachable;
  `--direct` / `--offline` force direct.
- Output text unchanged (the same `format!` block in
  `run_trust_decide`) so existing scripts/tests continue to
  parse.

**Exit criteria:**

- Handler tests cover signature-valid trust grant, signature-
  invalid (rejected with `bad_signature`), missing actor
  (`actor_not_found`), supersedes-not-found, byte-identical
  re-delivery (idempotent success), byte-different
  `statement_id` collision (`already_exists`).
- Round-trip integration test: `kairo trust grant` against a
  running in-process daemon writes the same statement to the
  store as the direct path.
- The existing `end_to_end_trust_grant_show_list` test passes
  unchanged on `--direct`; a new mirrored test runs it through
  the daemon harness.

**Deferred:** all other statement kinds; capability enforcement
on the write path; signed-request auth.

---

### Slice 3 — Object revisions, branches, version-tags

**Ships:**

- Dispatcher expansion to `ObjectRevisionBody`, `ObjectBranchBody`,
  `ObjectVersionTagBody`. Each routes to its matching store
  `put_*` method.
- Handler tests per kind covering signature-valid, signature-
  invalid, parent-not-found (revisions), supersedes-not-found
  (branches, tags), byte-identical re-delivery.
- CLI dispatch wired for `kairo revision create`, `kairo branch
  set`, `kairo tag bind`. Same probe-and-fall-back as slice 2;
  output text unchanged.

**Exit criteria:**

- Round-trip integration tests for each of the three commands.
- Existing direct-mode tests pass unchanged.
- Mirrored daemon-mode tests cover the same commands.

**Deferred:** object genesis, capabilities.

---

### Slice 4 — Object genesis + capabilities

**Ships:**

- Dispatcher expansion to `ObjectGenesisBody`,
  `ActorCapabilityGrantBody`, `ActorCapabilityRevocationBody`.
- CLI dispatch wired for `kairo object create`, `kairo capability
  grant`, `kairo capability revoke`.
- `kairo actor create` stays direct in Phase A — actor genesis is
  unsigned (see `project_genesis_signature_asymmetry` direction),
  and the keystore-generation step it bundles is not a daemon
  concern. Documented as a Phase A-permanent exception in
  `CLI.md` §3.3.

**Exit criteria:**

- Round-trip integration tests for genesis, grant, revocation.
- The 2-hop delegation chain in `scripts/smoke-seed.sh` can be
  reproduced end-to-end through the daemon (release-binary
  smoke).
- Per-kind handler tests for the new dispatcher arms.

**Deferred:** `kairo actor create` daemon-side; signed-request
auth; capability enforcement on the write path.

---

### Slice 5 — CLI dispatch cutover + integration polish

**Ships:**

- `CLI.md` §3.3 updated: the "writes always direct" v1 line is
  replaced with a per-command table; commands without daemon-side
  support are listed explicitly (currently `actor create` and any
  future direct-only verb).
- A single audit pass through `kairo-cli`'s tests to make sure
  every write command has both a `--direct` and a `--daemon`
  integration test.
- `kairo daemon status` output extends to include "write surface:
  enabled" so the inspector / operator can tell the daemon's
  Phase A surface is live (`status.daemon_running` is already
  true; this is an informational counterpart to the existing
  read-endpoint count).
- Changelog / release notes entry summarizing Phase A and
  pointing at `PHASE_2_DAEMON_WRITES.md` slice plan for the
  build history.

**Exit criteria:**

- `kairo daemon status` reports the new field; existing
  consumers (the inspector) ignore it gracefully.
- `cargo test --workspace` passes.
- The smoke seed runs end-to-end through both `--direct` and
  `--daemon`.

**Deferred:** anything Phase B.

---

## 4. Deliberate Gaps

- **No per-request auth.** The daemon's write surface is Unix-
  socket-trusted exactly like the v1 reads. Phase B adds the
  signed-request envelope.
- **No capability enforcement on writes.** Phase A enforces only
  data-model invariants (signature validity, supersedes/parent
  shape). Cross-actor authority checks are Phase B work because
  the authorization input — "who is making this request" — is
  Phase B's authentication output.
- **No `kairo-web` write pass-through.** The proxy returns 405
  for non-GET methods through Phase A.
- **No mutation policy.** `DAEMON.md §12`'s policy oracle is
  post-MVP.
- **No batch writes.** A future "submit a transactional set of
  statements" endpoint is a separate decision; Phase A does one
  statement per request.
- **No streaming write bodies.** Statement payloads are bounded
  (no blob bytes inline); a streaming write path would land
  alongside a statement kind that needed it, not here.
- **`kairo actor create` stays direct.** Actor genesis bundles a
  keystore-generation step; moving the keystore server-side is
  out of scope for Phase A and arguably for the whole daemon.

---

## 5. Risk Notes

- **Idempotency window.** A client that retries a write after a
  network blip would normally fear a duplicate. Phase A's
  byte-identical re-delivery → idempotent success rule absorbs
  the common case (same signed statement re-sent), and the
  byte-different collision → 409 case is the genuine bug surface
  worth surfacing. Worth a focused test in slice 2.
- **Proxy method-filter drift.** A future `kairo-web` change
  that accidentally widens the allowed-method set would silently
  open the daemon's write surface to the browser. Slice 1's test
  fixture pins the allowlist to `[GET, HEAD]` exactly; any
  expansion must update the test.
- **Direct vs daemon parity.** Each write command has two code
  paths now (direct + daemon). Slice 5's audit ensures every
  command has matching tests; without that, a direct-mode bug
  fix could fail to land on the daemon side or vice versa.
- **Phase B forward-compat.** Sub-decision 7 in `DECISIONS.md
  §13` reserves wire-format freedom for Phase B. The
  immediate risk is a handler middleware shape that pre-commits
  to a specific header layout (e.g., wrapping all writes in a
  layer that strips a `X-Kairo-Signature` header that doesn't
  exist yet). Phase A handlers must keep auth concerns out of
  the request lifecycle entirely.
