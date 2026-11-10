#!/usr/bin/env bash
# Archive rustc's source, make Rust's object or its next revision, and
# tag it 1.99.1 (packages/rust/README.md, packages/common/import.sh).
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
    "6451598a99e77f07571960f6b21d577399d3a30f33e80cbd2e3a4aae6535d265 rustc-1.99.1-src.tar.xz
     https://static.rust-lang.org/dist/rustc-1.99.1-src.tar.xz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "rust 1.99.1 for x86-64, built by 1.98.1" 1.99.1
