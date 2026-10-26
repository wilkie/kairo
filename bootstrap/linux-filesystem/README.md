# linux-filesystem: the merged-/usr layout

An object that doesn't build (`specs/DECISIONS.md` §50): its tree under
`files/` is its artifact, installed at `/`. It holds three links,
committed as links:

| Link | Target |
|---|---|
| `/bin` | `usr/bin` |
| `/lib` | `usr/lib` |
| `/sbin` | `usr/sbin` |

as live-bootstrap's `merged_usr` step makes them, and as distributions
with a merged `/usr` have them. It provides
`data:linux-filesystem:merged-usr` 1.0.0 and depends on nothing.

A package that needs the layout wherever it is installed requires the
label. live-bootstrap's shared musl does (§51): its loader is
`usr/lib/ld-musl-i386.so.1`, and what is linked against it names
`/lib/ld-musl-i386.so.1`. Through `requires` (§46), anything built
against the shared musl, and anything that needs it at run time, gets
the layout beside it.

`/usr/sbin → bin`, which `merged_usr` also makes, is left out: a package
with files under `usr/sbin/` (live-bootstrap's util-linux) would collide
with it.

```sh
bootstrap/linux-filesystem/import.sh <your actor id>
```
