#!/usr/bin/env bash
# Archive gcc 7.5.0's sources and make its object
# (packages/gcc-7.5.0/README.md).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# 1. gcc's, gmp's, mpfr's, and mpc's tarballs are archived as file
#    sources with `kairo source fetch-file`, from GNU or else one of
#    live-bootstrap's mirrors.
# 2. This directory's kairo.toml and src/ are committed as SHA-256
#    history and created as an object, or, with --wrapper, committed on
#    top of that existing object's history as its next revision.
#
# What the targets build from must be imported: packages/build-base/,
# and live-bootstrap's six passes and linux-filesystem for x86, the
# 64-bit bootstrap's stage 3 for x86-64 (bootstrap/amd64/). Extra
# arguments go before the command in every `kairo` call, e.g. `--store
# <path>`. KAIRO names the kairo binary (default: kairo on PATH). Prints
# the object's id.
set -euo pipefail
shopt -s inherit_errexit

# sha256 name url...
DISTFILES=(
    "b81946e7f01f90528a1f7352ab08cc602b9ccc05d4e44da4bd501c5a189ee661 gcc-7.5.0.tar.xz
     https://mirrors.kernel.org/gnu/gcc/gcc-7.5.0/gcc-7.5.0.tar.xz"
    "fd4829912cddd12f84181c3451cc752be224643e87fac497b69edddadc49b4f2 gmp-6.2.1.tar.xz
     https://mirrors.kernel.org/gnu/gmp/gmp-6.2.1.tar.xz
     https://samuelt.me/pub/live-bootstrap/gmp-6.2.1.tar.xz
     https://live-bootstrap.stikonas.eu/gmp-6.2.1.tar.xz
     https://archive.org/download/live-bootstrap-sources/gmp-6.2.1.tar.xz"
    "0c98a3f1732ff6ca4ea690552079da9c597872d30e96ec28414ee23c95558a7f mpfr-4.1.0.tar.xz
     https://mirrors.kernel.org/gnu/mpfr/mpfr-4.1.0.tar.xz
     https://samuelt.me/pub/live-bootstrap/mpfr-4.1.0.tar.xz
     https://live-bootstrap.stikonas.eu/mpfr-4.1.0.tar.xz
     https://archive.org/download/live-bootstrap-sources/mpfr-4.1.0.tar.xz"
    "17503d2c395dfcf106b622dc142683c1199431d095367c6aacba6eec30340459 mpc-1.2.1.tar.gz
     https://mirrors.kernel.org/gnu/mpc/mpc-1.2.1.tar.gz
     https://samuelt.me/pub/live-bootstrap/mpc-1.2.1.tar.gz
     https://live-bootstrap.stikonas.eu/mpc-1.2.1.tar.gz
     https://archive.org/download/live-bootstrap-sources/mpc-1.2.1.tar.gz"
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

# 2. The object (../../bootstrap/wrapper.sh).
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
cp -r "$HERE/kairo.toml" "$HERE/src" "$WORK/"
commit_wrapper "gcc 7.5.0 for x86 and x86-64" kairo.toml src/build.sh

cat <<OUT
object = $WRAPPER

# With what the targets build from imported:
#   kairo build object:$WRAPPER --target x86 --actor $ACTOR
#   kairo build object:$WRAPPER --target x86-64 --actor $ACTOR
OUT
