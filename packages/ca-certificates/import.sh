#!/usr/bin/env bash
# Archive ca-certificates's sources, make its object or its next revision, and
# tag it 3.119.1 (packages/ca-certificates/README.md, packages/common/import.sh).
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
    "a903b3cd05231e39332515ef7ebe37e697262f39515a52015c23c62805b73cd0 certdata.txt
     https://hg-edge.mozilla.org/projects/nss/raw-file/NSS_3_119_1_RTM/lib/ckfw/builtins/certdata.txt
     https://samuelt.me/pub/live-bootstrap/certdata.txt
     https://live-bootstrap.stikonas.eu/certdata.txt
     https://archive.org/download/live-bootstrap-sources/certdata.txt"
    "955f6e729ad6b3566260e8fef68620e76ba3c31acf0a18524416a185acf77992 curl-8.17.0.tar.xz
     https://curl.se/download/curl-8.17.0.tar.xz
     https://samuelt.me/pub/live-bootstrap/curl-8.17.0.tar.xz
     https://live-bootstrap.stikonas.eu/curl-8.17.0.tar.xz
     https://archive.org/download/live-bootstrap-sources/curl-8.17.0.tar.xz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "ca-certificates 3.119.1 for x86 and x86-64" 3.119.1
