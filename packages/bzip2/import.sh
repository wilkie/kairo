#!/usr/bin/env bash
# Archive bzip2's sources, make its object or its next revision, and tag
# it 1.0.8+4 (packages/bzip2/README.md, packages/common/import.sh).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# What it builds with must be imported: packages/base-round/ and the
# base's round 3 it names. Prints the object's id.
set -euo pipefail
shopt -s inherit_errexit

# sha256 name url...
DISTFILES=(
    "ab5a03176ee106d3f0fa90e381da478ddae405918153cca248e682cd0c4a2269 bzip2-1.0.8.tar.gz
     https://sourceware.org/pub/bzip2/bzip2-1.0.8.tar.gz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "bzip2 1.0.8 for x86 and x86-64, round 4, built with round 3" 1.0.8+4
