#!/usr/bin/env bash
# Archive ninja's sources, make its object or its next revision, and
# tag it 1.13.2 (packages/ninja/README.md, packages/common/import.sh).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# What it builds with must be imported: packages/build-base/ and the
# packages it names, and python.
# Prints the object's id.
set -euo pipefail
shopt -s inherit_errexit

# sha256 name url...
DISTFILES=(
    "974d6b2f4eeefa25625d34da3cb36bdcebe7fbce40f4c16ac0835fd1c0cbae17 ninja-1.13.2.tar.gz
     https://github.com/ninja-build/ninja/archive/refs/tags/v1.13.2.tar.gz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "ninja 1.13.2 for x86 and x86-64" 1.13.2
