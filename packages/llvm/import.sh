#!/usr/bin/env bash
# Archive LLVM's sources, make its object or its next revision, and tag
# it 21.1.8 (packages/llvm/README.md, packages/common/import.sh).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# What it builds with must be imported: packages/build-base/ and the
# packages it names, and cmake, ninja, python, zlib, and zstd.
# Prints the object's id.
set -euo pipefail
shopt -s inherit_errexit

# sha256 name url...
DISTFILES=(
    "4633a23617fa31a3ea51242586ea7fb1da7140e426bd62fc164261fe036aa142 llvm-project-21.1.8.src.tar.xz
     https://github.com/llvm/llvm-project/releases/download/llvmorg-21.1.8/llvm-project-21.1.8.src.tar.xz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "llvm 21.1.8 for x86-64" 21.1.8
