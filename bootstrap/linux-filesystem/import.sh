#!/usr/bin/env bash
# Make the linux-filesystem object (specs/DECISIONS.md §50): this
# directory's kairo.toml and its files/ links, committed as SHA-256
# history with fixed author and dates, so the same files always give
# the same commit.
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# With --wrapper, the files are committed on top of that existing
# object's history as its next revision. Extra arguments go before the
# command in every `kairo` call, e.g. `--store <path>`. KAIRO names the
# kairo binary (default: kairo on PATH). Prints the object's id.
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
# shellcheck source=../wrapper.sh
. "$HERE/../wrapper.sh"
parse_import_args "$@"

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
# The links are committed as links.
cp -a "$HERE/kairo.toml" "$HERE/files" "$WORK/"
commit_wrapper "linux-filesystem: the merged-/usr layout" kairo.toml files/bin files/lib files/sbin
# A release, 1.0.0, offered by label (DECISIONS.md §61).
tag_release 1.0.0

cat <<OUT
object = $WRAPPER
OUT
