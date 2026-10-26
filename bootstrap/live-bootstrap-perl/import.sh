#!/usr/bin/env bash
# Archive the distfiles of live-bootstrap's perl chain and make the
# native object that runs it for x86, going on from pass 1's root
# (specs/DECISIONS.md §49).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# 1. `kairo source fetch https://github.com/fosslinux/live-bootstrap`
#    archives live-bootstrap as a source object, as pass 1's import
#    does; a node that has imported pass 1 holds it already.
# 2. Each distfile this pass's steps list is archived as a file source
#    with `kairo source fetch-file`, from upstream or else one of
#    live-bootstrap's mirrors: its maintainers' two, then archive.org.
#    The gnulib snapshots have no upstream URL; upstream's mirror.sh
#    makes them with `git archive`, and the mirrors serve those bytes.
# 3. The native wrapper -- this directory's kairo.toml and
#    steps/manifest, and the start.sh and package.sh every later pass
#    shares (../live-bootstrap/pass/) and the musl split
#    (../live-bootstrap/split-musl.sh) -- is committed as SHA-256 history
#    and created as an object, or, with --wrapper, committed on top of
#    that existing wrapper object's history as its next revision.
#
# Pass 1 (bootstrap/live-bootstrap/import.sh) must be imported: its
# artifact, data:live-bootstrap:x86:zlib-1.3.1, is this build's input,
# and is built first if it isn't held. Extra arguments go before the
# command in every `kairo` call, e.g. `--store <path>`. KAIRO names the
# kairo binary (default: kairo on PATH). Prints the wrapper's object id.
set -euo pipefail
shopt -s inherit_errexit

LIVE_BOOTSTRAP_URL=https://github.com/fosslinux/live-bootstrap

# sha256 name url... (from steps/<step>/sources at b1ceced7, upstream's
# URL first, then live-bootstrap's mirrors)
DISTFILES=(
    "8b13473b31ca7fcf65e5e8a74224368ffd5df19275602a9c9567ba393f18577d bison-3.6.4.tar.xz
     https://mirrors.kernel.org/gnu/bison/bison-3.6.4.tar.xz
     https://samuelt.me/pub/live-bootstrap/bison-3.6.4.tar.xz
     https://live-bootstrap.stikonas.eu/bison-3.6.4.tar.xz
     https://archive.org/download/live-bootstrap-sources/bison-3.6.4.tar.xz"
    "67d68ce1e22192050525643fc0a7a22297576682bef6a5c51446903f5aeef3cf bison-3.7.6.tar.xz
     https://mirrors.kernel.org/gnu/bison/bison-3.7.6.tar.xz
     https://samuelt.me/pub/live-bootstrap/bison-3.7.6.tar.xz
     https://live-bootstrap.stikonas.eu/bison-3.7.6.tar.xz
     https://archive.org/download/live-bootstrap-sources/bison-3.7.6.tar.xz"
    "9bba0214ccf7f1079c5d59210045227bcf619519840ebfa80cd3849cff5a5bf2 bison-3.8.2.tar.xz
     https://mirrors.kernel.org/gnu/bison/bison-3.8.2.tar.xz
     https://samuelt.me/pub/live-bootstrap/bison-3.8.2.tar.xz
     https://live-bootstrap.stikonas.eu/bison-3.8.2.tar.xz
     https://archive.org/download/live-bootstrap-sources/bison-3.8.2.tar.xz"
    "713243dce27d7aa0bdbf52b2070de5ce449f9ffbcc14a93efbc6f2beff0f5ce8 perl-5.22.4.tar.xz
     https://www.cpan.org/src/5.0/perl-5.22.4.tar.xz
     https://samuelt.me/pub/live-bootstrap/perl-5.22.4.tar.xz
     https://live-bootstrap.stikonas.eu/perl-5.22.4.tar.xz
     https://archive.org/download/live-bootstrap-sources/perl-5.22.4.tar.xz"
    "7f080287ff64750270689843ae945f02159a33cb8f2fc910248c15befba5db84 perl-5.24.4.tar.xz
     https://www.cpan.org/src/5.0/perl-5.24.4.tar.xz
     https://samuelt.me/pub/live-bootstrap/perl-5.24.4.tar.xz
     https://live-bootstrap.stikonas.eu/perl-5.24.4.tar.xz
     https://archive.org/download/live-bootstrap-sources/perl-5.24.4.tar.xz"
    "6967595f2e3f3a94544c35152f9a25e0cb8ea24ae45f4bf1882f2e33f4a400f4 perl-5.30.3.tar.xz
     https://www.cpan.org/src/5.0/perl-5.30.3.tar.xz
     https://samuelt.me/pub/live-bootstrap/perl-5.30.3.tar.xz
     https://live-bootstrap.stikonas.eu/perl-5.30.3.tar.xz
     https://archive.org/download/live-bootstrap-sources/perl-5.30.3.tar.xz"
    "45a228daef66d02fdccc820e71f87e40d8e3df1fc4431f8d4580ec08033866bd perl-5.36.3.tar.xz
     https://www.cpan.org/src/5.0/perl-5.36.3.tar.xz
     https://samuelt.me/pub/live-bootstrap/perl-5.36.3.tar.xz
     https://live-bootstrap.stikonas.eu/perl-5.36.3.tar.xz
     https://archive.org/download/live-bootstrap-sources/perl-5.36.3.tar.xz"
    "73cf6cc1ea2b2b1c110a18c14bbbc73a362073003893ffcedc26d22ebdbdd0c3 perl-5.42.0.tar.xz
     https://www.cpan.org/src/5.0/perl-5.42.0.tar.xz
     https://samuelt.me/pub/live-bootstrap/perl-5.42.0.tar.xz
     https://live-bootstrap.stikonas.eu/perl-5.42.0.tar.xz
     https://archive.org/download/live-bootstrap-sources/perl-5.42.0.tar.xz"
    "5222da582ba59be26a9443a11c525c52aa070daf3bff08516805142b362ba48c Devel-Tokenizer-C-0.11.tar.gz
     https://cpan.metacpan.org/authors/id/M/MH/MHX/Devel-Tokenizer-C-0.11.tar.gz
     https://samuelt.me/pub/live-bootstrap/Devel-Tokenizer-C-0.11.tar.gz
     https://live-bootstrap.stikonas.eu/Devel-Tokenizer-C-0.11.tar.gz
     https://archive.org/download/live-bootstrap-sources/Devel-Tokenizer-C-0.11.tar.gz"
    "ee91f8f7db894ee7c6ee003daac10a99056c4948a674ef46acdbb63c81a4abeb Digest-SHA-6.04.tar.gz
     https://cpan.metacpan.org/authors/id/M/MS/MSHELOR/Digest-SHA-6.04.tar.gz
     https://samuelt.me/pub/live-bootstrap/Digest-SHA-6.04.tar.gz
     https://live-bootstrap.stikonas.eu/Digest-SHA-6.04.tar.gz
     https://archive.org/download/live-bootstrap-sources/Digest-SHA-6.04.tar.gz"
    "91eb0dfaafdf08e450aa03cd8a3285d47facfd432f75f812a5bdafd9e445297b Socket-2.000.tar.gz
     https://cpan.metacpan.org/authors/id/P/PE/PEVANS/Socket-2.000.tar.gz
     https://samuelt.me/pub/live-bootstrap/Socket-2.000.tar.gz
     https://live-bootstrap.stikonas.eu/Socket-2.000.tar.gz
     https://archive.org/download/live-bootstrap-sources/Socket-2.000.tar.gz"
    "f14c83cfebcc9427f2c3cea7258bd90df972d92eb26752da4ddad81c87a0faa4 autoconf-2.71.tar.xz
     https://mirrors.kernel.org/gnu/autoconf/autoconf-2.71.tar.xz
     https://samuelt.me/pub/live-bootstrap/autoconf-2.71.tar.xz
     https://live-bootstrap.stikonas.eu/autoconf-2.71.tar.xz
     https://archive.org/download/live-bootstrap-sources/autoconf-2.71.tar.xz"
    "ff2bf7656c4d1c6fdda3b8bebb21f09153a736bcba169aaf65eab25fa113bf3a automake-1.16.3.tar.xz
     https://mirrors.kernel.org/gnu/automake/automake-1.16.3.tar.xz
     https://samuelt.me/pub/live-bootstrap/automake-1.16.3.tar.xz
     https://live-bootstrap.stikonas.eu/automake-1.16.3.tar.xz
     https://archive.org/download/live-bootstrap-sources/automake-1.16.3.tar.xz"
    "10749417fd3010aae320a34181ad4cd6a4855c1fc63403b87fa4d630b18e966c perl-5.12.5.tar.bz2
     https://www.cpan.org/src/5.0/perl-5.12.5.tar.bz2
     https://samuelt.me/pub/live-bootstrap/perl-5.12.5.tar.bz2
     https://live-bootstrap.stikonas.eu/perl-5.12.5.tar.bz2
     https://archive.org/download/live-bootstrap-sources/perl-5.12.5.tar.bz2"
    "331fcc356fccfc47e86ed09c20c081858f9466e419324007a7578441f861177c perl-5.15.7.tar.bz2
     https://www.cpan.org/src/5.0/perl-5.15.7.tar.bz2
     https://samuelt.me/pub/live-bootstrap/perl-5.15.7.tar.bz2
     https://live-bootstrap.stikonas.eu/perl-5.15.7.tar.bz2
     https://archive.org/download/live-bootstrap-sources/perl-5.15.7.tar.bz2"
    "bb7bc735e6813b177dcfccd480defcde7eddefa173b5967eac11babd1bfa98e8 perl-5.16.3.tar.bz2
     https://www.cpan.org/src/5.0/perl-5.16.3.tar.bz2
     https://samuelt.me/pub/live-bootstrap/perl-5.16.3.tar.bz2
     https://live-bootstrap.stikonas.eu/perl-5.16.3.tar.bz2
     https://archive.org/download/live-bootstrap-sources/perl-5.16.3.tar.bz2"
    "a47c59fd3667011205be4d384f4da2deb0a7544bdf1cc2efa66e9fefea435576 perl-5.17.2.tar.bz2
     https://www.cpan.org/src/5.0/perl-5.17.2.tar.bz2
     https://samuelt.me/pub/live-bootstrap/perl-5.17.2.tar.bz2
     https://live-bootstrap.stikonas.eu/perl-5.17.2.tar.bz2
     https://archive.org/download/live-bootstrap-sources/perl-5.17.2.tar.bz2"
    "42e7eb0d726a6344bc54140be8a0e3636330e1be7be8793e0fb74f415666b95b perl-5.17.4.tar.bz2
     https://www.cpan.org/src/5.0/perl-5.17.4.tar.bz2
     https://samuelt.me/pub/live-bootstrap/perl-5.17.4.tar.bz2
     https://live-bootstrap.stikonas.eu/perl-5.17.4.tar.bz2
     https://archive.org/download/live-bootstrap-sources/perl-5.17.4.tar.bz2"
    "1fb4d27b75cd244e849f253320260efe1750641aaff4a18ce0d67556ff1b96a5 perl-5.18.4.tar.bz2
     https://www.cpan.org/src/5.0/perl-5.18.4.tar.bz2
     https://samuelt.me/pub/live-bootstrap/perl-5.18.4.tar.bz2
     https://live-bootstrap.stikonas.eu/perl-5.18.4.tar.bz2
     https://archive.org/download/live-bootstrap-sources/perl-5.18.4.tar.bz2"
    "1097fbcd48ceccb2bc735d119c9db399a02a8ab9f7dc53e29e47e6a8d0d72e79 perl-5.8.9.tar.bz2
     https://www.cpan.org/src/5.0/perl-5.8.9.tar.bz2
     https://samuelt.me/pub/live-bootstrap/perl-5.8.9.tar.bz2
     https://live-bootstrap.stikonas.eu/perl-5.8.9.tar.bz2
     https://archive.org/download/live-bootstrap-sources/perl-5.8.9.tar.bz2"
    "6b0c937e1aaef684a7b2f0c089bea293c7a9d255d6513e5108567f4c10e3f4dd 40501436c87602cc17baae64ee6b3ca26d74e354.tar.gz
     https://github.com/Perl/metaconfig/archive/40501436c87602cc17baae64ee6b3ca26d74e354.tar.gz
     https://samuelt.me/pub/live-bootstrap/40501436c87602cc17baae64ee6b3ca26d74e354.tar.gz
     https://live-bootstrap.stikonas.eu/40501436c87602cc17baae64ee6b3ca26d74e354.tar.gz
     https://archive.org/download/live-bootstrap-sources/40501436c87602cc17baae64ee6b3ca26d74e354.tar.gz"
    "4641f5127a55cf2e1e8f9253ec27fba10b5b77076b4163e7f4bdc7f670d1aaf0 5.26.1.tar.gz
     https://github.com/Perl/metaconfig/archive/5.26.1.tar.gz
     https://samuelt.me/pub/live-bootstrap/5.26.1.tar.gz
     https://live-bootstrap.stikonas.eu/5.26.1.tar.gz
     https://archive.org/download/live-bootstrap-sources/5.26.1.tar.gz"
    "5cf99c51bbec95192e1b9b1e0d8f62d9413cabf64d0f4be0845c1fab3c4c1c9c 5.30.3.tar.gz
     https://github.com/Perl/metaconfig/archive/5.30.3.tar.gz
     https://samuelt.me/pub/live-bootstrap/5.30.3.tar.gz
     https://live-bootstrap.stikonas.eu/5.30.3.tar.gz
     https://archive.org/download/live-bootstrap-sources/5.30.3.tar.gz"
    "0767f0566067c9fd5d17f5ff53fa04b9c45a182afa189dc05b5f8418d5888d92 5.36.0.tar.gz
     https://github.com/Perl/metaconfig/archive/5.36.0.tar.gz
     https://samuelt.me/pub/live-bootstrap/5.36.0.tar.gz
     https://live-bootstrap.stikonas.eu/5.36.0.tar.gz
     https://archive.org/download/live-bootstrap-sources/5.36.0.tar.gz"
    "ac3948bd684a3c017a241b6fc7b899c901f814b36d030ee79887374884e30301 5.42.0.tar.gz
     https://github.com/Perl/metaconfig/archive/5.42.0.tar.gz
     https://samuelt.me/pub/live-bootstrap/5.42.0.tar.gz
     https://live-bootstrap.stikonas.eu/5.42.0.tar.gz
     https://archive.org/download/live-bootstrap-sources/5.42.0.tar.gz"
    "857e295a3e3ff3121339b348fd295e03459ce8dc3a382870e94f98c2da99a573 79b14e84d83fb88c2b1a07e0dec3b62ccb9a388c.tar.gz
     https://github.com/Perl/metaconfig/archive/79b14e84d83fb88c2b1a07e0dec3b62ccb9a388c.tar.gz
     https://samuelt.me/pub/live-bootstrap/79b14e84d83fb88c2b1a07e0dec3b62ccb9a388c.tar.gz
     https://live-bootstrap.stikonas.eu/79b14e84d83fb88c2b1a07e0dec3b62ccb9a388c.tar.gz
     https://archive.org/download/live-bootstrap-sources/79b14e84d83fb88c2b1a07e0dec3b62ccb9a388c.tar.gz"
    "08a10e266927ebce7fe8cd24122b9d1274a173c57170414545633ebebd77d65c 7ae9aa2926eed6b1f0b68b0e61c80a0f39b14a25.tar.gz
     https://github.com/Perl/metaconfig/archive/7ae9aa2926eed6b1f0b68b0e61c80a0f39b14a25.tar.gz
     https://samuelt.me/pub/live-bootstrap/7ae9aa2926eed6b1f0b68b0e61c80a0f39b14a25.tar.gz
     https://live-bootstrap.stikonas.eu/7ae9aa2926eed6b1f0b68b0e61c80a0f39b14a25.tar.gz
     https://archive.org/download/live-bootstrap-sources/7ae9aa2926eed6b1f0b68b0e61c80a0f39b14a25.tar.gz"
    "20cfcd13d588abdcb502c1c4682fb1f33ffefd307625b3b877d21ad938a7b705 99eb95e214b87a84b0a7752adba30e9b0fec977f.tar.gz
     https://github.com/rmanfredi/dist/archive/99eb95e214b87a84b0a7752adba30e9b0fec977f.tar.gz
     https://samuelt.me/pub/live-bootstrap/99eb95e214b87a84b0a7752adba30e9b0fec977f.tar.gz
     https://live-bootstrap.stikonas.eu/99eb95e214b87a84b0a7752adba30e9b0fec977f.tar.gz
     https://archive.org/download/live-bootstrap-sources/99eb95e214b87a84b0a7752adba30e9b0fec977f.tar.gz"
    "0e08a32ce38456238485cfefee0b8d0c43081c01c8e92a1d5e1e16002e4c6bcc gnulib-7818455.tar.gz
     https://samuelt.me/pub/live-bootstrap/gnulib-7818455.tar.gz
     https://live-bootstrap.stikonas.eu/gnulib-7818455.tar.gz
     https://archive.org/download/live-bootstrap-sources/gnulib-7818455.tar.gz"
    "bc96b5966e4651580ef69fee4a2dfed5ae3d3ab8af04bbe6f10dcd2dd010398e gnulib-839ed05.tar.gz
     https://samuelt.me/pub/live-bootstrap/gnulib-839ed05.tar.gz
     https://live-bootstrap.stikonas.eu/gnulib-839ed05.tar.gz
     https://archive.org/download/live-bootstrap-sources/gnulib-839ed05.tar.gz"
    "4c14c95b301e74804dd82d3993176b03cffedc18221bf573db35d21f2f8a930e gnulib-ffbb0ce.tar.gz
     https://samuelt.me/pub/live-bootstrap/gnulib-ffbb0ce.tar.gz
     https://live-bootstrap.stikonas.eu/gnulib-ffbb0ce.tar.gz
     https://archive.org/download/live-bootstrap-sources/gnulib-ffbb0ce.tar.gz"
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
commit_wrapper "live-bootstrap b1ceced7 x86 pass 2: the perl chain" kairo.toml start.sh package.sh split-musl.sh steps/manifest

cat <<OUT
wrapper = $WRAPPER

# With pass 1 imported, and the same config:
#   kairo build object:$WRAPPER --target x86 --actor $ACTOR
OUT
