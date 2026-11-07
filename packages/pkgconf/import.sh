#!/usr/bin/env bash
# Archive pkgconf's sources, make its object or its next revision, and
# tag it 3.0.10 (packages/pkgconf/README.md, packages/common/import.sh).
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
    "9371b2f6372da574d4c6dc96fe9b1463037bcf6afcb6671c1555afde7b17e2de pkgconf-3.0.10.tar.xz
     https://distfiles.ariadne.space/pkgconf/pkgconf-3.0.10.tar.xz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "pkgconf 3.0.10 for x86 and x86-64" 3.0.10
