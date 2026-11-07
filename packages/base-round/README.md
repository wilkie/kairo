# base-round

The base's packages of one round, each at its snapshot: what the base's
next round builds with (`specs/DECISIONS.md` §64). It doesn't build,
and it holds no files: like `../build-base/`, it is a list of
dependencies that a build depending on `data:base-round` takes as its
own (§57). It names the same labels, but each with `offered_at`, the
snapshot of the round that offers it.

**Why it exists.** The base's packages build with one another: binutils
with make, make with binutils. Through `build-base`, which takes the
newest release of each, a round built with itself is a cycle, as
`bootstrap-base` was made to avoid for the first round (§59). This
object names the round before by snapshot, so each round builds with
the last, and the base never builds with itself. Everything else,
Python and the rest, builds with `build-base` and gets the newest
round.

**Its versions are rounds.** `1.0.0` is round 1, the base's first
generation, built with `../bootstrap-base/`; `2.0.0`, this revision, is
round 2, built with round 1. The base's round N+1 depends on
`data:base-round` with `^N`; once it is built, this object's next
revision names its snapshots, tagged `N+1.0.0`, and the revision before
stays a release, for the round it names to be found by. Round 2's
snapshots, in the archive's canonical store:

| Package | Release | Snapshot |
|---|---|---|
| `bash` | 5.2.15+2 | `zQmQiJqNmdFsn6VTaWGo7uotdz3Fh6FST6WyYiAVQgzzrYY` |
| `coreutils` | 9.4.0+2 | `zQmZQqekSsrBhnL88kNA7Z3VcxAin6gE9eAZEewPeyzDf1u` |
| `sed` | 4.8.0+2 | `zQmV1oxcaQzKR2NdMhMn5PrAgtrGf9rGopXp5QgJZqP12Bh` |
| `grep` | 3.12.0+2 | `zQmabWaWvjQdjX7v84QpDTdeGSfjHuvvwBPmkXSDSrHD4AR` |
| `gawk` | 5.3.0+2 | `zQmQ6GMHcYG6PCgjEUUkwoDDJ4ghtcab8WtePnL3BB56TNM` |
| `make` | 4.2.1+2 | `zQmdMqNxBjhrfEV7UePv3LkQtqXyGVZqnSDUwBaFDM73yCV` |
| `diffutils` | 3.10.0+2 | `zQmfNtsnmUf8NEbhaLBuGWdS6kJzfspN8uU4yQ8BSLGGcz7` |
| `findutils` | 4.10.0+2 | `zQmP4sb35yRtJcAEEwn2JwBwMioT8Hut45AXqwMoMAy5yZ2` |
| `patch` | 2.8.0+2 | `zQmaZVGQxduQxY9gzxiQUfEMfwR1PMHn6zioqEPG2JGXmVS` |
| `tar` | 1.34.0+2 | `zQmbMoSCtk1xF36zRybb8QusqkhwFYFYkWu884aqQjxWNKx` |
| `gzip` | 1.13.0+2 | `zQmc582VtDz26dB6iwLtG5tNKu8Wmg2MJtzZRZ9n2N9R4hw` |
| `xz` | 5.6.4+2 | `zQmWg6e5iEv9xrJgVPNeRnkD4kwRj67srn1LU8g3Lz4Sagb` |
| `bzip2` | 1.0.8+2 | `zQmV9pKgYs5MfYX4P9xSeG1MD2p3rjFdpF7CjEjtBN1V2BX` |
| `m4` | 1.4.19+2 | `zQmSMLYHpiFv57dbWEqS65g3A2Uza8obaMd1GJ4W6e51gdM` |
| `binutils` | 2.41.0+2 | `zQmf3uUbxYsQDsLgPRH5dgt7eAUgCvNz5LV3aXnS1et79uh` |
| `gcc` | 15.2.0+2 | `zQmYVCzGaPWo71fCGjMi5nVU8VZoqAYQYEt7Z7U4YGHAnEz` |
| `musl` | 1.2.5+2 | `zQmb9UZU2t3fvtnaVcbFHxtNfTi6r4iav2bGJo1YF3Ag48K` |
| `linux-headers` | 4.14.336+2 | `zQmcxGqyVc4rCxrU9cJav82wX7vosmqPxhYQRAWcus6ZWRF` |

Each package's x86 and x86-64 children are of its one snapshot, so a
label resolves per environment (§53), as `build-base`'s do.

| File | What it is |
|---|---|
| `kairo.toml` | The label it provides, `data:base-round` at the round's version, and the dependencies it brings, each at its snapshot. |
| `import.sh` | Creates the object or its next revision with `../../bootstrap/wrapper.sh`, tagged with the round. |

## Running it

```sh
packages/base-round/import.sh <your actor id>
```

Then a package of the base's next round depends on it:

```toml
[[build.targets.dependencies]]
kind = "provides"
provides = "data:base-round"
version = "^2"
```
