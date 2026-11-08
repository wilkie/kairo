#!/usr/bin/env bash
# Archive diffutils's sources, make its object or its next revision, and
# tag it 3.10.0+4 (packages/diffutils/README.md,
# packages/common/import.sh).
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
    "90e5e93cc724e4ebe12ede80df1634063c7a855692685919bfe60b556c9bd09e diffutils-3.10.tar.xz
     https://mirrors.kernel.org/gnu/diffutils/diffutils-3.10.tar.xz
     https://samuelt.me/pub/live-bootstrap/diffutils-3.10.tar.xz
     https://live-bootstrap.stikonas.eu/diffutils-3.10.tar.xz
     https://archive.org/download/live-bootstrap-sources/diffutils-3.10.tar.xz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "diffutils 3.10 for x86 and x86-64, round 4, built with round 3" 3.10.0+4
