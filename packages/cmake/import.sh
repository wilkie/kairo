#!/usr/bin/env bash
# Archive cmake's sources, make its object or its next revision, and
# tag it 4.4.5 (packages/cmake/README.md, packages/common/import.sh).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# What it builds with must be imported: packages/build-base/ and the
# packages it names, and openssl.
# Prints the object's id.
set -euo pipefail
shopt -s inherit_errexit

# sha256 name url...
DISTFILES=(
    "a0c54c88abb8078dd8eccadb717fc6bb194bf18e7d44c86309910288886e04e2 cmake-4.4.5.tar.gz
     https://github.com/Kitware/CMake/releases/download/v4.4.5/cmake-4.4.5.tar.gz
     https://cmake.org/files/v4.4/cmake-4.4.5.tar.gz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "cmake 4.4.5 for x86 and x86-64" 4.4.5
