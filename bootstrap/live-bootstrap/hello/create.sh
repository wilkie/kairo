#!/usr/bin/env bash
# Create the hello object from this directory's kairo.toml, as the
# wrappers' import.sh does (../../wrapper.sh). Prints its object id.
#
#   create.sh <actor id> [--wrapper <object id>] [kairo args...]
set -euo pipefail
shopt -s inherit_errexit
KAIRO=${KAIRO:-kairo}
HERE=$(cd "$(dirname "$0")" && pwd)
kairo() { "$KAIRO" "${KAIRO_ARGS[@]}" "$@"; }
field() { sed -n "s/^$1 = //p" | head -n 1; }
fail() {
    echo "create.sh: $*" >&2
    exit 1
}
# shellcheck source=../../wrapper.sh
. "$HERE/../../wrapper.sh"
parse_import_args "$@"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
cp "$HERE/kairo.toml" "$WORK/"
commit_wrapper "hello from live-bootstrap's gcc" kairo.toml
echo "object = $WRAPPER"
