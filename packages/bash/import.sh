#!/usr/bin/env bash
# Archive bash's sources, make its object or its next revision, and tag
# it 5.2.15+3 (packages/bash/README.md, packages/common/import.sh).
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
    "13720965b5f4fc3a0d4b61dd37e7565c741da9a5be24edc2ae00182fc1b3588c bash-5.2.15.tar.gz
     https://mirrors.kernel.org/gnu/bash/bash-5.2.15.tar.gz
     https://samuelt.me/pub/live-bootstrap/bash-5.2.15.tar.gz
     https://live-bootstrap.stikonas.eu/bash-5.2.15.tar.gz
     https://archive.org/download/live-bootstrap-sources/bash-5.2.15.tar.gz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "bash 5.2.15 for x86 and x86-64, round 3, built with round 2" 5.2.15+3
