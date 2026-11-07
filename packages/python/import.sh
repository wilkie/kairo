#!/usr/bin/env bash
# Archive python's sources, make its object or its next revision, and
# tag it 3.15.2 (packages/python/README.md, packages/common/import.sh).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# What it builds with must be imported: packages/build-base/ and the
# packages it names, and those of the libraries it builds against.
# Prints the object's id.
set -euo pipefail
shopt -s inherit_errexit

# sha256 name url...
DISTFILES=(
    "8b5536d280142ecf6cdfe15d12a21ba1c65e300bbf7b28a8d0cdb849575797de Python-3.15.2.tar.xz
     https://www.python.org/ftp/python/3.15.2/Python-3.15.2.tar.xz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "python 3.15.2 for x86 and x86-64" 3.15.2
