#!/usr/bin/env bash
# Archive libunistring's sources, make its object or its next revision, and
# tag it 1.5.0 (packages/libunistring/README.md, packages/common/import.sh).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# What it builds with must be imported: packages/build-base/ and the
# packages it names.
# Prints the object's id.
set -euo pipefail
shopt -s inherit_errexit

# sha256 name url...
DISTFILES=(
    "fe89007a8a414ae4790b816b86859e877d3ab135db3c37f06541ff7e85ac344b libunistring-1.5.tar.xz
     https://ftp.gnu.org/gnu/libunistring/libunistring-1.5.tar.xz
     https://mirrors.kernel.org/gnu/libunistring/libunistring-1.5.tar.xz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "libunistring 1.5 for x86 and x86-64" 1.5.0
