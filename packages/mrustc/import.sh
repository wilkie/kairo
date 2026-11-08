#!/usr/bin/env bash
# Archive mrustc's and rustc 1.90.0's sources, make mrustc's object or
# its next revision, and tag it 0.12.0 (packages/mrustc/README.md,
# packages/common/import.sh).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# What it builds with must be imported: packages/build-base/ and the
# packages it names, and llvm at 20, pkgconf, openssl, curl, libssh2,
# and zlib. Prints the object's id.
set -euo pipefail
shopt -s inherit_errexit

# sha256 name url...
DISTFILES=(
    "c1ba35f5fc5c4ca2952d9f5526e900dcb6632ea7fd4d71fa58029b3bb563ae56 mrustc-0.12.0.tar.gz
     https://github.com/thepowersgang/mrustc/archive/refs/tags/v0.12.0.tar.gz"
    "6bfeaddd90ffda2f063492b092bfed925c4b8c701579baf4b1316e021470daac rustc-1.90.0-src.tar.xz
     https://static.rust-lang.org/dist/rustc-1.90.0-src.tar.xz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "mrustc 0.12.0, bootstrapping rustc 1.90.0, for x86-64" 0.12.0
