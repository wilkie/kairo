#!/usr/bin/env bash
# Archive binutils's sources, make its object or its next revision, and tag
# it 2.41.0 (packages/binutils/README.md, packages/common/import.sh).
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
    "ae9a5789e23459e59606e6714723f2d3ffc31c03174191ef0d015bdf06007450 binutils-2.41.tar.xz
     https://mirrors.kernel.org/gnu/binutils/binutils-2.41.tar.xz
     https://samuelt.me/pub/live-bootstrap/binutils-2.41.tar.xz
     https://live-bootstrap.stikonas.eu/binutils-2.41.tar.xz
     https://archive.org/download/live-bootstrap-sources/binutils-2.41.tar.xz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "binutils 2.41 for x86 and x86-64" 2.41.0
