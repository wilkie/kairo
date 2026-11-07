#!/usr/bin/env bash
# Archive nghttp2's sources, make its object or its next revision, and
# tag it 1.71.0 (packages/nghttp2/README.md, packages/common/import.sh).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# What it builds with must be imported: packages/build-base/ and the
# packages it names, and pkgconf.
# Prints the object's id.
set -euo pipefail
shopt -s inherit_errexit

# sha256 name url...
DISTFILES=(
    "c0f55121d88650e4b9227aec28cebaf664d2417ca60fe70be0f8d38829f52116 nghttp2-1.71.0.tar.xz
     https://github.com/nghttp2/nghttp2/releases/download/v1.71.0/nghttp2-1.71.0.tar.xz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "nghttp2 1.71.0 for x86 and x86-64" 1.71.0
