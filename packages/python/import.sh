#!/usr/bin/env bash
# Archive python's sources, make its object or its next revision, and
# tag it 3.11.1 (packages/python/README.md, packages/common/import.sh).
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
    "85879192f2cffd56cb16c092905949ebf3e5e394b7f764723529637901dfb58f Python-3.11.1.tar.xz
     https://www.python.org/ftp/python/3.11.1/Python-3.11.1.tar.xz
     https://samuelt.me/pub/live-bootstrap/Python-3.11.1.tar.xz
     https://live-bootstrap.stikonas.eu/Python-3.11.1.tar.xz
     https://archive.org/download/live-bootstrap-sources/Python-3.11.1.tar.xz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "python 3.11.1 for x86 and x86-64" 3.11.1
