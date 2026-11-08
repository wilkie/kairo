# Builds Rust, rustc and cargo, into /kairo/package
# (packages/rust/README.md), for x86-64 alone (DECISIONS.md §65), with
# packages alone (data:build-base, §61), built by the release before:
# one child, rust-<version>.
. ./recipe.sh "$1"
NAME=rust
VERSION=1.91.1
CHILD=rust-$VERSION
RUST_TARGET=x86_64-unknown-linux-musl
LLVM_CONFIG=/usr/lib/llvm-21/bin/llvm-config
# The release before, which builds this one (§59): for 1.91.1+2, the
# double compile, 1.91.1 itself, built by mrustc's 1.90.0 (§65
# sub-decision 3), installed under /usr.
STAGE0=/usr
# Whether the release before is this one's own version, as for a
# double compile.
LOCAL_REBUILD=true

unpack "rustc-$VERSION-src"
cd "$BUILD/rustc-$VERSION-src"

# Rust's musl target links statically unless told not to; it links
# dynamically against the musl package by default instead, as Alpine's
# rust does (§65 sub-decision 6).
MUSL_SPEC=compiler/rustc_target/src/spec/targets/x86_64_unknown_linux_musl.rs
sed -i 's/base\.crt_static_default = true;/base.crt_static_default = false;/' "$MUSL_SPEC"
grep -q 'base\.crt_static_default = false;' "$MUSL_SPEC"

# Rust's own build, x.py, as a distribution configures it: for the musl
# target alone; with the release before as its compiler; against the
# LLVM package's 21, linked shared; with the crates its source carries,
# fetching nothing; installed under /usr, its libraries in the library
# directory (§60), without rpaths, found as the loader finds any;
# without documentation, debug information, or its codegen tests,
# which want LLVM's FileCheck; cargo with it. musl's
# libc.a and startup files are where the musl package puts them: Rust
# copies them beside its standard library, for programs that ask to be
# linked statically.
cat > bootstrap.toml <<TOML
change-id = "ignore"

[llvm]
link-shared = true

[build]
build = "$RUST_TARGET"
host = ["$RUST_TARGET"]
target = ["$RUST_TARGET"]
rustc = "$STAGE0/bin/rustc"
cargo = "$STAGE0/bin/cargo"
local-rebuild = $LOCAL_REBUILD
python = "python3"
submodules = false
vendor = true
locked-deps = true
extended = true
tools = ["cargo"]
docs = false
description = "Kairo"
jobs = $JOBS

[install]
prefix = "/usr"
sysconfdir = "/etc"
libdir = "${LIBDIR#/usr/}"

[rust]
channel = "stable"
rpath = false
debuginfo-level = 0
deny-warnings = false
remap-debuginfo = true
lld = false
llvm-tools = false
codegen-tests = false

[target.$RUST_TARGET]
llvm-config = "$LLVM_CONFIG"
cc = "cc"
cxx = "c++"
ar = "ar"
ranlib = "ranlib"
linker = "cc"
crt-static = false
musl-root = "/usr"
musl-libdir = "$LIBDIR"
TOML

# cargo's OpenSSL, libssh2, curl, and zlib are the packages', found by
# pkg-config, not the copies its crates would build in; libgit2, which
# no package has, is its own.
export OPENSSL_NO_VENDOR=1 LIBSSH2_SYS_USE_PKG_CONFIG=1
DESTDIR="$OUT/$CHILD" python3 x.py install
# What the installer writes about itself.
rm -f "$OUT/$CHILD$LIBDIR/rustlib/install.log" "$OUT/$CHILD$LIBDIR/rustlib/uninstall.sh"

finish

# It works, installed over what it was built with: rustc says what it
# is, built with LLVM 21, for the musl host; it builds a program that
# runs, linked dynamically against musl; and cargo builds a package
# with a procedural macro, which rustc loads as a library, that runs.
install_over
info=$(rustc -vV)
grep -qx "release: $VERSION" <<<"$info" || { echo "build.sh: rustc -vV: $info" >&2; false; }
grep -qx "host: $RUST_TARGET" <<<"$info" || { echo "build.sh: rustc -vV: $info" >&2; false; }
grep -q "^LLVM version: 21\." <<<"$info" || { echo "build.sh: rustc -vV: $info" >&2; false; }
[ "$(rustc --print sysroot)" = /usr ]
cargo --version | grep -q "^cargo $VERSION "
printf 'fn main() { println!("made"); }\n' >made.rs
rustc -o made made.rs
[ "$(./made)" = made ]
readelf -l made | grep -q "Requesting program interpreter: .*/ld-musl-x86_64\.so\.1"
mkdir -p crate/src crate/made-macro/src
cat > crate/Cargo.toml <<'TOML'
[package]
name = "made"
version = "0.1.0"
edition = "2021"

[dependencies]
made-macro = { path = "made-macro" }
TOML
cat > crate/made-macro/Cargo.toml <<'TOML'
[package]
name = "made-macro"
version = "0.1.0"
edition = "2021"

[lib]
proc-macro = true
TOML
cat > crate/made-macro/src/lib.rs <<'RS'
use proc_macro::TokenStream;
#[proc_macro]
pub fn made(_: TokenStream) -> TokenStream {
    "\"made by a macro\"".parse().unwrap()
}
RS
printf 'fn main() { println!("{}", made_macro::made!()); }\n' >crate/src/main.rs
(cd crate && CARGO_HOME="$BUILD/cargo-home" cargo build --offline --release)
[ "$(./crate/target/release/made)" = "made by a macro" ]
