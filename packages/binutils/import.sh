#!/usr/bin/env bash
# Archive GNU binutils's sources, make its object or its next revision, and
# tag it with the release's version (packages/binutils/README.md).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# 1. The tarball is archived as a file source with `kairo source
#    fetch-file`, from upstream or else one of live-bootstrap's mirrors.
# 2. This directory's kairo.toml and src/ are committed as SHA-256
#    history and created as an object, or, with --wrapper, committed on
#    top of that existing object's history as its next revision, and the
#    revision is tagged 2.41.0 (DECISIONS.md §59).
#
# What the targets build with must be imported: packages/bootstrap-base/
# and the bootstraps it names. Extra arguments go before the command in
# every `kairo` call, e.g. `--store <path>`. KAIRO names the kairo binary
# (default: kairo on PATH). Prints the object's id.
set -euo pipefail
shopt -s inherit_errexit

# sha256 name url...
DISTFILES=(
    "ae9a5789e23459e59606e6714723f2d3ffc31c03174191ef0d015bdf06007450 binutils-2.41.tar.xz
     https://mirrors.kernel.org/gnu/binutils/binutils-2.41.tar.xz
     https://samuelt.me/pub/live-bootstrap/binutils-2.41.tar.xz
     https://live-bootstrap.stikonas.eu/binutils-2.41.tar.xz
     https://archive.org/download/live-bootstrap-sources/binutils-2.41.tar.xz"
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
commit_wrapper "binutils 2.41 for x86 and x86-64" kairo.toml src/build.sh
tag_release 2.41.0

cat <<OUT
object = $WRAPPER

# With what it builds with imported:
#   kairo build object:$WRAPPER --target x86 --actor $ACTOR
#   kairo build object:$WRAPPER --target x86-64 --actor $ACTOR
OUT
