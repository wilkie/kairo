#!/usr/bin/env bash
# Archive coreutils's sources, make its object or its next revision, and
# tag it 9.4.0 (packages/coreutils/README.md, packages/common/import.sh).
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
    "ea613a4cf44612326e917201bbbcdfbd301de21ffc3b59b6e5c07e040b275e52 coreutils-9.4.tar.xz
     https://mirrors.kernel.org/gnu/coreutils/coreutils-9.4.tar.xz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "coreutils 9.4 for x86 and x86-64" 9.4.0
