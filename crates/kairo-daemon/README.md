# kairo-daemon

Long-running local service that fronts a Kairo store over an
HTTP+JSON API on a Unix domain socket. The daemon is the only
process that holds an open `FilesystemStore` for read access
served to multiple CLI clients; writes always go direct from
the CLI (see `specs/CLI.md` §3.3).

The crate ships:

- A `kairo-daemon` binary (`src/main.rs`) — foreground only;
  `kairo daemon start` invokes it with the user's `--store`.
- A library (`src/lib.rs`) exposing `serve(Config)` and the
  router used by tests and (post-v1) the web server.

## Phase

Phase 2 §2. Decisions: `specs/DECISIONS.md` §10 (MVP shape) and
§11 (two-process architecture, axum + tokio). Slice plan:
`specs/PHASE_2_DAEMON.md`. This file is updated as slices land.

## Slice 1 status

Crate scaffolding only. `serve` returns immediately; the binary
parses `--store`, installs the tracing subscriber, prints a
banner, and exits. The HTTP server, socket bind, lifecycle,
and handlers land in slice 2 onward.
