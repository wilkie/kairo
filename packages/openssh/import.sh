#!/usr/bin/env bash
# Archive openssh's sources, make its object or its next revision, and
# tag it 10.6.2 (packages/openssh/README.md, packages/common/import.sh).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# What it builds with must be imported: packages/build-base/ and the
# packages it names, and openssl and zlib.
# Prints the object's id.
set -euo pipefail
shopt -s inherit_errexit

# sha256 name url...
DISTFILES=(
    "9ae44872090cf2f06b3cf9ff147e7f16f309671340763ed2f99248398ae29d5b openssh-10.6p2.tar.gz
     https://cdn.openbsd.org/pub/OpenBSD/OpenSSH/portable/openssh-10.6p2.tar.gz
     https://ftp.openbsd.org/pub/OpenBSD/OpenSSH/portable/openssh-10.6p2.tar.gz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "openssh 10.6p2 for x86 and x86-64" 10.6.2
