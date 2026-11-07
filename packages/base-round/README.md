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
generation, built with `../bootstrap-base/`. Round 2 depends on
`data:base-round` with `^1`; once round 2 is built, this object's next
revision names its snapshots, tagged `2.0.0`, for round 3. Round 1's
snapshots, in the archive's canonical store:

| Package | Version | Snapshot |
|---|---|---|
| `bash` | 5.2.15 | `zQmUcFop16w13E562Ec9eu6bsSpBYD2v9tzihKQJTKG8zC7` |
| `coreutils` | 9.4.0 | `zQmPmysSWvqgwYehxvMC2xuHwuxfCfDfssSGn4Rx5f4uvsL` |
| `sed` | 4.8.0 | `zQmdF3KzFoCMFc8Xs9nppVzFCBu4igv4JBAMaGRb9FtozJn` |
| `grep` | 3.12.0 | `zQmRTxPvToPzH5kShU8ZrKM9JZhYZtbJNnC9CD5sCDrUPKY` |
| `gawk` | 5.3.0 | `zQmRn9Tr2pN9V15BiQTkdSystVFXar9rKPa8fx1S5axR2Bg` |
| `make` | 4.2.1 | `zQmYMLErDmsNFkqGVsLXbQiLfUHLzdyvp6WStpD7JhDXKhM` |
| `diffutils` | 3.10.0 | `zQmVyzwbdUShgQka4xynMQ3nzCxWKXdq8r8wxAev5Lc3vc9` |
| `findutils` | 4.10.0 | `zQmNo39VzGxoyiqRsNFJqR2Fmbu62KyEncMx6mHfAFSFnvk` |
| `patch` | 2.8.0 | `zQmREexC8Y5qdcLdatnjgrH8YyV3JBFsU5bfEwd5V62GvTU` |
| `tar` | 1.34.0 | `zQmQT8PNiBc9X5gmsCeWeFos33jPxHUJoWoqcKreaeprfGw` |
| `gzip` | 1.13.0 | `zQmcokrvs2aoXSQ8QdHejehXp5r1Bv3cg2kCdgSfR4dZuDn` |
| `xz` | 5.6.4 | `zQmX7gZHpLqdaaY1E78rWwv2uUyVegLNzQ8QBGNmhMn9A2R` |
| `bzip2` | 1.0.8 | `zQmfGJqHEuMPYe5hEnFS3jDr8tqsdcpWAJui9igSZ3U2kiK` |
| `m4` | 1.4.19 | `zQmWz7tJnAn8i3quMDdnpT2rsoSQ319PQHqRHSZNFfBkVte` |
| `binutils` | 2.41.0 | `zQmPuFvvtnLVaB7rU3tDN66c4K6qLpo25zrRK8WuKrs27hY` |
| `gcc` | 15.2.0 | `zQmWZCAUR8LTUnqtmbAKPdTmrGzzhvpv9Asrg7uQHt2TkZF` |
| `musl` | 1.2.5 | `zQmYjwMcdH6xHuFJSQMvrABeHRwJANaXKEPrNJ2mkLhG9j3` |
| `linux-headers` | 4.14.336 | `zQmZKw9fp2kRFaB8smYt5Gw5u4RMqakdoWvt6PUpo5mNsva` |

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
version = "^1"
```
