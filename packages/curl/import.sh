#!/usr/bin/env bash
# Archive curl's sources, make its object or its next revision, and
# tag it 8.23.0 (packages/curl/README.md, packages/common/import.sh).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# What it builds with must be imported: packages/build-base/ and the
# packages it names, and openssl, zlib, zstd, brotli, nghttp2, libpsl,
# libidn2, libssh2, and pkgconf.
# Prints the object's id.
set -euo pipefail
shopt -s inherit_errexit

# sha256 name url...
DISTFILES=(
    "8dadd7d2536a314f982ad05a651b713f426bf7d02cc2c76cffb9796f2218e6c5 curl-8.23.0.tar.xz
     https://curl.se/download/curl-8.23.0.tar.xz
     https://github.com/curl/curl/releases/download/curl-8_23_0/curl-8.23.0.tar.xz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "curl 8.23.0 for x86 and x86-64" 8.23.0
