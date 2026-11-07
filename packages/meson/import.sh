#!/usr/bin/env bash
# Archive meson's sources, make its object or its next revision, and
# tag it 1.12.1 (packages/meson/README.md, packages/common/import.sh).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# What it builds with must be imported: packages/build-base/ and the
# packages it names, and python, ninja, and pkgconf.
# Prints the object's id.
set -euo pipefail
shopt -s inherit_errexit

# sha256 name url...
DISTFILES=(
    "ab0a6ca09f8ef70c564c8241fb5a23957886a0b53fb58412b5e07eaf07dba743 meson-1.12.1.tar.gz
     https://github.com/mesonbuild/meson/releases/download/1.12.1/meson-1.12.1.tar.gz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "meson 1.12.1 for x86 and x86-64" 1.12.1
