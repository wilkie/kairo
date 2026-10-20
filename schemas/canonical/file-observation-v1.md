# FileObservation v1 Canonical Encoding

## Type

```text
FileObservation
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

What one download of a file source saw (`specs/DECISIONS.md` §38
sub-decisions 1–2): fetching `url`, the observer got `size` bytes
hashing to `sha256`. Signing it claims the observer fetched that URL at
`created_at` and holds those bytes. Several mirrors serving one file
are several observations of one file source; a URL that later serves
other bytes is an observation of a different one. Observations are
never superseded.

## Example

```json
{
  "type": "FileObservation",
  "version": 1,
  "actor": "zQmTn1mdQDA1ryQZsiqYfRbqj5DGcG8TNvYcRmBrBLAuk5t",
  "subject": "object:zQmNPcfYNCVuiLNdZRXgfR2aMt71N7v4ShA5XxUTnoCNCwb",
  "created_at": "2026-10-20T12:00:00Z",
  "previous": [],
  "body": {
    "source": "zQmNPcfYNCVuiLNdZRXgfR2aMt71N7v4ShA5XxUTnoCNCwb",
    "url": "https://mirrors.kernel.org/gnu/mes/mes-0.27.1.tar.gz",
    "sha256": "183a40ea47ea49f8a1e3bd1b9d12e676374d64d63bc79e7bc1ae7d673dfdf25d",
    "size": 756876
  },
  "signature": { "...": "..." }
}
```

## Canonical Envelope Fields

As every statement: type `"FileObservation"`, version `1`, `actor`,
`subject` (`object:<file source id>`), `created_at`, `previous`, then
the body. **Actor chain** (`specs/DECISIONS.md` §23): the signer's
`object <file source id>` chain.

## Canonical Body Fields

| Field | Encoding |
|---|---|
| `source` | `ObjectId` payload as `string`: the id `sha256` derives (`file-source-genesis-v1.md`) |
| `url` | `string`: the URL asked for, in normal form |
| `sha256` | `string`: 64 lowercase hex digits |
| `size` | `u64`, big-endian: the byte count |

`url` is the URL the fetch asked for, not where a redirect led (a
redirect target is often a short-lived signed URL). Its normal form
(`FileUrl::parse`, `crates/kairo-statement/src/file_source.rs`):

| Input | Rule |
|---|---|
| scheme | `https`; `http` only for a loopback host (`localhost`, `127.0.0.0/8`, `[::1]`); lowercased |
| userinfo | refused |
| host | lowercased |
| port | dropped when it is the scheme's default |
| path, query | kept as written; a path is required |
| fragment | refused |

Unlike a Git source's URL this is not an identity, only what was
fetched, so path and query are not rewritten.

Shape rules: `source` must be the id `sha256` derives; `url` must be in
normal form.

## Pseudocode

```text
canonical_body =
  string(source) ||
  string(url) ||
  string(lowercase_hex(sha256)) ||
  u64(size)
```

## Test Vectors

Test vectors will be added once fixture generation is stabilized.
