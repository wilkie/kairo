# gcc 7.5.0 (x86 and x86-64)

gcc 7.5.0, with C and C++, for musl, as a package of its own: built on
a finished root, not as a step of a bootstrap (`specs/DECISIONS.md`
§55). It is the first of the gcc versions Kairo keeps as artifacts, so
that a build can choose its compiler by version, and a later gcc can be
built with any earlier one.

One object has a target for each environment it is built for, and no
environment of its own: each target's artifact runs where it was built
(§53 sub-decision 1).

| Target | Runs in, built in | Built on | Triple |
|---|---|---|---|
| `x86` | `linux/x86` | live-bootstrap's finished x86 root (`data:live-bootstrap:x86:libarchive-3.5.2`) and the merged-/usr links | `i686-pc-linux-musl` |
| `x86-64` | `linux/x86-64` | the 64-bit bootstrap's root (`data:amd64-bootstrap:x86-64:root`, `bootstrap/amd64/`) | `x86_64-pc-linux-musl` |

Each gcc bootstraps: the root's gcc 15 builds the first stage, which
builds the second, which builds the third, and the last two are
compared. What the artifact holds is gcc 7's own work, whatever gcc the
root has. The first stage is built as C17 and C++98, with
`-fpermissive`, since gcc 7's sources predate what gcc 15 rejects. One
line of gcc 7 initializes a `bool` with `NULL`, which musl defines as
`nullptr` in C++11 and later; it is changed to `false`, as gcc 8 has it.

## Beside another gcc

A consumer's `/` is an overlay of its dependencies, the first listed on
top (§41). gcc 7 is built to sit beside the root's gcc there:

- Its runtime libraries, `libstdc++` and `libgcc_s` among them, and its
  C++ headers are in its own directory, `/usr/lib/gcc/<triple>/7.5.0/`
  (`--enable-version-specific-runtime-libs`), never over the root's
  newer `libstdc++.so.6`. libgcc installs `libgcc_s` one directory up,
  in `/usr/lib/gcc/<triple>/lib/`, which no version names, so it is
  moved into the versioned directory, where the driver looks when it
  links. libcc1, which would go in `/usr/lib`, isn't built. The build
  fails if any file but a program in `/usr/bin` is outside a directory
  named by the version.
- Its programs are `/usr/bin/gcc`, `g++`, and the rest, the same names
  as the root's: whichever gcc is listed first is the one a build runs,
  so a consumer lists `tool:gcc` before its root. In x86, live-bootstrap's
  gcc requires its consumer to choose a musl (`one_of`, §51), and this
  one does too; in x86-64, the root brings its own.
- A C++ program it links with the shared `libstdc++` runs against the
  newer one the root has in `/usr/lib`, as the libraries have kept their
  ABI since gcc 3.4. So the object offers `lib:stdc++:static` and not
  the shared libraries, which a consumer takes from the root.

## What it offers

Each target offers `tool:gcc` and `tool:g++` 7.5.0, the compiler for
the machine it runs on, their triple labels (`tool:gcc:i686-linux-musl`,
`tool:gcc:x86_64-linux-musl`), and `lib:stdc++:static`. The two targets
offer the same labels, each to builds in its own environment.

It requires what a gcc runs with: for `x86`, what live-bootstrap's own
gcc children do (binutils for `i686-linux-musl`, musl, and the kernel's
headers); for `x86-64`, the 64-bit root as a whole, which offers no
labels of its own for its parts yet.

libsanitizer, libmpx, and libcilkrts don't build against musl, and musl
has its own stack protector, so they and libssp are left out.

| File | What it is |
|---|---|
| `kairo.toml` | The sources, shared by both targets, and the two targets with their roots and what they offer. |
| `src/build.sh` | Builds gcc for the triple it is given, then checks that only its programs are outside its versioned directories, that they are the target's code, and that C and C++, static and dynamic, compile and run with it installed over the root. |
| `import.sh` | Archives the tarballs and creates the object with `../../bootstrap/wrapper.sh`. |

## Running it

Import the roots: live-bootstrap's six passes and linux-filesystem for
`x86`, the 64-bit bootstrap through stage 3 for `x86-64`. Then:

```sh
packages/gcc-7.5.0/import.sh <your actor id>
kairo build object:<object id> --target x86 --actor <your actor id>
kairo build object:<object id> --target x86-64 --actor <your actor id>
```
