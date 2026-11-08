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
generation, built with `../bootstrap-base/`; `2.0.0` is round 2, built
with round 1; `3.0.0`, this revision, is round 3, built with round 2.
The base's round N+1 depends on `data:base-round` with `^N`; once it
is built, this object's next revision names its snapshots, tagged
`N+1.0.0`, and the revision before stays a release, for the round it
names to be found by. Round 3's snapshots, in the archive's canonical
store:

| Package | Release | Snapshot |
|---|---|---|
| `bash` | 5.2.15+3 | `zQmSnsCrzjuUtSUFoGu6PWwUYwe6e6xD4BSZMfJT3qeL6P4` |
| `coreutils` | 9.4.0+3 | `zQmXJYX2NyjDm9f5D1962UnUr1w8RvWb3hmXMFbQeKGS7BF` |
| `sed` | 4.8.0+3 | `zQmZqCN1KPEAKj5Ey1zjzqMBxV7dXgEcK3yZnbc4GNmzY1m` |
| `grep` | 3.12.0+3 | `zQmTBSMPJAMuQVjhmZsq9b5YGjpQKLHyEoLFbWa5CQcZFu2` |
| `gawk` | 5.3.0+3 | `zQme9js9nbsRuX5YY5VCanm3GdnJ6SR9D9CKV249PCoUXba` |
| `make` | 4.2.1+3 | `zQmUZ9hMgWygq4x4YBaWK4rDeKQFfyLdpHoFBLiMHgDc2gM` |
| `diffutils` | 3.10.0+3 | `zQmY46y2gKosQZFGq2eUYrRLuz7HJvQSppbpSVdrUUgmDu7` |
| `findutils` | 4.10.0+3 | `zQmb12Fyw1zukSq7ZWmJWDrgdT8ZVn6yiHG3K9nf8aDK1sE` |
| `patch` | 2.8.0+3 | `zQmWXUmVY2FdzDbdEbJjc9yfGWpHVNC1EMDVGJV4Pp8ggfR` |
| `tar` | 1.34.0+3 | `zQmaGmYyZj2JYYjTPcH9QUgBcopxyiA6N3hVB3fSq8vZJqv` |
| `gzip` | 1.13.0+3 | `zQmf9X1JgeHiaXrcWag73b3sRgwwzR6wdcVyiiU4a61UP8h` |
| `xz` | 5.6.4+3 | `zQmcp6Bu7pCVyWkEyHpVQsviWtqAKe6U26CttwASMP5Finc` |
| `bzip2` | 1.0.8+3 | `zQmauP32mWo87yGEER24RZHNgRETchhGQJxe8HvwfjsqcLe` |
| `m4` | 1.4.19+3 | `zQma3oDFzg1tuBPReSKSuHJF29Z9ba86aBF4RXkqrHyY9PH` |
| `binutils` | 2.41.0+3 | `zQmayiDwcCatc8ei55PQqvfvLjuNxoENRrcHYcWqRNNz8m5` |
| `gcc` | 15.2.0+3 | `zQmcj2PCsUQDPcBmV6wA5FL4RP2jWj9Mp2Wi39P1iPk8ni6` |
| `musl` | 1.2.5+3 | `zQmX3oDoV8HupHBRQ5pika9BbLHYMS4LpzZWrBqdaoGPh3p` |
| `linux-headers` | 4.14.336+3 | `zQmQFvpdi1ygQQ8TpYKBvCiUSxovQdSECFAEiLxadaGmkMq` |

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
version = "^3"
```
