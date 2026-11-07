#!/usr/bin/env bash
# Archive libidn2's sources, make its object or its next revision, and
# tag it 2.3.8 (packages/libidn2/README.md, packages/common/import.sh).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# What it builds with must be imported: packages/build-base/ and the
# packages it names, and libunistring.
# Prints the object's id.
set -euo pipefail
shopt -s inherit_errexit

# sha256 name url...
DISTFILES=(
    "f557911bf6171621e1f72ff35f5b1825bb35b52ed45325dcdee931e5d3c0787a libidn2-2.3.8.tar.gz
     https://ftp.gnu.org/gnu/libidn/libidn2-2.3.8.tar.gz
     https://mirrors.kernel.org/gnu/libidn/libidn2-2.3.8.tar.gz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "libidn2 2.3.8 for x86 and x86-64" 2.3.8
