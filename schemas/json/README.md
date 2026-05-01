# JSON Schemas

This directory is reserved for JSON interchange schemas used by APIs, packages,
and external tools.

JSON schemas describe data shape for transport and validation. They are not
canonical hash inputs unless a canonical schema document explicitly says so.

Documents:

- `statement-envelope-v1.schema.json`
- `object-genesis-v1.schema.json`
- `object-revision-v1.schema.json`

These JSON schemas describe interchange shape only. Canonical hash and signature
inputs are defined by the corresponding documents in `schemas/canonical/`.
