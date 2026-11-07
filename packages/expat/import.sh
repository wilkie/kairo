#!/usr/bin/env bash
# Archive expat's sources, make its object or its next revision, and
# tag it 2.10.0 (packages/expat/README.md, packages/common/import.sh).
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
    "c624ba2998fc65df1163a49641ff3f9ab73138b11641d92c86bee0fb01b1bc7f expat-2.10.0.tar.xz
     https://github.com/libexpat/libexpat/releases/download/R_2_10_0/expat-2.10.0.tar.xz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "expat 2.10.0 for x86 and x86-64" 2.10.0
