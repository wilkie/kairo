# POLICY.md

## Status

Draft specification.

This document defines the Kairo policy system. Policy governs **local decisions**
about trust, execution, storage, and publication. It sits between core validation
and daemon action.

This specification is intentionally prescriptive enough to guide implementation.

---

## 1. Purpose

Policy answers:

> Given a semantically valid (or invalid/indeterminate) snapshot, what is this node
> allowed or willing to do?

Policy is responsible for:

1. Trust decisions (actors, objects, origins)
2. Execution permissions
3. Runtime capability approval
4. Federation publish/ingest rules
5. Store retention and pinning defaults
6. Approval workflows (user prompts)
7. Safety constraints for executors
8. Environment and host restrictions

Policy is not responsible for:

1. Semantic validation (core)
2. Statement interpretation
3. Planning builds/runs
4. Executing workloads
5. Defining object correctness

---

## 2. Relationship to Other Specs

Policy interacts with:

- `CORE_LIBRARY.md` → provides validation status
- `DAEMON.md` → enforces policy decisions
- `EXECUTOR.md` → enforces capability limits
- `API.md` → exposes policy decisions
- `CLI.md` / `WEB_CLIENT.md` → surfaces approvals

Policy never overrides core validation.

---

## 3. Core Principle

```text
Core: determines truth
Policy: determines permission
```

Examples:

```text
VALID + ALLOW  → proceed
VALID + DENY   → blocked
VALID + APPROVAL → user decision required

INVALID → always blocked (policy irrelevant)
INDETERMINATE → policy may fetch or deny
```

---

## 4. Policy Decision Model

### 4.1 Decision Enum

```rust
pub enum PolicyDecision {
    Allow,
    Deny { reason: String },
    RequireUserApproval {
        reason: String,
        approval_request_id: ApprovalRequestId
    },
}
```

Policy must produce structured, explainable decisions.

---

## 5. Policy Domains

### 5.1 Trust Policy

Controls which actors or objects are trusted.

Examples:

- Trust actor A for metadata
- Distrust actor B entirely
- Trust specific object lineage

Trust may affect:

- Whether validation is considered acceptable
- Whether execution is allowed

---

### 5.2 Execution Policy

Controls whether builds/runs are allowed.

Examples:

- Allow builds for trusted snapshots
- Deny execution for untrusted authors
- Require approval for interactive runs

---

### 5.3 Capability Policy

Controls runtime privileges.

Examples:

```text
Allow display/audio
Deny network
Require approval for filesystem write
Deny host process access
```

Capabilities must be explicit.

---

### 5.4 Federation Policy

Controls:

- What can be fetched
- What can be published

Examples:

```text
Allow fetch from federation
Do not auto-pin remote data
Deny publishing private objects
Allow publishing public metadata only
```

---

### 5.5 Store Policy

Controls local retention and garbage collection.

Examples:

```text
Auto-pin user-imported objects
Do not auto-pin fetched data
Retain last N builds
Allow GC of unpinned blobs
```

---

### 5.6 Executor Policy

Controls which executors are allowed.

Examples:

```text
Allow container executor
Allow DOSBox executor
Deny local-process executor by default
Require approval for VM executor
```

---

## 6. Approval Workflow

Some actions require explicit user approval.

### 6.1 Approval Request

```rust
pub struct ApprovalRequest {
    pub id: ApprovalRequestId,
    pub snapshot: SnapshotRef,
    pub purpose: ExecutionPurpose,
    pub requested_capabilities: Vec<RuntimeCapability>,
    pub reason: String,
}
```

### 6.2 Approval Outcomes

```rust
pub enum ApprovalOutcome {
    Approved,
    Denied,
}
```

Approval must be:

- explicit
- auditable
- optionally persisted

---

## 7. Policy Inputs

Policy decisions may consider:

- validation status
- actor identity
- object identity
- snapshot identity
- requested capabilities
- executor type
- federation provenance
- user configuration
- environment (dev vs prod)
- prior approvals

---

## 8. Policy Evaluation Flow

```text
Input:
  snapshot + purpose + plan + capabilities

Process:
  check validation status
  check trust rules
  check capability rules
  check executor rules
  check federation/store rules
  produce decision

Output:
  PolicyDecision
```

---

## 9. Policy Persistence

Policy configuration must be persisted.

Example config:

```toml
[trust]
trusted_actors = ["z6MkActor"]

[execution]
allow_build = true
require_approval_for_run = true

[capabilities]
network = "deny"
filesystem_write = "approval"

[executors]
allow = ["container", "dosbox"]
deny = ["local_process"]
```

---

## 10. Policy Precedence

Order of evaluation:

```text
1. Validation status (core)
2. Explicit denies
3. Explicit allows
4. Capability rules
5. Executor rules
6. Default policy
```

Explicit deny always wins.

---

## 11. Audit and Logging

Policy decisions should be logged:

```text
timestamp
snapshot
purpose
decision
reason
capabilities
executor
```

Audit logs must not expose sensitive data unnecessarily.

---

## 12. Security Requirements

Policy must:

1. Default to safe-deny for dangerous capabilities
2. Never allow execution of invalid snapshots
3. Require approval for risky operations
4. Prevent privilege escalation
5. Protect local/private data
6. Prevent unintended federation publication
7. Be explicit and auditable

---

## 13. Implementation Checklist

A conforming implementation should provide:

1. Policy decision engine
2. Configurable rules
3. Trust model
4. Capability rules
5. Executor rules
6. Approval system
7. API exposure
8. CLI/Web integration
9. Audit logging
10. Safe defaults

---

End of POLICY.md
