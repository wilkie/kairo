#!/usr/bin/env bash
# Archive grep's sources, make its object or its next revision, and tag
# it 3.12.0+2 (packages/grep/README.md, packages/common/import.sh).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# What it builds with must be imported: packages/base-round/ and the
# base's round 1 it names. Prints the object's id.
set -euo pipefail
shopt -s inherit_errexit

# sha256 name url...
DISTFILES=(
    "2649b27c0e90e632eadcd757be06c6e9a4f48d941de51e7c0f83ff76408a07b9 grep-3.12.tar.xz
     https://mirrors.kernel.org/gnu/grep/grep-3.12.tar.xz
     https://samuelt.me/pub/live-bootstrap/grep-3.12.tar.xz
     https://live-bootstrap.stikonas.eu/grep-3.12.tar.xz
     https://archive.org/download/live-bootstrap-sources/grep-3.12.tar.xz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "grep 3.12 for x86 and x86-64, round 2, built with round 1" 3.12.0+2
