#!/usr/bin/env bash
# Archive tar's sources, make its object or its next revision, and tag
# it 1.34.0+3 (packages/tar/README.md, packages/common/import.sh).
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
    "63bebd26879c5e1eea4352f0d03c991f966aeb3ddeb3c7445c902568d5411d28 tar-1.34.tar.xz
     https://mirrors.kernel.org/gnu/tar/tar-1.34.tar.xz
     https://samuelt.me/pub/live-bootstrap/tar-1.34.tar.xz
     https://live-bootstrap.stikonas.eu/tar-1.34.tar.xz
     https://archive.org/download/live-bootstrap-sources/tar-1.34.tar.xz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "tar 1.34 for x86 and x86-64, round 3, built with round 2" 1.34.0+3
