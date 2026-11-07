#!/usr/bin/env bash
# Archive sqlite's sources, make its object or its next revision, and
# tag it 3.54.1 (packages/sqlite/README.md, packages/common/import.sh).
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
    "926ef8d3eab93fbf6e06011901273eec076b25594a4e1cdadd50c987f4f6d64e sqlite-autoconf-3540100.tar.gz
     https://sqlite.org/2026/sqlite-autoconf-3540100.tar.gz
     https://www.sqlite.org/2026/sqlite-autoconf-3540100.tar.gz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "sqlite 3.54.1 for x86 and x86-64" 3.54.1
