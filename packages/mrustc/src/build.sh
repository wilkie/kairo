# Builds mrustc and, with it, rustc 1.90.0 and cargo into /kairo/package
# (packages/mrustc/README.md), for x86-64 alone (DECISIONS.md §65), with
# packages alone (data:build-base, §61): one child,
# rust-bootstrap-1.90.0, under /usr/lib/rust-bootstrap-1.90.0/, for the
# rust package's first release to build with, and nothing else.
. ./recipe.sh "$1"
NAME=mrustc
VERSION=0.12.0
RUST_VERSION=1.90.0
RUST_TARGET=x86_64-unknown-linux-musl
CHILD=rust-bootstrap-$RUST_VERSION
PREFIX=/usr/lib/$CHILD
LLVM_CONFIG=/usr/lib/llvm-20/bin/llvm-config

# mrustc's own route to a rustc (its build-1.90.0.sh and run_rustc):
# mrustc, a Rust compiler in C++ that emits C, which gcc compiles,
# compiles rustc 1.90.0, its standard library, and cargo, with
# minicargo; that rustc then builds the standard library, and rustc and
# cargo again, optimised, with cargo, and the standard library with
# them. Each is for the target Rust names x86_64-unknown-linux-musl, and
# links LLVM 20, the package's, not the copy in rustc's source.
unpack "$NAME-$VERSION"
cd "$BUILD/$NAME-$VERSION"
# mrustc's commit aea331a4 (2026-02-17), after 0.12.0: its type
# inference wrote past the end of a vector, which glibc's allocator
# tolerates and musl's doesn't, crashing on crossbeam-epoch.
patch -p1 < "$SRC/mrustc-typeck-out-of-bounds.patch"

# rustc's source beside mrustc, with mrustc's patch for it. mrustc's
# makefile would fetch the source itself: the pinned tarball is
# unpacked here, and the files its rules make from the download are
# made up to date instead: the download older than anything, what is
# made from it newer than the patch.
tar -xJf "$SRC/distfiles/rustc-$RUST_VERSION-src.tar.xz" --no-same-owner
(cd "rustc-$RUST_VERSION-src" && patch -p0 < "../rustc-$RUST_VERSION-src.patch")
# Rust's musl target links statically unless told not to, with the C
# runtime and unwinder Rust ships beside its standard library, which
# this toolchain hasn't; and what is told not to by RUSTFLAGS misses
# build scripts. It links dynamically against the musl package by
# default instead, as Alpine's rust does (§65 sub-decision 6).
MUSL_SPEC=rustc-$RUST_VERSION-src/compiler/rustc_target/src/spec/targets/x86_64_unknown_linux_musl.rs
sed -i 's/base\.crt_static_default = true;/base.crt_static_default = false;/' "$MUSL_SPEC"
grep -q 'base\.crt_static_default = false;' "$MUSL_SPEC"
touch -d @1 "rustc-$RUST_VERSION-src.tar.gz"
touch "rustc-$RUST_VERSION-src/extracted"
touch "rustc-$RUST_VERSION-src/dl-version"

# Built for the musl target, mrustc's own default when it is itself
# built against musl, so that what build scripts run and what is built
# are for one target, as no cross-build is wanted; with as many jobs as
# the guest has; the C it emits compiled by gcc.
export RUSTC_VERSION=$RUST_VERSION MRUSTC_TARGET_VER=1.90 OUTDIR_SUF=-$RUST_VERSION
export RUSTC_TARGET=$RUST_TARGET CC=gcc CXX=g++
# cargo's libssh2 is the package's, found by pkg-config, not the copy
# its crate would build in; libgit2, which no package has, is its own.
export LIBSSH2_SYS_USE_PKG_CONFIG=1
OUT_M=output-$RUST_VERSION
# make agrees the source needs neither fetching nor patching.
make -f minicargo.mk -q "rustc-$RUST_VERSION-src/dl-version"
make -j"$JOBS" -f Makefile all
make -j"$JOBS" -C tools/minicargo
make -f minicargo.mk PARLEVEL="$JOBS" LLVM_CONFIG="$LLVM_CONFIG" LIBS
make -f minicargo.mk PARLEVEL="$JOBS" LLVM_CONFIG="$LLVM_CONFIG" RUSTC_INSTALL_BINDIR=bin "$OUT_M/rustc"
"./$OUT_M/rustc" --version | grep -q "^rustc $RUST_VERSION"
# That rustc has no native thread-locals, as mrustc emits none: each of
# its thread_local!s is a pthread key, and rustc 1.90.0 has more than
# musl's 128 ("out of TLS keys"), as Gentoo and Termux found
# (mrustc#400). It runs with tls-keys.c preloaded, which gives it as
# many as it needs, a table per thread under one musl key; run_rustc
# copies it as a wrapper that does so. Only that rustc runs with it:
# the rustc it builds is rustc's own, with native thread-locals. rustc
# finds its standard library beside itself, and run_rustc copies it to
# each stage's directory, so the wrapper gives the stage it was copied
# to as rustc's sysroot, unless its caller gives one.
gcc -O2 -fPIC -shared -o "$PWD/tls-keys.so" "$SRC/tls-keys.c"
mv "$OUT_M/rustc" "$OUT_M/rustc-mrustc"
cat > "$OUT_M/rustc" <<SH
#!/bin/sh
case " \$* " in
*" --sysroot"*) set -- "\$@" ;;
*) set -- --sysroot "\$(cd "\$(dirname "\$0")/.." && pwd)" "\$@" ;;
esac
LD_PRELOAD=$PWD/tls-keys.so exec $PWD/$OUT_M/rustc-mrustc "\$@"
SH
chmod 755 "$OUT_M/rustc"
"./$OUT_M/rustc" --version | grep -q "^rustc $RUST_VERSION"
# cargo with the packages' OpenSSL, curl, libssh2, and zlib, found by
# pkg-config, not the OpenSSL its crates would build in.
make -f minicargo.mk PARLEVEL="$JOBS" LLVM_CONFIG="$LLVM_CONFIG" "MINICARGO_FLAGS_$OUT_M/cargo=" "$OUT_M/cargo"
"./$OUT_M/cargo" --version | grep -q "^cargo $RUST_VERSION"

# run_rustc: the standard library, rustc, and cargo again, built by the
# rustc mrustc made.
make -C run_rustc RUSTC_VERSION="$RUST_VERSION" RUSTC_TARGET="$RUST_TARGET" OUTDIR_SUF="-$RUST_VERSION" \
    PARLEVEL="$JOBS" LLVM_CONFIG="$LLVM_CONFIG"
RUN=run_rustc/output-$RUST_VERSION/prefix

# The toolchain, under its own directory: rustc, a wrapper that finds
# its libraries beside it, as run_rustc's does but without its build's
# paths; cargo; and the standard library and rustc's own libraries.
mkdir -p "$OUT/$CHILD$PREFIX/bin" "$OUT/$CHILD$PREFIX/lib"
cp "$RUN/bin/rustc_binary" "$RUN/bin/cargo" "$OUT/$CHILD$PREFIX/bin/"
cp -a "$RUN/lib/." "$OUT/$CHILD$PREFIX/lib/"
rm -rf "$OUT/$CHILD$PREFIX/lib/rustlib/src" "$OUT/$CHILD$PREFIX/lib/rustlib/rustc-src"
cat > "$OUT/$CHILD$PREFIX/bin/rustc" <<'SH'
#!/bin/sh
here=$(dirname "$0")
LD_LIBRARY_PATH="$here/../lib:$here/../lib/rustlib/x86_64-unknown-linux-musl/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
    exec "$here/rustc_binary" "$@"
SH
chmod 755 "$OUT/$CHILD$PREFIX/bin/rustc"

finish

# It works, installed over what it was built with: rustc and cargo say
# what they are, rustc builds a program that runs, linked dynamically
# against musl by default, and cargo builds a package that runs.
install_over
"$PREFIX/bin/rustc" --version | grep -q "^rustc $RUST_VERSION"
"$PREFIX/bin/cargo" --version | grep -q "^cargo $RUST_VERSION"
printf 'fn main() { println!("made"); }\n' >made.rs
"$PREFIX/bin/rustc" -o made made.rs
[ "$(./made)" = made ]
readelf -l made | grep -q "Requesting program interpreter: .*/ld-musl-x86_64\.so\.1"
mkdir -p crate/src
printf '[package]\nname = "made"\nversion = "0.1.0"\nedition = "2021"\n' >crate/Cargo.toml
printf 'fn main() { println!("made by cargo"); }\n' >crate/src/main.rs
(cd crate && CARGO_HOME="$BUILD/cargo-home" RUSTC="$PREFIX/bin/rustc" \
    "$PREFIX/bin/cargo" build --offline --release)
[ "$(./crate/target/release/made)" = "made by cargo" ]
