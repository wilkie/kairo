# Versioning policy

Kairo's version number is one workspace-wide value (`workspace.package.version`
in the root `Cargo.toml`). Every crate inherits it via `version.workspace =
true`. While the project is pre-1.0, the same version moves on every release.
This is intentional — at this stage everything is in flux and per-crate
versioning would create overhead without buying clarity.

## Pre-1.0 rules (current)

We follow Cargo's pre-1.0 semver interpretation:

- **`0.x.0`** — bump `x` for any **breaking** change (Rust API or wire format).
- **`0.x.y`** — bump `y` for additions, fixes, doc/test changes, internal
  refactors. No public-API or wire-format break.

A "breaking change" is any of:

- A removed or renamed `pub` item, or a signature change that breaks callers.
- A structural change to a body type that alters its canonical bytes (see
  *Wire-format compatibility* below) — even if the Rust API is unchanged.
- An MSRV bump (covered separately below).

## Wire-format compatibility is the load-bearing surface

The Rust API matters; the **wire format matters more**. Once an `ActorId`,
`ObjectId`, `StatementId`, or canonical-statement byte string is published,
any change to how those bytes are derived forces every consumer to re-derive
every identifier they hold. That's the worst kind of break Kairo can ship,
and the CHANGELOG calls it out as a distinct category (`Wire format`)
alongside the standard `Added` / `Changed` / `Fixed` sections.

Concretely, a `Wire format` entry is required for any change that:

- Modifies a body type's `CanonicalEncode` impl (field order, prefix bytes,
  encoding of optional fields, integer endianness, etc.).
- Adds, removes, or reorders fields on an existing body type.
- Changes the domain prefix passed to `from_bytes` for any ID.
- Bumps a body type's `VERSION` constant.
- Alters the JSON DTO shape in a way that's not a pure rename of an
  unused field (since DTO drift can desync canonical bytes).

Wire-format changes always require a `0.x.0` bump pre-1.0 and a major bump
post-1.0.

Tools that check this:

- `kairo-statement/tests/property_tests.rs` and
  `kairo-identity/tests/property_tests.rs` exercise every body type's
  `CanonicalEncode` round-trip. A regression that changes canonical bytes
  for an unchanged body shape will surface here before reaching review.
- The `schemas/canonical/*-v1.md` documents are the human-readable source of
  truth. Touching them and the `CanonicalEncode` impl together keeps the
  doc and code aligned.

## MSRV

Minimum Supported Rust Version is pinned in `[workspace.package]`
(`rust-version = "1.95"`). Each crate inherits it via
`rust-version.workspace = true`.

MSRV bumps are treated as **breaking** changes:

- Pre-1.0: bump `0.x.0`.
- Post-1.0: bump major.

The MSRV is updated as a maintenance step from time to time, not on every
toolchain release. When bumping, document the reason in the `Changed`
section of the CHANGELOG (e.g., "MSRV bumped to 1.97 to use let-chains
in the verifier").

## Going to 1.0

`1.0.0` is reserved for the point at which:

1. The wire format is committed to long-term stability (or has an explicit
   migration path for any future change).
2. The Rust public API on each library crate is committed to semver.
3. External consumers exist and would meaningfully feel a break.

Until then, the pre-1.0 rules above apply.

## Releases and tags

We don't currently cut tagged releases. Development happens on `main` with
unreleased changes accumulating in `CHANGELOG.md` under `[Unreleased]`.
When a release is cut, the `[Unreleased]` heading is replaced with the
new version + date and a fresh `[Unreleased]` opens above it.
