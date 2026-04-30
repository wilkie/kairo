# STATEMENTS.md

## 1. Overview

Statements are signed, content-addressed claims about objects, artifacts, and their relationships.

Statements:

- are immutable
- are independently verifiable
- are not part of object versioned content
- form the basis of trust and federation

A statement represents:

"Actor X claims Y about Z."

---

## 2. Statement Identity

Each statement has:

- statement hash (content-addressed identity)
- signature (binding actor to the statement)

### 2.1 Statement Hash

The statement hash is computed from a canonical representation of the statement content.

Rules:

- canonical serialization (deterministic)
- excludes the signature itself
- identical content → identical hash

---

## 3. Signature Model

A statement includes:

{
  "id": "z6MkStatement...",
  "signature": {
    "actor": "z6MkActor...",
    "sig": "..."
  }
}

Signature semantics:

- signature = Sign(statement_hash)
- proves authorship
- does not imply correctness

---

## 4. Statement Types

### 4.1 Build Statement

Records a successful build:

{
  "type": "kairo.statement.build.v1",
  "subject": {
    "object": "z6MkObject...",
    "snapshot": "z6MkSnapshot..."
  },
  "target": "release",
  "resolvedManifest": {
    "hash": "z6MkResolvedManifest..."
  },
  "artifact": {
    "snapshot": "z6MkArtifactSnapshot..."
  },
  "result": "success"
}

---

### 4.2 Provides Statement

Declares a capability:

{
  "type": "kairo.statement.provides.v1",
  "subject": {
    "object": "z6MkObject...",
    "target": "release",
    "output": "static-lib"
  },
  "provides": "lib:zlib:static",
  "version": "1.3.1"
}

---

### 4.3 Observation Statement

Records observed behavior:

{
  "type": "kairo.statement.observation.v1",
  "observedStatement": "z6MkStatement...",
  "notes": [
    {
      "kind": "resolution-evidence",
      "requested": "tool:make",
      "resolved": "z6MkMakeObject...",
      "result": "success"
    }
  ]
}

---

### 4.4 Inference Statement

Records inferred properties:

{
  "type": "kairo.statement.inference.v1",
  "subject": {
    "object": "z6MkObject...",
    "snapshot": "z6MkSnapshot..."
  },
  "path": "file.exe",
  "inferred": ["data:exe:mz"],
  "method": "magic-bytes"
}

---

## 5. Canonicalization

Statements MUST be canonicalized before hashing.

Requirements:

- deterministic field order
- stable binary primitive encodings
- length-prefixed strings and bytes
- explicit option/list encodings
- no dependence on JSON object key ordering

Canonical forms are documented under:

```text
schemas/canonical/
```

The canonical schema for `ObjectGenesis` v1 is:

```text
schemas/canonical/object-genesis-v1.md
```

JSON interchange schemas belong under:

```text
schemas/json/
```

JSON interchange schemas describe external representation. They are not the
canonical hash input unless a canonical schema explicitly says so.

External clients that verify IDs or signatures must implement the relevant
canonical schema exactly.

---

## 6. Trust Model

Statements are not inherently trusted.

Trust is derived from:

- actor identity
- signature validity
- observed outcomes
- reproducibility
- social consensus

---

## 7. Statement Graph

Statements form a graph:

objects → statements → observations → trust

This enables:

- capability inference
- dependency resolution heuristics
- reproducibility verification

---

## 8. Federation

Statements are:

- shareable across nodes
- independently verifiable
- append-only

Nodes may:

- accept
- reject
- ignore
- prioritize

based on local trust policy.

---

## 9. Design Principles

- statements are claims, not truth
- signatures bind actors to claims
- trust is derived, not assigned
- statements are content-addressed
- federation is decentralized
