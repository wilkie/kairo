#!/usr/bin/env bash
# Archive gcc's sources, make its object or its next revision, and tag
# it with the release's version (packages/gcc/README.md,
# packages/common/import.sh). This revision is 15.2.0's third round,
# tagged 15.2.0+4, on top of its second and first, which is on top of
# 7.5.0: import those first, at their tags, then this with --wrapper.
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# What it builds with must be imported: packages/base-round/ and the
# base's round 3 it names. Prints the object's id.
set -euo pipefail
shopt -s inherit_errexit

# sha256 name url...
DISTFILES=(
    "438fd996826b0c82485a29da03a72d71d6e3541a83ec702df4271f6fe025d24e gcc-15.2.0.tar.xz
     https://mirrors.kernel.org/gnu/gcc/gcc-15.2.0/gcc-15.2.0.tar.xz
     https://samuelt.me/pub/live-bootstrap/gcc-15.2.0.tar.xz
     https://live-bootstrap.stikonas.eu/gcc-15.2.0.tar.xz
     https://archive.org/download/live-bootstrap-sources/gcc-15.2.0.tar.xz"
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

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "gcc 15.2.0 for x86 and x86-64, round 4, built with round 3" 15.2.0+4
