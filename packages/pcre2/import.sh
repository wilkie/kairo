#!/usr/bin/env bash
# Archive pcre2's sources, make its object or its next revision, and
# tag it 10.49.0 (packages/pcre2/README.md, packages/common/import.sh).
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
    "53c156e1ba416a20da8e65395daa132da0d80e76910424caca3fcdae7831d384 pcre2-10.49.tar.bz2
     https://github.com/PCRE2Project/pcre2/releases/download/pcre2-10.49/pcre2-10.49.tar.bz2"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "pcre2 10.49 for x86 and x86-64" 10.49.0
