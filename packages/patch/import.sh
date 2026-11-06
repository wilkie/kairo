#!/usr/bin/env bash
# Archive patch's sources, make its object or its next revision, and
# tag it 2.8.0 (packages/patch/README.md, packages/common/import.sh).
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
    "f87cee69eec2b4fcbf60a396b030ad6aa3415f192aa5f7ee84cad5e11f7f5ae3 patch-2.8.tar.xz
     https://mirrors.kernel.org/gnu/patch/patch-2.8.tar.xz
     https://samuelt.me/pub/live-bootstrap/patch-2.8.tar.xz
     https://live-bootstrap.stikonas.eu/patch-2.8.tar.xz
     https://archive.org/download/live-bootstrap-sources/patch-2.8.tar.xz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "patch 2.8 for x86 and x86-64" 2.8.0
