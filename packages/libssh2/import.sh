#!/usr/bin/env bash
# Archive libssh2's sources, make its object or its next revision, and
# tag it 1.11.1 (packages/libssh2/README.md, packages/common/import.sh).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# What it builds with must be imported: packages/build-base/ and the
# packages it names, and openssl and zlib.
# Prints the object's id.
set -euo pipefail
shopt -s inherit_errexit

# sha256 name url...
DISTFILES=(
    "9954cb54c4f548198a7cbebad248bdc87dd64bd26185708a294b2b50771e3769 libssh2-1.11.1.tar.xz
     https://www.libssh2.org/download/libssh2-1.11.1.tar.xz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "libssh2 1.11.1 for x86 and x86-64" 1.11.1
