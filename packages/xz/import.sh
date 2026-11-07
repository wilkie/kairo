#!/usr/bin/env bash
# Archive xz's sources, make its object or its next revision, and tag it
# 5.6.4+3 (packages/xz/README.md, packages/common/import.sh).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# What it builds with must be imported: packages/base-round/ and the
# base's round 2 it names. Prints the object's id.
set -euo pipefail
shopt -s inherit_errexit

# sha256 name url...
DISTFILES=(
    "176d510c30d80a23b8050bbc048f2ecaacb823ae48b6821727ed6591f0df9200 xz-5.6.4.tar.bz2
     https://github.com/tukaani-project/xz/releases/download/v5.6.4/xz-5.6.4.tar.bz2"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "xz 5.6.4 for x86 and x86-64, round 3, built with round 2" 5.6.4+3
