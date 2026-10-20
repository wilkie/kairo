# BuildResult v1 Canonical Encoding

## Type

```text
BuildResult
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

`BuildResult` is an ordinary signed statement. Its `StatementId` is
derived from the unsigned statement envelope and body; the signature
proves authorship of those canonical bytes but is not part of the id.

```text
StatementId = z<base58btc(multihash_sha2_256(domain || canonical_unsigned_statement_bytes))>
```

## Purpose

A `BuildResult` records what one run of one build plan produced
(`specs/DECISIONS.md` §33 sub-decision 8, §34 sub-decisions 4 and 7):
the plan (`resolved_manifest`, whose hash covers the whole input
closure), whether it succeeded, the artifact when it did, and what ran
it. It is evidence about a snapshot and **never joins the frontier**.
Any actor may sign one — the first builder, or a reproducer whose
agreement on the artifact id is what makes an artifact substitutable.
Timeouts and policy refusals are operational outcomes and are never
signed.

## Example

```json
{
  "type": "BuildResult",
  "version": 1,
  "actor": "zQmTn1mdQDA1ryQZsiqYfRbqj5DGcG8TNvYcRmBrBLAuk5t",
  "subject": "object:zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk",
  "created_at": "2026-10-19T12:00:00Z",
  "previous": [],
  "body": {
    "object": "zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk",
    "snapshot": "zQmPrz1SBNXD1XAPCaWrTtpBbEZHGevtUoWY8ibihamaApZ",
    "target": "release",
    "resolved_manifest": "zQmPlan...",
    "result": "success",
    "artifact": "zQmArtifact...",
    "executor": "native/1",
    "environment": "none",
    "host_dependent": true,
    "execution": "zQmRecord..."
  },
  "signature": { "...": "..." }
}
```

## Canonical Envelope Fields

| Field | Encoding |
|---|---|
| type `"BuildResult"` | `string` |
| version `1` | `u8` |
| `actor` | `ActorId` payload as `string` |
| `subject` | internal Kairo reference as `string` (`object:<id>`) |
| `created_at` | `Timestamp` (`i64` epoch seconds) |
| `previous` | `list<string>` — the signer's chain tips, ascending, at most 16 |
| `body` | body fields below |

**Actor chain** (`specs/DECISIONS.md` §23): chained in the signer's
`object <id>` chain (the body's `object`), like branches and tags.

## Canonical Body Fields

| Field | Encoding |
|---|---|
| `object` | `ObjectId` payload as `string` |
| `snapshot` | `SnapshotId` payload as `string` |
| `target` | `string`, the build target's name |
| `resolved_manifest` | `BlobId` payload as `string`: the plan hash (`resolved-manifest-v1.md`) |
| `result` | `string`: `success` or `failure` |
| `artifact` | `option<string>`: the artifact id (`artifact-tree-v1.md`), present exactly on success |
| `executor` | `string`: executor id and version, e.g. `native/1` |
| `environment` | `string`: the environment contract an engine provided, or `none` |
| `host_dependent` | `u8`: `1` when a host tool or the host itself took part, else `0` |
| `execution` | `BlobId` payload as `string`: the signer's execution record id |

Shape rules, enforced at construction: `success` requires `artifact`
and `failure` forbids it; `target`, `executor`, and `environment` are
non-empty.

## Excluded Fields

- signature
- the execution record's contents (local evidence; its id is cited so
  it can be asked for)
- the isolation engine and mounting strategy (the record's; the plan
  hash excludes them so results under different engines compare)
- received-at timestamps, source peer, local trust decisions

## Rust-Equivalent Pseudocode

```text
canonical_body =
  string(object) ||
  string(snapshot) ||
  string(target) ||
  string(resolved_manifest) ||
  string(result) ||
  option(artifact, string) ||
  string(executor) ||
  string(environment) ||
  u8(host_dependent) ||
  string(execution)

canonical_unsigned_statement =
  string("BuildResult") ||
  u8(1) ||
  string(actor) ||
  string(subject) ||
  i64_be(created_at_epoch_seconds) ||
  list(previous, string) ||
  canonical_body

statement_id =
  sha2_256_multihash_base58btc(
    "kairo.statement.v1" || canonical_unsigned_statement
  )
```

## Notes

- Two results agreeing on `(resolved_manifest, artifact)` from
  different signers are independent reproduction evidence; a node's
  substitution policy counts them (`DECISIONS.md` §34 sub-decision 7).
  A `host_dependent` result never counts.
- A valid signature proves only that the actor claims to have run the
  plan and seen this outcome. It proves nothing about the build's
  correctness.

## Test Vectors

Test vectors will be added once fixture generation is stabilized.
