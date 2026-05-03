# SANDBOX.md

## Status

Draft specification.

This document defines the **sandbox model** used by the executor surface for
runtime permissions, execution constraints, and policy decisions on a single
running artifact. *Sandbox capabilities* describe what an execution is allowed
to do (filesystem, network, GPU, display, …).

These are unrelated to **actor capabilities** (the distributed-systems sense:
delegated authority for one actor to issue statements on behalf of another).
Actor capabilities are specified in `CAPABILITIES.md` and seeded today by
`ACTORS.md` §10–12. When this document says "capability" without qualification,
it means *sandbox capability* — the runtime permission granted to an executing
artifact.

---

## 1. Purpose

Capabilities are used to:

1. Declare what a plan requires.
2. Allow the daemon to evaluate policy.
3. Allow executors to enforce restrictions.
4. Inform users before execution.
5. Provide auditability and reproducibility context.

Capabilities are **not** used to determine semantic validity.

---

## 2. Core Principle

```text
Plan requests capabilities
Policy approves or denies
Executor enforces
```

---

## 3. Capability Model

A capability is a structured, explicit permission.

```rust
pub struct Capability {
    pub kind: CapabilityKind,
    pub mode: CapabilityMode,
    pub scope: Option<CapabilityScope>,
}
```

---

## 4. Capability Kinds

### 4.1 IO Capabilities

```text
filesystem_read
filesystem_write
```

### 4.2 Network

```text
network
```

Modes:

```text
none
outbound
inbound
full
```

---

### 4.3 User Interface

```text
display
audio
keyboard_input
pointer_input
gamepad_input
```

---

### 4.4 System Access

```text
gpu
host_process
host_device
clipboard_read
clipboard_write
```

---

### 4.5 Sensors / External

```text
camera
microphone
```

---

### 4.6 Execution Extensions

```text
emulator_control
vm_control
container_control
```

---

## 5. Capability Mode

Defines intensity of access.

```text
none
read
write
execute
full
```

Not all kinds use all modes.

---

## 6. Capability Scope

Optional restriction.

Examples:

```json
{
  "kind": "filesystem_read",
  "scope": {
    "path": "/sandbox/input"
  }
}
```

```json
{
  "kind": "network",
  "scope": {
    "hosts": ["example.com"]
  }
}
```

---

## 7. Capability Requests in Plans

Plans must declare required capabilities.

Example:

```json
{
  "capabilities": [
    { "kind": "display", "mode": "full" },
    { "kind": "audio", "mode": "full" },
    { "kind": "network", "mode": "outbound" }
  ]
}
```

---

## 8. Policy Evaluation

Policy evaluates:

```text
requested capability vs allowed capability
```

Outcomes:

```text
allow
deny
require approval
```

---

## 9. Executor Enforcement

Executors must:

1. Enforce granted capabilities.
2. Deny non-approved capabilities.
3. Fail if enforcement is impossible.

Example:

```text
network denied → disable network stack
filesystem_write denied → mount read-only
```

---

## 10. Capability Escalation

If runtime requests additional capability:

```text
pause or fail
→ ask daemon policy
→ resume only if approved
```

---

## 11. Default Policy

Safe defaults:

```text
deny network
deny filesystem write
allow display/audio
deny host access
```

---

## 12. Capability Groups (Optional)

Capabilities may be grouped:

```text
ui_basic = [display, audio, input]
sandbox_safe = [no network, read-only fs]
```

Used for convenience only.

---

## 13. Serialization

Capabilities must be JSON-serializable.

Example:

```json
{
  "kind": "filesystem_write",
  "mode": "write",
  "scope": {
    "path": "/tmp/output"
  }
}
```

---

## 14. Reproducibility

Execution records must capture:

```text
requested capabilities
granted capabilities
denied capabilities
```

Differences affect reproducibility.

---

## 15. Security Requirements

Capabilities must:

1. Be explicit (no implicit privileges)
2. Default to deny
3. Be enforced or rejected
4. Be visible to user before execution
5. Prevent privilege escalation

---

## 16. API Mapping

From `API.md`:

```json
{
  "requested_capabilities": [...],
  "granted_capabilities": [...]
}
```

---

## 17. CLI Mapping

```text
kairo run ... --cap network --deny-cap filesystem_write
```

---

## 18. Web Client Mapping

UI must:

- display requested capabilities
- show risk levels
- require approval for sensitive capabilities

---

## 19. Implementation Checklist

1. Capability enum/types
2. Plan integration
3. Policy evaluation
4. Executor enforcement
5. Serialization
6. UI/CLI exposure
7. Audit logging

---

End of SANDBOX.md
