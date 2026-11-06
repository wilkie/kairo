#!/usr/bin/env bash
# Archive gzip's sources, make its object or its next revision, and
# tag it 1.13.0 (packages/gzip/README.md, packages/common/import.sh).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# What it builds with must be imported: packages/bootstrap-base/ and the
# bootstraps it names. Prints the object's id.
set -euo pipefail
shopt -s inherit_errexit

# sha256 name url...
DISTFILES=(
    "7454eb6935db17c6655576c2e1b0fabefd38b4d0936e0f87f48cd062ce91a057 gzip-1.13.tar.xz
     https://mirrors.kernel.org/gnu/gzip/gzip-1.13.tar.xz
     https://samuelt.me/pub/live-bootstrap/gzip-1.13.tar.xz
     https://live-bootstrap.stikonas.eu/gzip-1.13.tar.xz
     https://archive.org/download/live-bootstrap-sources/gzip-1.13.tar.xz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "gzip 1.13 for x86 and x86-64" 1.13.0
