#!/usr/bin/env bash
# Archive the distfiles of live-bootstrap's last stretch, gcc 15.2.0 and
# the packages after it, and make the native object that runs them for
# x86, going on from pass 5's root (specs/DECISIONS.md §49).
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
#    shares (../live-bootstrap/pass/) and the musl split
#    (../live-bootstrap/split-musl.sh) -- is committed as SHA-256 history
#    and created as an object, or, with --wrapper, committed on top of
#    that existing wrapper object's history as its next revision.
#
# Passes 1 to 5 (bootstrap/live-bootstrap/,
# bootstrap/live-bootstrap-perl/, bootstrap/live-bootstrap-gcc-4.7.4/,
# bootstrap/live-bootstrap-python/, and bootstrap/live-bootstrap-gcc-10.5.0/)
# must be imported: pass 5's artifact,
# data:live-bootstrap:x86:binutils-2.41_1, is this build's input, and is
# built first, with the earlier passes', if it isn't held.
# Extra arguments go before the command in every `kairo` call, e.g.
# `--store <path>`. KAIRO names the kairo binary (default: kairo on
# PATH). Prints the wrapper's object id.
set -euo pipefail
shopt -s inherit_errexit

LIVE_BOOTSTRAP_URL=https://github.com/fosslinux/live-bootstrap

# sha256 name url... (from steps/<step>/sources at b1ceced7, upstream's
# URL first, then live-bootstrap's mirrors)
DISTFILES=(
    "f0b19ff39c3c9a5898a219497ababbadab99d8178acc980155c7e1271089b5a0 libarchive-3.5.2.tar.xz
     https://libarchive.org/downloads/libarchive-3.5.2.tar.xz
     https://samuelt.me/pub/live-bootstrap/libarchive-3.5.2.tar.xz
     https://live-bootstrap.stikonas.eu/libarchive-3.5.2.tar.xz
     https://archive.org/download/live-bootstrap-sources/libarchive-3.5.2.tar.xz"
    "1bd6aa42275313af3141c7cf2e5b964e8b1fd488025caf2f971f43b00776b332 libmd-1.1.0.tar.xz
     https://archive.hadrons.org/software/libmd/libmd-1.1.0.tar.xz
     https://samuelt.me/pub/live-bootstrap/libmd-1.1.0.tar.xz
     https://live-bootstrap.stikonas.eu/libmd-1.1.0.tar.xz
     https://archive.org/download/live-bootstrap-sources/libmd-1.1.0.tar.xz"
    "835a4d043e4415a76668c8f38d5605f4e6f8ac2279dfab7e61c3f06e9228dd1c sarif-v2.1.0-errata01-os-complete.html
     https://docs.oasis-open.org/sarif/sarif/v2.1.0/errata01/os/sarif-v2.1.0-errata01-os-complete.html
     https://samuelt.me/pub/live-bootstrap/sarif-v2.1.0-errata01-os-complete.html
     https://live-bootstrap.stikonas.eu/sarif-v2.1.0-errata01-os-complete.html
     https://archive.org/download/live-bootstrap-sources/sarif-v2.1.0-errata01-os-complete.html"
    "4e98828056d6266bd8f2c93e6ecf12a63a71dbfd70a5ea99ccd4ab6d0745adf0 opendoas-6.8.2.tar.xz
     https://github.com/Duncaen/OpenDoas/releases/download/v6.8.2/opendoas-6.8.2.tar.xz
     https://samuelt.me/pub/live-bootstrap/opendoas-6.8.2.tar.xz
     https://live-bootstrap.stikonas.eu/opendoas-6.8.2.tar.xz
     https://archive.org/download/live-bootstrap-sources/opendoas-6.8.2.tar.xz"
    "6969279236fe3152768573a38c9f83cb9ca109851a5a990aec1fc672ac2cfcd2 shadow-4.14.3.tar.xz
     https://github.com/shadow-maint/shadow/releases/download/4.14.3/shadow-4.14.3.tar.xz
     https://samuelt.me/pub/live-bootstrap/shadow-4.14.3.tar.xz
     https://live-bootstrap.stikonas.eu/shadow-4.14.3.tar.xz
     https://archive.org/download/live-bootstrap-sources/shadow-4.14.3.tar.xz"
    "55fdfa2696fb4d55a592fa9ad14a9df897c7b0008ddb3b30c419914841f85f33 libbsd-0.11.8.tar.xz
     https://libbsd.freedesktop.org/releases/libbsd-0.11.8.tar.xz
     https://samuelt.me/pub/live-bootstrap/libbsd-0.11.8.tar.xz
     https://live-bootstrap.stikonas.eu/libbsd-0.11.8.tar.xz
     https://archive.org/download/live-bootstrap-sources/libbsd-0.11.8.tar.xz"
    "90e5e93cc724e4ebe12ede80df1634063c7a855692685919bfe60b556c9bd09e diffutils-3.10.tar.xz
     https://mirrors.kernel.org/gnu/diffutils/diffutils-3.10.tar.xz
     https://samuelt.me/pub/live-bootstrap/diffutils-3.10.tar.xz
     https://live-bootstrap.stikonas.eu/diffutils-3.10.tar.xz
     https://archive.org/download/live-bootstrap-sources/diffutils-3.10.tar.xz"
    "ca9c16d3d11d0ff8c69d79dc0b47267e1329a69b39b799895604ed447d3ca90b gawk-5.3.0.tar.xz
     https://mirrors.kernel.org/gnu/gawk/gawk-5.3.0.tar.xz
     https://samuelt.me/pub/live-bootstrap/gawk-5.3.0.tar.xz
     https://live-bootstrap.stikonas.eu/gawk-5.3.0.tar.xz
     https://archive.org/download/live-bootstrap-sources/gawk-5.3.0.tar.xz"
    "438fd996826b0c82485a29da03a72d71d6e3541a83ec702df4271f6fe025d24e gcc-15.2.0.tar.xz
     https://mirrors.kernel.org/gnu/gcc/gcc-15.2.0/gcc-15.2.0.tar.xz
     https://samuelt.me/pub/live-bootstrap/gcc-15.2.0.tar.xz
     https://live-bootstrap.stikonas.eu/gcc-15.2.0.tar.xz
     https://archive.org/download/live-bootstrap-sources/gcc-15.2.0.tar.xz"
    "7454eb6935db17c6655576c2e1b0fabefd38b4d0936e0f87f48cd062ce91a057 gzip-1.13.tar.xz
     https://mirrors.kernel.org/gnu/gzip/gzip-1.13.tar.xz
     https://samuelt.me/pub/live-bootstrap/gzip-1.13.tar.xz
     https://live-bootstrap.stikonas.eu/gzip-1.13.tar.xz
     https://archive.org/download/live-bootstrap-sources/gzip-1.13.tar.xz"
    "63aede5c6d33b6d9b13511cd0be2cac046f2e70fd0a07aa9573a04a82783af96 m4-1.4.19.tar.xz
     https://mirrors.kernel.org/gnu/m4/m4-1.4.19.tar.xz
     https://samuelt.me/pub/live-bootstrap/m4-1.4.19.tar.xz
     https://live-bootstrap.stikonas.eu/m4-1.4.19.tar.xz
     https://archive.org/download/live-bootstrap-sources/m4-1.4.19.tar.xz"
    "97dfbad67832641bc7f73437617b78abeafb9946723f19cf4c2ceecfc65fa48d gnulib-3639c57.tar.gz
     https://samuelt.me/pub/live-bootstrap/gnulib-3639c57.tar.gz
     https://live-bootstrap.stikonas.eu/gnulib-3639c57.tar.gz
     https://archive.org/download/live-bootstrap-sources/gnulib-3639c57.tar.gz"
    "56f1221eb682c3502ee097f583f44673570753cb452346ad4806d94560c3fac9 gnulib-5651802.tar.gz
     https://samuelt.me/pub/live-bootstrap/gnulib-5651802.tar.gz
     https://live-bootstrap.stikonas.eu/gnulib-5651802.tar.gz
     https://archive.org/download/live-bootstrap-sources/gnulib-5651802.tar.gz"
    "72e7bb2d1d75e63d1c46d33b8dd22e8eb60afdba4af3e7251151b5c2a6f00bfb gnulib-5d2fe24.tar.gz
     https://samuelt.me/pub/live-bootstrap/gnulib-5d2fe24.tar.gz
     https://live-bootstrap.stikonas.eu/gnulib-5d2fe24.tar.gz
     https://archive.org/download/live-bootstrap-sources/gnulib-5d2fe24.tar.gz"
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
    "$HERE/../live-bootstrap/split-musl.sh" "$WORK/"
cp "$HERE/steps/manifest" "$WORK/steps/"
commit_wrapper "live-bootstrap b1ceced7 x86 pass 6: to gcc 15.2.0 and the end" kairo.toml start.sh package.sh split-musl.sh steps/manifest

cat <<OUT
wrapper = $WRAPPER

# With passes 1 to 5 imported, and the same config:
#   kairo build object:$WRAPPER --target x86 --actor $ACTOR
OUT
