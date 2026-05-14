# PHASE_2_WEB_CLIENT_AUTH.md

## Status

Draft, locked 2026-05-14.

Slice plan for the first post-§5 web-client increment: moving the
inspector from anonymous-only to actor-attributed via daemon-side
sessions. Scope is deliberately auth + actor picker + session shape;
each write surface gates on its own subsequent slice.

References:

- `PHASE_2.md` §5 — the read-only inspector this builds on.
- `PHASE_2_WEB_CLIENT.md` — slice plan that shipped §5, especially
  §2.2 (request flow), §2.3 (anonymous viewer model), and §4
  (deliberate gaps).
- `PHASE_2.md` §15 / `PHASE_2_DAEMON_WRITES.md` (Phase A) and
  `PHASE_2_DAEMON_AUTH.md` (Phase B) — the daemon write-path this
  slice eventually unlocks for the browser.
- `DECISIONS.md` §10 / §11 — daemon MVP shape and two-process split.
- Memory `project_daemon_auth_direction.md` — post-v1 daemon auth is
  actor signing-key + capability statements; JWT/OIDC stays out of
  the daemon.
- Memory `project_keystore_design.md` — file-perms-only keystore;
  passphrase intentionally deferred.

Out of scope (deferred, each its own slice): write UIs, passphrase
unlock, `ActorProfile` / display names, browser-held signing keys,
TLS / non-loopback exposure, multi-device session sync, session
persistence across daemon restart.

---

## 1. Goals and Non-Goals

### 1.1 In scope

- **Daemon-side session machinery.** In-memory session table bound
  to `actor_id`, cookie-identified, with idle + absolute timeouts.
- **Four new daemon endpoints** for session lifecycle and keystore
  actor discovery, fully `utoipa`-annotated and reflected in the
  checked-in `openapi/kairo-daemon.json`.
- **Frontend `<ActorPicker />`** in the app shell, with
  `useSession()` and `useKeystoreActors()` hooks plus MSW mocks and
  Playwright coverage of the sign-in / sign-out cycle.
- **Anonymous-by-default.** Every shipped §5 inspector surface
  continues to function with no session present; the picker only
  *adds* attribution, never gates reads.

### 1.2 Out of scope (deferred to subsequent slices)

- **Write UIs.** No object/revision/branch/tag/trust create or
  update flows. The auth surface produced here is consumed by
  later slices; no existing write endpoint is touched in this
  slice.
- **Passphrase-protected keystore.** `POST /api/v1/sessions`
  reserves the body shape so a future `passphrase?` field is
  additive; no passphrase logic exists in either the daemon or the
  keystore in this slice.
- **`ActorProfile` statement type + display names.** Actor picker
  shows truncated IDs only; `display_name` is intentionally absent
  from every wire payload. Proposed home for the follow-up is
  `PHASE_2.md` §12 (Statement Type Evolution) — see §8 below.
- **Browser-held keys (external signing).** The
  `Authorization: KairoActor1` request-signing path from Phase B
  stays untouched in this slice. §2.3 documents the forward-compat
  seam so the external-signing path can land later without
  changing the session contract.
- **TLS / non-loopback.** `kairo-web` is loopback-only per
  `PHASE_2_WEB_CLIENT.md` §4; that invariant is load-bearing for
  the session threat model (see §2.2).

---

## 2. Architecture

### 2.1 Session model

A session is an in-memory record on the daemon:

```rust
struct Session {
    actor_id: ActorId,
    created_at: SystemTime,
    last_used_at: SystemTime,
    expires_at: SystemTime,   // = created_at + absolute_timeout
}
```

Session IDs are 256 bits of `getrandom` entropy, base64url-encoded
without padding (43 characters). The daemon holds a
`DashMap<SessionId, Session>` for the lifetime of the process.

**Timeouts** (config keys on the daemon, defaults baked in):

- Idle timeout: **30 min** (sliding; resets on every authenticated
  request).
- Absolute timeout: **24 h** (hard cap from `created_at`).

Lookups are lazy-evicting: when a cookie hits the daemon and the
session has expired by either metric, the record is removed and the
request is treated as anonymous (or rejected, depending on the
endpoint).

**Per-actor multiplicity.** Multiple sessions for the same actor are
allowed (different tabs, different browsers). No coupling between
them; sign-out terminates only the session whose ID matches.

**Key-binding policy.** Sessions bind to an `actor_id`, *not* to a
specific `key_id`. When the daemon later signs on the actor's
behalf (in a future write slice), it resolves the active key at
signing time exactly the way the CLI does. Mid-session rotation or
revocation is detected by the existing §5.5 active-key-at-causal-
position check; no session-invalidation logic.

**Daemon restart loses sessions.** Acceptable for v1; a persisted
session store is an explicit deferral.

### 2.2 Trust boundary

The session's authentication claim is operational: *the user has
opened the daemon's loopback socket and asked to sign as actor X*.
Three invariants make this safe:

1. The daemon listens on a Unix socket (not TCP), so reaching it
   already requires local filesystem access to the socket path,
   which is created with restricted permissions per
   `DAEMON.md`.
2. `kairo-web` binds **127.0.0.1 only** and refuses non-loopback
   binds (`PHASE_2_WEB_CLIENT.md` §3 Slice 3 exit criteria). The
   browser cannot reach `kairo-web` from off-host.
3. The session cookie is `HttpOnly; SameSite=Strict; Path=/`. The
   `SameSite=Strict` setting is the CSRF defense: another local
   origin running in the same browser cannot ride the cookie via a
   cross-site form post.

**Why sessions live in the daemon, not `kairo-web`.** `kairo-web`
has no access to `~/.kairo/keys/` — that's a deliberate split per
`DECISIONS.md` §11. Putting session state in `kairo-web` would
either require keystore access in the proxy (privilege expansion
that defeats the two-process design) or every-request RPCs back to
the daemon to validate (which just moves the storage and adds a
hop). Sessions on the daemon side keep the trust boundary clean.

**Not a parallel auth stack.** The daemon's authoritative
authorization remains `(actor_id, capability)`. Sessions are an
authentication front end that produces the same `actor_id` value
as the existing `Authorization: KairoActor1` header. Per
`project_daemon_auth_direction.md`, JWT / OIDC for federated
multi-user `kairo-web` deployments stays out of the daemon.

### 2.3 Forward-compat seam: external signing

The current Phase B write endpoint (`POST /api/v1/statements`)
takes a signed envelope and verifies a request-level signature in
`Authorization: KairoActor1`. When the eventual browser-side
signing slice lands, it will reuse that same endpoint unchanged.

The session-based write surface that follows *this* slice will
extend `POST /api/v1/statements` (and any new write routes) to
accept either:

- `Authorization: KairoActor1` — body is a fully signed envelope
  (existing Phase B path, used by CLI today, used by browser-held
  keys later).
- `Cookie: kairo_session=<id>` — body is `{ body, body_kind }`
  unsigned; daemon signs the envelope server-side using the
  session's actor.

Both paths fall through to the same capability check and
persistence layer. **Nothing in this slice touches `POST
/api/v1/statements`** — the seam is described here only so the
session design isn't accidentally hostile to it. The "two payload
shapes on one endpoint" pattern is committed to as the long-term
shape so external signing doesn't grow a parallel write stack.

### 2.4 Lifecycle diagram

```text
                                        ┌───────────────────────────┐
                                        │ ~/.kairo/keys/<actor>.json│
                                        └─────────────┬─────────────┘
                                                      │ keystore read
                                                      ▼
   browser ─► kairo-web ─► Unix socket ─► kairo-daemon
   │ Set-Cookie kairo_session=...                │ DashMap<SessionId, Session>
   │                                             │
   ▼                                             ▼
   ActorPicker UI                       /api/v1/sessions
   useKeystoreActors / useSession       /api/v1/keystore/actors
```

---

## 3. Daemon Surface

### 3.1 Session storage

A new module `kairo-daemon::sessions` exposes:

```rust
pub struct SessionStore { /* DashMap<SessionId, Session> */ }

impl SessionStore {
    pub fn new(config: SessionConfig) -> Self;
    pub fn create(&self, actor_id: ActorId) -> (SessionId, Session);
    pub fn touch(&self, id: &SessionId) -> Option<Session>;
    pub fn delete(&self, id: &SessionId) -> Option<Session>;
    pub fn lookup(&self, id: &SessionId) -> Option<Session>;
    pub fn purge_expired(&self);  // exposed for tests; lazy in prod
}

pub struct SessionConfig {
    pub idle_timeout: Duration,
    pub absolute_timeout: Duration,
}
```

`SessionId` is a newtype wrapping `[u8; 32]` with base64url-no-pad
display/parse. `Session` is the struct from §2.1. The store is
held as `Arc<SessionStore>` in the daemon's shared state alongside
the existing `FilesystemStore` / `FilesystemKeystore` handles.

Eviction is lazy on `touch` / `lookup`. A unit test exercises
absolute and idle timeout paths against a controllable clock
(reuse the existing `kairo-test-support::clock` if present;
otherwise introduce a small `Clock` trait scoped to this module).

### 3.2 New endpoints

All four endpoints land under `/api/v1/` and are reflected in
`openapi/kairo-daemon.json`. Endpoint count rises from 16 → 20.

#### `GET /api/v1/keystore/actors`

Lists actor IDs the local keystore can sign for. Implementation is
a directory listing of `~/.kairo/keys/`; no statement reads, no
ActorGenesis walks.

**Response** (200):

```json
{
  "actors": [
    { "actor_id": "z6MkAlice…" },
    { "actor_id": "z6MkBob…" }
  ]
}
```

`display_name` is intentionally absent. Adding it later when
`ActorProfile` lands is additive, not a schema break.

**Errors:** 500 if the keystore directory is unreadable.

#### `POST /api/v1/sessions`

**Request body:**

```json
{ "actor_id": "z6MkAlice…" }
```

**Behavior:**

1. Verify `~/.kairo/keys/<actor_id>.json` exists and is loadable.
2. Verify the actor's active key (per `ACTORS.md` §5.5) is not
   revoked at the current head. If revoked, return `409
   key_revoked` with the revoking statement's ID for inspection.
3. If a `kairo_session` cookie is already present and valid,
   delete the prior session (one session per cookie; the new one
   replaces it).
4. Create a fresh session via `SessionStore::create`.
5. Set the cookie and return.

**Response** (201):

```json
{
  "actor_id": "z6MkAlice…",
  "created_at": "2026-05-14T12:34:56Z",
  "expires_at": "2026-05-15T12:34:56Z"
}
```

**`Set-Cookie`:**

```
kairo_session=<43-char base64url>; HttpOnly; SameSite=Strict; Path=/; Max-Age=1800
```

`Max-Age` matches the idle timeout (not the absolute). The
absolute cap is enforced server-side via `expires_at`.

**Errors:** 400 `invalid_actor_id` (malformed ID), 404
`actor_not_in_keystore` (no key file), 409 `key_revoked`.

#### `GET /api/v1/sessions/current`

**Behavior:** Read `kairo_session` cookie, look up + touch in
`SessionStore`. Returns the session metadata or 401 if absent /
expired.

**Response** (200):

```json
{
  "actor_id": "z6MkAlice…",
  "created_at": "2026-05-14T12:34:56Z",
  "expires_at": "2026-05-15T12:34:56Z",
  "idle_expires_at": "2026-05-14T13:04:56Z"
}
```

`idle_expires_at` reflects the sliding deadline; `expires_at` is
the absolute cap. The frontend may pre-emptively re-auth based on
either.

**Errors:** 401 `no_session` (no cookie or expired).

#### `DELETE /api/v1/sessions/current`

**Behavior:** Read cookie, `SessionStore::delete`, clear the
cookie via `Set-Cookie: kairo_session=; Max-Age=0`.

**Response** (204). Idempotent — calling with no cookie or an
already-expired cookie still returns 204 and a clearing
`Set-Cookie`.

### 3.3 Auth extractor

**Unchanged in this slice.** The existing Phase B `Authorization:
KairoActor1` extractor is not touched. The session cookie is read
only by the four new endpoints; no other handler observes it. The
"session ⇒ daemon signs body" wiring is deferred to whichever
later slice introduces a write surface that uses it.

### 3.4 Cookie attributes

| attribute      | value                  | rationale                             |
|----------------|------------------------|---------------------------------------|
| Name           | `kairo_session`        | namespaced; not generic `session`     |
| Value          | base64url(32 bytes)    | 256 bits of entropy                   |
| `HttpOnly`     | yes                    | JS cannot read the ID (defense-in-depth) |
| `SameSite`     | `Strict`               | CSRF defense                          |
| `Path`         | `/`                    | every same-origin route               |
| `Max-Age`      | `1800` (idle timeout)  | browser auto-discards on idle expiry  |
| `Secure`       | **no**                 | loopback HTTP only; no TLS in v1      |
| `Domain`       | unset                  | inherit origin                        |

The `Secure` omission is the v1-loopback compromise; the moment
TLS or non-loopback exposure is on the table, that flag flips on.

### 3.5 OpenAPI

`utoipa::path` macros annotate all four handlers; `ToSchema`
derives land on the new request / response DTOs in
`kairo-daemon-client::dto`. The drift-detection test
(`openapi_drift.rs`) covers the new schemas automatically. The
`kairo-daemon dump-openapi` subcommand regenerates the checked-in
file.

---

## 4. `kairo-web` Proxy

No new proxy routes. The four new daemon endpoints are reached via
the same `/api/v1/*` passthrough that already serves the read
endpoints (`PHASE_2_WEB_CLIENT.md` §2.2). The proxy must:

- Pass `Cookie` headers daemon-bound on every request.
- Pass `Set-Cookie` headers browser-bound on every response.

The existing `kairo_daemon_client::Client` forwards all headers by
default; the only verification needed is a test that confirms
`Set-Cookie` survives the round trip.

`kairo-web`'s loopback-only enforcement (Slice 3 of `PHASE_2_
WEB_CLIENT.md`) is load-bearing for the session threat model and
must be reasserted by an integration test that boots `kairo-web`
and verifies non-loopback binds still fail.

---

## 5. Frontend

### 5.1 Hooks (in `packages/api-client`)

```ts
// Lists actors the local keystore can sign for.
useKeystoreActors(): UseQueryResult<KeystoreActor[]>;

// Reads the current session, or null when anonymous.
useSession(): UseQueryResult<SessionInfo | null>;

// Opens a session for an actor; invalidates session queries on success.
useSignIn(): UseMutationResult<SessionInfo, ApiError, { actorId: string }>;

// Terminates the current session; invalidates session queries on success.
useSignOut(): UseMutationResult<void, ApiError, void>;
```

Query keys live in the existing module per `WEB_CLIENT.md` §7.1:

```ts
queryKeys.session.current()        // ['session', 'current']
queryKeys.session.keystoreActors() // ['session', 'keystore-actors']
```

`useSession()` polls on the React Query default cadence (`5 min`
stale) plus invalidates on `window.focus`, so an idle-timeout
expiry surfaces within one focus cycle without a tight polling
loop.

### 5.2 `<ActorPicker />` (in `packages/ui`)

Anchored in the app-shell header (right side, before any future
notifications/user menu).

**States:**

- **Anonymous:** renders a "Sign in" button. Click opens a popover
  listing `useKeystoreActors()` results as buttons showing the
  truncated ID (`z6MkAlice…7Fp`) with the full ID in a `title`
  tooltip. Selecting an actor shows a **confirmation step**: "Sign
  in as `z6MkAlice…7Fp`?" with **Confirm** / **Cancel**. Confirm
  fires `useSignIn`.
- **Signed in:** renders the truncated ID + a "Sign out" button.
  No identicons or display names in v1 (those wait on
  `ActorProfile`).
- **Error states:** `key_revoked` → inline message in the popover
  explaining the revocation; `actor_not_in_keystore` → reload the
  actors list (the keystore changed underneath); other errors →
  generic retry banner.

**Empty keystore.** If `useKeystoreActors()` returns `[]`, the
popover renders "No local actors. Run `kairo keygen` first." (No
in-browser keygen in v1.)

### 5.3 App-shell integration

- The picker mounts in `apps/web-client`'s root layout, alongside
  the existing daemon-status indicator.
- Every existing route continues to render identically when
  signed-out; no panel reads `useSession()` for gating in this
  slice.
- Optional: a single tasteful "Signed in as `z6MkAlice…7Fp`"
  attribution strip in the footer of statement-detail and
  validation-result panels, so it's visually obvious which actor
  is active. Acceptable to defer if it inflates the slice.

### 5.4 Test harness

- **Vitest unit tests** on the picker component: anonymous render,
  popover open, confirmation step, sign-in success, sign-out,
  empty-keystore, revoked-key error, network error.
- **MSW handlers** for the four new endpoints in
  `packages/api-client/mocks`. Two seeded keystore actors (reuse
  Alice / Bob from the existing mock registry); cookie state held
  in a small in-memory map keyed on a fake cookie string.
- **Playwright e2e** (one new spec under `apps/web-client/e2e/`):
  load the dashboard signed-out, sign in as Alice, navigate to an
  object, sign out, confirm the header reflects each transition.
- **Cross-process integration test** in `crates/kairo-web/tests/`:
  spin up in-process daemon + web; assert `POST
  /api/v1/sessions` round-trips the `Set-Cookie` header and a
  subsequent `GET /api/v1/sessions/current` reads it.

---

## 6. Slice Sequence

### Slice 1 — Daemon session machinery + endpoints

**Ships:**

- `kairo-daemon::sessions` module with `SessionStore`,
  `SessionId`, `Session`, `SessionConfig` and unit tests covering
  idle timeout, absolute timeout, replace-on-reauth, and concurrent
  access.
- `getrandom` added to `kairo-daemon`'s deps (if not already
  present transitively).
- Four new handlers under `/api/v1/`:
  - `GET /keystore/actors`
  - `POST /sessions`
  - `GET /sessions/current`
  - `DELETE /sessions/current`
- DTOs in `kairo-daemon-client::dto` (`KeystoreActor`,
  `OpenSessionRequest`, `SessionInfo`, etc.) with `ToSchema`
  derives.
- `kairo_daemon_client::Client` gains `keystore_actors()`,
  `open_session(actor_id)`, `current_session()`,
  `close_session()`.
- `utoipa::path` annotations on all four handlers; regenerated
  `openapi/kairo-daemon.json`; the existing
  `openapi_drift.rs` test passes.
- Cookie attribute set and Set-Cookie format verified by handler
  tests.
- Integration test (in-process daemon over Unix socket) covering
  the full sign-in / current / sign-out cycle plus idle-expiry and
  absolute-expiry behaviour with a fake clock.

**Exit criteria:**

- `cargo test --workspace` passes.
- `kairo-daemon dump-openapi --out openapi/kairo-daemon.json` is
  idempotent.
- Daemon endpoint count rises to 20 in the published schema.
- A request to any existing read endpoint with no cookie still
  works (no regressions for anonymous viewers).

**Deferred:** persistent session store, passphrase-protected
sign-in, write endpoints consuming the session, daemon-side body
signing.

---

### Slice 2 — Frontend actor picker + integration tests

**Ships:**

- `pnpm generate:api` against the regenerated OpenAPI; new types
  land in `packages/api-client/src/generated/`.
- Hand-written hooks and Zod envelopes for the four new endpoints
  in `packages/api-client/src/session.ts`.
- MSW handlers for the four new endpoints in
  `packages/api-client/mocks`, with cookie state tracked in-memory.
- `<ActorPicker />` component in `packages/ui`, with Storybook
  stories for each state (anonymous, popover-open, confirmation,
  signed-in, error variants).
- App-shell integration in `apps/web-client`: picker mounted in
  the header.
- Vitest unit tests on the picker.
- Playwright e2e spec covering sign-in / sign-out and idle
  re-auth.

**Exit criteria:**

- `pnpm turbo run typecheck lint test build` passes from the
  frontend root.
- `pnpm exec playwright test` passes with the new spec.
- Every Phase 2 §5 inspector flow continues to work signed-out
  (manual smoke + the existing e2e suite still green).
- The picker reflects state changes within one render cycle of
  `useSignIn` / `useSignOut` resolving.

**Deferred:** display names, identicons, per-route signed-in
gating, write surfaces.

---

## 7. Risk Notes

- **Loopback trust expansion.** Anyone with local-shell access to
  the daemon's Unix socket can already use the CLI to sign as any
  local actor. Sessions don't broaden that audience — they just
  add a browser front-end to the same trust set. The risk *would*
  expand if `kairo-web` ever bound to a non-loopback address;
  that's why slice 3 of `PHASE_2_WEB_CLIENT.md` refuses to. If
  TLS / multi-user kairo-web ever lands, sessions need re-review.
- **Session-restart UX.** Daemon restart logs everyone out without
  warning. The picker handles this via `useSession()` polling +
  401 → anonymous fallback; e2e covers it. Documented as a
  deferral, not a bug.
- **`SameSite=Strict` corner cases.** Strict mode breaks
  cross-origin POST flows but the loopback design has no other
  origin, so this is the right setting. If the inspector ever
  embeds a third-party widget that needs to call back through the
  cookie, revisit.
- **Cookie-jar leakage across users on a shared host.** Different
  OS-level users have different browser profiles and different
  `~/.kairo` directories, so cookies bound to one daemon's port
  don't reach another. Single-user-host model is the v1
  assumption.
- **Replacing a session on `POST /sessions`.** Switching actors
  invalidates the prior session immediately. If a tab in the
  background holds the prior session in cache, its next request
  401s and the picker re-renders to anonymous. Acceptable v1
  behaviour; documented in the slice 2 e2e.
- **Empty / unreadable keystore directory.** Returns an empty list
  rather than a 500 when the directory exists but is empty. 500
  is reserved for unreadable / permission-denied cases so
  operators can distinguish "no actors" from "keystore broken."

---

## 8. Open Follow-Ups

Each of these is its own future slice; none gate slices 1 or 2.

- **`ActorProfile` statement type + display names.** Proposed home
  in `PHASE_2.md` §12 (Statement Type Evolution). Signed, latest-
  wins, GC-friendly (older profile statements not retained); when
  it lands, `GET /api/v1/keystore/actors` and the picker grow
  optional `display_name` fields without a schema break.
- **Passphrase-protected sign-in.** Adds `passphrase?` to
  `POST /api/v1/sessions` body and a passphrase prompt to the
  confirmation step. Paired keystore work: per
  `project_keystore_design.md`, the current MVP is file-perms
  only, so this slice extends the keystore format too.
- **Daemon-side body signing for browser writes.** The first
  consumer of the session — extends `POST /api/v1/statements` (and
  any new write routes) to accept the cookie + unsigned-body
  payload shape from §2.3.
- **Browser-held signing keys (external signing).** A second
  payload mode on the same write endpoints, using
  `Authorization: KairoActor1` from the browser. Requires a
  separate browser-actor lifecycle (delegation from a CLI actor,
  WebCrypto Ed25519 or WASM fallback, recovery story).
- **Session persistence across daemon restart.** Preferred direction
  is **AEAD-encrypted cookie sessions** rather than an on-disk
  session directory:
  - Daemon holds a long-lived secret at
    `~/.kairo/daemon/session-secret.key` (`0600`, auto-generated on
    first start, never logged).
  - Session payload — `actor_id`, `created_at`, `expires_at`,
    `idle_expires_at`, possibly `key_id_at_login` — is serialised,
    AEAD-encrypted under that secret, and the ciphertext is the
    cookie value. No `SessionStore` table; the browser holds the
    only copy of the session.
  - Every request re-decodes the cookie and re-checks
    `expires_at` / `idle_expires_at` in the plaintext.
  - Daemon restart no longer logs everyone out; secret rotation
    via a small keys-ring (old + new during rollover) is the only
    operational lever.
  - **Tradeoff: revocation becomes best-effort.**
    `DELETE /sessions/current` only clears the cookie on the
    requesting browser; other tabs can keep replaying until the
    bake-in expiry. Mitigated by the short idle timeout and by
    `ActorKeyRevocation` invalidating signing at the protocol
    layer regardless of session state. If crisp revocation is
    ever required, add a bounded recently-revoked-session-id
    denylist; the bound is `absolute_timeout`, so it stays small.
  - **Paired with passphrase work.** When passphrase-protected
    sign-in lands, the recommended split is: passphrase unlocks
    the *keystore* key for the duration of the session (mirrors
    CLI behaviour); the session secret stays a single daemon-held
    key, *not* a passphrase-derived key. The stronger
    passphrase-derived-cookie variant remains an option if the
    threat model later wants at-rest protection on the cookie
    itself.
- **Per-actor concurrent-session cap / global session count
  ceiling.** Operational hardening; defer until usage suggests a
  ceiling matters.
- **Federation actor delegation.** Out of scope for the local
  inspector; lives with the federation work (`PHASE_2.md` §4).

---

End of PHASE_2_WEB_CLIENT_AUTH.md
