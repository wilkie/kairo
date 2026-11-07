#!/usr/bin/env bash
# Archive libffi's sources, make its object or its next revision, and
# tag it 3.5.2 (packages/libffi/README.md, packages/common/import.sh).
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
    "f3a3082a23b37c293a4fcd1053147b371f2ff91fa7ea1b2a52e335676bac82dc libffi-3.5.2.tar.gz
     https://github.com/libffi/libffi/releases/download/v3.5.2/libffi-3.5.2.tar.gz
     https://samuelt.me/pub/live-bootstrap/libffi-3.5.2.tar.gz
     https://live-bootstrap.stikonas.eu/libffi-3.5.2.tar.gz
     https://archive.org/download/live-bootstrap-sources/libffi-3.5.2.tar.gz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "libffi 3.5.2 for x86 and x86-64" 3.5.2
