#!/usr/bin/env bash
# Archive make's sources, make its object or its next revision, and tag
# it 4.2.1+2 (packages/make/README.md, packages/common/import.sh).
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
    "e40b8f018c1da64edd1cc9a6fce5fa63b2e707e404e20cad91fbae337c98a5b7 make-4.2.1.tar.gz
     https://mirrors.kernel.org/gnu/make/make-4.2.1.tar.gz
     https://samuelt.me/pub/live-bootstrap/make-4.2.1.tar.gz
     https://live-bootstrap.stikonas.eu/make-4.2.1.tar.gz
     https://archive.org/download/live-bootstrap-sources/make-4.2.1.tar.gz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "make 4.2.1 for x86 and x86-64, round 2, built with round 1" 4.2.1+2
