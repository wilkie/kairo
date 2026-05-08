# kairo-daemon-client

Rust client for the Kairo daemon's Unix-socket HTTP+JSON API.

In v1, the only consumer is `kairo-cli`'s daemon-mode dispatch.
Post-v1, the same crate backs a future Rust web-server (the
`kairo-web` process — `specs/DECISIONS.md` §11). The crate
deliberately keeps a small, synchronous-feeling surface (`async
fn` per endpoint, typed error enum) so callers do not have to
reason about hyper, tower, or socket lifecycles.

## Phase

Phase 2 §2. Slice plan: `specs/PHASE_2_DAEMON.md`. This file is
updated as slices land.

## Slice 1 status

Scaffolding only. The `Client` type stores a socket path; the
hyper-over-Unix-socket transport, the `probe` / `version` /
`status` methods, and DTO types land in slice 3.
