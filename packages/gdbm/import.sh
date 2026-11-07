#!/usr/bin/env bash
# Archive gdbm's sources, make its object or its next revision, and
# tag it 1.26.0 (packages/gdbm/README.md, packages/common/import.sh).
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
    "6a24504a14de4a744103dcb936be976df6fbe88ccff26065e54c1c47946f4a5e gdbm-1.26.tar.gz
     https://ftp.gnu.org/gnu/gdbm/gdbm-1.26.tar.gz
     https://mirrors.kernel.org/gnu/gdbm/gdbm-1.26.tar.gz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "gdbm 1.26 for x86 and x86-64" 1.26.0
