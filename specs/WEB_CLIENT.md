# WEB_CLIENT.md

## Status

Draft specification.

This document defines the Kairo web client implementation. The web client is a
TypeScript/React application that communicates with the Kairo daemon API. It is
responsible for presenting Kairo objects, snapshots, validation results, tasks,
builds, runs, federation/search results, and runtime interaction surfaces.

This specification is intentionally prescriptive enough to guide implementation.

---

## 1. Purpose

The Kairo web client provides a browser-based interface for interacting with a
local or remote Kairo daemon.

The web client is responsible for:

1. Browsing local and federated objects.
2. Inspecting objects and snapshots.
3. Displaying core validation status and validation issues.
4. Displaying effective object state returned by the daemon/core.
5. Requesting fetch, sync, build, run, reproduce, import, export, pin, and unpin operations.
6. Displaying daemon task progress and logs.
7. Displaying build and runtime plans.
8. Displaying runtime capability requests before execution.
9. Rendering artifact viewers where safe and supported.
10. Managing client-side navigation, caching, and UI state.

The web client is not responsible for:

1. Defining object semantics.
2. Defining statement interpretation.
3. Defining authority rules.
4. Determining snapshot validity.
5. Performing build planning.
6. Performing run planning.
7. Executing object content outside daemon-approved runtime surfaces.
8. Implementing federation protocols.
9. Replacing the daemon API.

---

## 2. Dependency Relationship

The web client depends on:

- `DAEMON.md`
- `CORE_LIBRARY.md`
- `STORE.md`
- `FEDERATION.md`
- `CLI.md` where behavior should match user-facing terminology
- `OBJECT.md`
- `STATEMENTS.md`
- `BUILD.md`
- `PLANNER.md`

The web client must treat daemon API responses as the source of truth for:

- Snapshot validation status
- Effective object state
- Build plans
- Run plans
- Policy decisions
- Task status
- Federation status

The web client may validate API response shape, but it must not independently
validate Kairo object semantics.

---

## 3. Technology Stack

The web client implementation must use:

```text
pnpm
Turborepo
Vite
React
TypeScript
TanStack Router
TanStack Query
```

Recommended supporting libraries:

```text
OpenAPI-generated TypeScript types
Zod
openapi-typescript
openapi-fetch or a thin custom fetch wrapper
React Hook Form where forms become complex
Vitest
Playwright
Storybook
ESLint
Prettier
```

The exact visual component library is not mandated. If a component library is used,
it must not obscure validation, policy, or safety states.

---

## 4. Monorepo Layout

The frontend should live in a pnpm/Turborepo workspace.

Recommended layout:

```text
frontend/
  package.json
  pnpm-workspace.yaml
  turbo.json
  apps/
    web-client/
      package.json
      index.html
      src/
  packages/
    api-client/
      package.json
      src/
    ui/
      package.json
      src/
    object-model/
      package.json
      src/
    validation-viewer/
      package.json
      src/
    artifact-viewers/
      package.json
      src/
```

### 4.1 `apps/web-client`

The React application.

Responsibilities:

- Routing
- Application shell
- Page composition
- Authentication/session integration if needed
- Daemon connection setup
- Query client setup
- Feature integration

### 4.2 `packages/api-client`

Typed daemon API client.

Responsibilities:

- Generated OpenAPI types
- Fetch wrapper
- Zod runtime validation for important responses
- Error normalization
- TanStack Query helpers
- Task streaming helpers

### 4.3 `packages/ui`

Reusable UI components.

Examples:

- Buttons
- Panels
- Dialogs
- Status badges
- Tables
- Tabs
- Forms
- Empty states
- Error displays

### 4.4 `packages/object-model`

TypeScript API DTO types and helper functions.

This package must not implement independent semantic validation. It may include:

- Generated DTO types
- Formatting helpers
- ID parsing/formatting helpers
- Display enums
- Type guards for API payload shape

### 4.5 `packages/validation-viewer`

Components for visualizing validation results.

Examples:

- Validation issue list
- Statement graph view
- Authority chain view
- Conflict viewer
- Snapshot closure viewer

### 4.6 `packages/artifact-viewers`

Safe artifact viewer components.

Examples:

- Text viewer
- Image viewer
- Audio viewer
- Video viewer
- JSON viewer
- Markdown viewer
- Dataset/table preview
- Emulator/runtime launch surface

Artifact viewers must respect daemon policy and browser security constraints.

---

## 5. API Contract Strategy

The daemon API contract must be described with OpenAPI.

The web client must consume daemon API types generated from the OpenAPI schema.

Recommended flow:

```text
Rust daemon
  -> serves or emits OpenAPI schema
  -> frontend generates TypeScript DTO types
  -> api-client wraps generated calls
  -> Zod validates important runtime responses
  -> TanStack Query exposes typed query/mutation hooks
```

### 5.1 OpenAPI as contract

OpenAPI is the cross-language API contract between Rust daemon and TypeScript web client.

The web client must not use hand-written TypeScript types as the primary API contract
when generated OpenAPI types are available.

### 5.2 Zod runtime validation

Zod may be used to validate:

- API response shape
- API error envelopes
- User-entered form data
- Client-side settings
- Cached data loaded from local browser storage

Zod must not be used to reimplement Kairo semantic validation.

### 5.3 Type generation

Recommended tools:

```text
openapi-typescript
openapi-fetch
```

Generated files should be clearly marked and should not be edited manually.

Recommended package scripts:

```json
{
  "scripts": {
    "generate:api": "openapi-typescript ../../openapi/kairo-daemon.json -o src/generated/schema.ts",
    "typecheck": "tsc --noEmit",
    "test": "vitest run"
  }
}
```

---

## 6. Daemon API Client

The API client should expose high-level functions, not raw endpoint calls only.

Example shape:

```ts
export interface KairoApiClient {
  inspectSnapshot(input: InspectSnapshotInput): Promise<InspectSnapshotResult>;
  verifySnapshot(input: VerifySnapshotInput): Promise<VerifySnapshotResult>;
  fetchObject(input: FetchObjectInput): Promise<TaskOrResult>;
  buildSnapshot(input: BuildSnapshotInput): Promise<TaskRef>;
  runSnapshot(input: RunSnapshotInput): Promise<TaskRef>;
  getTask(id: TaskId): Promise<Task>;
  streamTask(id: TaskId): AsyncIterable<TaskEvent>;
}
```

The API client must normalize errors into a stable client-side shape:

```ts
export type ApiClientError =
  | { kind: "network"; message: string }
  | { kind: "daemon"; code: string; message: string; details?: unknown }
  | { kind: "validation"; status: ValidationStatus; issues: ValidationIssue[] }
  | { kind: "unauthorized"; message: string }
  | { kind: "decode"; message: string; details?: unknown };
```

---

## 7. TanStack Query Integration

The web client must use TanStack Query for daemon-backed server state.

### 7.1 Query keys

Query keys must be centralized.

Recommended structure:

```ts
export const kairoKeys = {
  daemon: ["daemon"] as const,
  daemonStatus: () => [...kairoKeys.daemon, "status"] as const,

  objects: ["objects"] as const,
  object: (id: string) => [...kairoKeys.objects, id] as const,

  snapshots: ["snapshots"] as const,
  snapshot: (id: string) => [...kairoKeys.snapshots, id] as const,
  snapshotValidation: (id: string, purpose: string) =>
    [...kairoKeys.snapshot(id), "validation", purpose] as const,

  tasks: ["tasks"] as const,
  task: (id: string) => [...kairoKeys.tasks, id] as const,
};
```

### 7.2 Queries

Queries should be thin wrappers around the API client.

Examples:

```ts
export function useDaemonStatus() {
  return useQuery({
    queryKey: kairoKeys.daemonStatus(),
    queryFn: () => apiClient.getDaemonStatus(),
  });
}
```

### 7.3 Mutations

Operations that create daemon tasks should use mutations.

Examples:

- Fetch object
- Sync object
- Build snapshot
- Run snapshot
- Reproduce snapshot
- Pin/unpin
- Policy approval/denial

Mutations should invalidate or update relevant query caches after success.

### 7.4 Streaming tasks

For long-running tasks, the web client should support:

- polling fallback
- server-sent events
- WebSocket
- readable stream
- daemon-provided event stream

The transport is implementation-defined, but task UI must reflect task state accurately.

---

## 8. Routing

The web client must use TanStack Router.

Recommended route structure:

```text
/
  dashboard
/objects
  object list/search
/objects/$objectId
  object detail
/objects/$objectId/snapshots/$snapshotId
  snapshot detail
/objects/$objectId/snapshots/$snapshotId/validation
  validation detail
/objects/$objectId/snapshots/$snapshotId/build
  build plan/build action
/objects/$objectId/snapshots/$snapshotId/run
  run plan/run action
/tasks
  task list
/tasks/$taskId
  task detail/logs
/federation
  federation status/search
/store
  store status
/policy
  policy settings/approvals
/settings
  client and daemon settings
```

Routes must not rely on global mutable state for semantic data. Required server
state should be loaded through TanStack Query and daemon APIs.

---

## 9. Required UI Concepts

The web client must provide UI representations for:

1. Object
2. Snapshot
3. Snapshot purpose
4. Validation status
5. Validation issue
6. Conflict
7. Statement
8. Statement graph
9. Actor
10. Authority/capability path
11. Artifact
12. Blob
13. Build plan
14. Run plan
15. Runtime capability request
16. Task
17. Policy decision
18. Federation source/provenance
19. Store/locality state

---

## 10. Validation Status Display

The web client must display core validation status exactly.

Statuses:

```text
valid
invalid
conflicted
indeterminate
unverified
```

The UI must distinguish:

- Core validation status
- Policy decision
- Task status
- API/network error
- Unverified search/preview data

Example labels:

```text
Validation: Valid
Policy: Requires approval
Task: Running
Source: Federation preview, unverified
```

The web client must not present unverified, indeterminate, or conflicted data as valid.

---

## 11. Policy and Safety UI

Before requesting execution, the web client must display:

1. Snapshot ID.
2. Validation status.
3. Purpose.
4. Runtime/build plan summary.
5. Requested runtime capabilities.
6. Policy decision.
7. User approval requirement, if any.

The web client must require explicit user action before:

- Running a snapshot.
- Building when policy requires approval.
- Granting network access.
- Granting filesystem write access.
- Publishing local/private data to federation.
- Deleting or garbage-collecting user-visible data.

The web client must not bypass daemon policy.

---

## 12. Artifact Viewing

Artifact viewers must be treated as potentially unsafe surfaces.

### 12.1 Safe inline viewers

The web client may render safe previews for:

- Plain text
- Images
- Audio
- Video
- JSON
- CSV/table data
- Markdown, if sanitized
- Static metadata

### 12.2 Unsafe or active content

For active content such as:

- HTML applications
- JavaScript
- Emulators
- VM displays
- Interactive notebooks
- Web-based objects
- Native binaries

The web client must request a daemon-approved run plan or sandboxed viewer session.

The web client must not directly execute arbitrary artifact code simply because an artifact is viewable.

### 12.3 Viewer registry

Artifact viewers should be registered through a typed registry:

```ts
export interface ArtifactViewer {
  canView(artifact: ArtifactRecord): boolean;
  Viewer: React.ComponentType<ArtifactViewerProps>;
  safety: "inline-safe" | "sandbox-required" | "daemon-runtime-required";
}
```

---

## 13. Runtime Session UI

For daemon-managed runtime sessions, the web client may provide:

- Embedded display frame
- Console/log output
- Input controls
- Audio controls
- Filesystem mount browser, if allowed
- Stop/restart controls
- Capability status display

Runtime sessions must be tied to daemon task/session IDs.

The web client must not assume a runtime is safe merely because it is visible in the browser.

---

## 14. Federation and Search UI

Federation search results must be treated as unverified previews until core validation is performed.

Search result cards must clearly show:

- Source/provenance when available
- Whether data is local or remote
- Whether validation has been performed
- Whether the object/snapshot is pinned locally
- Available actions: fetch, inspect, verify, pin

The web client must not imply that a search result is valid unless daemon/core validation says so.

---

## 15. Store and Locality UI

The web client should show whether data is:

- Local
- Remote
- Cached
- Pinned
- Partial
- Missing
- Fetching
- Unavailable

Locality must be distinct from validity.

An object can be local and invalid. An object can be remote and valid after validation if closure data is sufficient.

---

## 16. Task UI

The web client must represent daemon tasks distinctly from validation.

Task statuses:

```text
queued
running
succeeded
failed
cancelled
interrupted
```

Task pages should display:

- Task kind
- Status
- Progress
- Logs
- Related object/snapshot
- Result
- Error details
- Cancellation controls where supported

Long-running actions should navigate or link to task detail pages.

---

## 17. Error Handling

The web client must distinguish:

1. API/network errors.
2. Daemon operational errors.
3. Core validation statuses.
4. Policy denials.
5. Runtime failures.
6. Decode/schema validation failures.
7. User cancellation.

Errors should be actionable.

Example:

```text
Validation is indeterminate because authority data is missing.
Action: Fetch missing closure data.
```

The web client must not collapse all errors into generic failure banners.

---

## 18. Client State

The web client may store local UI preferences, such as:

- Theme
- Sidebar layout
- Recently viewed objects
- Preferred daemon URL
- Table column preferences
- Viewer preferences

Client state must not be treated as semantic Kairo state.

Semantic state belongs to daemon/core/store.

---

## 19. Authentication and Authorization

If the daemon API requires authentication, the web client must support it.

Potential modes:

- Local trusted origin
- Token-based auth
- OAuth/device flow
- Local session cookie
- OS-integrated auth

The exact mechanism is defined by the daemon/API spec.

The web client must not store long-lived secrets insecurely in browser storage unless the daemon/API security model permits it.

---

## 20. Accessibility

The web client should meet WCAG 2.1 AA where practical.

Requirements:

1. Keyboard navigation for primary workflows.
2. Accessible status badges with text labels.
3. Color not required to understand state.
4. Screen-reader-friendly validation and policy messages.
5. Focus management for dialogs.
6. Reduced-motion support.

---

## 21. Performance

The web client should handle large objects and statement graphs.

Requirements:

1. Paginate or virtualize large lists.
2. Avoid rendering huge graphs all at once.
3. Use lazy loading for heavy artifact viewers.
4. Cache daemon responses through TanStack Query.
5. Avoid storing large blobs directly in React state.
6. Prefer streaming or range requests for large artifacts.

---

## 22. Testing

The web client should include:

1. Unit tests with Vitest.
2. Component tests where useful.
3. Storybook stories for shared components.
4. Accessibility checks for major views.
5. Playwright end-to-end tests for critical workflows.
6. API-client contract tests against mock OpenAPI responses.

Critical workflows:

- Inspect object.
- Verify snapshot.
- Display invalid validation.
- Display conflicted validation.
- Fetch object.
- Build plan-only.
- Run plan-only.
- Start task and follow progress.
- Display policy approval prompt.
- Display federation search result as unverified.

---

## 23. Build and Deployment

The web client must be buildable as a static Vite application.

Recommended commands:

```text
pnpm install
pnpm generate:api
pnpm typecheck
pnpm test
pnpm build
```

The daemon may serve the built web client, or it may be deployed separately.

If served by the daemon, the web client must discover or be configured with the daemon API base URL.

---

## 24. Package Scripts

Recommended root scripts:

```json
{
  "scripts": {
    "build": "turbo build",
    "dev": "turbo dev",
    "test": "turbo test",
    "typecheck": "turbo typecheck",
    "lint": "turbo lint",
    "generate:api": "turbo generate:api"
  }
}
```

Recommended app scripts:

```json
{
  "scripts": {
    "dev": "vite",
    "build": "tsc --noEmit && vite build",
    "preview": "vite preview",
    "test": "vitest run",
    "typecheck": "tsc --noEmit"
  }
}
```

---

## 25. Security Requirements

The web client must:

1. Treat daemon/federation preview data as untrusted until validated.
2. Not execute arbitrary artifact content inline.
3. Sanitize rendered Markdown/HTML.
4. Require daemon-approved runtime sessions for active content.
5. Display runtime capabilities before execution.
6. Respect daemon policy decisions.
7. Avoid leaking local data to remote federation endpoints.
8. Avoid storing sensitive secrets insecurely.
9. Avoid relying on color alone for safety-critical state.
10. Avoid silently retrying dangerous mutations.

---

## 26. Implementation Checklist

A conforming initial implementation should provide:

1. pnpm workspace.
2. Turborepo configuration.
3. Vite React app.
4. TypeScript strict mode.
5. TanStack Router setup.
6. TanStack Query setup.
7. OpenAPI type generation.
8. API client package.
9. Zod validation for API envelopes/errors.
10. Application shell.
11. Daemon status page.
12. Object list/search page.
13. Object detail page.
14. Snapshot detail page.
15. Validation viewer.
16. Task list/detail pages.
17. Build plan-only flow.
18. Run plan-only flow.
19. Policy approval UI.
20. Federation search UI with unverified labels.
21. Store/locality indicators.
22. Safe artifact viewer registry.
23. Error boundary and structured error displays.
24. Unit/component tests.
25. Basic Playwright workflow tests.

---

End of `WEB_CLIENT.md`.
