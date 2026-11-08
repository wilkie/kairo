#!/usr/bin/env bash
# Archive rustc's source, make Rust's object or its next revision, and
# tag it 1.91.1+2 (packages/rust/README.md, packages/common/import.sh).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# What it builds with must be imported: packages/build-base/ and the
# packages it names, mrustc, llvm at 21, python, pkgconf, openssl,
# curl, libssh2, and zlib. Prints the object's id.
set -euo pipefail
shopt -s inherit_errexit

# sha256 name url...
DISTFILES=(
    "66401bb815e236cc6b2aacbbe23b61b286c1fe27a67902e7c0222cfe77b3dbab rustc-1.91.1-src.tar.xz
     https://static.rust-lang.org/dist/rustc-1.91.1-src.tar.xz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "rust 1.91.1 for x86-64, the double compile, built by 1.91.1" 1.91.1+2
