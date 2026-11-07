#!/usr/bin/env bash
# Archive mpdecimal's sources, make its object or its next revision, and
# tag it 4.0.1 (packages/mpdecimal/README.md, packages/common/import.sh).
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
    "96d33abb4bb0070c7be0fed4246cd38416188325f820468214471938545b1ac8 mpdecimal-4.0.1.tar.gz
     https://www.bytereef.org/software/mpdecimal/releases/mpdecimal-4.0.1.tar.gz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "mpdecimal 4.0.1 for x86 and x86-64" 4.0.1
