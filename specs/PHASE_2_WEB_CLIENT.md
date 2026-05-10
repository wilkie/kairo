# PHASE_2_WEB_CLIENT.md

## Status

Slice plan for the Phase 2 §5 web client. The shape is locked in
`DECISIONS.md` §12; this document decomposes the build into
reviewable commits.

References:

- `DECISIONS.md` §10 — daemon MVP shape (the 11 read endpoints
  this client consumes).
- `DECISIONS.md` §11 — two-process architecture; `kairo-web` is
  the Rust binary half of that split.
- `DECISIONS.md` §12 — the nine sub-decisions this plan executes.
- `WEB_CLIENT.md` — long-term TypeScript surface; v1 covers the
  read-only-inspector subset.
- `API.md`, `DAEMON.md`, `CLI.md` — long-term contracts; v1
  surface tagged by the §10/§12 decisions.
- `PHASE_2.md` §5 — top-level bullet list (this spec is the
  decomposition of those bullets).

Out of scope (deferred to later phases): browser-side actor auth,
bearer-token capability flow, build/run/policy/federation/tasks/
runtime UI, write paths, TLS, multi-user hardening, OAuth,
embedded SPA bundle, async verify, server-sent events.

---

## 1. Crate and Workspace Layout

One new Rust crate plus a new pnpm/Turborepo workspace land in
this phase:

```text
crates/kairo-web              binary + lib (axum app)
frontend/                     pnpm + Turborepo workspace root
  apps/web-client             React + Vite + TanStack
  packages/api-client         generated TS types + fetch wrapper
  packages/object-model       DTO helpers, formatters
  packages/ui                 reusable components
  packages/validation-viewer  validation/graph components
  packages/artifact-viewers   blob/text/image viewers
openapi/kairo-daemon.json     checked-in OpenAPI schema
```

`kairo-web` depends on `kairo-daemon-client` (already in the
workspace) and adds `axum`, `tokio`, `tower-http`, `tracing`,
`tracing-subscriber`, `clap`. The `frontend/` workspace lives at
the repo root, parallel to `crates/` and `specs/`.

`kairo-cli` gains two new commands: `kairo web start | status |
stop`. CLI dispatch (probe-and-fall-back) does **not** apply to
web-server lifecycle commands — they always operate on the
`kairo-web` process directly.

---

## 2. Cross-cutting Conventions

### 2.1 OpenAPI source of truth

`utoipa` annotations on daemon handlers and DTOs are authoritative.
The daemon emits the schema two ways:

- `GET /api/v1/openapi.json` — live introspection.
- A `kairo-daemon dump-openapi` subcommand that writes the schema
  to `openapi/kairo-daemon.json` (checked in). CI verifies the
  file matches the live output.

The frontend regenerates TS types via
`pnpm generate:api`, which runs `openapi-typescript
../../openapi/kairo-daemon.json -o packages/api-client/src/generated/schema.ts`.

### 2.2 `kairo-web` request flow

```text
browser -> 127.0.0.1:<port> -> kairo-web (axum)
                                  ├── ServeDir (--spa-dir)
                                  └── /api/v1/* -> kairo-daemon-client -> Unix socket -> kairo-daemon
```

The `/api/v1/*` surface in `kairo-web` is a thin passthrough: each
route deserializes the request, calls the matching method on a
shared `kairo_daemon_client::Client`, and serializes the response.
No business logic in `kairo-web` for v1.

### 2.3 Anonymous viewer model

Every request reaching `kairo-web` is treated as the public
non-actor persona. There is no session, no cookie, no token. v1
endpoints in the daemon are all read-only and public-readable, so
nothing in the web surface needs an auth check. Future signed-in
flows will layer over this without changing the v1 routes.

### 2.4 TS code conventions

- TypeScript strict mode workspace-wide.
- Generated TS types under `packages/api-client/src/generated/`
  are not edited by hand.
- `Zod` schemas live alongside hand-written types; runtime
  validation runs at the API boundary only (envelopes, errors,
  user input). Not used to reimplement core semantics.
- TanStack Query is the only server-state cache; React local
  state stays UI-only per `WEB_CLIENT.md` §18.
- Query keys live in a single module per `WEB_CLIENT.md` §7.1.

### 2.5 Test harness

- **Daemon side:** existing `kairo-test-support` harness drives
  the new `verify-object` endpoint and the OpenAPI emitter.
- **`kairo-web` side:** integration tests spin up an in-process
  daemon (via the existing harness) plus the web app on an
  ephemeral TCP port; assert browser-shaped requests round-trip.
- **Frontend:** Vitest for unit tests, Playwright for end-to-end
  workflows, Storybook for shared components per `WEB_CLIENT.md`
  §22.

---

## 3. Slice Sequence

### Slice 1 — utoipa + OpenAPI emission

**Ships:**

- `utoipa` and `utoipa-axum` (or equivalent) added to
  `kairo-daemon`'s deps.
- `#[utoipa::path(...)]` annotations on all 11 existing handlers
  plus `ToSchema` derives on every public DTO in
  `kairo-daemon-client::dto`.
- `GET /api/v1/openapi.json` route serving the live schema.
- `kairo-daemon dump-openapi --out <path>` subcommand that writes
  the same schema to disk.
- `openapi/kairo-daemon.json` checked in.
- Snapshot test in `kairo-daemon` that asserts the on-disk schema
  matches the live one (so drift fails CI).

**Exit criteria:**

- `curl --unix-socket .../daemon.sock
  http://localhost/api/v1/openapi.json` returns a valid OpenAPI
  3.x document.
- `cargo run -p kairo-daemon -- dump-openapi --out openapi/kairo-daemon.json`
  is idempotent (no diff on the second run).
- Existing handler tests still pass; no behavior changes.

**Deferred:** validating clients against the schema; multi-version
schema files.

---

### Slice 2 — `verify-object` daemon endpoint

**Ships:**

- `GET /api/v1/verify-object/:id` handler that:
  1. Loads the object's genesis + revisions + branches via
     `FilesystemStore`.
  2. Runs the existing `kairo-core` verify pipeline synchronously
     inside `spawn_blocking`.
  3. Returns a `ValidationResult` DTO modeled on `API.md` §28
     (status + issues list, statement-graph summary).
- `ValidationResult` DTO added to `kairo-daemon-client::dto`,
  including the `status` enum from `WEB_CLIENT.md` §10
  (`valid` / `invalid` / `conflicted` / `indeterminate` /
  `unverified`).
- Client method `Client::verify_object(id)`.
- `utoipa` annotations on the new handler and DTO; regenerate
  `openapi/kairo-daemon.json`.
- CLI: a quiet wiring of the verify path through `kairo verify
  object <id> --json` if it isn't already, so the same code path
  is exercised by both surfaces.

**Exit criteria:**

- Handler tests cover valid / invalid / conflicted / indeterminate
  fixtures; round-trip integration test through
  `kairo-daemon-client`.
- Verify on a missing object returns 404 (`not_found`), not an
  empty result.
- Daemon endpoint count rises to 12; the new endpoint appears in
  the OpenAPI schema.

**Deferred:** snapshot-level verification (post-v1); async or
streaming validation; deep-fetch for missing closure data.

---

### Slice 3 — `kairo-web` crate scaffolding

**Ships:**

- `crates/kairo-web` with a binary + lib target.
- `kairo-web serve --spa-dir <path> --bind 127.0.0.1:<port>
  --daemon-socket <path>` flags. Defaults: port from a fixed v1
  constant (e.g., `7878`), socket from `<store>/daemon.sock`.
- Axum router that:
  - Mounts `tower_http::services::ServeDir` at `/` for the SPA
    bundle (with `index.html` fallback for client-side routes).
  - Mounts `/api/v1/*` as a thin proxy: each route calls into the
    long-lived `kairo_daemon_client::Client`.
  - Binds **127.0.0.1 only** (refuses non-loopback `--bind`
    addresses in v1).
- `tracing` subscriber + `tower_http::trace::TraceLayer` matching
  `kairo-daemon` style.
- `kairo-web --version` and a banner-on-start.
- One smoke integration test: spin up an in-process daemon and
  `kairo-web`; assert `GET http://127.0.0.1:<port>/api/v1/version`
  and `GET /` (a placeholder index) both return 200.

**Exit criteria:**

- `cargo check --workspace` and `cargo test --workspace` pass.
- `kairo-web serve` against a running daemon proxies the existing
  read endpoints byte-for-byte (modulo `Server` headers).
- Refusing a non-loopback `--bind` is covered by a test.

**Deferred:** TLS, auth, CORS, rate limiting, structured-JSON
logs, embedded bundle, hot-reload helpers.

---

### Slice 4 — CLI verbs `kairo web start | status | stop`

**Ships:**

- `kairo web start [--store <path>] [--spa-dir <path>]
  [--bind <addr>]` runs `kairo-web` in the foreground.
- `kairo web status [--bind <addr>]` probes
  `http://127.0.0.1:<port>/api/v1/version` and prints
  reachable/unreachable + the daemon-version it proxied.
- `kairo web stop [--bind <addr>] [--wait]` reads
  `<store>/web.pid` (analogue of `daemon.pid`), sends `SIGTERM`,
  optionally waits.
- Help output and exit codes per `CLI.md` §10.
- End-to-end CLI test mirroring the `daemon start | status | stop`
  test from `PHASE_2_DAEMON.md` slice 4.

**Exit criteria:**

- Round-trip lifecycle test passes against a fresh tempdir store
  (with daemon already running).
- `kairo web status` against a missing process returns
  not-running output and the right exit code.

**Deferred:** `web restart`, `web logs`, supervisor integration,
drop-privileges-after-bind.

---

### Slice 5 — Frontend monorepo scaffold

**Ships:**

- `frontend/` directory with `package.json`,
  `pnpm-workspace.yaml`, `turbo.json`, root `tsconfig.base.json`,
  ESLint + Prettier configs.
- Five package shells:
  - `apps/web-client` — Vite + React + TS, blank shell page that
    fetches `/api/v1/version` and shows the response.
  - `packages/api-client` — `pnpm generate:api` script,
    re-exports `schema.ts`, exports a `createKairoClient(baseUrl)`
    using `openapi-fetch`.
  - `packages/object-model` — empty shell with a `DTO type` re-
    export plan.
  - `packages/ui` — empty shell with a `Button` placeholder.
  - `packages/validation-viewer` — empty shell with a
    `ValidationBadge` placeholder.
  - `packages/artifact-viewers` — empty shell with a
    `BlobPreview` placeholder.
- Root scripts: `pnpm install`, `pnpm generate:api`,
  `pnpm typecheck`, `pnpm test`, `pnpm build`, all routed through
  Turborepo.
- A README at `frontend/README.md` that explains the layout, the
  generated-types pipeline, and the `kairo-web --spa-dir` flag.
- `frontend/apps/web-client/dist/` is the canonical build output;
  the README documents pointing `--spa-dir` at it.

**Exit criteria:**

- `cd frontend && pnpm install && pnpm generate:api && pnpm
  typecheck && pnpm build` runs cleanly.
- The shell page, when served via `kairo-web --spa-dir
  frontend/apps/web-client/dist`, fetches and renders the daemon
  version.
- Generated `schema.ts` is committed (or rebuilt-on-CI — pick at
  slice time).

**Deferred:** Storybook, Playwright, Vitest setup beyond a smoke
test.

---

### Slice 6 — `api-client` + `object-model` + TanStack Query

**Ships:**

- `packages/api-client`:
  - Typed `KairoApiClient` interface with one method per daemon
    endpoint (12 methods after slice 2).
  - `Zod` schemas for the response envelope (`ApiResult` /
    `ApiError`) and the error code enum from `API.md` §8.
  - Error normalization to the `ApiClientError` shape from
    `WEB_CLIENT.md` §6.
  - TanStack Query helpers: a `kairoKeys` module per
    `WEB_CLIENT.md` §7.1 and a `useDaemonStatus` /
    `useObject(id)` / etc. hooks set.
- `packages/object-model`:
  - DTO type re-exports from generated schema.
  - Identifier formatters (truncated display, copy-to-clipboard
    helpers).
  - Display enums for validation status, locality, statement kind.
- A small set of Vitest contract tests in `api-client` against
  mock OpenAPI responses (no live daemon).

**Exit criteria:**

- `apps/web-client` consumes only `api-client` and `object-model`
  for daemon access — no raw fetch outside the package.
- All 12 endpoints have a typed hook.
- Errors from the daemon (`not_found`, `bad_request`, etc.) reach
  React land as discriminated `ApiClientError` variants.

**Deferred:** SSE/WebSocket streaming hooks; mutation hooks
(no write endpoints in v1).

---

### Slice 7 — App shell, routing, and `ui` package

**Ships:**

- `packages/ui`: panel, table, badge, tabs, dialog, empty state,
  error display, status badge components — minimum needed to
  compose the inspector pages without reinventing layout per
  page.
- `apps/web-client` shell:
  - TanStack Router setup with the v1 route tree:
    ```text
    /                         dashboard (daemon status)
    /objects                  object list (placeholder for now)
    /objects/$id              object detail
    /actors/$id               actor detail
    /statements/$id           statement detail
    /blobs/$id                blob preview
    /settings                 settings (daemon URL, theme)
    ```
  - Top-level layout with sidebar nav + main content area.
  - Daemon-status dashboard at `/` showing version, store path,
    schema version, reachable/unreachable.
  - Error boundary + structured error display.
- Storybook setup with stories for the `ui` package primitives.

**Exit criteria:**

- All v1 routes render at least a placeholder.
- Dashboard pulls live daemon status via `useDaemonStatus`.
- Storybook builds and renders the `ui` primitives.

**Deferred:** keyboard shortcuts, theming polish, command palette.

---

### Slice 8 — Object browser

**Ships:**

- `/objects/$id` page composing:
  - Genesis envelope panel (object id, type, signer, created_at).
  - Branches table (one row per `(actor, name)` head).
  - Tags table (name, version, latest tip resolution).
  - Revision history list (chronological).
  - Capability heads table (grantor side).
  - Trust opinions panel (who trusts whom for this object).
- Each row links to the corresponding statement detail page.
- `/actors/$id` page showing the actor genesis and a list of
  signed statements observable in this store.
- `/statements/$id` page showing the raw envelope (pretty-printed
  JSON) plus a typed summary based on statement kind.
- Locality badges per `WEB_CLIENT.md` §15 (just `local` for v1
  since there's no federation yet).

**Exit criteria:**

- A test fixture with a multi-revision, multi-branch, multi-tag
  object renders correctly end-to-end.
- Cross-actor capability-honored tag resolution shows the
  successor's envelope, matching the daemon's behavior.
- Statement detail pages handle all statement kinds the daemon
  returns.

**Deferred:** snapshot detail, build/run plan UI, runtime sessions.

---

### Slice 9 — `validation-viewer` + verify integration

**Ships:**

- `packages/validation-viewer`:
  - `ValidationBadge` (status enum → labeled badge with
    text + color, never color-only per `WEB_CLIENT.md` §10/§20).
  - `ValidationIssueList` component.
- Integration in `apps/web-client`:
  - Object detail page calls `/api/v1/verify-object/:id` and shows
    the badge + issue list inline.
  - Issue links to the offending statement detail page.
  - "Unverified" badge wherever data hasn't been verified yet
    (e.g., raw statement listings).

**Exit criteria:**

- Valid / invalid / conflicted / indeterminate fixtures all
  render distinct, accessible badges.
- The browser never displays a "valid" badge for data that came
  from a non-verify source per `WEB_CLIENT.md` §10.

**Deferred:**

- `StatementGraphView` and `AuthorityChainView` — pulled out of
  slice 9 as a follow-up. Pass A's `ValidationBadge` +
  `ValidationIssueList` + object-detail integration meets every
  slice 9 exit-criteria item, and the graph views are
  *visualizations* of data the inspector already exposes via
  tables (revisions with parents on the object page, capability
  heads + trust opinions panels). The inspector stays usable
  without them; revisit when user feedback prioritizes a
  visual DAG over the existing tabular surface, or when slice
  10 finds Playwright would benefit from a graph snapshot.
  Implementation guidance for the follow-up:
  - `StatementGraphView`: minimal SVG over the
    `useRevisions(object)` data; nodes per revision, edges per
    `parents` entry. No `dagre` dep for v1 — vertical timeline
    with parent arrows is enough for the linear/near-linear
    histories the daemon emits today.
  - `AuthorityChainView`: capability + trust path summary; can
    reuse `useCapabilitiesForObject` / `useTrustAbout` data
    that already drive the object detail page.
- Federation-preview labels — post-v1, no federation endpoints
  yet.
- Deep statement graph performance work — out of scope until
  the basic graph views actually ship.

---

### Slice 10 — `artifact-viewers` + blob preview, tests, close-out

**Ships:**

- `packages/artifact-viewers` with the v1 viewer set:
  - Plain text viewer.
  - JSON viewer (read-only, syntax-colored).
  - Binary placeholder + download button.
  - `ArtifactViewer` registry interface from `WEB_CLIENT.md`
    §12.3, with all v1 viewers tagged `inline-safe`.
- `/blobs/$id` page using the registry to choose a viewer based
  on content sniff (text vs binary; no MIME guessing).
- Playwright tests for the critical workflows from
  `WEB_CLIENT.md` §22:
  - Inspect an object end-to-end.
  - View an invalid validation result.
  - View a conflicted validation result.
  - Render an "unverified" raw statement listing.
  - Preview a text blob; preview a binary blob.
- A CI job that runs the full frontend pipeline (`pnpm install`,
  `pnpm generate:api`, `pnpm typecheck`, `pnpm test`,
  `pnpm build`) plus the new `cargo test -p kairo-web` integration
  tests.
- `PHASE_2.md` §5 close-out: tick the bullets, add a "what
  shipped" summary referencing this spec.

**Exit criteria:**

- All slice 1–9 features survive the Playwright pass.
- `frontend/apps/web-client/dist` builds and `kairo-web --spa-dir
  ./dist` serves the inspector against a real daemon.
- §5 bullets in `PHASE_2.md` are checked.

**Deferred:** image/audio/video/markdown/CSV viewers; sandboxed
runtime viewer; emulator surfaces. Each is a clean follow-up
slice once the registry is in place.

---

## 4. Deliberate Gaps

The following are explicitly out of v1 and have no slice in this
plan; each is its own decision in a later phase:

- **Browser-side actor auth.** Bearer tokens, capability
  statements per request, signing-key flows — all wait for a
  separate auth-direction phase.
- **TLS / non-loopback exposure.** v1 binds 127.0.0.1 only.
- **Write paths.** No object/revision/branch/tag/trust create from
  the browser. The daemon stays read-only for v1.
- **Build / run / runtime sessions / federation / policy / tasks
  UI.** Each gates on its own daemon endpoint group.
- **Embedded SPA bundle.** `--spa-dir` only; embedding is a clean
  follow-up.
- **Streaming events (SSE / WebSocket).** All daemon endpoints
  are sync; verify is sync; no need yet.
- **Snapshot UI.** Snapshots aren't first-class in the daemon's
  v1 read endpoints; surface them when they are.

---

## 5. Risk Notes

- **OpenAPI drift.** The whole frontend pipeline depends on the
  daemon's schema being accurate. The CI check that compares
  on-disk `openapi/kairo-daemon.json` to live introspection is
  the safety net; treat it as load-bearing.
- **Async/sync split widens.** `kairo-web` is the third async
  crate (after `kairo-daemon` and `kairo-daemon-client`). The
  rest of the workspace stays sync per the §11 boundary.
- **Localhost-only in shared environments.** Other local users on
  a shared machine can reach 127.0.0.1. v1 accepts this; the
  bearer-token slice is the right place to harden it. Document
  the limitation in the `kairo-web` startup banner.
- **Validation source-of-truth.** `WEB_CLIENT.md` §10 is firm:
  never display "valid" for data the core didn't validate. Slice
  9's badge component is the chokepoint that enforces this; UI
  reviewers should push back on any path that bypasses it.
- **Test flakiness around port lifecycle.** Slice 3's
  `kairo-web` integration tests must use ephemeral ports
  (`127.0.0.1:0`, read back the bound port) and always clean up
  the spawned process on drop. Mirror the daemon harness pattern.

---

End of `PHASE_2_WEB_CLIENT.md`.
