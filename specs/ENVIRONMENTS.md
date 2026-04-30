# ENVIRONMENTS.md

## Status

Draft specification.

This document defines environment descriptors in Kairo: the standardized way to
describe the execution and build environments used by planners and executors.

---

## 1. Purpose

Environments answer:

> What kind of runtime/build context is required to execute this plan?

They are:

- declared by planners
- interpreted by executors
- constrained by policy
- recorded for reproducibility

---

## 2. Core Principle

```
Environment = abstract requirement
Executor = concrete implementation
```

A plan declares an environment. The executor chooses how to realize it.

---

## 3. Environment Descriptor

```ts
type EnvironmentDescriptor = {
  kind: string
  version?: string
  config?: Record<string, any>
}
```

Examples:

```json
{ "kind": "container", "version": "node:18" }
{ "kind": "dosbox", "config": { "cpu": "386", "memory": "16mb" } }
{ "kind": "browser", "config": { "mode": "static-web" } }
```

---

## 4. Standard Environment Kinds

### 4.1 Container

```
kind: "container"
```

Config:

- image
- command
- env vars

---

### 4.2 Virtual Machine

```
kind: "vm"
```

Config:

- disk image
- memory
- cpu type

---

### 4.3 Emulator

```
kind: "emulator"
```

Examples:

- dosbox
- qemu-system-x86

---

### 4.4 Browser

```
kind: "browser"
```

Used for:

- web apps
- data viewers

---

### 4.5 Native

```
kind: "native"
```

Runs directly on host (unsafe by default).

---

### 4.6 Language Runtime

```
kind: "node"
kind: "python"
kind: "java"
```

---

## 5. Environment Matching

Executors must match:

- kind
- version (if specified)
- required capabilities

If unsupported:

→ reject plan

---

## 6. Environment Identity

Executors should record:

- resolved environment
- version/image hash
- runtime configuration

---

## 7. Reproducibility

For reproducibility:

- prefer immutable environments
- record exact versions
- avoid floating tags (latest)

---

## 8. Policy Interaction

Policy may:

- allow/deny environments
- require approval
- restrict configs

---

## 9. API Representation

```json
{
  "environment": {
    "kind": "container",
    "version": "node:18"
  }
}
```

---

## 10. Implementation Checklist

1. Environment descriptor type
2. Standard kinds
3. Matching logic
4. Executor integration
5. API exposure
6. Recording in execution logs

---

End of ENVIRONMENTS.md
