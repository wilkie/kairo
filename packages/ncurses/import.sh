#!/usr/bin/env bash
# Archive ncurses's sources, make its object or its next revision, and
# tag it 6.6.0 (packages/ncurses/README.md, packages/common/import.sh).
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
    "355b4cbbed880b0381a04c46617b7656e362585d52e9cf84a67e2009b749ff11 ncurses-6.6.tar.gz
     https://ftp.gnu.org/gnu/ncurses/ncurses-6.6.tar.gz
     https://mirrors.kernel.org/gnu/ncurses/ncurses-6.6.tar.gz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "ncurses 6.6 for x86 and x86-64" 6.6.0
