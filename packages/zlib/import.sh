#!/usr/bin/env bash
# Archive zlib's sources, make its object or its next revision, and
# tag it 1.3.1 (packages/zlib/README.md, packages/common/import.sh).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# What it builds with must be imported: packages/build-base/ and the
# packages it names.
# Prints the object's id.
set -euo pipefail
shopt -s inherit_errexit

# sha256 name url...
DISTFILES=(
    "9a93b2b7dfdac77ceba5a558a580e74667dd6fede4585b91eefb60f03b72df23 zlib-1.3.1.tar.gz
     https://zlib.net/fossils/zlib-1.3.1.tar.gz
     https://samuelt.me/pub/live-bootstrap/zlib-1.3.1.tar.gz
     https://live-bootstrap.stikonas.eu/zlib-1.3.1.tar.gz
     https://archive.org/download/live-bootstrap-sources/zlib-1.3.1.tar.gz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "zlib 1.3.1 for x86 and x86-64" 1.3.1
