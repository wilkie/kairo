#!/usr/bin/env bash
# Archive zstd's sources, make its object or its next revision, and
# tag it 1.5.7 (packages/zstd/README.md, packages/common/import.sh).
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
    "eb33e51f49a15e023950cd7825ca74a4a2b43db8354825ac24fc1b7ee09e6fa3 zstd-1.5.7.tar.gz
     https://github.com/meta/zstd/releases/download/v1.5.7/zstd-1.5.7.tar.gz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "zstd 1.5.7 for x86 and x86-64" 1.5.7
