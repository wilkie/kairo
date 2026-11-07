#!/usr/bin/env bash
# Archive git's sources, make its object or its next revision, and
# tag it 2.56.0 (packages/git/README.md, packages/common/import.sh).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# What it builds with must be imported: packages/build-base/ and the
# packages it names, and curl, expat, pcre2, openssl, and zlib.
# Prints the object's id.
set -euo pipefail
shopt -s inherit_errexit

# sha256 name url...
DISTFILES=(
    "26c56c296b38c0695b26fa95f475f1d01704d2d38e73465ca30b0b2f5dc789d3 git-2.56.0.tar.xz
     https://www.kernel.org/pub/software/scm/git/git-2.56.0.tar.xz
     https://mirrors.edge.kernel.org/pub/software/scm/git/git-2.56.0.tar.xz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "git 2.56.0 for x86 and x86-64" 2.56.0
