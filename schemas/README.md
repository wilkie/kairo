# Kairo Schemas

This directory contains machine-readable and implementer-facing schema
documentation for Kairo data structures.

The schema namespace has two roles:

1. Document canonical bytes used for hashing, identity, and signatures.
2. Document interchange JSON used by APIs, packages, and external tools.

Canonical encoding and JSON interchange are separate. External clients must not
hash arbitrary JSON unless a canonical schema explicitly says JSON is the hash
input.

## Layout

```text
schemas/
  canonical/
    README.md
    object-genesis-v1.md
    object-revision-v1.md
  json/
    README.md
```

## Normative Order

When documents disagree, resolve in this order:

1. `specs/DECISIONS.md`
2. `schemas/canonical/*.md` for canonical hash/signature bytes
3. `specs/STATEMENTS.md` for the statement model
4. `specs/SCHEMA.md` for general schema and serialization rules
5. Examples elsewhere

JSON schemas describe interchange structure. Canonical schema documents describe
bytes for hashes and signatures.
