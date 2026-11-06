#!/usr/bin/env bash
# Archive findutils's sources, make its object or its next revision, and
# tag it 4.10.0 (packages/findutils/README.md, packages/common/import.sh).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# What it builds with must be imported: packages/bootstrap-base/ and the
# bootstraps it names. Prints the object's id.
set -euo pipefail
shopt -s inherit_errexit

# sha256 name url...
DISTFILES=(
    "1387e0b67ff247d2abde998f90dfbf70c1491391a59ddfecb8ae698789f0a4f5 findutils-4.10.0.tar.xz
     https://mirrors.kernel.org/gnu/findutils/findutils-4.10.0.tar.xz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "findutils 4.10.0 for x86 and x86-64" 4.10.0
