#!/usr/bin/env bash
# Archive util-linux's sources, make its object or its next revision, and
# tag it 2.42.4 (packages/util-linux/README.md, packages/common/import.sh).
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
    "fbd62a100ab7bb8746ba0661255c3c48185b1e9021507c624da01fbc696330ec util-linux-2.42.4.tar.xz
     https://cdn.kernel.org/pub/linux/utils/util-linux/v2.42/util-linux-2.42.4.tar.xz
     https://mirrors.edge.kernel.org/pub/linux/utils/util-linux/v2.42/util-linux-2.42.4.tar.xz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "util-linux 2.42.4 for x86 and x86-64" 2.42.4
