# Changelog

All notable changes to Kairo are recorded here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), with one
addition: a **`Wire format`** section calls out changes to canonical
bytes (which determine `ActorId` / `ObjectId` / `StatementId`). See
[`VERSIONING.md`](./VERSIONING.md) for the policy.

This project adheres to pre-1.0 [Cargo semver](https://doc.rust-lang.org/cargo/reference/semver.html):
`0.x.0` for breaking changes (Rust API or wire format), `0.x.y` for
additions and fixes.

This file was created mid-flight — entries before its introduction are
reconstructed from git history and grouped at coarse-grained slice level
rather than per-commit.

## [Unreleased]

### Added

- **OpenSSL, SQLite, and ca-certificates as packages, and Python with
  them.** OpenSSL 3.5.9, the long-term release, its configuration in
  `/etc/ssl`; SQLite 3.54.1 with FTS3, FTS4, FTS5, and R*Tree, its soname
  `libsqlite3.so.0` as distributions keep it; and the
  certificate authorities NSS 3.119.1 trusts for servers, made a bundle
  by curl's `mk-ca-bundle.pl`, at `/etc/ssl/cert.pem` where OpenSSL
  looks. Each is one object built for x86 and x86-64 with `build-base`.
  Python 3.11.1's next revision, tagged `3.11.1` again, builds against
  them and has `ssl`, `hashlib`'s OpenSSL digests, and `sqlite3`. A
  package's child may now hold only `etc/`.

- **Plans are recorded and reused (`DECISIONS.md` §62).** Each plan is
  recorded in the store's `cache/plans/` with fingerprints of what it
  read: each label's release offers in its architecture, the objects it
  named, the tiers of the owners it ranked, what it found held, the
  host, and the resolver's version. A later resolution reuses it while
  all of them match, and its providers' too, so a new release replans
  only the plans that looked at its label. A reused plan, its
  prerequisites, and its alternatives are what planning would give;
  `kairo build --replan` plans afresh. Against the canonical store a
  plan with `build-base` takes 0.13 s reused, 0.45 s planned.
  `scripts/plan-timing.sh` times both.

- **The base's round 2, built with round 1 (`DECISIONS.md` §64).** A
  provides dependency may name the snapshots whose offers alone satisfy
  it, `offered_at`. `packages/base-round/`, `data:base-round` 1.0.0,
  brings what `build-base` does, each label at the snapshot of the
  base's first round. The base's packages' next revisions, tagged
  `<version>+2`, build with it, gcc 15.2.0 with its own round 1, so no
  bootstrap's object is in their plans; through `build-base`, a round
  that builds with itself would be a cycle. Round 3, built with round 2
  through `base-round` 2.0.0 and tagged `<version>+3`, gives round 2's
  artifacts byte for byte in both environments, and in x86-64 round 2
  gives round 1's.

- **zlib, libffi, perl, and Python as packages, built with packages
  alone (`DECISIONS.md` §63).** zlib 1.3.1, libffi 3.5.2, perl 5.42.0,
  and Python 3.11.1, each one object built for x86 and x86-64 with
  `build-base`, Python against the zlib, libffi, xz, and bzip2
  packages: no bootstrap's object is in their plans. Only the packages
  `build-base` names build their first generation with
  `bootstrap-base`.

- **The base packages (`packages/`).** bash, coreutils, sed, grep, gawk,
  diffutils, findutils, patch, tar, gzip, xz, bzip2, m4, and the Linux
  4.14 headers, each one object built for x86 and x86-64 as a first
  generation with `bootstrap-base`, beside make, binutils, and musl;
  and gcc 15.2.0, the `gcc` object's next release, built with its own
  gcc 7.5.0 (`DECISIONS.md` §59). Every package `build-base` names is
  now a release, so `build-base` resolves, in both environments, to the
  packages alone (§61). What every package's recipe and import script
  share is in `packages/common/`.

- **Libraries in a directory named by the target (`DECISIONS.md`
  §60).** Every library built for musl goes in the environment's own
  directory, `/usr/lib/i686-unknown-linux-musl` in x86, as
  live-bootstrap already has it, and `/usr/lib/x86_64-linux-musl` in
  x86-64, which musl's loader alone searches, so that another C
  library's can sit beside them. The 64-bit root moves there, and gcc
  7.5.0's runtime children with it; `linux-v1.md` records the
  directories as a convention packages follow.

- **One object per software, generations, and `offered_by`
  (`DECISIONS.md` §59).** A package is one object whose releases are
  version tags; tagged snapshots offer their labels beside the head. A
  build uses its own object's offers only from an older generation, a
  tagged ancestor of its revision, or from another target of its
  snapshot. A provides dependency may name the objects whose offers
  alone satisfy it. `packages/bootstrap-base/` brings build-base's
  labels offered by the bootstraps alone, for a package's first
  generation; `packages/make`, `packages/binutils`, and `packages/musl`
  are the first, and gcc 7.5.0 becomes `packages/gcc`, tagged 7.5.0.

- **live-bootstrap's gcc in parts, and what its children load
  (`DECISIONS.md` §58).** Every later pass's packaging splits each gcc
  with shared runtime libraries, as it does musl: gcc 10.5.0 and 15.2.0
  are now a compiler child and a child for each of `libstdc++`,
  `libgcc_s`, `libgomp`, `libatomic`, `libitm`, and `libquadmath`, each
  conferring itself `>=` its version. The packaging also reports what
  each child loads, and `children.py` writes each child's `requires`
  from it (pass 6's m4 now requires `libatomic`). Passes 2 to 6 and
  everything built on them rebuild.

- **An object that doesn't build brings its dependencies
  (`DECISIONS.md` §57).** A build that depends on an object without a
  build, by label or by id, takes that object's `[[dependencies]]` as
  its own requirements, after it; a label the build names itself
  replaces a brought one, and such objects nest. So a shared build
  environment is an object holding only a list of dependencies:
  `packages/build-base/` (`data:build-base`) brings a shell, the GNU
  userland, make, the archivers, m4, binutils, gcc with C++ and its
  runtime libraries, musl, and the kernel's headers, and serves x86 and
  x86-64 builds alike. gcc 7.5.0 builds with it. `kairo build` shows
  each brought requirement and what brought it.

- **No whole roots: every package a child, runtime libraries apart
  (`DECISIONS.md` §56).** The 64-bit bootstrap's stage 3 ships each of
  its packages as a child object with labels of its own, and its whole
  artifact is installed nowhere; `data:amd64-bootstrap:x86-64:root` is
  withdrawn, and stage 4 builds on the children. A gcc's shared runtime
  libraries (`libstdc++`, `libgcc_s`, `libgomp`, `libatomic`, `libitm`,
  `libquadmath`) are children apart from its compiler, in `/usr/lib`,
  each conferring itself `>=` its version, so one copy, the newest,
  serves an environment. gcc 7.5.0 builds from packages by label and
  offers its own runtime libraries the same way. The children say what
  they load, which their recipes report.

- **Packages, starting with gcc 7.5.0 (`DECISIONS.md` §55,
  `packages/`).** Software kept as artifacts apart from the bootstraps,
  one object per release, built on a finished root. gcc 7.5.0, with C
  and C++, for musl, has an `x86` target built on live-bootstrap's root
  and an `x86-64` target built on the 64-bit bootstrap's, each offering
  `tool:gcc` 7.5.0 to builds in its own environment. It bootstraps
  itself, and keeps all but its programs in directories named by its
  version, so it sits beside the root's gcc.

- **Artifacts keep modes, directories, and hard links (`DECISIONS.md`
  §54).** An artifact tree records a file's permission bits where they
  are neither `0644` nor `0755`, directories that are empty or not
  `0755`, and files that are one inode as hard links to the first.
  Materializing applies every bit but setuid and setgid, a directory's
  once everything is placed in it. A build's command starts with the
  umask `022`. An object's own Git tree keeps only the executable bit.
  Kairo restores write permission before removing a tree, so a
  read-only directory no longer strands a workspace.

- **The 64-bit bootstrap (`bootstrap/amd64/`).** From live-bootstrap's
  finished x86 root, without any 64-bit binary from outside: an
  x86_64-linux-musl cross toolchain (stage 1); a minimal x86_64 root
  cross-built under x86 (stage 2); the root built natively in x86-64,
  gcc bootstrapping itself, with zlib, libffi, perl 5.42.0, and Python
  3.11.1 (stage 3); and its fixed point, the same build on stage 3's
  root, which makes stage 3's artifact again (stage 4).

- **What an object runs in apart from what it is built in, and target
  triples (`DECISIONS.md` §53).** An object's top-level environment
  dependency says what its artifacts run in, and a build target may name
  its own build environment, so a 64-bit program can be built by a
  32-bit environment and be offered to 64-bit builds. Compilers and
  binutils carry their target triple (`tool:gcc:i686-linux-musl`), and
  only native ones the plain label; live-bootstrap's gcc and binutils
  children gain theirs, and gcc requires binutils by triple.

- **live-bootstrap pass 6: gcc 15.2.0 and the end (`DECISIONS.md`
  §49).** `bootstrap/live-bootstrap-gcc-15.2.0/` goes on from pass 5's
  root through gcc 15.2.0, bootstrapped with C++, and libmd, libbsd,
  shadow, opendoas, gzip, diffutils, gawk, m4, and libarchive, to the end
  of upstream's CI pass 2: all 149 packages built and checked against
  upstream. The end-to-end test's timeout is 8 hours.

- **live-bootstrap pass 5: gcc 10.5.0 (`DECISIONS.md` §49).**
  `bootstrap/live-bootstrap-gcc-10.5.0/` goes on from pass 4's root to
  gcc 10.5.0 with C++ and binutils 2.41 rebuilt; its gcc child also
  provides `lib:stdc++:shared` and `lib:gcc:shared`, each conferring
  itself `>=10.5.0`, and `children.py` keeps such self-conferrals in one
  table, musl's shared part's among them.

- **live-bootstrap's later passes run upstream's scripts in upstream's
  environment (`DECISIONS.md` §49).** The shared start runs the
  generated script under `env -i PATH=/usr/bin`, as upstream's
  `rootfs.py` does, so musl's locale is `C.UTF-8` rather than the
  guest's `LANG=C`, which python 3.1's codec generation can't write.

- **live-bootstrap pass 4: python and guile (`DECISIONS.md` §49).**
  `bootstrap/live-bootstrap-python/` goes on from pass 3's root through
  upstream's python chain, python 2.0.1 to 3.11.1, with gperf, gettext,
  texinfo, and libffi, then libunistring, the Boehm GC, guile 3.0.11,
  which, grep 3.12, sed 4.8, and autogen 5.18.16; its 23 packages are
  checked against upstream and are its children.

- **Artifacts found by what a build consumes (`DECISIONS.md` §52).** A
  plan carries a `build_key`, the hash of what its guest sees and runs;
  a held build with the same key, at any snapshot, is its artifact, so
  edits to summaries, tags, children, and what an artifact requires or
  confers no longer rebuild it. `kairo build` says `status = held` for a
  target whose key is held. A target may set `source_date_epoch`, and
  `tree`, the part of its object's tree that is `/kairo/src`; a key
  covers only that part, and a content object only what is under its
  root, so edits beside them, to a README, change no key.

- **Conferring from what a build chooses, and `one_of` (`DECISIONS.md`
  §51).** Only a build's direct dependencies confer runtime
  requirements on its artifact; a label conferring itself without a
  version is pinned to the offer the build resolved, and a conferral of
  another label must give a range. A provides dependency takes
  `confer = true` or `false`. A `requires` entry may be `one_of`
  several labels, an obligation on whatever builds with the artifact to
  request one itself, refused at plan time when it doesn't.
  live-bootstrap's musl is split into headers, static, and shared
  children; gcc requires one of static or shared musl and confers
  nothing; the hello consumer has a dynamic and a static target.

- **An object that doesn't build is its own artifact (`DECISIONS.md`
  §50).** `[content]` takes `root` (the part of the tree that is its
  artifact) and `install` (where it is installed when required); a
  required content object with `install` is installed as a built
  artifact is, links and all, including through a runtime closure. A
  content object without them is unchanged.

- **live-bootstrap pass 3: to gcc 4.7.4 (`DECISIONS.md` §49).**
  `bootstrap/live-bootstrap-gcc-4.7.4/` goes on from pass 2's root
  through openssl 3.6.0, ca-certificates, curl 8.17.0, and patch 2.8 to
  gcc 4.7.4 with C++, then binutils 2.41 and musl 1.2.5 rebuilt; its
  gcc child provides `tool:gcc`, `tool:g++`, and `lib:stdc++`. Every
  pass after the first shares `bootstrap/live-bootstrap/pass/start.sh`
  and `package.sh`, which now ship `/etc` too. The hello consumer
  prints the version of the gcc that built it.

- **live-bootstrap pass 2: the perl chain (`DECISIONS.md` §49).**
  `bootstrap/live-bootstrap-perl/` goes on from pass 1's whole root,
  now provided as `data:live-bootstrap:x86:zlib-1.3.1`, through
  upstream's perl chain to perl 5.42.0, with automake 1.16.3, autoconf
  2.71, and bison 3.8.2, each package checked against upstream and a
  child of its own. Pass 1 ships the state a later pass needs in
  `state/`; `children.py` lowercases child names.

- **Owner preference among offers (`DECISIONS.md` §48).** The resolver
  ranks offers by owner before version: actors named by `kairo build
  --prefer` or `[resolve] prefer`, then this node's own actors, then
  those they trust at `general` scope, then everyone else. Among equal
  offers the most recently published wins before object id. Plans list
  each losing offer's owner, tier, and why it lost; `kairo provider
  resolve` ranks the same way and `kairo search` shows owners.

- **`kairo store verify` (`DECISIONS.md` §47).** Re-hashes every
  record and blob in the store, and the build's execution records,
  against the id each is filed under, lists every one that doesn't
  match, and exits 10 if any don't. It repairs nothing; run it when the
  disk is in doubt or on a schedule.

- **Stalled source fetches are retried (`DECISIONS.md` §40).** `kairo
  source fetch` runs a fetch of a source or a submodule's pinned
  commits again, up to three attempts, when git aborts the transfer
  itself (too slow, dropped, ended early), before counting it failed
  or trying a mirror; wrong requests and the size and time limits fail
  at once.

- **Runtime requirements and what a build confers (`DECISIONS.md`
  §46).** A target or child declares `requires`, what its artifact
  needs beside it wherever it is installed, and a provides entry
  declares `confers`, what a build that uses the label gets as a
  runtime requirement of its own artifact, pinned to the provider's
  build or given as a range. The resolver records an artifact's runtime
  requirements in the plan that builds it, reads them back for
  consumers, gathers every requirement on a label across the closure,
  chooses one offer per label satisfying all of them (re-choosing a
  declared requirement when the closure asks more of it), installs more
  than one only where the providers declare `coexists`, and otherwise
  refuses the plan naming each requirement's chain. Plans gain
  `runtime` and `requires`; the executor installs the runtime closure
  and refuses co-existing providers that collide on a path.

- **Child versions, `kairo search`, and children over federation
  (`DECISIONS.md` §44, slice 2b).** A child declares its own
  `version`, which its labels take unless they give theirs, and a
  child depended on by id is chosen by that version among its parent's
  current and version-tagged snapshots, so one only an older tagged or
  pinned revision declares still resolves. `kairo search <query>`
  finds held objects and their children by name, summary, tags, and
  `tag:`, `provides:<label>`, and `provides:<kind>:*` filters, with no
  index yet. `kairo fetch` and `kairo federation publish`, `unpublish`,
  `follow`, and `unfollow` given a child's id act on its parent.
  A node answers `GET /federation/v1/children/{id}` for the children of
  what it publishes (§45), so `kairo fetch --from` of a child whose
  parent this node has never held asks the peer, checks that the id
  derives from the answer, and pulls the parent.

- **Re-importing a bootstrap wrapper extends it.** The stage0-posix and
  live-bootstrap `import.sh` take `--wrapper <object id>`: the new
  commit's parent is that object's current one and its revision names
  it, so a changed wrapper becomes the existing object's next revision
  instead of a second object offering the same labels; an unchanged
  one adds nothing. Both share `bootstrap/wrapper.sh`.

- **The highest version by default (`DECISIONS.md` §43).** Among the
  offers that satisfy a requirement the resolver takes the highest
  version, preferring those that satisfy every requirement the target
  makes of the label; an object dependency with neither `version` nor
  `snapshot` takes the object's highest version tag instead of being
  refused.

- **live-bootstrap's packages as child objects (`DECISIONS.md` §41
  sub-decision 6, slice 4).** The wrapper extracts each of its 77
  packages into `/kairo/package/<package>/`, and each is a child
  installed at `/`; the last revision of each package provides its
  label and version (`tool:bash` 5.2.15, `tool:gcc` 4.0.4, `lib:musl`
  1.2.5, ...), moved from the parent, which keeps only what the
  kaem-run steps built. `children.py` writes the entries.

- **Overlay provisioning (`DECISIONS.md` §41 sub-decision 5, slice 3).**
  Where bubblewrap has `--overlay-src` (0.11 and later), each
  installed dependency is a read-only layer cached under
  `<store>/cache/layers/` and stacked under the build's writable `/`,
  the first dependency on top; older engines copy, as before, and give
  the same tree. Execution records and `kairo build` say which was
  used (`provisioning`). Engine tests take `KAIRO_TEST_BWRAP` to name
  another bubblewrap.

- **Label kinds, tags, and child summaries (`DECISIONS.md` §42).** A
  provides label's kind must be `tool`, `lib`, `doc`, `data`, or
  `environment`, checked wherever a manifest declares or requires one,
  so a misspelt kind fails at parse time. `[kairo]` takes `tags`, and
  each `[[build.targets.children]]` entry `summary` and `tags`, for
  search; manifests without them keep their hashes.

- **Child objects (`DECISIONS.md` §41 sub-decision 4, slice 2a).** A
  build target declares `[[build.targets.children]]`, each a `name`, a
  `root` in its artifact, an install root, and provides labels. A
  child's id derives from `(parent, target, name)`
  (`schemas/canonical/child-genesis-v1.md`); consumers depend on it by
  id or by label, the plan pins the parent's artifact and the child's
  `root`, and the guest gets only that part, at the child's install
  root. Execution records list each child with the id of its slice of
  the artifact, and `kairo build` prints them.

- **One Linux contract, package roots, and install roots (`DECISIONS.md`
  §41, slice 1).** `environment:linux/<arch>` now has the guest root in
  its namespaces with a writable `/` holding each dependency's artifact
  at its install root (copied, in requirement order), the source tree at
  `/kairo/src`, and an empty `/kairo/package` where the build stages its
  artifact; `linux-root` is folded in and retired. Targets declare
  `[build.targets.artifact] root` and `install` (`/` by default, or
  `false` for `/kairo/artifact/<id>`), outputs are optional, provides may
  name a target's whole artifact, and placement paths may be absolute.
  The stage0-posix and live-bootstrap wrappers move onto it.

- **An empty, read-only `/sys` under `linux-root` (`DECISIONS.md` §39
  sub-decision 6).** Scripts that mount what isn't mounted, such as
  live-bootstrap's `finalize_fhs`, no longer mount the host's sysfs.

- **live-bootstrap into its CI pass 2 (`DECISIONS.md` §38
  sub-decision 16).** The x86 wrapper continues through
  `finalize_fhs`, bash 5.2, curl, xz, tar 1.34, coreutils 9.4, gmp,
  mpfr, mpc, and zlib; all 77 packages match `SHA256SUMS.pkgs`.

- **Mirrors for source fetches (`DECISIONS.md` §40).** `kairo source
  fetch --mirror <source url>=<mirror url>` fetches a source, or any of
  its submodules, through the mirror when its own URL fails, filing it
  under its own source id; the observation records `via` the mirror.
  The stage0-posix import names GitHub's mirror of mescc-tools, for
  when Savannah is unreachable.

- **live-bootstrap through its CI pass 1 (`DECISIONS.md` §38
  sub-decision 15).** The x86 wrapper continues through musl 1.2.5,
  the Linux headers, gcc 4.0.4's second build, util-linux, e2fsprogs,
  and the base tools upstream's first CI pass ends with; all 60
  packages match `SHA256SUMS.pkgs`. Relative link targets may now have
  empty segments (a trailing `/`), which are ignored.

- **live-bootstrap through its first gcc (`DECISIONS.md` §38
  sub-decision 14).** The x86 wrapper continues through the autotools
  chain, binutils 2.30, and gcc 4.0.4; all 49 packages match
  `SHA256SUMS.pkgs`. `import.sh` falls back to `ftp.gnu.org` for files
  on kernel.org's GNU mirror.

- **live-bootstrap through perl 5.6.2 (`DECISIONS.md` §38 sub-decision
  13).** The x86 wrapper continues through m4, flex, bison, diffutils,
  coreutils 6.10, gawk, and the perl chain, with live-bootstrap's own
  `uninstall:` steps; all 25 packages match `SHA256SUMS.pkgs`.

- **live-bootstrap through its first musl toolchain (`DECISIONS.md` §38
  sub-decision 12).** After bash 2.05b the wrapper runs live-bootstrap's
  first bash-run steps: tcc and musl 1.1.24 rebuilt against each other,
  grep, sed, and bzip2, every package matching `SHA256SUMS.pkgs`. The
  artifact gains a `repo` output of those packages, and `lib:musl` and
  `tool:grep` provides.

- **Symbolic links in artifacts (`DECISIONS.md` §39).** Outputs keep
  symbolic links as link entries (`{ path, link }`) instead of failing
  the build. A relative target must stay inside the artifact; an
  absolute one is the guest's path, kept as written; escaping relative
  targets and entries under a link are refused.
  Links are recorded without being followed and materialized as links.
  Artifacts without links keep their ids. `kairo build` lists a link as
  `<path> -> <target>`.
- **Root in `linux-root` (`DECISIONS.md` §39).** The guest runs as uid
  and gid 0 with every capability inside its own namespaces, still
  unprivileged on the host: chown to 0 and mounts work, `mknod` of
  devices doesn't. The `linux` contract keeps uid 1000.

- **live-bootstrap through bash-2.05b (`DECISIONS.md` §38 sub-decision
  11).** The x86 wrapper now runs every step live-bootstrap builds with
  kaem: after tcc, make, patch, gzip, tar, sed, bzip2, coreutils, yacc,
  and bash 2.05b, each passing upstream's checksums, with nine more
  distfiles pinned. It provides `tool:make`, `tool:bash`, and the rest.

- **Pushes carry sources (`DECISIONS.md` §38 sub-decision 10).** A Git
  or file source can be pushed like any object, and pushing a native
  object also carries the sources its revisions pin (a Git source at
  only the pinned commits, a file source whole), inside the same
  closure bundle under `sources/<id>/`. The receiver keeps only those
  its own copy of the manifests pins. `kairo push` output gains
  `sources=`.

- **`max_file_bytes` and `file_timeout_secs`** in `[federation.limits]`
  cap `kairo source fetch-file` downloads and file sources' bytes in
  pulls (1 GiB and 30 minutes by default), which reused the Git limits
  before.

- **File sources federate (`DECISIONS.md` §38 sub-decision 9).** A held
  file source can be published; its closure is its observations and its
  bytes, and its genesis route serves the `FileSourceGenesis`. Pulls
  re-derive the id from the hash, verify observations under their
  signers, and check the bytes against the hash within the Git size
  limit. `kairo source pull <sha256>` pulls one; publishing a native
  object also serves the file sources it pins, and pulling it brings
  them, so the live-bootstrap wrapper can be pulled and built.

- **live-bootstrap through tcc-0.9.27 for x86 (`DECISIONS.md` §38).**
  `bootstrap/live-bootstrap/` is a native wrapper that runs
  live-bootstrap `b1ceced7`'s own scripts, unmodified, under
  `environment:linux-root/x86` from the stage0-posix x86 artifact
  through Mes 0.27.1 to tcc 0.9.27, every step passing upstream's own
  checksums; live-bootstrap and M2libc are pinned as sources, the four
  distfiles as file sources. The stage0-posix wrapper gains an `x86`
  target. On an x86-64 host whose kernel runs i386 (probed in a
  sandbox), bubblewrap also implements `environment:linux/x86` and
  `environment:linux-root/x86`. Artifact offers are now candidates only
  when built for the consumer's architecture (sub-decision 8).

- **Placement options and the `linux-root` contract (`DECISIONS.md`
  §38 sub-decisions 4–5).** A dependency's `path` placement can take
  `from = "<subpath>"` (only that subtree) and `overlay = true` (fill
  in around the native tree's files, which win), and provides and
  object dependencies can now be placed too, copying the artifact or
  tree they resolve to. `environment:linux-root/<arch>`
  (`schemas/environment/linux-root-v1.md`) runs the guest with the
  build tree as `/`, `/proc`, `/dev`, and `/tmp` mounted over it, and
  nothing from `/kairo` or the host; every dependency must be placed.
  Bubblewrap implements it alongside `linux`.

- **File sources (`DECISIONS.md` §38 sub-decisions 1–3).** A single
  file, a distfile or release tarball, is a source object identified
  by the SHA-256 of its bytes: an unsigned `FileSourceGenesis` holding
  the hash, the bytes as the blob that hash names, and signed
  `FileObservation` statements of where each download came from, so
  mirrors are observations of one source. `kairo source fetch-file
  <url> [--sha256 <hex>]` downloads (`https`, one redirect), refuses a
  mismatched hash, and signs the observation; `kairo source show` and
  `kairo source id` take a hash. Manifests depend on one with
  `kind = "file"`, `sha256`, and `path`; the build places the file
  there, checked against its hash, and refuses a placement the tree
  already fills or reaches through a symlink. File observations don't
  federate yet.

- **Pulling a native object brings the sources it pins (`DECISIONS.md`
  §37 sub-decision 9).** Publishing a native object also serves each
  source its revisions pin, with that source's observations and only
  the pinned commits' history; the listing names them in a new
  optional `sources` field. `kairo fetch` then pulls each one,
  requiring exactly the commits its own copy of the manifests pins, so
  a pulled wrapper can plan and build. Output gains `sources = N`.
- **Sources federate (`DECISIONS.md` §37 sub-decision 9).** A source
  can be published like any object; its closure is its observations
  and every object they name. `kairo source pull <url> --from <peer>`
  (or `kairo fetch --object <source id>`) pulls it under the same id:
  the unsigned genesis is checked against its URL, each observation is
  verified under its signer (never re-signed), and the named history,
  annotated tags included, is pinned as observed.
- **The stage0-posix bootstrap is rebuilt on source objects
  (`DECISIONS.md` §37 sub-decision 8).** `import.sh` now archives
  stage0-posix and every submodule it pins from their real upstream
  URLs (`kairo source fetch --require-complete`) and creates one native
  wrapper with SHA-256 history that pins each source the AMD64 chain
  uses at its Release_1.9.1 commit and path. The build produces the
  same artifact as before. Placed sources are now writable copies made
  fresh per build, and placements may nest. Source fetches follow an
  initial redirect (Savannah needs it); federation fetches still follow
  none. The `seed:*` labels and object-form `[[runtime.seeds]]` are
  retired: seeds are `(source, revision)` pairs. The ignored
  `bootstrap_stage0` test now needs the network instead of a checkout.
- **Builds depend on sources by revision (`DECISIONS.md` §37
  sub-decisions 6–8).** A manifest dependency `kind = "source"` names a
  source by URL (hashed canonical) at an exact upstream `revision`, with
  optional `path` (placed read-only at that path in the source tree, so
  upstream layouts build unchanged) and `seed = true`. The resolver
  requires the exact commit to be held; seeds resolve only to `(source,
  revision)` pairs listed in `[[runtime.seeds]]`. Without a path, a
  source is at `/kairo/source/<id>/<oid>` in the contract.
- **Submodules are archived as their own sources (`DECISIONS.md` §37
  sub-decision 5).** `kairo source fetch` follows the submodules its
  observed commits pin, recursively (`--max-depth`, default 8;
  `--no-recurse-submodules` to skip), observing each submodule source
  once per level; a pinned commit the submodule's refs no longer reach
  is fetched by id and listed in the new optional `pinned` field of its
  observation. Unresolvable or unservable links are reported (`complete
  = no`); `--require-complete` fails on them.
- **`kairo source fetch` and `kairo source show` (`DECISIONS.md` §37).**
  Fetching a source brings every branch and tag into the Git cache
  through a quarantine, pins each fetched object as
  `refs/kairo/observed/<oid>` so nothing seen is ever dropped, and signs
  a `SourceObservation` of exactly what was fetched. A refetch reports
  what upstream changed (moved, added, deleted refs) while still holding
  everything. Annotated tags are recorded as their tag objects (the
  observation field is `target`). Sources may be `git://` as well as
  `https`. `kairo-test-support` gains a `git daemon` fixture.
- **Source objects, step one (`DECISIONS.md` §37).** Source geneses
  (unsigned, the object id derived from a canonical repository URL) and
  `SourceObservation` statements (every ref one fetch saw) with
  canonical and JSON schemas, a `sources/` store namespace, and a
  per-source observation index. `kairo source id <url>` prints a URL's
  canonical form and the source id every node derives. Fetching
  sources is the next step; federation refuses observations until
  sources federate.
- **`kairo git fetch` takes the remote's object format
  (`DECISIONS.md` §19).** It reads the remote branch's format too, so a
  new object fetching an existing repository needs no
  `--object-format`, and refuses a flag, cache repo, or revisions that
  disagree with it. The output says which source decided
  (`object_format = sha1 (the remote)`). History born in the archive is
  SHA-256 by default.
- **The stage0-posix bootstrap (`DECISIONS.md` §36).**
  `bootstrap/stage0-posix/import.sh` imports stage0-posix Release_1.9.1
  as a seed object (`seed:stage0-posix/amd64`) and a sources object
  whose `amd64` target builds M2-Planet, mescc-tools and
  mescc-tools-extra from 847 bytes of seed under the bubblewrap engine,
  with upstream's `amd64.answers` checked in the build. New
  `[[runtime.seeds]]` config: only listed seed objects satisfy `seed:*`
  labels, and only through their own content. The full chain is the
  ignored test `bootstrap_stage0` (needs `KAIRO_STAGE0_DIR`).
- **The bubblewrap engine (`DECISIONS.md` §35).** A target that
  requires `environment:linux/<arch>` builds under bubblewrap when it
  is allowlisted in `[[runtime.engines]]` and passes a probe: an empty
  guest root with only `/kairo` paths (source, providers read-only at
  canonical paths), `/proc`, a minimal `/dev`, and `/tmp`; no network;
  hostname `kairo`; uid 1000 (`schemas/environment/linux-v1.md`). Such
  builds need neither `allow_local_process` nor signer trust, carry
  `bubblewrap/1` and the contract in their `BuildResult`, and are not
  host-dependent. Host tools are refused under an engine. Content
  providers' `bin/` directories join `PATH`. Execution records gain the
  engine, kernel release, and known nondeterminism. Engine tests skip
  without bubblewrap and a static busybox unless `KAIRO_REQUIRE_ENGINE`
  is set.
- **`kairo reproduce` (`DECISIONS.md` §33 sub-decision 9).** `kairo
  reproduce statement:<id> --actor <id>` re-runs exactly the plan a
  `BuildResult` cites, signs its own result, and reports `matches` or
  `differs`; it refuses, listing them, when the plan's pinned inputs
  aren't held. `kairo bundle export --include-executions` carries the
  execution records behind an object's results, with their plans,
  artifacts and logs, fixity-checked on import, so another node can
  reproduce from the bundle alone. Execution records gain a random
  per-run id.
- **`BuildResult` statements (`DECISIONS.md` §33 sub-decision 8).**
  `kairo build --actor <id>` signs a `BuildResult` for every completed
  build (`schemas/canonical/build-result-v1.md`): object, snapshot,
  target, plan hash, outcome, artifact, executor, environment,
  `host_dependent`, and the execution record id. Results are indexed
  per object (`BuildResultResolver`), ride in bundles and federation
  closures, and are accepted by the daemon's statement endpoint. The
  resolver pins an artifact provider to a held artifact when an
  accepted signer's successful result names it, so `kairo build`
  builds a plan's prerequisites first, one at a time, then the target.
- **Development-mode builds (`DECISIONS.md` §33–§34).** `kairo build
  <snapshot-ref> --target <name>` runs the plan with the new
  `kairo-build` crate's native executor — a host process with a
  scrubbed environment and no isolation — behind `[runtime]
  allow_local_process = true` and a revision signer this node holds a
  key for or trusts (exit 7 otherwise). Declared outputs become a
  content-addressed artifact (`schemas/canonical/artifact-tree-v1.md`)
  and an unsigned execution record is written under
  `<store>/executions/`. `kairo-git` can export a commit's tree.
- **Build planning (`DECISIONS.md` §33–§34).** `kairo build
  <snapshot-ref> --target <name> --plan-only` resolves a target's
  requirements against the local store and host into a
  `kairo.resolved-manifest.v1` document (new `kairo-resolver` crate),
  stores it by plan hash along with the plans of the builds it needs
  first, and prints it. `[[provides]]` entries may name a `target` and
  `output` (artifact providers). `kairo provider list|show|resolve`
  show what held objects provide. `[runtime]` config:
  `allow_local_process`, `timeout_secs`, `[[runtime.host.tools]]`.
  Running the plan is the next slice.
- **Build targets in `kairo.toml` (`DECISIONS.md` §33).** Manifest
  schema 1 accepts `[[build.targets]]` (name, command, workdir,
  per-target dependencies, declared outputs) and keeps a provides
  dependency's `version` requirement, which it used to drop. Version
  requirements are checked against Cargo's syntax. `kairo manifest
  inspect` shows targets.

- **`git-remote-kairo` (`DECISIONS.md` §31).** With the new
  `git-remote-kairo` binary on `PATH`, `git clone
  kairo://<host>/<object-id>` pulls the object through the Kairo
  verifier into the user's store and serves Git from its Git cache:
  the creator's branches and tags as heads and tags, other actors'
  under `refs/kairo/actors/`, only refs whose signed revision's commit
  is held, in the object's Git format. It follows `kairo fetch`'s peer
  policy (`kairo.allowPeer` to override) and refuses `git push`. A
  later `git fetch` brings new signed revisions and sends only history
  the clone's refs don't already reach.

- **Read-triggered acquisition (`DECISIONS.md` §30, on-demand fetch
  slice 3).** With `[federation.on_demand] on_read = true`, a `404`
  from `GET /api/v1/objects/{id}` or `verify-object` queues an
  acquisition and says so in a `Kairo-Acquisition` header, and an
  incomplete `verify-object` result reports it in an `acquisition`
  field. Reads never wait.
- **Daemon acquisition (`DECISIONS.md` §30, on-demand fetch slice
  2).** `POST /api/v1/federation/fetch` queues an acquisition and
  answers at once (`queued`, `running`, `no_sources`, `disabled`); the
  attempt shows as a `fetch` run in federation status. One attempt per
  object at a time, within `max_concurrent`.
  `[federation.on_demand]` sets `on_read` (off) and `cooldown_secs`
  (300) for read-triggered acquisitions.
- **Acquisition (`DECISIONS.md` §30, on-demand fetch slice 1).**
  `kairo fetch --object <id>` without `--from` fetches the object from
  wherever it is held: the listed peers and vouched hinted holders,
  those that served it before first, stopping at the first that serves
  it.

- **Using hints (`DECISIONS.md` §29, discovery slice 4).** The daemon
  runs discovery every `interval_secs`. With `[federation.discovery]
  use_hints = true` and `vouched_by = "<actor>"`, sync also pulls a
  followed object from hinted holders that actor trusts with
  `federation:index` (directly or as the relay), after listed peers
  and with each holder's node actor pinned.
- **Relayed inventories (`DECISIONS.md` §29, discovery slice 3).**
  `GET /federation/v1/inventories` relays, one hop, the inventories a
  node fetched directly from nodes its actor trusts with
  `federation:index`, each verbatim with the node's actor export;
  `kairo federation discover` verifies and keeps them, recording the
  relaying node.
- **Discovery hints (`DECISIONS.md` §29, slice 2).**
  `kairo federation discover` fetches and verifies the inventory of
  every listed peer and `[[federation.bootstrap]]` node and keeps the
  newest per node in `<store>/federation/hints.json`;
  `kairo federation holders --object <id>` lists who has an object.
  `[federation.discovery]` sets `hint_ttl_secs` (a week) and
  `use_hints` (off). Hints grant nothing.
- **Signed inventories (`DECISIONS.md` §29, discovery slice 1).**
  `GET /federation/v1/inventory` serves the node's published objects,
  signed by its node actor when the published view is built
  (`schemas/canonical/inventory-v1.md`). Nodes advertise `inventory`.

- **Follows and the sync engine (`DECISIONS.md` §28, slice 1).**
  - `kairo federation follow | unfollow (--object <id> | --trust-by
    <actor> --scope <scope>)` keeps a list of what this node keeps
    current, in `<store>/federation/follows.json`.
  - `kairo federation sync` pulls every follow (or one) from every
    listed peer, reporting each peer as `fetched`, `not_published`, or
    `failed`; `kairo federation status` lists follows with their last
    attempt and success. The engine lives in the new
    `kairo-federation-service` crate, ready for the daemon.
  - `kairo trust forget` also unfollows the forgotten chains.
  - Inbox processing moved into the engine (`receive_inbox`), returning
    typed outcomes; `kairo federation receive` prints the same lines.
  - The daemon runs it (`DAEMON.md` §5.3): a scheduler re-reads the
    config each tick, processes the inbox every `inbox_secs`, and syncs
    due follows every `interval_secs`, at most `max_concurrent` at once
    (`[federation.sync]`, defaults 30 / 900 / 2).
    `GET /api/v1/federation/status` shows follows, the schedule, and
    recent runs; `POST /api/v1/federation/sync` starts follows now.
    New error code `409 federation_disabled`.

- **Trust transport and `forget` (`DECISIONS.md` §27).**
  - `kairo trust publish | unpublish --by <id> --scope <scope>` chooses
    which (truster, scope) chains a node serves, at
    `GET /federation/v1/trust` and `/federation/v1/trust/{by}?scope=`;
    nothing is served by default. Nodes advertise `trust`.
  - `kairo trust fetch --by <id> --scope <scope> --from <url>` pulls
    one chain through the pull verifier. Fetched opinions stay their
    truster's; `trust show | list | history` label them `imported`.
  - `kairo trust forget --by <id> [--scope <scope>]` deletes an
    imported truster's opinions, their index entries, and provenance;
    it refuses a truster with a local key.

- **Learning a node from a file (`DECISIONS.md` §26 sub-decision 7).**
  `kairo actor export --actor <id> --out <path>` writes an actor's
  genesis and every key event, exactly as stored
  (`kairo.fed.actor-export.v1`). `kairo federation learn --file
  <path> [--node-actor <id>]` verifies it like a pull and stores it,
  so a node that is never reachable, such as a laptop, can push.

- **Upload by push, sending only the delta (`DECISIONS.md` §26
  sub-decisions 6 and 8).**
  - `kairo push --object <id> --to <url>` sends the peer the object's
    closure inventory (`POST /federation/v1/push/plan`), learns what it
    lacks and which commits it has, and uploads a closure bundle of
    just that (`POST /federation/v1/push?object=<id>`), with Git
    history as a thin pack. Nothing is uploaded when the peer holds
    everything.
  - The receiver checks everything but the signature before reading
    the body, streams it into the inbox up to `max_push_bytes`
    (`413 fed_too_large`), and deletes it unless the signature over its
    SHA-256 verifies. New codes: `fed_bad_push`, `fed_too_large`. Nodes
    advertise `push`.
  - Closure bundles (`kairo.closure-bundle.v1`, a tar) are a new bundle
    type: a published closure, including authority and key events.
    `kairo federation receive` ingests them through the pull verifier,
    with provenance naming `push:<node>`.
  - Git ingest runs `index-pack --fix-thin`, so thin packs complete
    against the pool.

- **Push by announcement (`DECISIONS.md` §26).**
  - `POST /federation/v1/announce` takes a `KairoNode1`-signed list of
    objects with the announcer's base URL. The receiving node requires
    a `trusted` `federation:push` opinion of the announcer and a
    `[[federation.accept]]` entry for every object, then writes the
    announcement to `<store>/federation/inbox/`. New codes:
    `fed_not_trusted`, `fed_not_accepted`, `fed_bad_announcement`.
    Nodes advertise `announce`.
  - `config.toml` gains `[[federation.accept]]` (`object`, `from`,
    `publish`) and the limits `max_push_bytes` and
    `max_inbox_entries`. `kairo-federation-http` reads them at
    startup.
  - `kairo federation learn --from <url>` stores a peer's
    authenticated node actor, so its signed requests can be checked.
    `kairo federation announce` sends an announcement, and
    `kairo federation receive` pulls each accepted object with the
    announcer pinned, publishing it when the entry says so.

- **`KairoNode1` request signatures (`DECISIONS.md` §25 sub-decision
  3).** `kairo-federation-core` builds, signs, and parses the
  `Authorization: KairoNode1` header over the
  `kairo-federation-request-v1` signing string, which names the
  receiver's base URL. `Node::authenticate_request` verifies one: a
  known `node` actor, its key active and unrevoked at the timestamp,
  ±300 s, and a nonce accepted once per window. Failures are the new
  `401 fed_unauthenticated`. No endpoint requires it yet; push will.

### Wire format

- **A provides dependency gains `offered_by` (`DECISIONS.md` §59)**, in
  the manifest's canonical bytes and the plan's requested requirement,
  only when given, so manifests and plans without it keep their bytes.
- **A plan's requirement gains `brought_by` (`DECISIONS.md` §57)**,
  only on a requirement an object that doesn't build brought, so a plan
  with nothing brought keeps its bytes. Build keys don't read it.
- **Artifact trees gain `mode`, directory entries, and `hardlink`
  entries (`DECISIONS.md` §54)**, each only where it says something, so
  a tree with none of them keeps its bytes and id. Build keys gain
  `collect` (`2`), so every key changes and every held build is built
  once more.
- **Plans gain `build_key` and `source_date_epoch` (`DECISIONS.md`
  §52)**, and an artifact resolution its provider's `build_key`, so
  every plan hash changes and every held build is built once more. A
  target's `source_date_epoch` is encoded only when given.
- **`one_of` and `confer` (`DECISIONS.md` §51)** are encoded only where
  given: a `one_of` requirement as `string("one_of")` and its
  alternatives, a provides dependency's `confer` after its placement;
  plans carry `one_of` entries in `requires`. Manifests and plans
  without them keep their bytes and hashes.
- **`[content]` `root` and `install` (`DECISIONS.md` §50)** are encoded
  in a manifest's canonical bytes after `kind`, each only when given,
  and a plan's `content` resolution carries them only when the object
  declares them; manifests and plans without them keep their bytes and
  hashes.

### Changed

- **gcc 15.2.0 without multiarch, and with the stack protector's local
  helper.** The gcc package's next revision, tagged `15.2.0` again:
  - is configured with `--disable-multiarch` and checks that `gcc
    -print-multiarch` reports nothing. gcc had turned multiarch on in
    x86, where the bootstrap's root it was built on has
    `/usr/lib/libc.so`, and reported glibc's `i386-linux-gnu` for a
    musl target, which Python's configure refused.
  - builds libssp for its `libssp_nonshared.a` alone and links it
    whenever a stack protector is asked for, as Alpine's gcc does. On
    i386, protected position-independent code calls
    `__stack_chk_fail_local`, which musl keeps hidden in libc, so perl
    failed to link in x86. The build checks a protected PIE program.

- **An index makes every label's offers once.** `Index::offers` looked
  through every candidate's manifest and child for each label asked;
  the index now makes every label's offers when it is built, in their
  tie-break order, and a lookup is a lookup. The resolver counts the
  indexes it builds and the offers they make (`Resolver::index_work`),
  and a test holds a resolution to one index and each offer made once.
  `scripts/plan-timing.sh` times plans with a release build against a
  store: against the canonical store, planning with `build-base` takes
  0.38 s in x86 and 0.37 s in x86-64.

- **A resolution plans each provider once.** Deciding whether a held
  build is a provider's artifact plans the provider (§54), and each
  provider's plan plans its own, so a build whose labels resolved to
  many packages planned the bootstrap chain under each of them again:
  a build with `build-base` took minutes to plan. A resolver now keeps
  the plans it has made, by object, snapshot, and target, as a plan is
  a function of those and the store; and `kairo build` keeps, for one
  command, each object's build results, each plan's build key, and
  which blobs and signers it has checked, rather than reading and
  parsing every build result of a provider for each question,
  forgetting the build results once it builds one, before it plans
  again. Planning
  with `build-base` went from 231 s to 3.4 s in x86 and from 994 s to
  2.9 s in x86-64, to the same plans. The resolver counts the plans it
  computes (`Resolver::plans_made`), and a test holds a shared chain to
  each provider planned once.

- **Only releases are offered by label (`DECISIONS.md` §61).** A
  requirement that names no object (`offered_by`) is satisfied only by
  a version-tagged snapshot; an untagged head is offered only to a
  requirement naming its object. The bootstraps name the stage before
  them by its canonical id; stage0-posix (1.9.1) and linux-filesystem
  (1.0.0) are tagged releases; every package, gcc 7.5.0 among them, is
  a first generation built with `bootstrap-base` until the base it
  names is released.

- **The guest's tree lacks `kairo.toml`, and `SOURCE_DATE_EPOCH` is
  fixed (`DECISIONS.md` §52).** `/kairo/src` no longer holds the
  manifest, and `SOURCE_DATE_EPOCH` is the target's `source_date_epoch`
  or 315532800 (1980-01-01) rather than the revision's time.

- **A conferral of another label needs a version (`DECISIONS.md`
  §51).** A `confers` entry naming a label other than the provider's own
  without a version, which §46 pinned to a sibling of the provider, is
  refused; give its range. A label conferring itself without a version
  is pinned to the offer the build resolved.

- **A file source's bytes are read as stored (`DECISIONS.md` §47).**
  They were checked against their hash when put; the read no longer
  re-hashes them, and `kairo store verify` is what finds a file the
  storage changed.

- **Push plans and closure bundles gained optional `sources` fields**
  (`DECISIONS.md` §38 sub-decision 10): the plan request lists pinned
  sources' inventories, the plan answers for each, and the bundle
  manifest lists the source bundles under `sources/`. A receiver that
  predates them refuses a push that uses them; pushes without pinned
  sources are unchanged on the wire.
- **Manifest version requirements are validated.** A dependency
  `version` that isn't a Cargo-syntax requirement (e.g. `">=4 <5"`
  without the comma) is now rejected. Manifest hashes are unchanged
  for manifests without build targets or provides-dependency
  versions; those two add new canonical fields
  (`schemas/canonical/object-manifest-v1.md`).

- **`kairo bundle import` verifies (`DECISIONS.md` §32).** Object
  bundles now go through the same verifier as `kairo fetch`:
  signatures against the key state this node holds, chains, and Git
  packs through the quarantine, stopping at the first record that
  fails and flagging revoked-key statements. A signer whose key
  rotation this node lacks is refused with a hint to `kairo fetch`
  first. `--unverified` keeps fixity-only import.

- **Nodes sign their node document, and pulls authenticate the peer
  (`DECISIONS.md` §25 sub-decisions 1–2).**
  - The node document gains `signed_at` and `signature`, by the node
    actor's active key over the bytes in
    `schemas/canonical/node-document-v1.md`. `kairo-federation-http`
    loads the key from `--keys` (default `<store>/keys`) and refuses
    to start with a key that isn't the node actor's active, unrevoked
    key.
  - A node always serves its node actor's listing and key events,
    whether or not a published closure names it.
  - A pull refuses a document whose `base_url` isn't the URL pulled
    from, then fetches the node actor's genesis and key events and
    refuses a signature not by the key active at `signed_at`. The pull
    report, provenance, `kairo fetch`, and the web client's provenance
    badge now call `peer_actor` authenticated rather than claimed.

- **`ActorTrust` gains a scope (`DECISIONS.md` §18).**
  - Every trust statement names one scope, `general` or a
    feature-defined one such as `federation:push`, encoded in its
    canonical body. The chain key becomes `(truster, trusted, scope)`:
    scopes are separate chains with no fallback, and the store refuses
    a successor that changes its predecessor's scope.
  - `kairo trust grant|block|withdraw|show|history` take `--scope`
    (default `general`), and `trust list` shows each opinion's scope.
    `GET /api/v1/trust/{by}/{of}` takes `?scope=`, and trust heads
    carry their scope. The web inspector writes `general` opinions.
  - `verify object` keeps reporting `general` trust.

- **Pulls fetch statements in batches (`DECISIONS.md` §24).**
  - `POST /federation/v1/statement-batch` returns up to 256 requested
    statements, each the stored JSON verbatim, in request order. Ids
    the node doesn't serve are left out, so missing and unpublished
    stay indistinguishable.
  - Nodes advertise `statement-batch` in `supports`. The pull uses it
    when the node does, and otherwise fetches per record. Anything a
    batch leaves out is fetched on its own, and every statement is
    still verified individually.
  - A new limit, `max_batch_bytes` (default 32 MiB), is configurable
    under `[federation.limits]`.

- **The statement envelope gains `previous` (`DECISIONS.md` §23,
  step 1 of actor chains).**
  - Every signed statement's envelope now carries `previous`, the
    signer's chain tips: strictly ascending statement ids, at most 16,
    encoded after `created_at` and required in JSON. Every
    `StatementId` changes, since the field is part of the canonical
    bytes.
  - Branches, version tags, capability grants and revocations, and key
    events are chained kinds. `ObjectRevision` and `ActorTrust` are
    unchained and refuse a non-empty `previous`.
  - Every write path signs with the current tips: CLI commands
    (including emergency key prepare; `submit` keeps the prepared
    envelope's `previous`) and daemon session signing. Unchained kinds
    are unaffected. `GET /api/v1/actors/{id}/chains` lists an actor's
    chains and tips for clients that sign their own statements.
  - Federation pulls use the chains:
    - Fetched key events and statements are ingested in chain order.
    - Each closure actor's object chain and `keys` chain are checked
      afterwards. `kairo fetch` reports `incomplete_chains` with the
      missing ids, and flags statements whose `previous` names a
      statement outside their chain. Revoked-key statements don't
      count toward gaps.
    - `--require-complete` fails the fetch (exit 11) on any gap, after
      storing what verified. The two-node smoke now fetches with it.
  - Verification uses the chains:
    - `kairo verify object` and `GET /api/v1/verify-object/{id}` check
      the chains the result depends on. Those are the object and
      `keys` chains of the creator, the revision's and branch's
      signers, and their grantors.
    - A gap or a contradiction makes the result `INDETERMINATE`. The
      CLI lists each chain under `chains:`; the API adds `chains` and
      `chain_incomplete` / `chain_contradiction` issues.
  - The store indexes every actor chain per signer
    (`actor_chains/`, rebuilt by `rebuild-indexes`). It reports each
    chain's tips, and completeness with the missing ids. It also
    reports contradictions: statements whose `previous` names a held
    statement outside the chain. A revocation chains in its grant's
    scope, and while that grant is missing, the grant is reported as
    the gap. Callers can discount statements signed with revoked keys.

- **Git history federates over Git's smart HTTP protocol v2
  (`DECISIONS.md` §22).**
  - Each published object's Git history is served at
    `/federation/v1/objects/{id}/git` (`info/refs`,
    `git-upload-pack`), and a pull runs `git fetch` against it. A
    re-pull negotiates and fetches only the new objects.
  - The server filters every request. It answers `ls-refs` from the
    published commits, refuses unpublished `want`s (`400
    fed_bad_git_request`), drops unpublished `have`s, and refuses
    other commands and protocol v0/v1. The shared pool's unpublished
    commits stay unreachable.
  - Both sides run Git with host config ignored and safety settings
    pinned. The client caps a fetch's bytes and time by watching its
    quarantine.
  - The old single-pack endpoint and the server's pack cache are
    removed, so the server writes nothing again.
    `--max-concurrent-packs` still caps concurrent fetches, and a
    30-second stall timeout replaces the size-based deadline.
  - Any Git client can `git clone --mirror` a published object's
    commits (content only; verification still needs the
    statements).

- **Git transfer follows Git-host practice (`DECISIONS.md` §21).**
  - Packing holds no lock: it snapshots the refs and packs with
    `pack-objects --revs`. Serving caps concurrent fetches
    (`--max-concurrent-packs`, `503 fed_busy` with `Retry-After` past
    it); see §22 above for the transport.
  - Incoming packs (federation pull, bundle import) go through a
    quarantine and only reach the pool when complete, with no lock
    held during the download.
  - `kairo git fetch` no longer holds the pool lock.

- **Revision history is a Git-like log (`DECISIONS.md` §20).**
  - A per-object revision index (revision id → statements) lets
    history be walked along parent links, newest first.
  - `GET /api/v1/objects/{id}/log` and `kairo revision log` page
    through it, replacing `GET /api/v1/revisions/{object}` and
    `kairo revision list`.
  - Federation closures and bundle exports carry only revisions
    reachable from branches, tags, and the genesis's initial revision.
    Bundle export no longer walks the whole store.
  - The inspector's Revisions panel pages with "Load older
    revisions".

- **Git object formats (`DECISIONS.md` §19).**
  - Revision ids are `git:sha1:<40 hex>` or `git:sha256:<64 hex>`,
    parsed strictly; the label decides the format, never the length.
    They are enforced at `revision create` / `object create`, the
    daemon write path, bundle import, and federation pull. One
    object's history has one format.
  - The Git cache keeps one pool per format (`pool-sha1/`,
    `pool-sha256/`, replacing `pool/`), and gix is upgraded 0.66 →
    0.89 with SHA-256 support, so SHA-256 repositories verify.
  - `kairo git fetch` takes `--object-format`.
  - Bundle manifests list typed revisions in `expected_commits`.
  - Fixtures that labeled SHA-1 commits `git:sha256:` are relabeled.
- **Federation fix-ups (`PHASE_2_FEDERATION.md`).**
  - Pulled and bundled Git packs are checked for every commit's full
    closure before anything is pinned, and a failed check leaves no
    repo behind.
  - The server advertises a pack only when the object's repo has
    refs.
  - Statements signed with a revoked key are stored and flagged
    instead of skipped (the report field is now `flagged`).
  - A pinned peer prints "claim matches, node not authenticated".
  - Peer URLs are compared case-insensitively.
  - DECISIONS §16 and §18 now state that withheld revocations can't
    be detected from one peer, and that discovery needs node
    authentication.

### Added

- **Two-node federation check in CI (`PHASE_2_FEDERATION.md`
  slice 6).** `scripts/federation-smoke.sh` builds an origin node
  with a real Git-backed revision, serves it, and pulls it into a
  second store through a pinned config entry. It checks that
  `verify object --no-cwd-repo` is VALID from the cache alone, that
  provenance names the origin, and that a re-fetch is incremental.
  CI runs it for SHA-1 and SHA-256 histories. The real-stack
  Playwright suite pulls an object the same way and checks the
  inspector's "Fetched from" badges.

- **Federation policy and visibility (`PHASE_2_FEDERATION.md`
  slice 5).**
  - A node config file, `<store>/config.toml`, parsed strictly by
    the new `kairo-config` crate (`DECISIONS.md` §17). It has a
    `[federation]` section: `enabled`, limit overrides, and
    `[[federation.peers]]` entries with optional `node_actor` pins.
  - `kairo fetch` refuses unlisted peers unless `--allow-peer`. It
    also refuses a pinned peer whose claimed node actor changed
    (before fetching anything), and refuses under `--offline` or
    `enabled = false`.
  - New commands: `kairo federation status` (publish list, peers,
    pins, last pull per peer) and `kairo config path | validate`.
  - The daemon serves `GET /api/v1/provenance/{record}`, and the
    inspector shows a "Fetched from <peer>" badge on statement,
    object, and actor pages.

- **Federation Git transfer (`PHASE_2_FEDERATION.md` slice 4).**
  `GET /federation/v1/objects/{id}/git` streams the object's Git pack
  from the managed cache. `kairo fetch` ingests it when a pulled
  revision's commit is missing locally, checks every commit resolves,
  and pins them under `refs/kairo/imported/`. A fetched object now
  passes `kairo verify object --no-cwd-repo` from the cache alone.
  `kairo fetch` reports a `git =` outcome.

- **Federation pull (`PHASE_2_FEDERATION.md` slice 3).** `kairo fetch
  --object <id> --from <url>` pulls a published object's closure from
  a peer: its actors and their key events, its genesis, its grants and
  revocations, its statements, and its blobs. It fetches only what
  the store lacks, so a re-pull fetches nothing new. Each record is
  checked for schema, fixity (its content matches its id), placement
  (`ActorTrust` is always refused), and signature before it is
  stored. Statements signed by a revoked key are skipped. Each record
  gets a provenance entry naming the peer, and `kairo federation
  provenance --ref <typed-ref>` shows them. The client requires
  `https://` except on loopback, follows no redirects, and enforces
  size, count, and time limits. It adds `ureq` with rustls, the
  workspace's first TLS dependency.

- **Federation serving (`PHASE_2_FEDERATION.md` slice 2).** New
  `kairo-federation-http` crate and binary: a separate, read-only,
  loopback-by-default process that serves the closures of this
  node's published objects (`DECISIONS.md` §16). It exposes the node
  document, published-object and per-object listings, object
  genesis, actor key-event listings, and single statements and
  blobs. An id outside every published closure gets the same `404`
  as one the store doesn't hold. `kairo-store` gains
  `object_closure`, `actor_key_event_ids`, and
  `get_object_genesis_bytes`. Pulling (slice 3) is not implemented
  yet.

- **Federation publish list and wire types (`PHASE_2_FEDERATION.md`
  slice 1).** New `kairo-federation-core` crate with the object-pull
  wire types (node document, published-object, object and actor
  listings, pagination, `fed_*` error envelope), which validate at
  parse time. `kairo federation publish --object <id>`,
  `unpublish`, and `published` manage the node's opt-in publish
  list at `<store>/federation/published.json` (`DECISIONS.md` §16).
  Nothing serves it yet; that is slice 2. `ActorKind::node()` names
  a federation node's own actor.

- **Create objects from the web inspector (`PHASE_2_WEB_WRITES.md`
  slice 7).** Cookie-mode `POST /api/v1/statements` now accepts
  `ObjectGenesis` with a body of just `{ object_kind,
  initial_revision? }`; the daemon fills `created_by`, `created_at`,
  and a fresh nonce and signs the body (genesis has no envelope).
  A "New object" action on the Objects page and the signed-in
  actor's own page creates one and opens it. This completes the
  browser write surface planned in `PHASE_2.md` §16.

- **Capability grant / revoke dialogs in the web inspector
  (`PHASE_2_WEB_WRITES.md` slice 6).** "Grant capability" on the
  object page's Capability heads panel writes an object-scoped
  `ActorCapabilityGrant` (kinds, delegable, optional expiry); a grant
  to someone who already holds one from the signer updates that chain
  with a derived `supersedes`. A per-row Revoke on grants the signer
  issued writes `ActorCapabilityRevocation`, forward-only unless the
  warned, reason-required retroactive option is chosen. The table now
  shows kinds, delegable, expiry, and revocation status.

- **Real-stack e2e suite (`PHASE_2_WEB_WRITES.md` slice 8).**
  `pnpm -F @kairo/web-client e2e:real` runs Playwright against the
  built SPA served by a real `kairo-web` + `kairo-daemon` over a
  store seeded by `scripts/smoke-seed.sh` (which now also writes
  `seed-ids.json`): sessions, every shipped write dialog, a daemon
  capability refusal, and the proxy guards. New CI job `e2e-real`.
  The Playwright configs force software compositing under WSL2,
  where headless Chromium otherwise never fires
  `requestAnimationFrame`.

- **Browser write path (`PHASE_2.md` §16,
  `PHASE_2_WEB_WRITES.md`, `DECISIONS.md §15`).** `POST
  /api/v1/statements` accepts an unsigned `{ body_kind, body }`
  under a `kairo_session` cookie; the daemon signs as the session
  actor and runs the same verification and capability checks as
  signed mode (both credentials at once is `400
  ambiguous_auth`). The web inspector
  gains trust-opinion and branch create/update dialogs on top of
  it.

- **Version-tag dialog in the web inspector (`PHASE_2_WEB_WRITES.md`
  slice 5).** A "Tag version" action on the object page's Tags
  panel writes `ObjectVersionTag` via the session; one form covers
  creating a tag and retargeting the signed-in actor's existing
  one (`supersedes` is derived automatically). Revocation is not
  exposed yet.

- **Daemon write-path Phase B — signed-request auth +
  capability authz (`PHASE_2.md` §15.1,
  `PHASE_2_DAEMON_AUTH.md`, `DECISIONS.md §14`).** The daemon's
  `POST /api/v1/statements` write surface now requires an
  `Authorization: KairoActor1 …` header carrying an ed25519
  signature over a domain-prefixed canonical string covering
  method, path-and-query, body SHA-256, timestamp, and a
  per-request nonce. The daemon resolves the request actor
  through the same `ActorResolver` as the rest of the system,
  enforces a ±60s timestamp window (no server-side nonce cache
  in v1; the nonce is in the signed input so a stateful cache
  can be added later without a wire-format change), and
  verifies the signature against the actor's active signing
  key at `request.timestamp`. Read endpoints stay
  unauthenticated; their trust model is the Unix socket's
  filesystem permissions, unchanged. Seven new 401 error codes
  (`missing_authorization`, `malformed_authorization`,
  `unsupported_auth_scheme`, `timestamp_out_of_window`,
  `bad_request_signature`, `actor_not_found_for_request`,
  `request_key_id_unknown`) surface each failure mode of the
  pipeline. The CLI signs the request envelope with the same
  actor key it already uses for the statement body; the
  command shape and output text are unchanged.
- **Per-kind authorization on the daemon write surface.**
  After authentication, the daemon runs a request-time
  capability check that mirrors `CAPABILITIES.md §6.1`:
  - `ActorTrust`, `ObjectGenesis` — identity-only (signer is
    the authority by construction).
  - `ObjectRevision`, `ObjectBranch`, `ObjectVersionTag` —
    root-authority short-circuit, otherwise
    `evaluate_capability(grantee=signer, scope=Object(...),
    kind, at=request.timestamp)` must return `Held`.
  - `ActorCapabilityGrant` — signer must be root authority OR
    hold every granted kind with `delegable=true` via the new
    `evaluate_capability_for_regrant`.
  - `ActorCapabilityRevocation` — signer must equal the
    original grant's `actor()`.

  Any other outcome is a 403 `not_authorized` carrying the
  underlying `CapabilityEvaluation` variant in the message.
  Direct-mode writes still bypass authz entirely (the
  operator's recovery path); the daemon is the policy
  enforcement layer.
- **`kairo daemon status` reports the live auth posture.**
  `StatusInfo` grew an `auth_mode: String` field, defaulting
  to `"filesystem-perms"` for forward-compat with pre-Phase-B
  clients. Phase B daemons emit `"signed-request"`; the CLI
  prints it as `auth    = signed-request` so operators and the
  inspector can tell at a glance whether the daemon has Phase B
  enforcement active.
- **`DECISIONS.md §14` — Phase B daemon write-path auth shape
  locked.** Eight sub-decisions paralleling §13's pattern:
  two-mechanism split (authn + authz); `Authorization:
  KairoActor1` header with structured params; domain-prefixed
  canonical signing string; SHA-256 body hash always present;
  ±60s timestamp-only replay defense (forward-compatible with a
  future stateful nonce cache); per-kind authz matrix;
  `kairo-web` POST-block remains in place during Phase B (web
  inspector login is its own decision); out-of-scope list
  covering web auth, federation peer auth, mTLS, rate limiting,
  attestation-surface request signing.

- **Daemon write-path Phase A (`PHASE_2.md` §15,
  `PHASE_2_DAEMON_WRITES.md`).** The Phase 2 §2 read-only daemon
  gains a single generic write endpoint, `POST /api/v1/statements`,
  that accepts a `SignedStatement<*>` JSON envelope and dispatches
  on the `type` discriminator. The Phase A allowlist covers seven
  kinds: `ActorTrust`, `ObjectRevision`, `ObjectBranch`,
  `ObjectVersionTag`, `ObjectGenesis`, `ActorCapabilityGrant`,
  `ActorCapabilityRevocation`. Other kinds (attestation-key
  surfaces, key rotation/revocation) stay direct-only in Phase A.
  Verification re-runs the same `verify_envelope_statement`
  (and `verify_signature` for genesis) the direct-mode CLI uses;
  the store-side `put_*` paths are unchanged.
- **CLI dispatch for write commands (`CLI.md` §3.3).** `kairo
  trust grant/block/withdraw`, `revision create`, `branch set`,
  `tag bind/revoke`, `object create`, `capability grant/revoke`
  now route through `cli::dispatch::resolve` — probe the
  `<store>/daemon.sock` socket and post the locally-signed
  envelope through the daemon if reachable, else fall through to
  the existing direct path. Output text is byte-identical across
  modes. The keystore stays the local signing surface either
  way; signed envelopes go over the wire, never private keys.
  `kairo actor create` remains direct-only by design (Phase
  A-permanent exception — keystore-generation is not a daemon
  concern).
- **`kairo daemon status` reports the live write surface.** The
  `StatusInfo` DTO grows a `write_surface: Vec<String>` field
  (single-sourced from the daemon's dispatcher allowlist); the
  CLI prints `writes  = <kinds joined by ,>` so operators and
  the inspector can tell which kinds the daemon currently
  accepts POSTs for. `#[serde(default)]` keeps older clients
  forward-compatible.
- **`kairo-web` proxy POST-block during Phase A
  (`DECISIONS.md §13` sub-decision 5).** The TCP front-end
  filters request methods to `[GET, HEAD]` and returns `405
  Method Not Allowed` with `Allow: GET, HEAD` for everything
  else, before opening a connection to the daemon. Prevents
  browser-side XSS or DNS-rebinding from tunnelling a write
  through to the daemon's unauthenticated write surface during
  Phase A; lifted when Phase B's signed-request envelope lands.
- **`DECISIONS.md §13` — daemon write-path MVP shape locked.**
  Eight sub-decisions, mirroring §10's daemon-MVP pattern:
  two-phase split (writes-first under filesystem-perms, signed-
  request auth later), single generic POST endpoint, signing
  stays client-side, data-model-only validation in Phase A,
  kairo-web POST-block, dispatch parity with reads, no wire-
  format pre-commits for Phase B, out-of-scope list.
- **Cold-storage attestation keys (`ACTORS.md` §5.5).** New per-actor
  attestation key set declared in `ActorGenesis.attestation_keys`,
  used to sign emergency key events without touching the operational
  signing key. New body types: `ActorEmergencyKeyRotation`,
  `ActorEmergencyKeyRevocation`, `ActorAttestationKeyAdd`,
  `ActorAttestationKeyRevocation`. Recovery flow lets an operator
  rotate to a fresh active signing key after compromise of the
  operational key.
- **M-of-N attestation thresholds (`ACTORS.md` §5.5.3).** New
  `attestation_threshold: u8` on `ActorGenesis`. Attestation-surface
  envelopes carry `signatures: Vec<Signature>` (≥ threshold distinct
  attestation-set signers). New body type
  `ActorAttestationThresholdChange` with the asymmetric authority
  rule (raises require `max(current, new)` distinct sigs, lowers
  require `current`).
- **`MultiSignedStatement<B>` envelope** in `kairo-statement`, used by
  every attestation-surface body. Sorted, distinct-`key_id`
  signatures; `verify_envelope_multi_statement` checks threshold +
  per-signature membership in the attestation set.
- **CLI verb tree for attestation keys.**
  - `kairo actor recover-key {sign,prepare,submit}` — emergency
    key rotation via cold-storage attestation key.
  - `kairo actor add-attestation-key {sign,prepare,submit}` —
    append a new attestation key to the actor's set.
  - `kairo actor revoke-attestation-key {sign,prepare,submit}` —
    retract a previously-held attestation key.
  - `kairo actor change-attestation-threshold {sign,prepare,submit}` —
    mutate the M-of-N quorum.
  - `kairo actor co-sign --prepared <path> --actor <id>
    --attestation-key-seed <path>` — append a single signature to a
    partial multi-sig envelope.
  - `kairo actor key-history` extended with attestation adds,
    revocations, and the threshold trajectory (with per-event
    `quorum_at_event`).
- **Capability / delegation model (`CAPABILITIES.md`).** Body types
  `ActorCapabilityGrant` and `ActorCapabilityRevocation`, with scope
  (Object / Actor), statement-kind allowlist, delegation depth, and
  expiration constraints. `evaluate_capability` resolves chained
  grants with cycle detection. New CLI verb tree:
  `kairo capability {grant,revoke,list}`.
- **First-person trust (`STATEMENTS.md` §4.2e).** Body type
  `ActorTrust` with `Trusted` / `Untrusted` / `withdraw`
  decisions, parameterized by the asking truster. `verify object`
  surfaces trust as `trusted` / `untrusted` / `unknown` /
  `unevaluated`. New CLI verb tree:
  `kairo trust {grant,block,withdraw,show,list,history}`.
- **Object branches and version tags.** Body types `ObjectBranch`
  and `ObjectVersionTag`, with cross-actor `supersedes` chains.
  Snapshot identity over the resolved frontier. New CLI verbs:
  `kairo branch {set,show,list}`, `kairo tag
  {bind,revoke,show,list,history}`, `kairo snapshot compute`.
- **Object bundles (`PACKAGE.md`).** Directory-format export/import
  carrying actors, objects, statements, blobs, and a manifest with
  expected Git commit references. New CLI verbs:
  `kairo bundle {export,import}`. Git history is _not_ in the bundle
  in v1; the manifest declares which commits the receiver needs.
- **End-to-end object verification.** `kairo verify object` rolls up
  six independent dimensions (genesis fixity, frontier resolution,
  signature, actor consistency, manifest binding, content layer)
  into a single `VALID` / `INDETERMINATE` / `INVALID` verdict.
  Optional `--as <id>` to evaluate trust from a specific truster.
- **Git integration (`kairo-git`).** Read-only `gix`-backed
  repository handle used by `verify object` to look up the storage
  commit and validate the manifest against the commit's tree.
- **Property tests** for canonical-encoding determinism + JSON DTO
  round-trip across every body type
  (`kairo-statement/tests/property_tests.rs`,
  `kairo-identity/tests/property_tests.rs`).
- **README walkthrough integration test**
  (`examples_readme_walkthrough_round_trip` in `kairo-cli/src/tests.rs`)
  mirrors `examples/README.md` step-for-step so the walkthrough doesn't
  bit-rot when CLI verbs change.
- **Threat model** (`specs/THREAT_MODEL.md`) consolidating the
  security argument: assets, adversaries, defended attacks with
  mechanism cross-references, residual risk, explicit non-goals.
- **MSRV: Rust 1.95.** Pinned in `[workspace.package].rust-version`.

### Changed

- **`CapabilityHeadDto` carries the grant's facts.** Both
  `GET /api/v1/capabilities/{grantor}` and
  `.../for-object/{object}` now return `statement_kinds`,
  `delegable`, `constraints`, and the grantor's `revocation` (if any)
  per head. Additive and `#[serde(default)]`; older clients keep
  decoding.

- **Rust toolchain pinned to 1.99.1.** `rust-toolchain.toml` and
  the CI workflow move from floating `stable` to `1.99.1`, so new
  releases can't break `fmt --check` / `clippy -D warnings` without a
  code change; bump policy in `VERSIONING.md` ("Toolchain"). The three
  hex decoders flagged by 1.99's new `chunks_exact`-with-constant lint
  now use `as_chunks::<2>()`. MSRV is unchanged (1.95).

- **`kairo-web` lifts its POST-block for an exact allowlist
  (`DECISIONS.md §15`).** The proxy now forwards `POST
  /api/v1/sessions`, `DELETE /api/v1/sessions/current`, and
  `POST /api/v1/statements` in addition to `GET`/`HEAD`, so
  sign-in and browser writes reach a real daemon (previously they
  worked only against MSW mocks). Every request must carry a
  loopback `Host` (`403 web_forbidden_host` otherwise — DNS-rebind
  defense); writes must carry `Origin: http://<Host>` (`403
  web_cross_origin`). Off-allowlist methods are `405
  web_method_not_allowed` with a per-path `Allow` header and a
  JSON error envelope instead of plain text. `kairo web status`
  now sends the dialed loopback address as `Host`.

- **Mock version tags use strict semver.** The MSW registry seeded
  `v1.0.0` / `v1.1.0`, which the daemon's strict-semver parser
  rejects; seeds are now `1.0.0` / `1.1.0`, and the
  `GET /api/v1/version-tags/{object}/{version}` OpenAPI example
  matches.

- Renamed `kairo actor recover-key import` and `kairo actor
  add-attestation-key import` to `submit` for symmetry with
  `prepare` / `co-sign`. The same verb finalizes an envelope
  whether it was assembled inline (`--signature <path>`) or through
  the cosign flow.
- `kairo actor recover-key prepare` and `add-attestation-key
  prepare` now emit a partial envelope with `signatures: []`
  (previously a placeholder-signature hack). `co-sign` and `submit`
  append signatures into the empty array; `submit` constructs the
  `MultiSignedStatement` and verifies the threshold.
- `kairo-cli`'s `main.rs` split into focused modules: `cli` (clap
  definitions), `error` (`CliError`), `format` (verification
  reports), `store_paths` (path resolution), `commands/` (per-verb
  runners), `tests` (the integration test block). 10458-line
  single file → 4640-line dispatcher with sibling modules.

### Wire format

Items in this section change the canonical bytes a body hashes to,
or otherwise alter the on-the-wire shape. Each is a `0.x.0` bump.

- **`ActorGenesis` v1 schema gained
  `attestation_keys: Vec<PublicKey>` and `attestation_threshold:
  u8`.** Both participate in canonical bytes, so the resulting
  `ActorId` differs from any pre-attestation-keys actor body. Every
  existing actor in pre-introduction stores must be re-derived;
  there is no compatibility shim because nothing was deployed yet.
- **Attestation-surface envelopes now carry `signatures` (array)
  instead of `signature` (single).** Affects the JSON shape for
  `ActorEmergencyKeyRotation`, `ActorEmergencyKeyRevocation`,
  `ActorAttestationKeyAdd`, `ActorAttestationKeyRevocation`. The
  multi-sig envelope's canonical bytes exclude signatures (same as
  the single-sig envelope), so the body StatementId is unchanged.
- **New body type
  `ActorAttestationThresholdChange`** added to the canonical
  schema set; carries `new_threshold: u8`. See
  `schemas/canonical/actor-attestation-threshold-change-v1.md`.
- **`ObjectBranch` v1 gained an optional `supersedes`
  field.** Pre-supersedes branch records remain
  parseable (the field is `None`); new records that set it derive
  a different StatementId.

### Fixed

- Federation reads retry when a signal interrupts the HTTP client
  (`EINTR`), which happened in processes that also run servers or
  child processes, such as the daemon and the test suites.

- **Mock capability grants used `kinds`.** The MSW registry seeded
  grant bodies with `kinds` instead of the wire field
  `statement_kinds`.

- **`@kairo/ui` Vitest suite.** jest-dom matchers registered on
  the workspace's vitest 2.x `expect` rather than `@kairo/ui`'s
  4.x, so 24 of the package's existing tests failed with
  `Invalid Chai property`. The setup file now extends its own
  `expect`.

[Unreleased]: https://example.invalid/kairo
