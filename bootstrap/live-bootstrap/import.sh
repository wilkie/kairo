#!/usr/bin/env bash
# Archive live-bootstrap and its distfiles, and make the native object
# that runs it through perl 5.6.2 for x86 (specs/DECISIONS.md §38).
#
# Usage:
#   import.sh <actor id> [kairo args...]
#
# 1. `kairo source fetch https://github.com/fosslinux/live-bootstrap`
#    archives live-bootstrap as a source object, with the submodules it
#    pins. This needs the network.
# 2. Each distfile the milestone's steps list is archived as a file
#    source with `kairo source fetch-file`, from the first of its
#    upstream mirrors that serves the bytes live-bootstrap's checksum
#    names.
# 3. The native wrapper -- this directory's kairo.toml, start.kaem, and
#    steps/ (bootstrap.cfg and the truncated manifest) -- is committed as
#    SHA-256 history and created as an object.
#
# The stage0-posix wrapper (bootstrap/stage0-posix/import.sh) must be
# imported and its x86 target built first: its artifact is this
# build's input. Extra arguments go before the command in every `kairo`
# call, e.g. `--store <path>`. KAIRO names the kairo binary (default:
# kairo on PATH). Prints the wrapper's object id.
set -euo pipefail
shopt -s inherit_errexit

LIVE_BOOTSTRAP_URL=https://github.com/fosslinux/live-bootstrap

# sha256 path mirror... (from steps/<step>/sources at b1ceced7)
DISTFILES=(
    "183a40ea47ea49f8a1e3bd1b9d12e676374d64d63bc79e7bc1ae7d673dfdf25d mes-0.27.1.tar.gz
     https://mirrors.kernel.org/gnu/mes/mes-0.27.1.tar.gz"
    "708c943f89c972910e9544ee077771acbd0a2c0fc6d33496fe158264ddb65327 nyacc-1.00.2-lb1.tar.gz
     https://github.com/Googulator/nyacc/releases/download/V1.00.2-lb1/nyacc-1.00.2-lb1.tar.gz
     https://archive.org/download/live-bootstrap-sources/nyacc-1.00.2-lb1.tar.gz
     https://files.bootstrapping.world/nyacc-1.00.2-lb1.tar.gz"
    "6b8cbd0a5fed0636d4f0f763a603247bc1935e206e1cc5bda6a2818bab6e819f tcc-0.9.26.tar.gz
     https://lilypond.org/janneke/tcc/tcc-0.9.26-1147-gee75a10c.tar.gz"
    "de23af78fca90ce32dff2dd45b3432b2334740bb9bb7b05bf60fdbfc396ceb9c tcc-0.9.27.tar.bz2
     https://download.savannah.gnu.org/releases/tinycc/tcc-0.9.27.tar.bz2"
    "e2c1a73f179c40c71e2fe8abf8a8a0688b8499538512984da4a76958d0402966 make-3.82.tar.bz2
     https://mirrors.kernel.org/gnu/make/make-3.82.tar.bz2"
    "ecb5c6469d732bcf01d6ec1afe9e64f1668caba5bfdb103c28d7f537ba3cdb8a patch-2.5.9.tar.gz
     https://mirrors.kernel.org/gnu/patch/patch-2.5.9.tar.gz"
    "1ca41818a23c9c59ef1d5e1d00c0d5eaa2285d931c0fb059637d7c0cc02ad967 gzip-1.2.4.tar.gz
     https://mirrors.kernel.org/gnu/gzip/gzip-1.2.4.tar.gz"
    "c6c37e888b136ccefab903c51149f4b7bd659d69d4aea21245f61053a57aa60a tar-1.12.tar.gz
     https://mirrors.kernel.org/gnu/tar/tar-1.12.tar.gz"
    "c365874794187f8444e5d22998cd5888ffa47f36def4b77517a808dec27c0600 sed-4.0.9.tar.gz
     https://mirrors.kernel.org/gnu/sed/sed-4.0.9.tar.gz"
    "ab5a03176ee106d3f0fa90e381da478ddae405918153cca248e682cd0c4a2269 bzip2-1.0.8.tar.gz
     https://sourceware.org/pub/bzip2/bzip2-1.0.8.tar.gz"
    "c25b36b8af6e0ad2a875daf4d6196bd0df28a62be7dd252e5f99a4d5d7288d95 coreutils-5.0.tar.bz2
     https://mirrors.kernel.org/gnu/coreutils/coreutils-5.0.tar.bz2"
    "eb0866e740b79bd3a23e0ca47885eb3148aab18d77a4bedba96e979d8b4ebfe1 oyacc-6.6.tar.gz
     https://github.com/ibara/yacc/releases/download/oyacc-6.6/oyacc-6.6.tar.gz"
    "ba03d412998cc54bd0b0f2d6c32100967d3137098affdc2d32e6e7c11b163fe4 bash-2.05b.tar.gz
     https://mirrors.kernel.org/gnu/bash/bash-2.05b.tar.gz"
    "1370c9a812b2cf2a7d92802510cca0058cc37e66a7bedd70051f0a34015022a3 musl-1.1.24.tar.gz
     https://musl.libc.org/releases/musl-1.1.24.tar.gz"
    "a32032bab36208509466654df12f507600dfe0313feebbcd218c32a70bf72a16 grep-2.4.tar.gz
     https://mirrors.kernel.org/gnu/grep/grep-2.4.tar.gz"
    "a88f3ddaa7c89cf4c34284385be41ca85e9135369c333fdfa232f3bf48223213 m4-1.4.7.tar.bz2
     https://mirrors.kernel.org/gnu/m4/m4-1.4.7.tar.bz2"
    "9f233d8b78e4351fe9dd2d50d83958a0e5af36f54e9818521458a08e058691ba heirloom-devtools-070527.tar.bz2
     https://downloads.sourceforge.net/project/heirloom/heirloom-devtools/070527/heirloom-devtools-070527.tar.bz2"
    "68aa10c473b6010ffad680cada09fc4eec6b3cc6e415cc2339e5fc2385ccc142 d160f0247ba1611aa59d28f027d6292ba24abb50.tar.gz
     https://github.com/westes/flex/archive/d160f0247ba1611aa59d28f027d6292ba24abb50.tar.gz"
    "e87aae032bf07c26f85ac0ed3250998c37621d95f8bd748b31f15b33c45ee995 flex-2.6.4.tar.gz
     https://github.com/westes/flex/releases/download/v2.6.4/flex-2.6.4.tar.gz"
    "27159ac5ebf736dffd5636fd2cd625767c9e437de65baa63cb0de83570bd820d bison-3.4.1.tar.xz
     https://mirrors.kernel.org/gnu/bison/bison-3.4.1.tar.xz"
    "d5f2489c4056a31528e3ada4adacc23d498532b0af1a980f2f76158162b139d6 diffutils-2.7.tar.gz
     https://mirrors.kernel.org/gnu/diffutils/diffutils-2.7.tar.gz"
    "8b05bba1b2726a164e444c314e3f359604b58216be704bed8f2e028449cc6204 coreutils-6.10.tar.lzma
     https://mirrors.kernel.org/gnu/coreutils/coreutils-6.10.tar.lzma"
    "5cc35def1ff4375a8b9a98c2ff79e95e80987d24f0d42fdbb7b7039b3ddb3fb0 gawk-3.0.4.tar.gz
     https://mirrors.kernel.org/gnu/gawk/gawk-3.0.4.tar.gz"
    "1ae43c8d2983404b9eec61c96e3ffa27e7b07e08215c95c015a4ab0095373ef3 perl-5.000.tar.gz
     https://github.com/Perl/perl5/archive/perl-5.000.tar.gz"
    "9fa29beb2fc4a3c373829fc051830796de301f32a719d0b52a400d1719bbd7b1 perl-5.003.tar.gz
     https://github.com/Perl/perl5/archive/perl-5.003.tar.gz"
    "1184478b298978b164a383ed5661e3a117c48ab97d6d0ab7ef614cdbe918b9eb perl5.004_05.tar.gz
     https://www.cpan.org/src/5.0/perl5.004_05.tar.gz"
    "93f41cd87ab8ee83391cfa39a63b076adeb7c3501d2efa31b98d0ef037122bd1 perl5.005_03.tar.gz
     https://www.cpan.org/src/5.0/perl5.005_03.tar.gz"
    "a5e66f6ebf701b0567f569f57cae82abf5ce57af70a2b45ae71323b61f49134e perl-5.6.2.tar.gz
     https://www.cpan.org/src/5.0/perl-5.6.2.tar.gz"
)

if [ $# -lt 1 ]; then
    echo "usage: $0 <actor id> [kairo args...]" >&2
    exit 2
fi
ACTOR=$1
shift
KAIRO_ARGS=("$@")
KAIRO=${KAIRO:-kairo}
HERE=$(cd "$(dirname "$0")" && pwd)

kairo() { "$KAIRO" "${KAIRO_ARGS[@]}" "$@"; }
field() { sed -n "s/^$1 = //p" | head -n 1; }
fail() {
    echo "import.sh: $*" >&2
    exit 1
}

# 1. live-bootstrap, and the submodules it pins.
kairo source fetch "$LIVE_BOOTSTRAP_URL" --actor "$ACTOR" --require-complete >&2

# 2. The distfiles, each from the first mirror that serves its bytes.
for entry in "${DISTFILES[@]}"; do
    read -r -a words <<<"$(tr '\n' ' ' <<<"$entry")"
    sha256=${words[0]}
    name=${words[1]}
    fetched=no
    for url in "${words[@]:2}"; do
        if kairo source fetch-file "$url" --sha256 "$sha256" --actor "$ACTOR" >&2; then
            fetched=yes
            break
        fi
        echo "import.sh: $url failed; trying the next mirror" >&2
    done
    [ "$fetched" = yes ] || fail "no mirror served $name ($sha256)"
done

# 3. The native wrapper, with fixed author and dates, so importing again
# gives the same revision.
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
export GIT_AUTHOR_NAME=kairo-bootstrap GIT_AUTHOR_EMAIL=bootstrap@kairo.invalid
export GIT_COMMITTER_NAME=kairo-bootstrap GIT_COMMITTER_EMAIL=bootstrap@kairo.invalid
export GIT_AUTHOR_DATE=2026-01-01T00:00:00Z GIT_COMMITTER_DATE=2026-01-01T00:00:00Z
mkdir "$WORK/steps"
cp "$HERE/kairo.toml" "$HERE/start.kaem" "$WORK/"
cp "$HERE/steps/bootstrap.cfg" "$HERE/steps/manifest" "$WORK/steps/"
git -C "$WORK" init -q --initial-branch=main --object-format=sha256
git -C "$WORK" config commit.gpgsign false
git -C "$WORK" add kairo.toml start.kaem steps/bootstrap.cfg steps/manifest
git -C "$WORK" commit -q -m "live-bootstrap b1ceced7 x86 through perl 5.6.2"
OID=$(git -C "$WORK" rev-parse HEAD)

WRAPPER=$(kairo object create --actor "$ACTOR" --kind software | field object)
[ -n "$WRAPPER" ] || fail "object create printed no id"
kairo git fetch --object "$WRAPPER" --remote "file://$WORK" --branch main \
    --object-format sha256 >/dev/null
STATEMENT=$(kairo revision create --actor "$ACTOR" --object "$WRAPPER" \
    --revision "git:sha256:$OID" --manifest "$WORK/kairo.toml" | field statement)
[ -n "$STATEMENT" ] || fail "revision create printed no statement"
kairo branch set --actor "$ACTOR" --object "$WRAPPER" --revision "$STATEMENT" >/dev/null

cat <<OUT
wrapper = $WRAPPER

# With the stage0-posix wrapper's x86 target built, and the same
# [[runtime.seeds]] and [[runtime.engines]] config:
#   kairo build object:$WRAPPER --target x86 --actor $ACTOR
OUT
