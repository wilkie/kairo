#!/usr/bin/env bash
# Archive perl's sources, make its object or its next revision, and
# tag it 5.42.0 (packages/perl/README.md, packages/common/import.sh).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# What it builds with must be imported: packages/build-base/ and the
# packages it names.
# Prints the object's id.
set -euo pipefail
shopt -s inherit_errexit

# sha256 name url...
DISTFILES=(
    "73cf6cc1ea2b2b1c110a18c14bbbc73a362073003893ffcedc26d22ebdbdd0c3 perl-5.42.0.tar.xz
     https://www.cpan.org/src/5.0/perl-5.42.0.tar.xz
     https://samuelt.me/pub/live-bootstrap/perl-5.42.0.tar.xz
     https://live-bootstrap.stikonas.eu/perl-5.42.0.tar.xz
     https://archive.org/download/live-bootstrap-sources/perl-5.42.0.tar.xz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "perl 5.42.0 for x86 and x86-64" 5.42.0
