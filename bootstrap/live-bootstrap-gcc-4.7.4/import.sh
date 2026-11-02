#!/usr/bin/env bash
# Archive the distfiles of live-bootstrap's steps from openssl to gcc
# 4.7.4, binutils 2.41, and musl 1.2.5 rebuilt, and make the native
# object that runs them for x86, going on from pass 2's root
# (specs/DECISIONS.md §49).
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
# Passes 1 and 2 (bootstrap/live-bootstrap/ and
# bootstrap/live-bootstrap-perl/) must be imported: pass 2's artifact,
# data:live-bootstrap:x86:perl-5.42.0, is this build's input, and is
# built first, with pass 1's, if it isn't held. Extra arguments go
# before the command in every `kairo` call, e.g. `--store <path>`.
# KAIRO names the kairo binary (default: kairo on PATH). Prints the
# wrapper's object id.
set -euo pipefail
shopt -s inherit_errexit

LIVE_BOOTSTRAP_URL=https://github.com/fosslinux/live-bootstrap

# sha256 name url... (from steps/<step>/sources at b1ceced7, upstream's
# URL first, then live-bootstrap's mirrors)
DISTFILES=(
    "764f420cedfc8b43d9fec251c957a5d55fc45d40f6573f162990ed1dce7e36e0 ArabicShaping-16.0.0.txt
     https://www.unicode.org/Public/16.0.0/ucd/ArabicShaping.txt
     https://samuelt.me/pub/live-bootstrap/ArabicShaping-16.0.0.txt
     https://live-bootstrap.stikonas.eu/ArabicShaping-16.0.0.txt
     https://archive.org/download/live-bootstrap-sources/ArabicShaping-16.0.0.txt"
    "f3907b395d410f1b97342292ca6bc83dd12eb4b205f2a0c48efdef99e517d7b0 Blocks-16.0.0.txt
     https://www.unicode.org/Public/16.0.0/ucd/Blocks.txt
     https://samuelt.me/pub/live-bootstrap/Blocks-16.0.0.txt
     https://live-bootstrap.stikonas.eu/Blocks-16.0.0.txt
     https://archive.org/download/live-bootstrap-sources/Blocks-16.0.0.txt"
    "6f1f9c588eb4a5c718d9e8f93b782685e5c7fec872cf05e8e6878053599e09bb CaseFolding-16.0.0.txt
     https://www.unicode.org/Public/16.0.0/ucd/CaseFolding.txt
     https://samuelt.me/pub/live-bootstrap/CaseFolding-16.0.0.txt
     https://live-bootstrap.stikonas.eu/CaseFolding-16.0.0.txt
     https://archive.org/download/live-bootstrap-sources/CaseFolding-16.0.0.txt"
    "89e83cf9cc8bef6c1f8bf77e42cf6f0341dfa42e66261f4dbe9b492e7a23c8ee CompositionExclusions-16.0.0.txt
     https://www.unicode.org/Public/16.0.0/ucd/CompositionExclusions.txt
     https://samuelt.me/pub/live-bootstrap/CompositionExclusions-16.0.0.txt
     https://live-bootstrap.stikonas.eu/CompositionExclusions-16.0.0.txt
     https://archive.org/download/live-bootstrap-sources/CompositionExclusions-16.0.0.txt"
    "39d35161f2954497f69e08bdb9e701493f476a3d30222de20028feda36c1dabd DerivedCoreProperties-16.0.0.txt
     https://www.unicode.org/Public/16.0.0/ucd/DerivedCoreProperties.txt
     https://samuelt.me/pub/live-bootstrap/DerivedCoreProperties-16.0.0.txt
     https://live-bootstrap.stikonas.eu/DerivedCoreProperties-16.0.0.txt
     https://archive.org/download/live-bootstrap-sources/DerivedCoreProperties-16.0.0.txt"
    "43adc76c0686a42cb370764eb8cfe2b2a45b10b855e5572a2db4a0eecce15d5b EastAsianWidth-16.0.0.txt
     https://www.unicode.org/Public/16.0.0/ucd/EastAsianWidth.txt
     https://samuelt.me/pub/live-bootstrap/EastAsianWidth-16.0.0.txt
     https://live-bootstrap.stikonas.eu/EastAsianWidth-16.0.0.txt
     https://archive.org/download/live-bootstrap-sources/EastAsianWidth-16.0.0.txt"
    "e97e4259d0d20fab150b9c7b4b28abfae5cd78ca97e7f4ac6ed20d685d5f4a7c LineBreak-16.0.0.txt
     https://www.unicode.org/Public/16.0.0/ucd/LineBreak.txt
     https://samuelt.me/pub/live-bootstrap/LineBreak-16.0.0.txt
     https://live-bootstrap.stikonas.eu/LineBreak-16.0.0.txt
     https://archive.org/download/live-bootstrap-sources/LineBreak-16.0.0.txt"
    "53d614508e2a0b2305a8aa21cd60d993de9326cdf65993660dfcce4503548583 PropList-16.0.0.txt
     https://www.unicode.org/Public/16.0.0/ucd/PropList.txt
     https://samuelt.me/pub/live-bootstrap/PropList-16.0.0.txt
     https://live-bootstrap.stikonas.eu/PropList-16.0.0.txt
     https://archive.org/download/live-bootstrap-sources/PropList-16.0.0.txt"
    "9e88f0a677df47311106340be8ede2ecdacd9c1c931831218d2be6d5508e0039 Scripts-16.0.0.txt
     https://www.unicode.org/Public/16.0.0/ucd/Scripts.txt
     https://samuelt.me/pub/live-bootstrap/Scripts-16.0.0.txt
     https://live-bootstrap.stikonas.eu/Scripts-16.0.0.txt
     https://archive.org/download/live-bootstrap-sources/Scripts-16.0.0.txt"
    "8d5de354eef79f2395a54c9c7dcebbaf3d30fc962d0f85611ea97aa973a0c451 SpecialCasing-16.0.0.txt
     https://www.unicode.org/Public/16.0.0/ucd/SpecialCasing.txt
     https://samuelt.me/pub/live-bootstrap/SpecialCasing-16.0.0.txt
     https://live-bootstrap.stikonas.eu/SpecialCasing-16.0.0.txt
     https://archive.org/download/live-bootstrap-sources/SpecialCasing-16.0.0.txt"
    "c29360bd6f7132811d701d29069541e827eb44bfc4c8fbde8c370d6982689dc1 GraphemeBreakProperty-16.0.0.txt
     https://www.unicode.org/Public/16.0.0/ucd/auxiliary/GraphemeBreakProperty.txt
     https://samuelt.me/pub/live-bootstrap/GraphemeBreakProperty-16.0.0.txt
     https://live-bootstrap.stikonas.eu/GraphemeBreakProperty-16.0.0.txt
     https://archive.org/download/live-bootstrap-sources/GraphemeBreakProperty-16.0.0.txt"
    "476464e71a4b7b779b8ba7c5671f4338fea77da8e6b6b05fb82b3fdd14603779 WordBreakProperty-16.0.0.txt
     https://www.unicode.org/Public/16.0.0/ucd/auxiliary/WordBreakProperty.txt
     https://samuelt.me/pub/live-bootstrap/WordBreakProperty-16.0.0.txt
     https://live-bootstrap.stikonas.eu/WordBreakProperty-16.0.0.txt
     https://archive.org/download/live-bootstrap-sources/WordBreakProperty-16.0.0.txt"
    "f1365a5173eee18e1f98b240cdc492e84a25f1ce7e0c9d1094eb29c41a22696a emoji-data-16.0.0.txt
     https://www.unicode.org/Public/16.0.0/ucd/emoji/emoji-data.txt
     https://samuelt.me/pub/live-bootstrap/emoji-data-16.0.0.txt
     https://live-bootstrap.stikonas.eu/emoji-data-16.0.0.txt
     https://archive.org/download/live-bootstrap-sources/emoji-data-16.0.0.txt"
    "909eef4adbeddbdddcd9487c856fe8cdbb8912aa8eb315ed7885b6ef65f4dc4c PropList-3.0.1.txt
     https://www.unicode.org/Public/3.0-Update1/PropList-3.0.1.txt
     https://samuelt.me/pub/live-bootstrap/PropList-3.0.1.txt
     https://live-bootstrap.stikonas.eu/PropList-3.0.1.txt
     https://archive.org/download/live-bootstrap-sources/PropList-3.0.1.txt"
    "955f6e729ad6b3566260e8fef68620e76ba3c31acf0a18524416a185acf77992 curl-8.17.0.tar.xz
     https://curl.se/download/curl-8.17.0.tar.xz
     https://samuelt.me/pub/live-bootstrap/curl-8.17.0.tar.xz
     https://live-bootstrap.stikonas.eu/curl-8.17.0.tar.xz
     https://archive.org/download/live-bootstrap-sources/curl-8.17.0.tar.xz"
    "b6a5f44b7eb69e3fa35dbf15524405b44837a481d43d81daddde3ff21fcbb8e9 openssl-3.6.0.tar.gz
     https://github.com/openssl/openssl/releases/download/openssl-3.6.0/openssl-3.6.0.tar.gz
     https://samuelt.me/pub/live-bootstrap/openssl-3.6.0.tar.gz
     https://live-bootstrap.stikonas.eu/openssl-3.6.0.tar.gz
     https://archive.org/download/live-bootstrap-sources/openssl-3.6.0.tar.gz"
    "a903b3cd05231e39332515ef7ebe37e697262f39515a52015c23c62805b73cd0 certdata.txt
     https://hg-edge.mozilla.org/projects/nss/raw-file/NSS_3_119_1_RTM/lib/ckfw/builtins/certdata.txt
     https://samuelt.me/pub/live-bootstrap/certdata.txt
     https://live-bootstrap.stikonas.eu/certdata.txt
     https://archive.org/download/live-bootstrap-sources/certdata.txt"
    "ae9a5789e23459e59606e6714723f2d3ffc31c03174191ef0d015bdf06007450 binutils-2.41.tar.xz
     https://mirrors.kernel.org/gnu/binutils/binutils-2.41.tar.xz
     https://samuelt.me/pub/live-bootstrap/binutils-2.41.tar.xz
     https://live-bootstrap.stikonas.eu/binutils-2.41.tar.xz
     https://archive.org/download/live-bootstrap-sources/binutils-2.41.tar.xz"
    "92e61c6dc3a0a449e62d72a38185fda550168a86702dea07125ebd3ec3996282 gcc-4.7.4.tar.bz2
     https://mirrors.kernel.org/gnu/gcc/gcc-4.7.4/gcc-4.7.4.tar.bz2
     https://samuelt.me/pub/live-bootstrap/gcc-4.7.4.tar.bz2
     https://live-bootstrap.stikonas.eu/gcc-4.7.4.tar.bz2
     https://archive.org/download/live-bootstrap-sources/gcc-4.7.4.tar.bz2"
    "f87cee69eec2b4fcbf60a396b030ad6aa3415f192aa5f7ee84cad5e11f7f5ae3 patch-2.8.tar.xz
     https://mirrors.kernel.org/gnu/patch/patch-2.8.tar.xz
     https://samuelt.me/pub/live-bootstrap/patch-2.8.tar.xz
     https://live-bootstrap.stikonas.eu/patch-2.8.tar.xz
     https://archive.org/download/live-bootstrap-sources/patch-2.8.tar.xz"
    "a9a118bbe84d8764da0ea0d28b3ab3fae8477fc7e4085d90102b8596fc7c75e4 musl-1.2.5.tar.gz
     https://musl.libc.org/releases/musl-1.2.5.tar.gz
     https://samuelt.me/pub/live-bootstrap/musl-1.2.5.tar.gz
     https://live-bootstrap.stikonas.eu/musl-1.2.5.tar.gz
     https://archive.org/download/live-bootstrap-sources/musl-1.2.5.tar.gz"
    "377786991f43e21623fa5cbc0fd98f2d8fbe1f95e0d0cad3be9421efd86f39dd gnulib-9829f0a.tar.gz
     https://samuelt.me/pub/live-bootstrap/gnulib-9829f0a.tar.gz
     https://live-bootstrap.stikonas.eu/gnulib-9829f0a.tar.gz
     https://archive.org/download/live-bootstrap-sources/gnulib-9829f0a.tar.gz"
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
commit_wrapper "live-bootstrap b1ceced7 x86 pass 3: to gcc 4.7.4" kairo.toml start.sh package.sh split-musl.sh split-gcc.sh steps/manifest

cat <<OUT
wrapper = $WRAPPER

# With passes 1 and 2 imported, and the same config:
#   kairo build object:$WRAPPER --target x86 --actor $ACTOR
OUT
