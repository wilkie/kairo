#!/usr/bin/env bash
# Archive musl's sources, make its object or its next revision, and tag
# it 1.2.5+4 (packages/musl/README.md, packages/common/import.sh).
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
    "a9a118bbe84d8764da0ea0d28b3ab3fae8477fc7e4085d90102b8596fc7c75e4 musl-1.2.5.tar.gz
     https://musl.libc.org/releases/musl-1.2.5.tar.gz
     https://samuelt.me/pub/live-bootstrap/musl-1.2.5.tar.gz
     https://live-bootstrap.stikonas.eu/musl-1.2.5.tar.gz
     https://archive.org/download/live-bootstrap-sources/musl-1.2.5.tar.gz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "musl 1.2.5 for x86 and x86-64, round 4, built with round 3" 1.2.5+4
