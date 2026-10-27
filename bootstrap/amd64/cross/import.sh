#!/usr/bin/env bash
# Archive the x86_64-linux-musl cross toolchain's sources and make its
# object (bootstrap/amd64/README.md, stage 1).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# 1. binutils, gcc, musl, and Linux's tarballs are archived as file
#    sources with `kairo source fetch-file`, from upstream or else one of
#    live-bootstrap's mirrors.
# 2. This directory's kairo.toml and src/ are committed as SHA-256
#    history and created as an object, or, with --wrapper, committed on
#    top of that existing object's history as its next revision.
#
# live-bootstrap's six passes (bootstrap/live-bootstrap*/) and
# linux-filesystem must be imported: pass 6's root,
# data:live-bootstrap:x86:libarchive-3.5.2, is this build's input. Extra
# arguments go before the command in every `kairo` call, e.g. `--store
# <path>`. KAIRO names the kairo binary (default: kairo on PATH). Prints
# the object's id.
set -euo pipefail
shopt -s inherit_errexit

# sha256 name url...
DISTFILES=(
    "ae9a5789e23459e59606e6714723f2d3ffc31c03174191ef0d015bdf06007450 binutils-2.41.tar.xz
     https://mirrors.kernel.org/gnu/binutils/binutils-2.41.tar.xz
     https://samuelt.me/pub/live-bootstrap/binutils-2.41.tar.xz
     https://live-bootstrap.stikonas.eu/binutils-2.41.tar.xz
     https://archive.org/download/live-bootstrap-sources/binutils-2.41.tar.xz"
    "438fd996826b0c82485a29da03a72d71d6e3541a83ec702df4271f6fe025d24e gcc-15.2.0.tar.xz
     https://mirrors.kernel.org/gnu/gcc/gcc-15.2.0/gcc-15.2.0.tar.xz
     https://samuelt.me/pub/live-bootstrap/gcc-15.2.0.tar.xz
     https://live-bootstrap.stikonas.eu/gcc-15.2.0.tar.xz
     https://archive.org/download/live-bootstrap-sources/gcc-15.2.0.tar.xz"
    "a9a118bbe84d8764da0ea0d28b3ab3fae8477fc7e4085d90102b8596fc7c75e4 musl-1.2.5.tar.gz
     https://musl.libc.org/releases/musl-1.2.5.tar.gz
     https://samuelt.me/pub/live-bootstrap/musl-1.2.5.tar.gz
     https://live-bootstrap.stikonas.eu/musl-1.2.5.tar.gz
     https://archive.org/download/live-bootstrap-sources/musl-1.2.5.tar.gz"
    "0820fdb7971c6974338081c11fbf2dc869870501e7bdcac4d0ed58ba1f57b61c linux-4.14.336.tar.xz
     https://cdn.kernel.org/pub/linux/kernel/v4.x/linux-4.14.336.tar.xz
     https://mirrors.kernel.org/pub/linux/kernel/v4.x/linux-4.14.336.tar.xz"
)

KAIRO=${KAIRO:-kairo}
HERE=$(cd "$(dirname "$0")" && pwd)

kairo() { "$KAIRO" "${KAIRO_ARGS[@]}" "$@"; }
field() { sed -n "s/^$1 = //p" | head -n 1; }
fail() {
    echo "import.sh: $*" >&2
    exit 1
}
# shellcheck source=../../wrapper.sh
. "$HERE/../../wrapper.sh"
parse_import_args "$@"

# 1. The sources, each from the first mirror that serves its bytes.
fetch_distfiles

# 2. The object (../../wrapper.sh).
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
cp -r "$HERE/kairo.toml" "$HERE/src" "$WORK/"
commit_wrapper "amd64 stage 1: the x86_64-linux-musl cross toolchain" kairo.toml src/build.sh

cat <<OUT
object = $WRAPPER

# With live-bootstrap's passes and linux-filesystem imported:
#   kairo build object:$WRAPPER --target x86 --actor $ACTOR
OUT
