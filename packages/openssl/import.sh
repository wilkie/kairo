#!/usr/bin/env bash
# Archive openssl's sources, make its object or its next revision, and
# tag it 3.5.9 (packages/openssl/README.md, packages/common/import.sh).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# What it builds with must be imported: packages/build-base/ and the
# packages it names, and perl.
# Prints the object's id.
set -euo pipefail
shopt -s inherit_errexit

# sha256 name url...
DISTFILES=(
    "603f5602e2eef00d77fbd429d34dcd5822bb301757a1bc9cdb24c670f1eb859a openssl-3.5.9.tar.gz
     https://github.com/openssl/openssl/releases/download/openssl-3.5.9/openssl-3.5.9.tar.gz
     https://www.openssl.org/source/openssl-3.5.9.tar.gz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "openssl 3.5.9 for x86 and x86-64" 3.5.9
