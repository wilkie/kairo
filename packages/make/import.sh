#!/usr/bin/env bash
# Archive GNU make's sources, make its object or its next revision, and
# tag it with the release's version (packages/make/README.md).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# 1. The tarball is archived as a file source with `kairo source
#    fetch-file`, from upstream or else one of live-bootstrap's mirrors.
# 2. This directory's kairo.toml and src/ are committed as SHA-256
#    history and created as an object, or, with --wrapper, committed on
#    top of that existing object's history as its next revision, and the
#    revision is tagged 4.2.1 (DECISIONS.md §59).
#
# What the targets build with must be imported: packages/bootstrap-base/
# and the bootstraps it names. Extra arguments go before the command in
# every `kairo` call, e.g. `--store <path>`. KAIRO names the kairo binary
# (default: kairo on PATH). Prints the object's id.
set -euo pipefail
shopt -s inherit_errexit

# sha256 name url...
DISTFILES=(
    "e40b8f018c1da64edd1cc9a6fce5fa63b2e707e404e20cad91fbae337c98a5b7 make-4.2.1.tar.gz
     https://mirrors.kernel.org/gnu/make/make-4.2.1.tar.gz
     https://samuelt.me/pub/live-bootstrap/make-4.2.1.tar.gz
     https://live-bootstrap.stikonas.eu/make-4.2.1.tar.gz
     https://archive.org/download/live-bootstrap-sources/make-4.2.1.tar.gz"
)

KAIRO=${KAIRO:-kairo}
HERE=$(cd "$(dirname "$0")" && pwd)

kairo() { "$KAIRO" "${KAIRO_ARGS[@]}" "$@"; }
field() { sed -n "s/^$1 = //p" | head -n 1; }
fail() {
    echo "import.sh: $*" >&2
    exit 1
}
# shellcheck source=../../bootstrap/wrapper.sh
. "$HERE/../../bootstrap/wrapper.sh"
parse_import_args "$@"

# 1. The sources, each from the first mirror that serves its bytes.
fetch_distfiles

# 2. The object (../../bootstrap/wrapper.sh), tagged with the release.
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
cp -r "$HERE/kairo.toml" "$HERE/src" "$WORK/"
commit_wrapper "make 4.2.1 for x86 and x86-64" kairo.toml src/build.sh
tag_release 4.2.1

cat <<OUT
object = $WRAPPER

# With what it builds with imported:
#   kairo build object:$WRAPPER --target x86 --actor $ACTOR
#   kairo build object:$WRAPPER --target x86-64 --actor $ACTOR
OUT
