#!/usr/bin/env bash
# Archive sed's sources, make its object or its next revision, and tag
# it 4.8.0+3 (packages/sed/README.md, packages/common/import.sh).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# What it builds with must be imported: packages/base-round/ and the
# base's round 2 it names. Prints the object's id.
set -euo pipefail
shopt -s inherit_errexit

# sha256 name url...
DISTFILES=(
    "f79b0cfea71b37a8eeec8490db6c5f7ae7719c35587f21edb0617f370eeff633 sed-4.8.tar.xz
     https://mirrors.kernel.org/gnu/sed/sed-4.8.tar.xz
     https://samuelt.me/pub/live-bootstrap/sed-4.8.tar.xz
     https://live-bootstrap.stikonas.eu/sed-4.8.tar.xz
     https://archive.org/download/live-bootstrap-sources/sed-4.8.tar.xz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "sed 4.8 for x86 and x86-64, round 3, built with round 2" 4.8.0+3
