# gcc 7.5.0 (x86 and x86-64)

gcc 7.5.0, with C and C++, for musl, as a package of its own: built on
finished packages, not as a step of a bootstrap (`specs/DECISIONS.md`
§55). It is the first of the gcc versions Kairo keeps as artifacts, so
that a build can choose its compiler by version, and a later gcc can be
built with any earlier one.

One object has a target for each environment it is built for, and no
environment of its own: each target's artifact runs where it was built
(§53 sub-decision 1).

| Target | Runs in, built in | Built from | Triple |
|---|---|---|---|
| `x86` | `linux/x86` | live-bootstrap's packages, its children | `i686-pc-linux-musl` |
| `x86-64` | `linux/x86-64` | the 64-bit bootstrap's packages, stage 3's children (`bootstrap/amd64/native-root/`) | `x86_64-pc-linux-musl` |

Neither builds on a whole root (§56). Each depends on `data:build-base`
(`../build-base/`, §57), which brings a shell, the GNU userland, make,
the archivers, m4 for gmp, binutils, gcc and its runtime libraries,
musl, and the kernel's headers, each the package for the target's
environment. Beyond that, each names the gcc 15 its first stage is
built by (`^15.2.0`), which replaces build-base's gcc of any version,
and static musl, for its checks.

Each gcc bootstraps: gcc 15 builds the first stage, which
builds the second, which builds the third, and the last two are
compared. What the artifact holds is gcc 7's own work, whatever gcc
built it. The first stage is built as C17 and C++98, with
`-fpermissive`, since gcc 7's sources predate what gcc 15 rejects. One
line of gcc 7 initializes a `bool` with `NULL`, which musl defines as
`nullptr` in C++11 and later; it is changed to `false`, as gcc 8 has it.

## Children

Each target's artifact is installed nowhere (`install = false`); it is
the target's children side by side (§56).

| Child | Where | Offers |
|---|---|---|
| `gcc-7.5.0` | `/usr/bin` and `/usr/lib/gcc/<triple>/7.5.0/`, `/usr/libexec/gcc/<triple>/7.5.0/` | `tool:gcc`, `tool:g++`, their triple labels (`i686-linux-musl`, `x86_64-linux-musl`), `lib:stdc++:static` |
| `libstdc++-7.5.0`, `libgcc_s-7.5.0`, `libgomp-7.5.0`, `libatomic-7.5.0`, `libitm-7.5.0`, `libquadmath-7.5.0` | `/usr/lib` | `lib:stdc++:shared`, `lib:gcc:shared`, `lib:gomp:shared`, `lib:atomic:shared`, `lib:itm:shared`, `lib:quadmath:shared`, each conferring itself `>=7.5.0` |

The two targets have the same children and offer the same labels, each
to builds in its own environment.

**The compiler sits beside another gcc.** A consumer's `/` is an
overlay of its dependencies, the first listed on top (§41).

- **Its own directories:** its runtime libraries and C++ headers are in
  its versioned directory, `/usr/lib/gcc/<triple>/7.5.0/`
  (`--enable-version-specific-runtime-libs`). It links against them
  there, so a program records gcc 7's symbol versions.
- **libgcc_s moved:** libgcc installs `libgcc_s` one directory up, in
  `/usr/lib/gcc/<triple>/lib/`, which no version names, so the recipe
  moves it into the versioned directory.
- **libcc1 left out:** it would go in `/usr/lib`, so it isn't built.
- **Checked:** the build fails if any of the compiler's files, other
  than its programs in `/usr/bin`, is outside a directory named by the
  version.
- **Programs:** they are `/usr/bin/gcc`, `g++`, and the rest, the same
  names as any other gcc's. Whichever gcc is listed first is the one a
  build runs.

It requires binutils for its target, musl's headers, the kernel's
headers, and the shared musl and `libgcc_s` its own programs load. It also requires
`one_of` musl's static or shared libc, which whatever builds with it
chooses (§51).

**Each runtime library is a child of its own.** Each is a copy of the
library's file and its soname's link, in `/usr/lib`, where the loader
finds it. These libraries keep one soname and their ABI from release
to release, so the newest one runs what any earlier gcc linked. In an
environment with gcc 7's and gcc 15's `libstdc++`, the confers choose
one, and it is 15.2.0's whenever anything there was linked by gcc 15.
The build checks that a C++ program gcc 7 links runs both against the
newer library its environment has and against gcc 7's own child.

libsanitizer, libmpx, and libcilkrts don't build against musl, and musl
has its own stack protector, so they and libssp are left out.

| File | What it is |
|---|---|
| `kairo.toml` | The sources, shared by both targets, and the two targets with what they build from and their children. |
| `src/build.sh` | Builds gcc for the triple it is given and lays out its children. It then checks that only the compiler's programs are outside its versioned directories, that every program and library is the target's code, and that C and C++, static and dynamic, compile and run with it installed. It also reports what each child loads. |
| `import.sh` | Archives the tarballs and creates the object with `../../bootstrap/wrapper.sh`. |

## Running it

Import what the targets build from: `../build-base/`, and
live-bootstrap's six passes and linux-filesystem for `x86`, the 64-bit
bootstrap through stage 3 for `x86-64`. Then:

```sh
packages/gcc-7.5.0/import.sh <your actor id>
kairo build object:<object id> --target x86 --actor <your actor id>
kairo build object:<object id> --target x86-64 --actor <your actor id>
```
