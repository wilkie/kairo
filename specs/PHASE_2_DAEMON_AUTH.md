# PHASE_2_DAEMON_AUTH.md

## Status

Slice plan for Phase B of the daemon write-path — adding signed-
request authentication and capability-statement authorization on
top of Phase A's `POST /api/v1/statements` surface. Phase A's
filesystem-perms-only trust model goes away on the write
endpoint once Phase B's first auth-enforcing slice ships; the
read surface is unaffected.

References:

- `DECISIONS.md` §14 — the eight Phase B sub-decisions (wire
  format, replay defense, authorization scope, kairo-web
  POST-block disposition, out-of-scope list).
- `DECISIONS.md` §13 — Phase A's shape, which Phase B layers on.
- `DAEMON.md` §18.1, `API.md` §9.4 — the direction lock that
  §14 turns into concrete bytes.
- `CAPABILITIES.md` — the authorization model Phase B enforces
  on the write path. `evaluate_capability` is the oracle.
- `PHASE_2_DAEMON_WRITES.md` — the Phase A slice plan this
  parallels.
- `PHASE_2.md` §15 — top-level bullets (this spec is the
  decomposition of those bullets).

Out of scope (per `DECISIONS.md §14` sub-decision 8): web auth,
federation peer auth, mutual TLS, per-actor rate limiting,
server-side nonce cache, attestation-surface request signing,
browser-side keystore, signing-key delegation to session tokens.

---

## 1. Crate Layout

No new crates. Phase B extends the four crates Phase A touched:

```text
crates/kairo-daemon         + auth middleware + per-kind authz hooks
crates/kairo-daemon-client  + Authorization header builder
crates/kairo-cli            + dispatch hook to sign requests
crates/kairo-statement      + (none — verify primitives stay where they are)
```

A small free-standing module — likely `kairo-daemon-client::auth`
— owns the canonical signing-string assembly and header
formatting. Tests pin its byte output against fixed fixtures so
Phase B's wire format is auditable without spinning up a daemon.

---

## 2. Cross-cutting Conventions

### 2.1 Signing-string assembly

The single canonical signing string is

```text
kairo-daemon-write-v1\n
<METHOD>\n
<path-and-query>\n
<sha256-hex of body>\n
<timestamp-epoch-seconds>\n
<nonce-hex>\n
```

Newlines are LF (`\n`, byte `0x0A`), not CRLF. `<METHOD>` is the
uppercase ASCII verb. `<path-and-query>` is the request's
`:path` exactly as it appears on the wire (no normalization,
no decoding). `<sha256-hex of body>` is the lowercase-hex SHA-256
of the raw body bytes (empty body → the SHA-256 of zero bytes).
`<timestamp-epoch-seconds>` is the i64 rendered as a decimal
ASCII integer. `<nonce-hex>` is the lowercase-hex encoding of
16 random bytes.

### 2.2 `Authorization` header layout

```http
Authorization: KairoActor1 actor=<actor-id>, key_id=<key-id>,
  timestamp=<i64>, nonce=<32-hex>, signature=<base64>
```

Scheme tag is `KairoActor1` (carries a version digit so future
wire-format revisions don't collide). Parameters are
comma-separated `key=value` pairs; ordering is not significant on
the wire, but the canonical builder emits them in the order
above for diffability. Whitespace after commas is permitted but
not required.

### 2.3 Error mapping

Authn failures: `401 unauthorized`, with structured codes:

```text
missing_authorization     -> 401  missing_authorization
malformed_authorization   -> 401  malformed_authorization
unsupported_auth_scheme   -> 401  unsupported_auth_scheme
timestamp_out_of_window   -> 401  timestamp_out_of_window
signature_invalid         -> 401  bad_request_signature
actor_not_found           -> 401  actor_not_found_for_request
key_id_unknown            -> 401  request_key_id_unknown
```

Authz failures: `403 forbidden`, with a single code:

```text
not_authorized            -> 403  not_authorized
```

(carrying the underlying `CapabilityEvaluation` outcome in the
human `message` for diagnostics: `NotHeld`, `Revoked(stmt)`,
`Expired(stmt)`, `DelegationTooDeep`, `GrantorLacksAuthority`).

The error envelope shape is the existing `API.md §7.2`. No new
DTOs.

### 2.4 Test harness

Reuses the Phase A harness in `kairo-test-support` plus a new
helper that wraps `kairo-daemon-client::Client::put_statement`
to elide the auth headers (so a test can ship deliberately
malformed envelopes). The wire-format primitive tests in slice
1 are pure (no daemon needed); each subsequent slice extends
the existing in-process daemon tests.

---

## 3. Slice Sequence

### Slice 1 — Wire-format primitives

**Ships:**

- `kairo-daemon-client::auth` module with:
  - `SigningString::build(method, path_and_query, body, timestamp, nonce) -> String`
    — pure assembly of the §2.1 canonical string.
  - `AuthorizationHeader::format(envelope) -> String`
    — pure construction of the §2.2 header value.
  - `AuthorizationHeader::parse(header_value) -> Result<Envelope, AuthHeaderError>`
    — pure parser of the same shape, used by the daemon.
- Body-hash helper: SHA-256 over the raw body bytes (already
  available in `kairo-core` or `ring` / `sha2` — pick the
  workspace dep that already exists, no new dependency).
- Fixed-fixture tests: a known method/path/body/timestamp/nonce
  pair has a known signing string and header serialization,
  byte-for-byte. Round-trip parse: format → parse → equal envelope.
- Malformed-input tests: scheme-tag mismatch, missing fields,
  trailing garbage, base64 decode failure, hex decode failure
  — each returns a typed error.

**Exit criteria:**

- `cargo test -p kairo-daemon-client auth` covers every error
  variant.
- No daemon-side code touched yet.

**Deferred:** signature verification (slice 2), all daemon
wiring.

---

### Slice 2 — Daemon-side authentication enforcement

**Ships:**

Server-side:

- A `RequestAuth` tower layer (or axum extractor) that runs
  *before* the write dispatcher and:
  1. Extracts and parses the `Authorization` header (401
     `missing_authorization` / `malformed_authorization` /
     `unsupported_auth_scheme`).
  2. Validates the timestamp window (`401 timestamp_out_of_window`).
  3. Resolves the actor via the store-backed `ActorResolver`
     (`401 actor_not_found_for_request`).
  4. Confirms `key_id` matches the actor's active signing key
     at `request.timestamp` (`401 request_key_id_unknown`).
  5. Recomputes the signing string from the request and verifies
     the ed25519 signature (`401 bad_request_signature`).
  6. Stashes the resolved `ActorId` on the request extensions for
     downstream use.
- Read endpoints (everything not `POST /api/v1/statements`) skip
  this layer — the read surface is unauthenticated per the
  decision lock.

Client-side:

- `kairo-daemon-client::Client::put_statement` (and the typed
  per-kind wrappers shipped in Phase A) gains a required
  `secret: &SecretSigningKey` argument. The client builds the
  signing string, signs it, and emits the `Authorization`
  header alongside the JSON body.
- New `ClientError` variants for the auth failure codes so CLI
  callers can render specific messages.

CLI-side:

- `cli::dispatch::Mode::Daemon` is given the local keystore so
  `run_trust_decide` / `run_revision_command` / `run_branch_set`
  / `run_tag_bind` / `run_object_create` / `run_capability_grant`
  / `run_capability_revoke` pass the secret signing key into
  the client. The keystore is already opened locally for the
  statement signature (`DECISIONS.md §13` sub-decision 3); the
  same key signs the request.

**Exit criteria:**

- Per-kind handler tests gain a "missing/invalid Authorization
  → 401" case.
- Round-trip integration test: every Phase A write command runs
  through `--daemon` with the new envelope, success path
  unchanged.
- Direct-mode write commands are unaffected (they don't touch
  the wire).
- Authorization checks (capability enforcement) are *not* yet
  enforced — every authenticated request still falls through to
  the existing handler.

**Deferred:** capability enforcement (slices 3 / 4).

---

### Slice 3 — Capability enforcement on object writes

**Ships:**

- A `CapabilityGate` helper that wraps the capability evaluator
  for the request-time check (signer holds covering grant OR is
  root authority). Sits between the `RequestAuth` layer and the
  per-kind dispatcher arms.
- Per-kind enforcement for `ObjectRevision`, `ObjectBranch`,
  `ObjectVersionTag`:
  - Root-authority short-circuit: if `signer == object.created_by`,
    allow.
  - Otherwise `evaluate_capability(grantee=signer, scope=Object(body.object), kind=K, at=request.timestamp)` must return `Held`.
- Failure path returns 403 `not_authorized` carrying the
  underlying `CapabilityEvaluation` variant in the message.

**Exit criteria:**

- Handler tests cover: root-authority happy path, cross-actor
  with valid grant, cross-actor without grant (403), cross-actor
  with revoked grant (403), cross-actor with expired grant (403),
  cross-actor with delegation-depth violation (403).
- The smoke seed's 2-hop delegation chain runs end-to-end
  through `--daemon` — Bob can issue an `ObjectVersionTag`
  through the daemon because Alice granted him the capability
  (and the daemon-side check now enforces it instead of
  rubber-stamping).

**Deferred:** capability-grant authorization (slice 4).

---

### Slice 4 — Capability enforcement on grant + revocation

**Ships:**

- `ActorCapabilityGrant`: the grantor (signer) must either be
  the object's root authority OR hold the *exact* capability
  being granted with `delegable=true`. This is the recursive
  case in `evaluate_capability` and Phase B routes through it.
- `ActorCapabilityRevocation`: enforce the existing direct-mode
  invariant server-side (signer == grantor of the revoked
  grant). Failure → 403 `not_authorized`.

**Exit criteria:**

- Handler tests cover: root grants by the object's creator,
  delegable re-grant by a holder, non-delegable re-grant attempt
  (403), revocation by the original grantor, revocation attempt
  by a third party (403).
- Smoke-seed re-delegation step (Bob → Carol) still works under
  the new check; an attempt to forge an Alice-issued grant by
  Carol is rejected.

**Deferred:** anything not in Phase B scope.

---

### Slice 5 — Polish + spec sweeps

**Ships:**

- `kairo daemon status` grows an `auth: signed-request` line
  (or extends the existing `write_surface` field with an auth
  flag) so the inspector / operator can see Phase B is live.
- `API.md` §9.4 flipped from forward-pointer to current; §10
  endpoint-group table updated.
- `DAEMON.md` §18.1 updated to point at `DECISIONS.md §14`.
- `CLI.md` §3.3 documents the request-signing step (still
  invisible to the user — same command shape — but the help
  output / FAQ notes it).
- `CHANGELOG.md` Phase B bullet under `[Unreleased] / Added`.
- The smoke seed runs end-to-end through `--daemon` against a
  daemon with Phase B enforcement live (release-binary smoke).

**Exit criteria:**

- `cargo test --workspace` passes.
- `daemon status` reports auth state.
- Smoke seed end-to-end through `--daemon` succeeds; the same
  seed under `--direct` is unaffected.
- All Phase 2 §15 Phase B bullets flipped to `[x]`.

---

## 4. Deliberate Gaps

- **No web write surface.** `kairo-web`'s POST-block stays in
  place for the duration of Phase B (`DECISIONS.md §14` sub-
  decision 7). Lifting it is a separate decision tied to the
  web inspector's login design.
- **No nonce cache.** Phase B's replay defense is the 60s
  timestamp window only. The nonce is in the signed input so a
  cache can be added later without changing the wire format.
- **No attestation-surface request signing.** The four emergency
  body kinds (`ActorEmergencyKeyRotation`,
  `ActorEmergencyKeyRevocation`, `ActorAttestationKeyAdd`,
  `ActorAttestationKeyRevocation`) stay direct-only — they
  require operator-presented cold-storage keys per `ACTORS.md`
  §5.5.2.
- **No federation peer auth.** Federation peers will eventually
  use the same actor-signing primitive but that's §4 work; the
  framing is different (peer-to-peer pull/push, not single-
  origin write requests).
- **No per-actor rate limiting.** Production-quality DoS
  protection on the write surface is post-Phase B.

---

## 5. Risk Notes

- **Clock skew.** The 60s timestamp window is generous for
  same-host but tight enough that a stuck client clock will
  fail authn. Slice 5 should make sure the CLI's error
  rendering for `timestamp_out_of_window` mentions the local
  clock as a possible cause.
- **Key rotation race.** A request signed at `t` reaches the
  daemon at `t + ε`. If a `KeyRotation` body was published with
  `created_at` between `t` and `t + ε`, the daemon's
  `active_key_at(t)` lookup and the client's "current active
  key" view can disagree. We pin verification to
  `request.timestamp` (not server-side `now()`), which makes
  the right key resolvable — but it means an in-flight request
  signed against a just-rotated-away key is still accepted up
  to its timestamp window. This is the intended semantics; the
  alternative (server-side `now()`) would break legitimate
  in-flight requests. Worth a focused test in slice 2.
- **Capability evaluator hot path.** Slice 3 invokes
  `evaluate_capability` on every object write. Existing
  per-actor / per-object indexes make this O(chain depth)
  rather than O(total grants); confirm the daemon's
  spawn_blocking wrapping keeps the hot path off the async
  reactor. If profiling surfaces it as a problem post-Phase B,
  caching the resolved decision per (signer, object, kind,
  window) is forward-compatible.
- **Direct-mode parity.** Direct-mode writes bypass authz
  entirely (they're the operator's own hand). That's by design
  — direct-mode is the recovery path. Slice 5's docs should
  call this out so operators don't expect `--direct` writes to
  be capability-checked.
- **Forward-compat for nonce cache.** Sub-decision 5 reserves
  the option to enforce nonce uniqueness later without a wire-
  format change. Slice 2's tests should *not* assert "duplicate
  nonces accepted" — assert only what's true today (timestamp
  window) so the test fixture doesn't pin Phase B's choice into
  stone.

---

## 6. Future Shapes (Web Writes)

Phase B does not lift the `kairo-web` POST-block (`DECISIONS.md
§14` sub-decision 7). Browser writes are a separate decision-and-
slice unit. This section is a forward pointer so that decision
doesn't re-litigate the wire format from scratch — the
`KairoActor1` envelope and `auth.rs` primitives are stable across
every plausible web-write shape; what changes is *who builds and
signs* the request.

The locked auth-direction memo (`project_daemon_auth_direction`
and `DAEMON.md §18.1`) gestures toward web-server-as-actor by
default but explicitly leaves "or a delegated capability" open.
Three architectures are on the table when the web-write decision
is locked:

### 6.1 Browser-signs-directly

The browser holds the user's signing key (WebAuthn-backed,
WebCrypto with non-exportable keys, or operator-imported
ed25519). The SPA builds the canonical signing string from
`auth::build_signing_string`, signs locally, and ships the same
`Authorization: KairoActor1` header through the kairo-web
proxy to the daemon. kairo-web is a transparent pass-through;
the daemon never knows a browser is involved.

- **Pros:** one auth surface across CLI, browser, and (eventually)
  federation peers. Per-user attribution at the data-model layer
  is preserved. kairo-web stays a thin loopback proxy.
- **Cons:** real in-browser keystore work. WebAuthn-backed keys
  are the safest UX but have export / portability limits;
  operator-imported keys move the risk surface to the user's
  device.
- **Code touch:** add a TypeScript port of `auth.rs`'s pure
  functions (likely under `frontend/packages/api-client/auth.ts`)
  with byte-for-byte fixture tests cross-validated against the
  Rust ones; SPA integrates a keystore module.

### 6.2 Web-server-as-actor

`kairo-web` runs as a configured Kairo actor with its own
signing key (stored alongside the daemon's). End users log in
with normal browser auth — cookie session, OIDC, WebAuthn,
whatever the operator picks — which kairo-web terminates
locally. kairo-web validates the session, then re-signs the
daemon request using its own actor identity via `auth.rs`.

- **Pros:** standard browser auth, no in-browser keystore.
  kairo-web absorbs all the JWT/OIDC/cookie surface so the
  daemon stays single-mechanism.
- **Cons:** every browser-originated write is attributed to the
  web-server actor at the data-model layer. Per-user attribution
  has to be reconstructed somewhere (kairo-web's session log,
  per-user delegated capabilities, audit metadata outside the
  statement). Operationally: a long-lived signing key on the
  server.
- **Code touch:** kairo-web grows a `--actor` flag (or config-
  file equivalent) and a keystore handle; the proxy stops
  rejecting writes from authenticated sessions and re-signs them.

### 6.3 Browser-builds, external-tool-signs

The browser builds the signing string and presents it to the
user; the user signs externally (CLI, hardware key, USB device,
phone) and the browser POSTs the resulting signature. Suited
to high-stakes writes (capability grants, attestation-surface
operations) where per-request confirmation outside the browser
trust boundary is the point.

- **Pros:** no signing key in the browser; per-user attribution
  preserved.
- **Cons:** UX friction. Not the right default for routine
  writes; reasonable as an opt-in second factor on top of (1) or
  (2).
- **Code touch:** SPA flow to display the signing string and
  collect the signature; existing CLI `--sign-payload` or
  similar verb on the operator side.

### Cross-cutting

Whichever path the web-write decision picks, the wire format
stays stable: clients build the signing string per `§2.1`, fill
in an `AuthEnvelope`, and emit `envelope.format()` as the
`Authorization` header. The daemon's `RequestAuth` middleware
from Slice 2 doesn't need to know which path generated the
signature — it verifies a signed request against the resolved
actor's active key and that's the entire contract.

What the future decision *does* need to pin:

- **Browser keystore policy** (paths 6.1 / 6.3): WebAuthn-only?
  Imported keys allowed? Per-origin or per-account?
- **Web-server identity policy** (path 6.2): one actor per
  deployment? Operator-configured at startup? Rotated how?
- **Per-user attribution** (path 6.2 specifically): delegated
  capability per session? Session-to-actor mapping logged
  outside the statement layer? Out of scope for the data model?
- **POST-block lifecycle**: lifted when which slice ships;
  partial lift (e.g., GET + POST `/api/v1/statements` only)
  during the rollout?

None of these need to be answered to ship Phase B. They become
load-bearing when web writes are scoped.
