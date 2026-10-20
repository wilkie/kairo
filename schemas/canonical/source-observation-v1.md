# SourceObservation v1 Canonical Encoding

## Type

```text
SourceObservation
```

## Version

```text
1
```

## Domain Separator

```text
kairo.statement.v1
```

## Derived ID

An ordinary signed statement: its `StatementId` is derived from the
unsigned envelope and body.

## Purpose

What one fetch of a source saw (`specs/DECISIONS.md` §37 sub-decisions
2–4): every ref the remote advertised under `refs/heads/` and
`refs/tags/`, each at its commit as a typed revision. Signing it claims
the observer fetched that URL at `created_at` and holds each commit's
full closure. Commit ids are content hashes, so two observers listing
the same commit for a ref retrieved the same content independently.

Observations are never superseded: each is one moment, and together
they are the source's ref history. A ref listed by an earlier
observation and absent from a later one was deleted upstream; a moved
tag or force-pushed branch shows as a different commit. No observed
commit is dropped from the archive (sub-decision 3).

## Example

```json
{
  "type": "SourceObservation",
  "version": 1,
  "actor": "zQmTn1mdQDA1ryQZsiqYfRbqj5DGcG8TNvYcRmBrBLAuk5t",
  "subject": "object:zQmSource...",
  "created_at": "2026-10-20T12:00:00Z",
  "previous": [],
  "body": {
    "source": "zQmSource...",
    "url": "https://github.com/oriansj/stage0-posix",
    "refs": [
      { "name": "refs/heads/master", "commit": "git:sha1:6435980..." },
      { "name": "refs/tags/Release_1.9.1", "commit": "git:sha1:93006ab..." }
    ]
  },
  "signature": { "...": "..." }
}
```

## Canonical Envelope Fields

As every statement: type `"SourceObservation"`, version `1`, `actor`,
`subject` (`object:<source id>`), `created_at`, `previous`, then the
body. **Actor chain** (`specs/DECISIONS.md` §23): the signer's
`object <source id>` chain.

## Canonical Body Fields

| Field | Encoding |
|---|---|
| `source` | `ObjectId` payload as `string`: the id the URL derives (`source-genesis-v1.md`) |
| `url` | `string`: the canonical URL fetched |
| `refs` | `list<ref>`, strictly ascending by name |

`ref` is `string(name) || string(commit)`: `name` is `refs/heads/<x>` or
`refs/tags/<x>`; `commit` is `git:sha1:<oid>` or `git:sha256:<oid>`.

Shape rules: `source` must be the URL's id; names unique and ascending;
every commit in one object format, since a repository has one. Other
ref namespaces (`HEAD`, `refs/pull/*`, `refs/remotes/*`) are not
observed.

## Pseudocode

```text
canonical_body =
  string(source) ||
  string(url) ||
  list(refs, string(name) || string(commit))
```

## Test Vectors

Test vectors will be added once fixture generation is stabilized.
