# gcc

gcc, with C and C++, for musl, as a package of its own: built on
finished packages, not as a step of a bootstrap (`specs/DECISIONS.md`
§55). One object holds every release Kairo keeps, each a version tag on
its history, so that a build can choose its compiler by version, and
each later gcc is built with an older one, an older generation of the
same object (§59).

| Release | Tag | Built with | Recipe |
|---|---|---|---|
| gcc 7.5.0 | `7.5.0` | the bootstraps' gcc 15, through `bootstrap-base` | the object's revision at its tag, this directory before 15.2.0 |
| gcc 15.2.0 | `15.2.0` | gcc 7.5.0, this object's own release, with the rest from `bootstrap-base` | this directory |

The object has a target for each environment it is built for, and no
environment of its own: each target's artifact runs where it was built
(§53 sub-decision 1).

| Target | Runs in, built in | Triple |
|---|---|---|
| `x86` | `linux/x86` | `i686-pc-linux-musl` |
| `x86-64` | `linux/x86-64` | `x86_64-pc-linux-musl` |

Neither builds on a whole root (§56). Each depends on
`data:bootstrap-base` (`../bootstrap-base/`, §57, §59), which brings a
shell, the GNU userland, make, the archivers, m4 for gmp, binutils, the
runtime libraries the compilers' programs load, musl, static too for
the checks, and the kernel's headers, each the bootstraps' package for
the target's environment; and 15.2.0's targets ask for `tool:gcc` and
`tool:g++` `^7`, which only gcc 7.5.0 here offers, and which replace
`bootstrap-base`'s gcc. Only releases are offered by label (§61), and a
build of this object may use its own offers only from a tagged
ancestor of its revision, so 15.2.0 takes 7.5.0 at its tag and 7.5.0
never takes 15.2.0.

Each gcc bootstraps: the gcc it is built with builds its first stage,
which builds the second, which builds the third, and the last two are
compared. What the artifact holds is that gcc's own work, whatever gcc
built it. gcc 7.5.0's first stage, built by gcc 15, is built as C17 and
C++98, with `-fpermissive`, since its sources predate what gcc 15
rejects, and one line of it initializes a `bool` with `NULL`, which
musl defines as `nullptr` in C++11 and later; it is changed to `false`,
as gcc 8 has it.

## Children

Each target's artifact is installed nowhere (`install = false`); it is
the target's children side by side (§56).

| Child | Where | Offers |
|---|---|---|
| `gcc-<version>` | `/usr/bin` and `<libdir>/gcc/<triple>/<version>/`, `/usr/libexec/gcc/<triple>/<version>/` | `tool:gcc`, `tool:g++`, their triple labels (`i686-linux-musl`, `x86_64-linux-musl`), `lib:stdc++:static` |
| `libstdc++-<version>`, `libgcc_s-<version>`, `libgomp-<version>`, `libatomic-<version>`, `libitm-<version>`, `libquadmath-<version>` | `<libdir>` | `lib:stdc++:shared`, `lib:gcc:shared`, `lib:gomp:shared`, `lib:atomic:shared`, `lib:itm:shared`, `lib:quadmath:shared`, each conferring itself `>=<version>` |

The two targets have the same children and offer the same labels, each
to builds in its own environment. `<libdir>` is the environment's
directory for musl's libraries (§60): `/usr/lib/i686-unknown-linux-musl`
in x86, `/usr/lib/x86_64-linux-musl` in x86-64. gcc is configured with
it, so it finds musl there; on x86-64 its multilib directory for the OS
is the directory itself, not `lib64`.

**The compiler sits beside another gcc.** A consumer's `/` is an
overlay of its dependencies, the first listed on top (§41).

- **Its own directories:** its runtime libraries and C++ headers are in
  its versioned directory, `<libdir>/gcc/<triple>/<version>/`
  (`--enable-version-specific-runtime-libs`). It links against them
  there, so a program records that gcc's symbol versions.
- **libgcc_s moved:** where libgcc installs `libgcc_s` one directory
  up, in `<libdir>/gcc/<triple>/lib/`, which no version names, the
  recipe moves it into the versioned directory.
- **libcc1 left out:** it would go in `<libdir>` under no version, so it
  isn't built.
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
library's file and its soname's link, in `<libdir>`, where the loader
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
| `src/build.sh` | Builds gcc for the triple it is given, with `../common/recipe.sh`, which `import.sh` puts beside it, and lays out its children. It then checks that only the compiler's programs are outside its versioned directories, that every program and library is the target's code, and that C and C++, static and dynamic, compile and run with it installed. It also reports what each child loads. |
| `import.sh` | Archives the tarballs, creates the object or its next revision with `../common/import.sh`, and tags it with the release's version. |

## Running it

Import what the targets build with: `../bootstrap-base/`, and
live-bootstrap's six passes and linux-filesystem for `x86`, the 64-bit
bootstrap through stage 3 for `x86-64`. Then import 7.5.0 from this
directory as it was at that release, build it, and import this
directory, 15.2.0, as its next revision:

```sh
git show <7.5.0's commit>:packages/gcc/...   # or check that commit out
packages/gcc/import.sh <your actor id>                       # at 7.5.0
kairo build object:<object id> --target x86 --actor <your actor id>
kairo build object:<object id> --target x86-64 --actor <your actor id>
packages/gcc/import.sh <your actor id> --wrapper <object id> # 15.2.0
kairo build object:<object id> --target x86 --actor <your actor id>
kairo build object:<object id> --target x86-64 --actor <your actor id>
```
