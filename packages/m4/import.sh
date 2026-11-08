#!/usr/bin/env bash
# Archive m4's sources, make its object or its next revision, and tag it
# 1.4.19+4 (packages/m4/README.md, packages/common/import.sh).
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
    "63aede5c6d33b6d9b13511cd0be2cac046f2e70fd0a07aa9573a04a82783af96 m4-1.4.19.tar.xz
     https://mirrors.kernel.org/gnu/m4/m4-1.4.19.tar.xz
     https://samuelt.me/pub/live-bootstrap/m4-1.4.19.tar.xz
     https://live-bootstrap.stikonas.eu/m4-1.4.19.tar.xz
     https://archive.org/download/live-bootstrap-sources/m4-1.4.19.tar.xz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "m4 1.4.19 for x86 and x86-64, round 4, built with round 3" 1.4.19+4
