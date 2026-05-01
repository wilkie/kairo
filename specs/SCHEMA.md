# SCHEMA.md

## Status

Draft specification.

This document defines serialization, schema versioning, and compatibility rules
for all Kairo data structures (objects, statements, snapshots, API DTOs, packages).

Detailed schema artifacts live under `schemas/`:

- `schemas/canonical/` documents canonical bytes used for hashes, identities,
  and signatures.
- `schemas/json/` documents JSON interchange shapes for APIs, packages, and
  external tools.

Canonical schemas and JSON schemas are related but distinct.

---

## 1. Purpose

SCHEMA.md ensures:

1. Stable serialization across implementations
2. Forward/backward compatibility
3. Safe evolution of formats
4. Deterministic encoding where required

---

## 2. Core Principles

```text
- Schemas must be explicit
- Unknown fields must be preserved when possible
- Breaking changes require version bump
- Validation must not depend on field ordering
```

---

## 3. Schema Identification

Every serialized structure must include:

```json
{
  "schema": "kairo.<domain>.v1"
}
```

Examples:

```text
kairo.object.v1
kairo.statement.v1
kairo.snapshot.v1
kairo.api.result.v1
kairo.package.v1
```

---

## 4. Versioning Rules

### 4.1 Major versions

Breaking changes:

```text
v1 → v2
```

Requires:

- new schema identifier
- migration strategy or rejection

---

### 4.2 Minor additions (within same version)

Allowed:

- adding optional fields
- adding enum values (must be safely ignored)
- adding endpoints

Not allowed:

- removing fields
- changing meaning of fields

---

## 5. Serialization Format

Primary interchange format:

```text
JSON (UTF-8)
```

Rules:

1. Keys must be strings
2. No duplicate keys
3. Numbers must not exceed JSON safe limits unless encoded as strings
4. Binary data must be referenced (blobs), not embedded

JSON is the **interchange** format. JSON bytes are never the hash or signature
input for Kairo identities. All hashing and signing operate on the canonical
binary form documented under `schemas/canonical/`. See section 6.

---

## 6. Canonical Form vs JSON Interchange

Kairo separates two roles cleanly:

- **Canonical bytes** (under `schemas/canonical/`) are the only input to any
  Kairo hash, identity derivation, or signature. They are domain-tagged and
  produced by the deterministic length-prefixed binary encoding implemented in
  `kairo-core::canonical`.
- **JSON interchange** (under `schemas/json/`) is the wire format for APIs,
  packages, files, and external tooling. JSON is parsed into typed Rust models;
  those models then produce canonical bytes for hashing or signing.

Implementations MUST NOT hash JSON text, TOML text, or any other interchange
representation. If two implementations disagree about what bytes to hash, the
canonical document under `schemas/canonical/` is authoritative.

Canonical JSON (sorted keys, no whitespace, stable numbers) is reserved for
external interoperability use cases that explicitly require deterministic JSON.
It is not Kairo's identity or signature input.

---

## 7. Unknown Fields

Implementations must:

```text
- ignore unknown fields
- preserve unknown fields when round-tripping (where possible)
```

---

## 8. Required vs Optional Fields

Schemas must clearly define:

```text
required fields
optional fields
```

Missing required fields → invalid structure.

---

## 9. Enum Handling

Clients must:

```text
- handle unknown enum values safely
- not crash on new values
```

Recommended behavior:

```text
unknown → treat as "unsupported"
```

---

## 10. Binary Data

Binary data must NOT be embedded in JSON.

Use:

```text
blob references
```

Example:

```json
{
  "blob_id": "z6MkBlob..."
}
```

---

## 11. Time Format

JSON interchange timestamps use **RFC 3339 / ISO 8601** with these strict
rules:

1. UTC only; the offset MUST be the literal `Z` suffix.
2. Second granularity; fractional seconds MUST NOT be included.
3. Date-time form `YYYY-MM-DDTHH:MM:SSZ` with a literal `T` separator.

Example:

```text
2026-04-30T00:00:00Z
```

Parsers MUST reject offsets other than `Z`, fractional seconds, and any other
RFC 3339 variant. This restriction exists so an interchange string maps to
exactly one canonical i64 epoch-seconds value.

The canonical binary form of a timestamp is `i64` Unix epoch seconds, big-
endian. This is what enters hash and signature inputs; the JSON form is for
interchange only.

---

## 12. Determinism Requirements

Deterministic encoding required for:

- package export (optional strict mode)
- hashing inputs
- snapshot identity (if derived from JSON)

---

## 13. Validation

Schema validation must include:

1. required fields present
2. field types correct
3. identifiers valid
4. no structural corruption

Schema validation ≠ semantic validation.

---

## 14. Migration

Implementations should support:

- version detection
- migration where possible
- safe rejection when unsupported

---

## 15. API Compatibility

API responses must:

- include schema field
- remain backward-compatible
- avoid breaking existing clients

---

## 16. Security Considerations

Schemas must:

1. avoid executable content
2. avoid ambiguous parsing
3. enforce strict typing
4. limit size where necessary

---

## 17. Implementation Checklist

1. Schema identifiers everywhere
2. Version parsing
3. JSON validation
4. Unknown field tolerance
5. Canonical serialization support
6. Migration handling
7. Enum fallback handling

---

End of SCHEMA.md
