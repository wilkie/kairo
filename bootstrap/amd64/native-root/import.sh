#!/usr/bin/env bash
# Archive the native x86_64 root's sources and make its object
# (bootstrap/amd64/README.md, stages 3 and 4).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# 1. Each tarball is archived as a file source with `kairo source
#    fetch-file`, from upstream or else one of live-bootstrap's mirrors.
#    Most are the same tarballs live-bootstrap builds; coreutils is GNU's
#    release, not live-bootstrap's own tarball of it, and findutils 4.10.0
#    is newer than live-bootstrap's. They are stage 2's, and perl's,
#    python's, zlib's, and libffi's.
# 2. This directory's kairo.toml and src/ are committed as SHA-256
#    history and created as an object, or, with --wrapper, committed on
#    top of that existing object's history as its next revision.
#
# Stage 2 (../cross-root/) and what it needs must be imported: its root
# is the x86-64 target's input. Extra
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
    "13720965b5f4fc3a0d4b61dd37e7565c741da9a5be24edc2ae00182fc1b3588c bash-5.2.15.tar.gz
     https://mirrors.kernel.org/gnu/bash/bash-5.2.15.tar.gz
     https://samuelt.me/pub/live-bootstrap/bash-5.2.15.tar.gz
     https://live-bootstrap.stikonas.eu/bash-5.2.15.tar.gz
     https://archive.org/download/live-bootstrap-sources/bash-5.2.15.tar.gz"
    "ea613a4cf44612326e917201bbbcdfbd301de21ffc3b59b6e5c07e040b275e52 coreutils-9.4.tar.xz
     https://mirrors.kernel.org/gnu/coreutils/coreutils-9.4.tar.xz"
    "e40b8f018c1da64edd1cc9a6fce5fa63b2e707e404e20cad91fbae337c98a5b7 make-4.2.1.tar.gz
     https://mirrors.kernel.org/gnu/make/make-4.2.1.tar.gz
     https://samuelt.me/pub/live-bootstrap/make-4.2.1.tar.gz
     https://live-bootstrap.stikonas.eu/make-4.2.1.tar.gz
     https://archive.org/download/live-bootstrap-sources/make-4.2.1.tar.gz"
    "f79b0cfea71b37a8eeec8490db6c5f7ae7719c35587f21edb0617f370eeff633 sed-4.8.tar.xz
     https://mirrors.kernel.org/gnu/sed/sed-4.8.tar.xz
     https://samuelt.me/pub/live-bootstrap/sed-4.8.tar.xz
     https://live-bootstrap.stikonas.eu/sed-4.8.tar.xz
     https://archive.org/download/live-bootstrap-sources/sed-4.8.tar.xz"
    "2649b27c0e90e632eadcd757be06c6e9a4f48d941de51e7c0f83ff76408a07b9 grep-3.12.tar.xz
     https://mirrors.kernel.org/gnu/grep/grep-3.12.tar.xz
     https://samuelt.me/pub/live-bootstrap/grep-3.12.tar.xz
     https://live-bootstrap.stikonas.eu/grep-3.12.tar.xz
     https://archive.org/download/live-bootstrap-sources/grep-3.12.tar.xz"
    "ca9c16d3d11d0ff8c69d79dc0b47267e1329a69b39b799895604ed447d3ca90b gawk-5.3.0.tar.xz
     https://mirrors.kernel.org/gnu/gawk/gawk-5.3.0.tar.xz
     https://samuelt.me/pub/live-bootstrap/gawk-5.3.0.tar.xz
     https://live-bootstrap.stikonas.eu/gawk-5.3.0.tar.xz
     https://archive.org/download/live-bootstrap-sources/gawk-5.3.0.tar.xz"
    "63bebd26879c5e1eea4352f0d03c991f966aeb3ddeb3c7445c902568d5411d28 tar-1.34.tar.xz
     https://mirrors.kernel.org/gnu/tar/tar-1.34.tar.xz
     https://samuelt.me/pub/live-bootstrap/tar-1.34.tar.xz
     https://live-bootstrap.stikonas.eu/tar-1.34.tar.xz
     https://archive.org/download/live-bootstrap-sources/tar-1.34.tar.xz"
    "176d510c30d80a23b8050bbc048f2ecaacb823ae48b6821727ed6591f0df9200 xz-5.6.4.tar.bz2
     https://github.com/tukaani-project/xz/releases/download/v5.6.4/xz-5.6.4.tar.bz2
     https://samuelt.me/pub/live-bootstrap/xz-5.6.4.tar.bz2
     https://live-bootstrap.stikonas.eu/xz-5.6.4.tar.bz2
     https://archive.org/download/live-bootstrap-sources/xz-5.6.4.tar.bz2"
    "7454eb6935db17c6655576c2e1b0fabefd38b4d0936e0f87f48cd062ce91a057 gzip-1.13.tar.xz
     https://mirrors.kernel.org/gnu/gzip/gzip-1.13.tar.xz
     https://samuelt.me/pub/live-bootstrap/gzip-1.13.tar.xz
     https://live-bootstrap.stikonas.eu/gzip-1.13.tar.xz
     https://archive.org/download/live-bootstrap-sources/gzip-1.13.tar.xz"
    "ab5a03176ee106d3f0fa90e381da478ddae405918153cca248e682cd0c4a2269 bzip2-1.0.8.tar.gz
     https://sourceware.org/pub/bzip2/bzip2-1.0.8.tar.gz
     https://samuelt.me/pub/live-bootstrap/bzip2-1.0.8.tar.gz
     https://live-bootstrap.stikonas.eu/bzip2-1.0.8.tar.gz
     https://archive.org/download/live-bootstrap-sources/bzip2-1.0.8.tar.gz"
    "90e5e93cc724e4ebe12ede80df1634063c7a855692685919bfe60b556c9bd09e diffutils-3.10.tar.xz
     https://mirrors.kernel.org/gnu/diffutils/diffutils-3.10.tar.xz
     https://samuelt.me/pub/live-bootstrap/diffutils-3.10.tar.xz
     https://live-bootstrap.stikonas.eu/diffutils-3.10.tar.xz
     https://archive.org/download/live-bootstrap-sources/diffutils-3.10.tar.xz"
    "f87cee69eec2b4fcbf60a396b030ad6aa3415f192aa5f7ee84cad5e11f7f5ae3 patch-2.8.tar.xz
     https://mirrors.kernel.org/gnu/patch/patch-2.8.tar.xz
     https://samuelt.me/pub/live-bootstrap/patch-2.8.tar.xz
     https://live-bootstrap.stikonas.eu/patch-2.8.tar.xz
     https://archive.org/download/live-bootstrap-sources/patch-2.8.tar.xz"
    "63aede5c6d33b6d9b13511cd0be2cac046f2e70fd0a07aa9573a04a82783af96 m4-1.4.19.tar.xz
     https://mirrors.kernel.org/gnu/m4/m4-1.4.19.tar.xz
     https://samuelt.me/pub/live-bootstrap/m4-1.4.19.tar.xz
     https://live-bootstrap.stikonas.eu/m4-1.4.19.tar.xz
     https://archive.org/download/live-bootstrap-sources/m4-1.4.19.tar.xz"
    "1387e0b67ff247d2abde998f90dfbf70c1491391a59ddfecb8ae698789f0a4f5 findutils-4.10.0.tar.xz
     https://mirrors.kernel.org/gnu/findutils/findutils-4.10.0.tar.xz"
    "73cf6cc1ea2b2b1c110a18c14bbbc73a362073003893ffcedc26d22ebdbdd0c3 perl-5.42.0.tar.xz
     https://www.cpan.org/src/5.0/perl-5.42.0.tar.xz
     https://samuelt.me/pub/live-bootstrap/perl-5.42.0.tar.xz
     https://live-bootstrap.stikonas.eu/perl-5.42.0.tar.xz
     https://archive.org/download/live-bootstrap-sources/perl-5.42.0.tar.xz"
    "85879192f2cffd56cb16c092905949ebf3e5e394b7f764723529637901dfb58f Python-3.11.1.tar.xz
     https://www.python.org/ftp/python/3.11.1/Python-3.11.1.tar.xz
     https://samuelt.me/pub/live-bootstrap/Python-3.11.1.tar.xz
     https://live-bootstrap.stikonas.eu/Python-3.11.1.tar.xz
     https://archive.org/download/live-bootstrap-sources/Python-3.11.1.tar.xz"
    "9a93b2b7dfdac77ceba5a558a580e74667dd6fede4585b91eefb60f03b72df23 zlib-1.3.1.tar.gz
     https://zlib.net/fossils/zlib-1.3.1.tar.gz
     https://samuelt.me/pub/live-bootstrap/zlib-1.3.1.tar.gz
     https://live-bootstrap.stikonas.eu/zlib-1.3.1.tar.gz
     https://archive.org/download/live-bootstrap-sources/zlib-1.3.1.tar.gz"
    "f3a3082a23b37c293a4fcd1053147b371f2ff91fa7ea1b2a52e335676bac82dc libffi-3.5.2.tar.gz
     https://github.com/libffi/libffi/releases/download/v3.5.2/libffi-3.5.2.tar.gz
     https://samuelt.me/pub/live-bootstrap/libffi-3.5.2.tar.gz
     https://live-bootstrap.stikonas.eu/libffi-3.5.2.tar.gz
     https://archive.org/download/live-bootstrap-sources/libffi-3.5.2.tar.gz"
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
commit_wrapper "amd64 stages 3 and 4: the x86_64 root, built natively, and its fixed point" kairo.toml src/build.sh

cat <<OUT
object = $WRAPPER

# With stage 2 imported:
#   kairo build object:$WRAPPER --target x86-64 --actor $ACTOR
#   kairo build object:$WRAPPER --target fixed-point --actor $ACTOR
OUT
