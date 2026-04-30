# IDENTIFIERS.md

## Status

Draft specification.

This document defines identifier and reference formats used across Kairo:
objects, statements, snapshots, blobs, actors, executions, tasks, and packages.

Identifiers are fundamental to addressing, deduplication, and integrity.

---

## 1. Purpose

Identifiers and references must:

1. Uniquely identify entities.
2. Be stable across systems.
3. Be safe for transport in JSON, TOML, URLs, logs, and filenames.
4. Avoid repeated type noise when a field already gives the type.
5. Support content-addressing where appropriate.
6. Provide a clear external form for federation and archival references.

---

## 2. IDs vs References

Kairo distinguishes **ID payloads** from **typed references**.

An ID payload is the opaque encoded value:

```text
<id>
```

Examples:

```text
zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk
zQmaFrbQVbbdb1HSDQFdPjhtB4XPZMhmyazswWUp57qpwr9
zQmPrz1SBNXD1XAPCaWrTtpBbEZHGevtUoWY8ibihamaApZ
zQmfBE2w2UqKJhGZAxK4ZWb4JuCrvxnN9P4YKuvzfbPSvD5
```

A typed reference adds semantic type context:

```text
object:<id>
statement:<id>
blob:<id>
actor:<id>
```

An external Kairo reference adds the Kairo namespace:

```text
kairo:object:<id>
kairo:statement:<id>
kairo:blob:<id>
kairo:actor:<id>
```

Bare ID payloads are only valid where the surrounding field, schema, or API
parameter supplies the type. Untyped strings must use typed references.

---

## 3. Canonical Forms

### 3.1 Typed Fields

Typed fields use bare ID payloads:

```toml
object = "z6MkObject..."
snapshot = "z6MkSnapshot..."
actor = "z6MkActor..."
blob = "z6MkBlob..."
```

Equivalent JSON fields:

```json
{
  "object_id": "z6MkObject...",
  "snapshot_id": "z6MkSnapshot...",
  "actor_id": "z6MkActor...",
  "blob_id": "z6MkBlob..."
}
```

### 3.2 Internal Typed References

Internal typed references are used for CLI arguments, logs, package manifests,
federation tokens, and other places where the string may stand alone:

```text
object:z6MkObject...
object:z6MkObject...:snapshot:z6MkSnapshot...
statement:z6MkStatement...
blob:z6MkBlob...
actor:z6MkActor...
```

### 3.3 External Kairo References

External references are used in federation, exported packages, documentation,
cross-system links, and long-lived public references:

```text
kairo:object:z6MkObject...
kairo:object:z6MkObject...:snapshot:z6MkSnapshot...
kairo:statement:z6MkStatement...
kairo:blob:z6MkBlob...
kairo:actor:z6MkActor...
```

External references must be parseable without local schema context.

---

## 4. Identifier Types

### 4.1 ObjectId

Represents a logical Object lineage.

Bare payload:

```text
z6MkObject...
```

Typed reference:

```text
object:z6MkObject...
```

External reference:

```text
kairo:object:z6MkObject...
```

An Object ID must be globally unique. It should be derived from or bound to the
Object genesis statement. It must not be derived from a mutable name or current
source revision.

### 4.2 SnapshotId

Represents a selected Object state.

Bare payload:

```text
z6MkSnapshot...
```

Typed reference:

```text
object:z6MkObject...:snapshot:z6MkSnapshot...
```

External reference:

```text
kairo:object:z6MkObject...:snapshot:z6MkSnapshot...
```

A Snapshot ID should be derived from:

```text
hash(object_id + frontier)
```

Snapshot IDs are distinct from storage revision IDs such as Git commits.

### 4.3 StatementId

Represents a single signed statement.

Typed reference:

```text
statement:z6MkStatement...
```

External reference:

```text
kairo:statement:z6MkStatement...
```

Statement IDs should be derived from:

```text
hash(canonical_statement_envelope_without_signature)
```

### 4.4 BlobId

Represents content-addressed bytes.

Typed reference:

```text
blob:z6MkBlob...
```

External reference:

```text
kairo:blob:z6MkBlob...
```

Blob IDs must be content-addressed. In v1, Kairo-native Blob IDs use the same
SHA-256 multihash payload format as other stable IDs. Implementations must not
infer the hash algorithm from digest length.

### 4.5 ActorId

Represents a signing identity.

Typed reference:

```text
actor:z6MkActor...
```

External reference:

```text
kairo:actor:z6MkActor...
```

The payload format depends on the key system.

### 4.6 ExecutionId, TaskId, PackageId

Runtime and package IDs may use bare payloads in typed fields and typed
references when standalone:

```text
execution:z6MkExecution...
task:z6MkTask...
package:z6MkPackage...
```

These IDs may be random unless their owning spec requires deterministic
derivation.

---

## 5. Encoding

The `<id>` payload for Kairo-native stable IDs must be:

```text
z<base58btc(multihash_sha2_256_digest)>
```

Requirements:

1. The first character must be the multibase base58btc prefix `z`.
2. The decoded bytes must be a multihash.
3. The multihash algorithm code must be `sha2-256`.
4. The digest length must be 32 bytes.
5. Implementations must reject other multihash algorithms for canonical Kairo IDs
   in v1.

This fixes canonical identity to one string for a given digest while still
making the hash algorithm self-describing.

---

## 6. Determinism

Where possible:

- StatementId is deterministic from statement content.
- BlobId is deterministic from blob content.
- SnapshotId is deterministic from Object ID and frontier.
- ObjectId is deterministic from, or cryptographically bound to, genesis.

Runtime IDs such as task and execution IDs may be random.

---

## 7. Names vs IDs

Names are not identifiers.

```text
name -> human label
id   -> canonical identity payload
ref  -> typed reference to an ID
```

Search may use names, but APIs must return IDs or typed references.

---

## 8. Validation

Implementations must:

- validate typed-reference grammar
- validate ID payload encoding
- reject malformed IDs and references
- avoid guessing ID types from payload shape
- normalize external `kairo:` references before semantic processing

---

## 9. Security

Identifiers must:

1. Not embed secrets.
2. Not rely on sequential IDs for security.
3. Resist collision attacks.
4. Be safe for untrusted input contexts.
5. Be parsed structurally, not with path or URL string concatenation.

---

## 10. API Representation

Typed API fields may use bare IDs:

```json
{
  "object_id": "z6MkObject...",
  "snapshot_id": "z6MkSnapshot..."
}
```

Fields named `ref`, `subject`, `target`, or similar untyped reference fields
must use internal or external typed references:

```json
{
  "ref": "object:z6MkObject...:snapshot:z6MkSnapshot..."
}
```

Clients must treat identifiers and references as opaque except for the grammar
defined here.

---

## 11. File System Mapping

Typed references must be encoded or mapped before use in paths. Stores may use a
type directory plus bare payload:

```text
objects/z6MkObject.json
statements/z6MkObject/z6MkStatement.json
```

Implementations must ensure path safety.

---

## 12. Unsupported Forms

The identifier forms in this document are the only supported forms for the
initial implementation. Implementations must reject unrecognized prefixes or
alternate spellings rather than accepting and normalizing them.

---

## 13. Implementation Checklist

1. Bare ID payload validation.
2. Typed-reference parser.
3. External `kairo:` reference parser.
4. Conversion between external and internal references.
5. Deterministic ID generation where required.
6. Random ID generation for runtime entities.
7. Collision handling strategy.
8. Safe serialization/deserialization.

---

End of IDENTIFIERS.md
