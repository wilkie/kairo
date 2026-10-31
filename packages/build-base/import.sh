#!/usr/bin/env bash
# Make the build-base object (packages/build-base/README.md).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# This directory's kairo.toml is committed as SHA-256 history and
# created as an object, or, with --wrapper, committed on top of that
# existing object's history as its next revision. It has no sources: it
# doesn't build, and what it names are labels. Extra arguments go before
# the command in every `kairo` call, e.g. `--store <path>`. KAIRO names
# the kairo binary (default: kairo on PATH). Prints the object's id.
set -euo pipefail
shopt -s inherit_errexit

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

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
cp "$HERE/kairo.toml" "$WORK/"
commit_wrapper "build-base: what a typical C or C++ package builds with" kairo.toml

cat <<OUT
object = $WRAPPER

# An object that depends on data:build-base takes its dependencies as
# its own.
OUT
