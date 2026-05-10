# AGENTS.md — `frontend/`

Guidance specific to the TypeScript/React monorepo under
`frontend/`. The repo-wide rules in `/AGENTS.md` still apply
(spec-led, decisions log, etc.); this file pins the
frontend-specific conventions agents should default to.

## Primary Specs

Read before editing:

- `specs/WEB_CLIENT.md` — long-term TS/React surface. The
  shape of validation, locality, accessibility, artifact
  viewing, and policy UI all live here.
- `specs/PHASE_2_WEB_CLIENT.md` — the slice plan that this
  monorepo is implementing.
- `specs/DECISIONS.md` §12 — the technology choices that are
  locked in (React 19, MUI 6, TanStack Query/Router, ky,
  MSW, Vitest/Playwright).

## Accessibility

Accessibility is a first-class requirement, not a polish
item. `WEB_CLIENT.md` §10 (validation status) and §20
(accessibility) are load-bearing — the inspector must remain
usable for color-blind users, keyboard-only users, and
screen-reader users.

Concretely, when writing or reviewing frontend code:

- **Heading structure is real.** Page titles render as
  `<h2>`, panel titles as `<h3>` (set the typography
  `component` explicitly when MUI defaults to `<span>`). A
  screen reader's "headings landmark" navigation is the
  primary way users skim a page; if `getByRole('heading')`
  doesn't find your section, neither does a screen reader.
- **Color is never the only signal.** Status, severity,
  validation, and locality badges all carry a text label —
  the badge component renders it, and the test asserts on
  it. If you find yourself reaching for a colored dot or a
  bare icon as the sole indicator, add a label.
- **Forms and controls have accessible names.** `aria-label`
  for icon-only controls, `<label htmlFor>` for inputs.
  Tooltips reinforce a label, they do not replace it.
- **Tables are tables.** Use the `Table` primitive from
  `@kairo/ui` (which renders MUI Table primitives — `<table>`
  with `<thead>` / `<tbody>` / `<th scope="col">`). Don't
  fake tabular data with a grid of `<div>`s.
- **Tests assert on roles, not text.** Prefer
  `getByRole('cell', { name: 'head' })`,
  `getByRole('heading', { level: 3, name: 'Validation' })`,
  `getByRole('link', { name: 'Download' })` over free-text
  matching. Roles exercise the same accessibility tree a
  screen reader walks; if the assertion passes, the
  semantic structure is correct. Use `{ exact: true }` when
  there's any chance of substring collision.

The Playwright suite under `apps/web-client/e2e/` is the
practical enforcement point: when a failure traces back to
"two heading elements for X" or "no element with role Y",
fix the *component* (give it the right semantics), not the
test (loosen the assertion to free text). The test catching
the issue is the win.

## Component / package conventions

- `@kairo/ui` is the design-system layer — wraps MUI with the
  Kairo prop API (`Panel`, `Table`, `StatusBadge`,
  `LocalityBadge`, etc.). Apps don't import `@mui/material/*`
  for primitives that exist here; reach for the wrapper.
- `@kairo/api-client` exposes typed methods, hooks, and the
  TanStack Query keys (`kairoKeys.*`). The `mock/` subpath
  hosts the MSW registry; tests and dev mode share it.
- `@kairo/object-model` holds display helpers, identifier
  formatters (`canonicalizeId` / `bareId` / `truncateId`),
  type guards, and DTO re-exports. Pure presentation —
  never semantic validation.
- Routes accept either the canonical `kairo:<kind>:<payload>`
  form or the bare payload; canonicalize once at the route
  boundary and pass the full form into hooks. `IdLink`
  navigates to the bare form so URLs stay readable.

## Verification

Local checks before sending changes:

```sh
pnpm install --frozen-lockfile
pnpm turbo typecheck lint test build
pnpm --filter @kairo/web-client e2e   # Playwright suite (MSW)
```

The `e2e` step needs the Chromium binary; run
`pnpm --filter @kairo/web-client e2e:install` once after a
clean install. CI mirrors these commands in
`.github/workflows/ci.yml`.
