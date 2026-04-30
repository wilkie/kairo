# Kairo Project Layout

Kairo is a federated software archival system for preserving, building, running, discovering, and verifying software artifacts over long periods of time.

The system is organized around a small set of durable concepts:

- **Objects**: versioned software artifacts, backed by Git or another content-addressed store.
- **Actors**: cryptographic identities that sign actions and claims.
- **Statements**: signed append-only facts about Objects, Actors, versions, builds, providers, trust, and federation state.
- **Providers**: Objects that provide capabilities, tools, runtimes, operating systems, architectures, or execution environments.
- **Resolution Records**: concrete dependency selections derived from abstract requirements.
- **Environment Plans**: concrete runtime/build execution plans, including provider chains and VM/container manifests.
- **Build Artifacts**: cached build outputs with exact provenance and runtime metadata.

The repository is divided into several subprojects. The most important rule is that the trusted data model lives in shared Rust crates, while user-facing applications and web interfaces use TypeScript.

---

## Language Strategy

| Area | Language | Rationale |
|---|---|---|
| Core identity/object/statement model | Rust | Correctness, portability, canonical hashing/signing, long-term durability |
| CLI | Rust | Single static-ish binary, filesystem/process control, direct reuse of core crates |
| Daemon | Rust | Long-running service, async networking, runtime orchestration, local store management |
| Federation subsystem | Rust | Signed statement exchange, peer discovery, DHT/token routing, trust-aware query handling |
| Dependency resolver / environment planner | Rust | Deterministic, trusted, reusable across daemon and CLI |
| Provider adapters | Rust | Process control, sandbox/runtime integration, strong typing |
| Web portal | TypeScript + React | Interactive browsing, editing, logs, search, file streaming |
| Web SDK | TypeScript | Browser/client integration with daemon/federation APIs |
| Schemas | TOML, JSON Schema, generated Rust/TypeScript types | Human-authored manifests; machine-validated interchange |
| Test fixtures and experiments | Rust, TypeScript, shell as needed | Integration tests, protocol fixtures, demo Objects |

---

## Top-Level Directory Structure

```text
kairo/
  README.md
  PROJECT_LAYOUT.md
  Cargo.toml
  rust-toolchain.toml
  pnpm-workspace.yaml
  package.json

  crates/
    kairo-core/
    kairo-identity/
    kairo-object/
    kairo-statement/
    kairo-store/
    kairo-resolver/
    kairo-planner/
    kairo-provider/
    kairo-build/
    kairo-trust/
    kairo-federation-core/
    kairo-federation-http/
    kairo-federation-dht/
    kairo-federation-service/
    kairo-daemon/
    kairo-cli/

  web/
    portal/
    sdk/

  schemas/
    object.schema.json
    statement.schema.json
    resolution.schema.json
    environment-plan.schema.json
    build-artifact.schema.json

  providers/
    docker/
    qemu/
    dosbox/
    wasmtime/
    singularity/

  protocol/
    federation/
    daemon-api/

  examples/
    objects/
    providers/
    federation/

  tests/
    fixtures/
    integration/
    federation/
    resolver/

  docs/
    architecture/
    federation/
    trust/
    object-model/
    provider-model/
```

---

## Rust Workspace

The Rust workspace contains the trusted implementation of Kairo's core model, local runtime behavior, federation, and CLI.

```text
crates/
```

### `kairo-core/`

**Language:** Rust

Shared primitive types and canonical data structures used by every other Rust crate.

Responsibilities:

- Object IDs
- Actor IDs
- Node IDs
- Statement IDs
- Hash types
- Common error types
- Canonical serialization helpers
- Shared time/version/range types

This crate must remain small and dependency-light.

Example concepts:

```rust
ObjectId
ActorId
NodeId
StatementId
ContentHash
RevisionId
CapabilityToken
EnvironmentTriple
```

---

### `kairo-identity/`

**Language:** Rust

Cryptographic identity and signature handling.

Responsibilities:

- Actor key material
- Public key encoding
- Signature verification
- Signing local statements
- Key rotation support
- Delegation verification primitives

This crate should not decide whether an Actor is trusted. It should only answer whether a signature is valid.

---

### `kairo-object/`

**Language:** Rust

Object metadata, revision identity, and Object repository handling.

Responsibilities:

- Parsing `kairo.toml`
- Validating Object manifests
- Mapping Object revisions to Git commits or content trees
- Reading Object metadata from a checked-out repository
- Normalizing Object manifests for hashing

The human-authored Object manifest should generally be TOML:

```text
kairo.toml
```

The parsed and normalized model may be serialized as canonical JSON or CBOR for hashing/signing.

---

### `kairo-statement/`

**Language:** Rust

Signed append-only facts.

Responsibilities:

- Statement envelope format
- Statement body schemas
- Canonical statement hashing
- Statement validation
- Statement type registry

Statement examples:

- `VersionTag`
- `ProvidesCapability`
- `ResolutionSucceeded`
- `ResolutionFailed`
- `BuildSucceeded`
- `BuildFailed`
- `RunSucceeded`
- `RunFailed`
- `Delegation`
- `OwnershipClaim`
- `Revocation`
- `TrustEndorsement`
- `TokenAdvertisement`

Statements are the primary unit exchanged over the federation.

---

### `kairo-store/`

**Language:** Rust

Local storage for Objects, statements, blobs, plans, builds, indexes, and caches.

Responsibilities:

- Local content-addressed blob store
- Statement storage
- Object repository storage
- Build artifact storage
- Environment plan storage
- Token indexes
- Search/index database integration

Likely storage choices:

- Filesystem CAS for large blobs
- SQLite for local single-user nodes
- PostgreSQL support for multi-user hosted nodes

The store is not the trust authority. It stores observed data; trust policy determines what is used.

---

### `kairo-resolver/`

**Language:** Rust

Dependency and capability resolution.

Responsibilities:

- Convert declared requirements into concrete Object/revision/build selections
- Match dependencies by capability token
- Match dependencies by exact Object ID
- Handle version ranges
- Rank candidates by policy and trust signals
- Produce Resolution Records

Resolution should distinguish:

```text
declared requirement → candidate providers → selected exact revisions/builds
```

---

### `kairo-planner/`

**Language:** Rust

Environment planning and provider-chain construction.

Responsibilities:

- Determine what environment an Object requires
- Find Provider chains from required environment to native runtime
- Generate Environment Plans
- Describe mounts, commands, runtime adapters, and execution topology
- Hash plans for reproducibility and caching

Example provider chain:

```text
DOS game
  requires environment: msdos/x86
    provided by DOSBox-X Object
      requires environment: linux/x86_64
        provided by Docker Provider
          requires native runtime: docker
```

---

### `kairo-provider/`

**Language:** Rust

Provider adapter interfaces and shared provider execution logic.

Responsibilities:

- Define the Provider adapter trait
- Execute provider-specific environment plans
- Prepare mounts and input artifacts
- Capture logs, outputs, and runtime metadata
- Provide common sandbox/process helpers

Concrete provider implementations may live in `providers/` or in separate crates.

---

### `kairo-build/`

**Language:** Rust

Build execution, Build Artifact creation, and build-cache logic.

Responsibilities:

- Execute builds through Environment Plans
- Capture logs
- Capture output manifests
- Compute Build Artifact metadata
- Record exact build inputs
- Record derived runtime requirements
- Manage build cache keys

The build cache key should include at least:

```text
source tree hash
build command hash
resolved dependency hash
environment plan hash
provider/runtime configuration hash
```

---

### `kairo-trust/`

**Language:** Rust

Local trust policy and trust graph evaluation.

Responsibilities:

- Local trust policy parsing
- Trust anchors
- Delegation-chain evaluation
- Capability-scoped trust
- Threshold trust
- Revocation handling
- Trust-aware statement filtering
- Trust-aware candidate ranking

Trust is local and contextual. Federation spreads information; trust decides what a node acts on.

---

## Federation Subsystem

The federation layer should be a distinct subsystem with its own tests and possibly its own release lifecycle. It still uses Rust and depends on core Kairo types.

The daemon should use the federation subsystem through a narrow internal API rather than embedding federation logic directly.

Dependency direction:

```text
kairo-federation-* → kairo-core / kairo-statement / kairo-identity
kairo-daemon → kairo-federation-service
```

The core crates must not depend on federation.

---

### `kairo-federation-core/`

**Language:** Rust

Protocol-independent federation data types.

Responsibilities:

- Node identity
- Peer references
- Federation query types
- Query response types
- Token advertisement model
- Holder records
- Token index records
- Federation error types

Important abstractions:

```rust
FederationQuery
FederationResponse
TokenAdvertisement
TokenIndexRoot
StatementHolder
NodeAdvertisement
PeerRef
```

---

### `kairo-federation-http/`

**Language:** Rust

HTTP transport for federation queries and content retrieval.

Responsibilities:

- `/.well-known/kairo-node`
- `POST /federation/query`
- `POST /federation/announce`
- `GET /statements/{hash}`
- `GET /blobs/{hash}`
- `GET /token/{token}`
- Peer-to-peer HTTP client
- Request signing, if needed
- Rate limits and pagination

HTTP should be the first supported federation transport because it is simple, inspectable, cacheable, and deployable.

---

### `kairo-federation-dht/`

**Language:** Rust

Optional DHT or delegated-routing support.

Responsibilities:

- Token-to-node discovery
- Token-to-announcement discovery
- Provider-record publication
- Delegated routing client/server support
- Integration with libp2p-like routing if adopted

The DHT should not answer high-level semantic queries. It should route from tokens to likely statement/index holders.

Example routing token:

```text
provider:environment:msdos/x86
```

The route should lead to signed advertisements, not trusted answers.

---

### `kairo-federation-service/`

**Language:** Rust

The local federation service used by the daemon.

Responsibilities:

- Manage peer list
- Maintain token indexes
- Announce local statements
- Sync remote token indexes
- Query peers
- Merge partial observed ledgers
- Track holders and advertisers
- Expose an internal API to `kairo-daemon`

Example internal interface:

```rust
trait FederationClient {
    async fn announce_statement(&self, statement: StatementId) -> Result<()>;
    async fn find_statements(&self, query: StatementQuery) -> Result<Vec<StatementEnvelope>>;
    async fn find_providers(&self, token: CapabilityToken) -> Result<Vec<ProviderCandidate>>;
    async fn find_holders(&self, hash: ContentHash) -> Result<Vec<NodeRef>>;
    async fn sync_token_index(&self, token: CapabilityToken) -> Result<TokenSyncReport>;
}
```

---

## Daemon and CLI

### `kairo-daemon/`

**Language:** Rust

Long-running local or hosted service that coordinates Object storage, builds, runs, federation, and the web/API surface.

Responsibilities:

- Local Object store management
- Actor authentication/signing service
- Statement creation
- Build/run orchestration
- Environment planning
- Provider execution
- Federation service integration
- Log streaming
- File tree browsing
- Web/API server
- Search index maintenance

The daemon owns the local node state.

Typical local state:

```text
~/.kairo/
  actors/
  objects/
  statements/
  blobs/
  builds/
  plans/
  indexes/
  workspaces/
  trust/
  federation/
```

---

### `kairo-cli/`

**Language:** Rust

Command-line interface.

Responsibilities:

- Authenticate as an Actor
- Talk to the local daemon
- Provide direct/offline operations when possible
- Fetch Objects
- Search federation/local store
- Build and run Objects
- Manage local workspaces
- Create signed statements

Example commands:

```sh
kairo search gcc
kairo fetch object:z6MkObject...
kairo checkout gcc@4.1.2
kairo build ./object
kairo run ./object
kairo providers provider:environment:msdos/x86
kairo resolve ./object
kairo tag 4.1.2
kairo trust actor actor:z6MkActor...
```

---

## Web Projects

```text
web/
```

### `web/portal/`

**Language:** TypeScript + React

Public and/or authenticated web portal for browsing and interacting with a Kairo node.

Responsibilities:

- Federated search UI
- Object browsing
- Revision browsing
- File tree navigation
- File content streaming
- Object editing through draft workspaces
- Build/run dashboards
- Console/log streaming
- Provider graph visualization
- Trust and provenance inspection
- Federation index inspection

The portal should not implement trusted Kairo semantics itself. It should call the daemon API and use generated TypeScript types.

---

### `web/sdk/`

**Language:** TypeScript

Typed client SDK for web apps and external integrations.

Responsibilities:

- Daemon API client
- Federation query client, if exposed directly
- Generated types from API schemas
- Browser-safe helpers for Object browsing and log streaming

This SDK should be generated or checked against the Rust API schema to avoid drift.

---

## Schemas

```text
schemas/
```

**Languages / Formats:** JSON Schema, TOML examples, generated Rust and TypeScript bindings

Schemas define stable interchange formats and validation rules.

Important schemas:

```text
object.schema.json
statement.schema.json
resolution.schema.json
environment-plan.schema.json
build-artifact.schema.json
provider.schema.json
trust-policy.schema.json
federation-query.schema.json
```

Recommended file roles:

```text
kairo.toml
  Human-authored Object manifest.

kairo.lock
  Machine-generated resolved dependency lockfile.

build-artifact.json
  Machine-generated Build Artifact metadata.

environment-plan.json
  Machine-generated execution plan.

statement.json / statement.cbor
  Canonical signed statement envelope.
```

Do not sign raw TOML text. Parse it into a typed model, normalize it, and sign/hash the canonical representation.

---

## Provider Implementations

```text
providers/
```

Provider implementations map Kairo Environment Plans to concrete runtime systems.

Each provider may be a Rust crate, a packaged Object, or both.

### `providers/docker/`

**Primary language:** Rust

Provides Linux container execution through Docker-compatible runtimes.

May provide:

```text
native:docker → environment:linux/x86_64
```

---

### `providers/qemu/`

**Primary language:** Rust

Provides machine emulation via QEMU.

May provide:

```text
native:linux/x86_64 → environment:machine:x86
native:linux/x86_64 → environment:machine:arm
```

---

### `providers/dosbox/`

**Primary language:** Rust

Provides DOS-like runtime environments using DOSBox, DOSBox-X, or similar emulators.

May provide:

```text
environment:linux/x86_64 → environment:msdos/x86
```

---

### `providers/wasmtime/`

**Primary language:** Rust

Provides WebAssembly execution environments.

May provide:

```text
native:linux/x86_64 → environment:wasm32-wasi
```

---

### `providers/singularity/`

**Primary language:** Rust

Provides Singularity/Apptainer integration for scientific and HPC preservation use cases.

May provide:

```text
native:singularity → environment:linux/x86_64
```

---

## Protocol Documentation

```text
protocol/
```

Protocol definitions, examples, fixtures, and compatibility notes.

### `protocol/federation/`

Documents:

- Node discovery
- `.well-known/kairo-node`
- Federation query types
- Token advertisements
- Token indexes
- Statement holder discovery
- Blob transfer
- Pagination
- Rate limiting
- Trust considerations

Example query types:

```text
FindStatements
FindStatementHolders
FindAdvertisers
FindProviders
FindSuccessfulResolutions
FindBuildArtifacts
FindVersions
FindObjectsByName
```

---

### `protocol/daemon-api/`

Documents the local daemon API consumed by the CLI, web portal, and SDK.

Topics:

- Authentication
- Object browsing
- Workspace editing
- Build/run requests
- Log streaming
- File streaming
- Search
- Federation operations
- Trust policy management

---

## Examples

```text
examples/
```

Example Objects, Providers, federation networks, and archival scenarios.

Suggested examples:

```text
examples/objects/hello-c/
examples/objects/gnu-make/
examples/objects/dos-game/
examples/providers/dosbox-provider/
examples/providers/docker-linux-provider/
examples/federation/two-node-network/
examples/federation/provider-discovery/
examples/federation/successful-resolution-reuse/
```

Examples should be usable in integration tests.

---

## Tests

```text
tests/
```

Top-level black-box and cross-crate integration tests.

Suggested organization:

```text
tests/fixtures/
  objects/
  statements/
  plans/
  builds/
  federation/

tests/integration/
  cli/
  daemon/
  build-run/

tests/federation/
  token-announcement/
  token-index-sync/
  holder-discovery/
  peer-query/
  partial-ledger-merge/

tests/resolver/
  capability-resolution/
  exact-object-resolution/
  version-range-resolution/
  trust-aware-ranking/
```

Federation should be testable independently from the daemon. The daemon should be tested against a mock or in-process federation service.

---

## Documentation

```text
docs/
```

Suggested documentation areas:

```text
docs/architecture/
  overview.md
  data-model.md
  lifecycle.md

docs/object-model/
  object-manifest.md
  revisions.md
  version-tags.md
  build-artifacts.md

docs/provider-model/
  capabilities.md
  environments.md
  provider-chains.md

docs/federation/
  overview.md
  token-routing.md
  announcements.md
  dht.md
  gossip.md
  query-protocol.md

docs/trust/
  trust-model.md
  actor-identity.md
  delegation.md
  revocation.md
  local-policy.md
```

---

## Dependency Direction Rules

The dependency graph should remain acyclic and intentional.

Preferred direction:

```text
identity/object/statement primitives
  ↓
store / resolver / planner / trust
  ↓
federation / build / provider
  ↓
daemon
  ↓
CLI and web clients
```

Rules:

1. `kairo-core` should not depend on high-level crates.
2. `kairo-identity` should verify signatures but not decide trust.
3. `kairo-trust` may depend on statements, identity, and store indexes.
4. `kairo-federation-*` may depend on core, identity, statements, and store abstractions.
5. `kairo-daemon` composes everything.
6. `web/portal` and `web/sdk` should not reimplement resolution, planning, signing, or trust evaluation.

---

## Recommended Initial Implementation Order

1. `kairo-core`
2. `kairo-identity`
3. `kairo-statement`
4. `kairo-object`
5. `kairo-store`
6. `kairo` minimal CLI
7. `kairo-resolver`
8. `kairo-planner`
9. `kairo-provider` with a simple local/process provider
10. `kairo-build`
11. `kairo-daemon`
12. `kairo-federation-core`
13. `kairo-federation-http`
14. Federation token index and announcement tests
15. `web/sdk`
16. `web/portal`
17. Optional DHT/gossip federation layers

---

## Summary

Kairo should be organized as a Rust-centered archival core with TypeScript user interfaces.

The federation layer should be an independently testable Rust subsystem that exchanges signed statements, token advertisements, holder records, and content-addressed blobs. The daemon should compose federation with local storage, resolution, planning, building, running, and trust policy.

The web portal and CLI are interfaces to the same underlying model, not separate implementations of Kairo semantics.
