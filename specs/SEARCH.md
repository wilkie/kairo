# SEARCH.md

## Status

Draft specification.

This document defines search and discovery semantics for Kairo, including local
indexing and federated search. It explicitly separates discovery from validation.

---

## 1. Purpose

Search answers:

> What objects/snapshots might be relevant?

Search does NOT answer:

> Are these objects valid or safe?

Validation remains the responsibility of the core library.

---

## 2. Core Principle

```text
Search returns candidates.
Core determines truth.
Policy determines permission.
```

Search results must never imply validity unless validated.

---

## 3. Search Domains

### 3.1 Local Search

Search over local store data.

Sources:

- Object metadata
- Statements (selected fields)
- Snapshot metadata
- Artifact metadata
- Tags and descriptions

### 3.2 Federated Search

Search over remote peers.

Results are:

- untrusted
- unverified
- partial

Must be clearly labeled.

---

## 4. Indexed Fields

Recommended searchable fields:

```text
object.name
object.description
object.tags
actor.id
artifact.type
artifact.name
environment
build/run metadata
```

Implementations may extend fields but must not assume uniform availability.

---

## 5. Query Model

Simple query syntax:

```text
free text
key:value filters
```

Examples:

```text
"doom dos"
tag:game
actor:john
env:dosbox
```

Query parsing must be tolerant of unknown filters.

---

## 6. Result Model

Each result must include:

```json
{
  "object_id": "z6MkObject...",
  "snapshot_id": null,
  "name": "Example",
  "description": "...",
  "tags": [],
  "source": "local",
  "validation": "unverified",
  "locality": "local"
}
```

### 6.1 Required flags

Results must indicate:

- validation status (or unverified)
- locality (local/remote/cached)
- provenance/source

---

## 7. Ranking

Ranking is implementation-defined but should consider:

- text relevance
- recency
- local vs remote preference
- user interaction history

Ranking must not imply validity.

---

## 8. Federation Semantics

Federated search must:

1. Return results without validation.
2. Include provenance where possible.
3. Avoid auto-fetching full objects unless requested.
4. Respect policy (privacy, rate limits).

---

## 9. Privacy

Search must respect:

- local privacy rules
- publication policy
- redaction of sensitive metadata

Local-only objects must not appear in federated search.

---

## 10. API Mapping

`API.md`:

```text
GET /api/v1/objects?q=...
GET /api/v1/federation/search?q=...
```

Responses must include metadata flags described above.

---

## 11. CLI Mapping

`CLI.md`:

```text
kairo search <query>
kairo federation search <query>
```

---

## 12. Web Client Mapping

`WEB_CLIENT.md`:

- search UI must label results as:
  - Local
  - Remote
  - Unverified
- must provide action to fetch/verify

---

## 13. Indexing

Local indexing may be:

- incremental
- background
- eventually consistent

Indexes must not be authoritative.

---

## 14. Security Requirements

Search must:

1. Not execute content.
2. Not imply validity.
3. Not expose private data.
4. Treat remote data as untrusted.

---

## 15. Implementation Checklist

1. Local search index
2. Federated query support
3. Result DTO with flags
4. CLI integration
5. API endpoints
6. Web client integration
7. Privacy enforcement

---

End of SEARCH.md
