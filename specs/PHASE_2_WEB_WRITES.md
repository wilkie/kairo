# PHASE_2_WEB_WRITES.md

## Status

Draft, locked 2026-10-16. Slices 1–5 and 8 shipped; 6–7 open.

Slice plan for `PHASE_2.md` §16: turning the signed-in web
inspector into a write surface. The locked shape is
`DECISIONS.md` §15; this document is the build decomposition.

Slices 1–3 landed before this plan was written (commits
`ebca094`, `190b8f7`, `f7903ca`) and are recorded here
retroactively. Until slice 4 they were exercised only against
MSW mocks: the `kairo-web` proxy still returned `405` for every
non-`GET`/`HEAD` method, so sign-in and writes could not reach a
real daemon.

References:

- `DECISIONS.md` §15 — the seven sub-decisions.
- `PHASE_2_WEB_CLIENT_AUTH.md` — sessions, cookie attributes,
  actor picker; §2.3 is the two-payload-shape seam slice 1 fills.
- `DECISIONS.md` §13 / §14, `PHASE_2_DAEMON_WRITES.md`,
  `PHASE_2_DAEMON_AUTH.md` — the write path and capability
  enforcement both payload shapes share.
- `PHASE_2_WEB_CLIENT.md` — inspector conventions (dumb `ui`
  components + smart containers, MSW, Playwright).

Out of scope: see `DECISIONS.md` §15 sub-decision 7.

---

## 1. Crate and Package Layout

| Location | Change |
|----------|--------|
| `crates/kairo-daemon/src/api/handlers/statements.rs` | cookie-mode dispatcher (`dispatch_session_unsigned`) beside the signed-envelope one |
| `crates/kairo-daemon-client` | `Client::write_via_session`, `SessionWriteRequest` DTO |
| `crates/kairo-web/src/proxy.rs` | request guards: loopback `Host`, `WRITE_ROUTES` allowlist, same-origin `Origin` |
| `crates/kairo-cli/src/commands/web.rs` | `kairo web status` probe sends the dialed loopback address as `Host` |
| `frontend/packages/api-client` | one mutation hook per write kind; MSW handlers approximate capability checks |
| `frontend/packages/ui` | one dumb dialog per write kind, with stories and Vitest |
| `frontend/apps/web-client` | smart containers that derive `supersedes` / defaults from live queries |

No new crates and no new daemon routes.

## 2. Cross-cutting Conventions

### 2.1 Request shape

Cookie-mode writes are `POST /api/v1/statements` with
`Content-Type: application/json` and:

```json
{ "body_kind": "ObjectBranch", "body": { "...": "unsigned body fields" } }
```

The daemon stamps `created_at` and the per-kind subject, signs as
the session actor, and returns the same `PutStatementResult` the
signed path returns. Same-origin `fetch` sends `Origin`
automatically; the frontend sets no auth headers.

### 2.2 Error surface

| Status | Code | Source |
|--------|------|--------|
| 401 | `no_session` / `missing_authorization` | daemon — session expired or absent |
| 400 | `ambiguous_auth` | daemon — cookie and `Authorization` both sent |
| 403 | `not_authorized` | daemon — capability check failed |
| 403 | `web_forbidden_host` | proxy — non-loopback `Host` |
| 403 | `web_cross_origin` | proxy — write without matching `Origin` |
| 405 | `web_method_not_allowed` | proxy — method/path not on the allowlist |
| 415 | `unsupported_statement_kind` | daemon — e.g. cookie-mode `ObjectGenesis` |

Proxy errors use the daemon's `kairo.api.error.v1` envelope so the
frontend has one error parser. Dialogs map `not_authorized` to an
inline "you don't hold this capability" message and `401` to the
picker's re-auth flow.

### 2.3 Cache invalidation

Each mutation invalidates the queries its statement can change
(for `ObjectBranch`: branch list, branch tip, actor statements).
No optimistic updates: the daemon may reject on capability
grounds.

### 2.4 Test harness

- Daemon: in-process daemon over a Unix socket
  (`tests/post_statements_session.rs`).
- Proxy: unit tests drive `handler` against a bogus socket (a
  guard rejection never connects; a pass surfaces as `Connect`);
  `tests/smoke.rs` runs daemon + `kairo-web` in-process and
  round-trips a real cookie-mode write.
- Frontend: Vitest per dialog; Playwright per flow against MSW.
  Slice 8 adds one real-stack Playwright run.

---

## 3. Slice Sequence

### Slice 1 — Daemon session-mode signing *(shipped)*

`POST /api/v1/statements` reads the `kairo_session` cookie; with
no `Authorization` header it dispatches the unsigned body through
`dispatch_session_unsigned`. Covers `ActorTrust`,
`ObjectRevision`, `ObjectBranch`, `ObjectVersionTag`,
`ActorCapabilityGrant`, `ActorCapabilityRevocation`.
`ObjectGenesis` returns `415` (slice 7). OpenAPI regenerated.

### Slice 2 — Trust-opinion dialog *(shipped)*

`<TrustOpinionDialog />` + `useCreateTrustOpinion` on the actor
page; e2e `express-trust.spec.ts`.

### Slice 3 — Branch create/update dialog *(shipped)*

`<BranchUpdateDialog />` + `useUpdateBranch` on the object page;
the container derives `supersedes` from the signed-in actor's
current tip for the chosen name. e2e covers update, create, and
the capability-denied path.

### Slice 4 — `kairo-web` POST-block lift *(shipped)*

**Ships:** the three proxy guards from `DECISIONS.md` §15
sub-decisions 2–4, in that evaluation order (`Host` first, so a
rebound request never learns which routes exist); unit tests
pinning the allowlist, every accepted loopback spelling, and
each rejection; smoke tests for the real-daemon sign-in →
write → sign-out round trip and for rebound / cross-port /
off-allowlist refusals; `kairo web status` probe `Host` fix.

**Exit criteria:** `cargo test -p kairo-web` green; a
cross-origin `POST /api/v1/sessions` returns `403` with no
`Set-Cookie`.

### Slice 5 — Version-tag dialog *(shipped)*

**Ships:** `<VersionTagDialog />` in `@kairo/ui` (stories +
Vitest) and `useSetVersionTag` in `@kairo/api-client`, mounted as
a "Tag version" action on the object page's Tags panel when
signed in. The container offers revisions newest-first and
derives `supersedes` from the signed-in actor's current head for
the typed version, so one form covers create and retarget. The
version field checks strict semver 2.0.0 client-side for UX only
(and suggests dropping a leading `v`); the daemon's
`SemverVersion::parse` stays authoritative. The MSW mock accepts
`ObjectVersionTag` and authorizes the object's creator *or* a
grantee of a seeded capability head whose grant lists the kind
(no chain walk). Mock tag seeds changed from `v1.0.0` / `v1.1.0`
to `1.0.0` / `1.1.0` — the daemon would reject the old spellings.
Daemon test `session_mode_version_tag_bind_retarget_and_grantee`
drives the exact bodies the hook sends against a real daemon.

**Tests:** Vitest per dialog state; mock-contract tests for the
capability-denied and non-semver paths (every keystore actor
holds `ObjectVersionTag` on Alpha, the only seeded object with
revisions, so denial is not reachable through the UI); Playwright
`tag-version.spec.ts` covers retarget, create, and the grantee
path.

**Deferred:** revocation (a tag with no `target`, which requires
`supersedes`) — needs a per-row action in the Tags table rather
than the form.

### Slice 6 — Capability grant / revoke dialogs

`<CapabilityGrantDialog />` (grantee, statement kinds, delegable)
and a revoke action on the capability-heads panel. Grant UI shows
only kinds the signed-in actor can re-grant; the daemon's
`evaluate_capability_for_regrant` remains the check. Revoke is
offered only on grants the signed-in actor issued.

### Slice 7 — Session-mode `ObjectGenesis` + create-object flow

Daemon helper for genesis's asymmetric envelope (signature over
`body.canonical_bytes()`, no envelope wrapping); remove the `415`
branch; then a "New object" flow from the objects list.

### Slice 8 — Real-stack e2e *(shipped, pulled ahead of 6–7)*

Landed before slices 6–7 so they are tested against the real daemon
from the start.

**Ships:** `playwright.real.config.ts` + `e2e-real/` in the web
client. Global setup seeds a temp store with `scripts/smoke-seed.sh`
(which now also writes `seed-ids.json`), starts `kairo-daemon` and
`kairo-web --spa-dir dist` on `127.0.0.1:7979`
(`KAIRO_E2E_REAL_PORT`), passes the generated IDs to tests via
`KAIRO_E2E_SEED`, and tears both down. Binaries come from
`KAIRO_BIN_DIR` (default `target/debug`). Serial, one worker — tests
share the store. `pnpm -F @kairo/web-client e2e:real`; CI job
`e2e-real`.

**Covers:** seeded reads; session survives reload and ends on
sign-out; tag create (persists across reload), tag retarget with
derived `supersedes`, a two-hop delegate (Alice → Bob → Carol)
tagging, a daemon `not_authorized` refusal for a branch write, an
owner branch write, a trust opinion; proxy guards (cross-origin
sign-in mints no cookie, rebound `Host`, `405` + `Allow`).
Mutation-checked: re-blocking `POST /api/v1/statements` in the proxy
fails the suite.

**Convention for slices 6–7:** each new write dialog adds a
real-stack test (happy path + daemon refusal) alongside its MSW e2e.

**Also:** `playwright.config.ts` exports `chromiumLaunchOptions`,
which forces software compositing under WSL2, where headless
Chromium otherwise never fires `requestAnimationFrame` and every
click hangs. CI is unaffected. `WEB_CLIENT.md` §19 / §22 describe
the shipped auth mechanism and the two e2e layers.

---

## 4. Deliberate Gaps

- **No `ObjectRevision` UI.** A revision names a Git tree and
  manifest hash the browser can't produce; it needs a design
  (e.g. pick a commit already in the managed Git cache) first.
- **No nonce or CSRF token.** Under the loopback-only model the
  `Host` + `Origin` guards plus `SameSite=Strict` are the CSRF
  defense; tokens would be redundant.
- **No CORS.** Cross-origin preflights fail at `405`.
- **Attestation-surface kinds** stay CLI-only.

## 5. Risk Notes

- **Loopback is load-bearing.** The guards assume `kairo-web`
  binds loopback only and serves plain HTTP. TLS or a
  non-loopback bind invalidates the `http://<Host>` origin rule
  and the threat model behind `SameSite`; both need re-review.
- **Any local page on the *same* origin can write.** An XSS in
  the inspector itself is not stopped by these guards. Mitigated
  by the inspector rendering statement content as text, never
  HTML; keep it that way.
- **Vite dev proxy.** Dev works because Vite forwards its own
  `Host` and the browser sends the matching `Origin`. Setting
  `changeOrigin: true` in `vite.config.ts` would break writes in
  dev (Host becomes `127.0.0.1:7878`, Origin stays
  `localhost:5173`).
- **Daemon key access.** Cookie mode means the daemon process can
  sign as any keystore actor. That matches what any local shell
  user with the keystore can already do, but a daemon compromise
  now yields signatures, not just store writes.

---

End of PHASE_2_WEB_WRITES.md
