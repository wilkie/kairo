#!/usr/bin/env bash
# Archive gawk's sources, make its object or its next revision, and tag
# it 5.3.0+3 (packages/gawk/README.md, packages/common/import.sh).
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
    "ca9c16d3d11d0ff8c69d79dc0b47267e1329a69b39b799895604ed447d3ca90b gawk-5.3.0.tar.xz
     https://mirrors.kernel.org/gnu/gawk/gawk-5.3.0.tar.xz
     https://samuelt.me/pub/live-bootstrap/gawk-5.3.0.tar.xz
     https://live-bootstrap.stikonas.eu/gawk-5.3.0.tar.xz
     https://archive.org/download/live-bootstrap-sources/gawk-5.3.0.tar.xz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "gawk 5.3.0 for x86 and x86-64, round 3, built with round 2" 5.3.0+3
