# Kairo frontend

pnpm + Turborepo monorepo for the Kairo web client.

This is Phase 2 §5 of the project — see `specs/DECISIONS.md`
§12 for the locked shape and `specs/PHASE_2_WEB_CLIENT.md` for
the slice plan. Slice 5 (this scaffold) ships the workspace,
the OpenAPI type-generation pipeline, and a "Hello Kairo" shell
page that fetches `/api/v1/version`.

## Layout

```text
frontend/
├── apps/
│   └── web-client/             Vite + React + TypeScript app shell.
└── packages/
    ├── api-client/             Typed daemon client (openapi-fetch
    │                           around generated TS types).
    ├── object-model/           Display helpers + DTO re-exports
    │                           (slice 6).
    ├── ui/                     Shared design-system components
    │                           (slice 7).
    ├── validation-viewer/      Validation badges, issue list,
    │                           statement-graph view (slice 9).
    └── artifact-viewers/       Text / JSON / binary preview
                                registry (slice 10).
```

The five-package layout is locked in `DECISIONS.md` §12.5 and
mirrors `WEB_CLIENT.md` §4. Packages start as thin shells; later
slices fill them in.

## Toolchain

- **pnpm** 10.x (workspace + dependency hoisting).
- **Turborepo** 2.x (task pipeline + caching).
- **Vite** 6.x (dev server + build for the app).
- **React** 19.x + **TypeScript** 5.7+ (strict mode workspace-wide).
- **TanStack Query** 5.x (server-state cache + suspense-friendly
  hooks).
- **TanStack Router** 1.x (file-system-style typed routing).
- **ky** 1.x as the HTTP transport — `KairoApiClient` wraps it
  with typed methods that unwrap the API envelope.
- **openapi-typescript** 7.x for `paths`/`components` types.
- **MSW** 2.x for the dev / test mock daemon (lazy-loaded — never
  in the production bundle).
- **ESLint 9** flat config with `typescript-eslint`,
  `eslint-plugin-react`, `eslint-plugin-react-hooks`,
  `eslint-plugin-jsx-a11y`, and `eslint-config-prettier`.
- **Prettier 3** with two non-default tweaks (`singleQuote: true`,
  `printWidth: 100`).

## OpenAPI pipeline

The Rust daemon (`crates/kairo-daemon`) is the source of truth.
The pipeline keeps the frontend's types aligned:

```text
utoipa annotations on Rust handlers/DTOs
  └─→ kairo-daemon dump-openapi --out openapi/kairo-daemon.json
        (Rust drift test asserts on-disk == live schema)
            └─→ openapi-typescript reads openapi/kairo-daemon.json
                  └─→ packages/api-client/src/generated/schema.ts
                        (committed; consumers typecheck without
                         running the generator)
```

Re-generate from this directory whenever the daemon's schema
changes:

```sh
pnpm generate:api
```

The Rust drift test (`cargo test -p kairo-daemon --test
openapi_drift`) catches drift on the daemon side; commit both
the regenerated `openapi/kairo-daemon.json` and the regenerated
`packages/api-client/src/generated/schema.ts` together.

## Development workflow

Run each component in its own terminal. The order matters:
the daemon needs to come up first because the web server proxies
to its socket.

```sh
# 1. Start a daemon over a tempdir store.
cargo run -p kairo-daemon -- --store /tmp/kairo-dev

# 2. Start kairo-web pointed at the dev SPA dir. Slice 5's app
#    shell is enough for this slice; later slices add real
#    routes. --spa-dir wants a built bundle, so build first.
cd frontend && pnpm install && pnpm build
kairo --store /tmp/kairo-dev web start \
  --spa-dir frontend/apps/web-client/dist

# Browse to http://127.0.0.1:7878
```

For frontend-only iteration there are two options.

**Option 1 — proxy to a running kairo-web (real daemon):**

```sh
# Terminals 1 & 2: daemon + kairo-web as above.
# Terminal 3:
cd frontend && pnpm dev
# Browse to http://127.0.0.1:5173
```

The proxy target is `127.0.0.1:7878` by default; override with
the `KAIRO_WEB_PORT` env var.

**Option 2 — mock daemon in-browser (no Rust required):**

```sh
cd frontend && VITE_USE_MOCK_API=true pnpm dev
# Browse to http://127.0.0.1:5173
```

When `VITE_USE_MOCK_API=true` is set, `apps/web-client/src/main.tsx`
dynamically imports `@kairo/api-client/mock` and starts an MSW
service worker before mounting React. Every `/api/v1/*` request
the SPA issues is answered by the in-process handlers in
`packages/api-client/src/mock/handlers.ts`, so frontend
development can proceed without the Rust daemon running. The
mock module is code-split — it never enters the production
bundle.

Test-side mocking lives at the `@kairo/api-client/mock/node`
subpath: Vitest setup files import `setupMockServer()` to
intercept fetch calls in Node.

## Scripts

Run from `frontend/`. All are routed through Turborepo so caching
+ task dependencies stay correct.

```sh
pnpm install          # Install workspace deps.
pnpm generate:api     # Regenerate src/generated/schema.ts.
pnpm typecheck        # tsc --noEmit across all packages.
pnpm lint             # ESLint across all packages.
pnpm lint:fix         # ESLint auto-fix from the workspace root.
pnpm build            # Build apps + libraries; produces
                      # apps/web-client/dist/.
pnpm dev              # Run all dev servers in parallel.
pnpm test             # Reserved; slice 10 wires Vitest +
                      # Playwright.
pnpm format           # Prettier write.
pnpm format:check     # Prettier check (CI-friendly).
```

## Build output

`pnpm build` writes the app bundle to
`apps/web-client/dist/`. Point `kairo web start --spa-dir` at
that directory to serve the production bundle.
