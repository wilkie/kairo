#!/usr/bin/env bash
# Archive the distfiles of live-bootstrap's gcc 10.5.0 and binutils 2.41
# rebuilt, and make the native object that runs them for x86, going on
# from pass 4's root (specs/DECISIONS.md §49).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# 1. `kairo source fetch https://github.com/fosslinux/live-bootstrap`
#    archives live-bootstrap as a source object, as the earlier passes'
#    imports do; a node that has imported them holds it already.
# 2. Each distfile this pass's steps list is archived as a file source
#    with `kairo source fetch-file`, from upstream or else one of
#    live-bootstrap's mirrors: its maintainers' two, then archive.org.
#    The gnulib snapshots have no upstream URL; upstream's mirror.sh
#    makes them with `git archive`, and the mirrors serve those bytes.
#    ftp.unicode.org's files are fetched from www.unicode.org, which
#    serves them over https.
# 3. The native wrapper -- this directory's kairo.toml and
#    steps/manifest, and the start.sh and package.sh every later pass
#    shares (../live-bootstrap/pass/) and the musl and gcc splits
#    (../live-bootstrap/split-musl.sh, split-gcc.sh) -- is committed as
#    SHA-256 history and created as an object, or, with --wrapper,
#    committed on top of that existing wrapper object's history as its
#    next revision.
#
# Passes 1 to 4 (bootstrap/live-bootstrap/,
# bootstrap/live-bootstrap-perl/, bootstrap/live-bootstrap-gcc-4.7.4/,
# and bootstrap/live-bootstrap-python/) must be imported: pass 4's
# artifact, data:live-bootstrap:x86:autogen-5.18.16, is this build's
# input, and is built first, with the earlier passes', if it isn't held.
# Extra arguments go before the command in every `kairo` call, e.g.
# `--store <path>`. KAIRO names the kairo binary (default: kairo on
# PATH). Prints the wrapper's object id.
set -euo pipefail
shopt -s inherit_errexit

LIVE_BOOTSTRAP_URL=https://github.com/fosslinux/live-bootstrap

# sha256 name url... (from steps/<step>/sources at b1ceced7, upstream's
# URL first, then live-bootstrap's mirrors)
DISTFILES=(
    "ae9a5789e23459e59606e6714723f2d3ffc31c03174191ef0d015bdf06007450 binutils-2.41.tar.xz
     https://mirrors.kernel.org/gnu/binutils/binutils-2.41.tar.xz
     https://samuelt.me/pub/live-bootstrap/binutils-2.41.tar.xz
     https://live-bootstrap.stikonas.eu/binutils-2.41.tar.xz
     https://archive.org/download/live-bootstrap-sources/binutils-2.41.tar.xz"
    "25109543fdf46f397c347b5d8b7a2c7e5694a5a51cce4b9c6e1ea8a71ca307c1 gcc-10.5.0.tar.xz
     https://mirrors.kernel.org/gnu/gcc/gcc-10.5.0/gcc-10.5.0.tar.xz
     https://samuelt.me/pub/live-bootstrap/gcc-10.5.0.tar.xz
     https://live-bootstrap.stikonas.eu/gcc-10.5.0.tar.xz
     https://archive.org/download/live-bootstrap-sources/gcc-10.5.0.tar.xz"
)

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

# 1. live-bootstrap, and the submodules it pins.
kairo source fetch "$LIVE_BOOTSTRAP_URL" --actor "$ACTOR" --require-complete >&2

# 2. The distfiles, each from the first mirror that serves its bytes.
fetch_distfiles

# 3. The native wrapper (../wrapper.sh).
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
mkdir "$WORK/steps"
cp "$HERE/kairo.toml" "$HERE/../live-bootstrap/pass/start.sh" "$HERE/../live-bootstrap/pass/package.sh" \
    "$HERE/../live-bootstrap/split-musl.sh" "$HERE/../live-bootstrap/split-gcc.sh" "$WORK/"
cp "$HERE/steps/manifest" "$WORK/steps/"
commit_wrapper "live-bootstrap b1ceced7 x86 pass 5: to gcc 10.5.0" kairo.toml start.sh package.sh split-musl.sh split-gcc.sh steps/manifest

cat <<OUT
wrapper = $WRAPPER

# With passes 1 to 4 imported, and the same config:
#   kairo build object:$WRAPPER --target x86 --actor $ACTOR
OUT
