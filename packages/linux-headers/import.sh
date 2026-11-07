#!/usr/bin/env bash
# Archive linux-headers's sources, make its object or its next revision,
# and tag it 4.14.336+2 (packages/linux-headers/README.md,
# packages/common/import.sh).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# What it builds with must be imported: packages/base-round/ and the
# base's round 1 it names. Prints the object's id.
set -euo pipefail
shopt -s inherit_errexit

# sha256 name url...
DISTFILES=(
    "0820fdb7971c6974338081c11fbf2dc869870501e7bdcac4d0ed58ba1f57b61c linux-4.14.336.tar.xz
     https://cdn.kernel.org/pub/linux/kernel/v4.x/linux-4.14.336.tar.xz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "linux-headers 4.14.336 for x86 and x86-64, round 2, built with round 1" 4.14.336+2
