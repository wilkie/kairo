#!/usr/bin/env bash
# Archive live-bootstrap and its distfiles, and make the native object
# that runs it through upstream's CI pass 1 for x86 (specs/DECISIONS.md §38).
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
    "4681bcbb9c9298c506f6405a7deb62c54fc3b339d3239a8f36a5df83daaec94f autoconf-2.52.tar.bz2
     https://mirrors.kernel.org/gnu/autoconf/autoconf-2.52.tar.bz2"
    "6b217a064c6d06603d50a3ad05129aef9435367810c10894210b8dad965d2306 autoconf-2.53.tar.bz2
     https://mirrors.kernel.org/gnu/autoconf/autoconf-2.53.tar.bz2"
    "a74aea954f36c7beeb6cc47b96a408c3e04e7ad635f614e65250dbcd8ec0bd28 autoconf-2.54.tar.bz2
     https://mirrors.kernel.org/gnu/autoconf/autoconf-2.54.tar.bz2"
    "f757158a04889b265203eecd8ca92568e2a67c3b9062fa6bff7a0a6efd2244ac autoconf-2.55.tar.bz2
     https://mirrors.kernel.org/gnu/autoconf/autoconf-2.55.tar.bz2"
    "e1035aa2c21fae2a934d1ab56c774ce9d22717881dab8a1a5b16d294fb793489 autoconf-2.57.tar.bz2
     https://mirrors.kernel.org/gnu/autoconf/autoconf-2.57.tar.bz2"
    "f0cde70a8f135098a6a3e85869f2e1cc3f141beea766fa3d6636e086cd8b90a7 autoconf-2.59.tar.bz2
     https://mirrors.kernel.org/gnu/autoconf/autoconf-2.59.tar.bz2"
    "93a2ceab963618b021db153f0c881a2de82455c1dc7422be436fcd5c554085a1 autoconf-2.61.tar.bz2
     https://mirrors.kernel.org/gnu/autoconf/autoconf-2.61.tar.bz2"
    "32d977213320b8ae76c71175305301197f2b0e04e72d70694bc3d3e2ae6c7248 autoconf-2.64.tar.xz
     https://mirrors.kernel.org/gnu/autoconf/autoconf-2.64.tar.xz"
    "64ebcec9f8ac5b2487125a86a7760d2591ac9e1d3dbd59489633f9de62a57684 autoconf-2.69.tar.xz
     https://mirrors.kernel.org/gnu/autoconf/autoconf-2.69.tar.xz"
    "e98ab43bb839c31696a4202e5b6ff388b391659ef2387cf9365019fad17e1adc automake-1.10.3.tar.bz2
     https://mirrors.kernel.org/gnu/automake/automake-1.10.3.tar.bz2"
    "4f46d1f9380c8a3506280750f630e9fc915cb1a435b724be56b499d016368718 automake-1.11.2.tar.bz2
     https://mirrors.kernel.org/gnu/automake/automake-1.11.2.tar.bz2"
    "af6ba39142220687c500f79b4aa2f181d9b24e4f8d8ec497cea4ba26c64bedaf automake-1.15.1.tar.xz
     https://mirrors.kernel.org/gnu/automake/automake-1.15.1.tar.xz"
    "ff2bf7656c4d1c6fdda3b8bebb21f09153a736bcba169aaf65eab25fa113bf3a automake-1.16.3.tar.xz
     https://mirrors.kernel.org/gnu/automake/automake-1.16.3.tar.xz"
    "0dbafacaf21e135cab35d357a14bdcd981d2f2d00e1387801be8091a31b7bb81 automake-1.6.3.tar.bz2
     https://mirrors.kernel.org/gnu/automake/automake-1.6.3.tar.bz2"
    "2dddc3b51506e702647ccc6757e15c05323fa67245d2d53e81ed36a832f9be42 automake-1.7.8.tar.bz2
     https://mirrors.kernel.org/gnu/automake/automake-1.7.8.tar.bz2"
    "6633ee1202375e3c8798a92e1b7f46894f78d541aeea7f49654503fdc0b28835 automake-1.7.tar.bz2
     https://mirrors.kernel.org/gnu/automake/automake-1.7.tar.bz2"
    "84c93aaa3c3651a9e7474b721b0e6788318592509e7de604bafe4ea8049dc410 automake-1.8.5.tar.bz2
     https://mirrors.kernel.org/gnu/automake/automake-1.8.5.tar.bz2"
    "8eccaa98e1863d10e4a5f861d8e2ec349a23e88cb12ad10f6b6f79022ad2bb8d automake-1.9.6.tar.bz2
     https://mirrors.kernel.org/gnu/automake/automake-1.9.6.tar.bz2"
    "6e46b8aeae2f727a36f0bd9505e405768a72218f1796f0d09757d45209871ae6 binutils-2.30.tar.xz
     https://mirrors.kernel.org/gnu/binutils/binutils-2.30.tar.xz"
    "e9bf58c761a4f988311aef6b41f12fd5c7e51d09477468fb73826aecc1be32e7 gcc-core-4.0.4.tar.bz2
     https://mirrors.kernel.org/gnu/gcc/gcc-4.0.4/gcc-core-4.0.4.tar.bz2"
    "a4adadf76b496a6bc50795702253ecfcb6f0d159b68038f31a5362009340bca2 help2man-1.36.4.tar.gz
     https://mirrors.kernel.org/gnu/help2man/help2man-1.36.4.tar.gz"
    "d81839fa4d566dbef7c286fdca9b430d3530983fff6d389fac0f08baf27e4c3a libtool-2.2.4.tar.lzma
     https://mirrors.kernel.org/gnu/libtool/libtool-2.2.4.tar.lzma"
    "813cd9405aceec5cfecbe96400d01e90ddad7b512d3034487176ce5258ab0f78 findutils-4.2.33.tar.gz
     https://mirrors.kernel.org/gnu/findutils/findutils-4.2.33.tar.gz"
    "a9a118bbe84d8764da0ea0d28b3ab3fae8477fc7e4085d90102b8596fc7c75e4 musl-1.2.5.tar.gz
     https://musl.libc.org/releases/musl-1.2.5.tar.gz"
    "0820fdb7971c6974338081c11fbf2dc869870501e7bdcac4d0ed58ba1f57b61c linux-4.14.336.tar.xz
     https://cdn.kernel.org/pub/linux/kernel/v4.x/linux-4.14.336.tar.xz"
    "d12e0f1be0257eadca6a2653acfb4792571c9f7189ed642873363f9b6797bb51 util-linux-2.19.1.tar.xz
     https://mirrors.kernel.org/pub/linux/utils/util-linux/v2.19/util-linux-2.19.1.tar.xz"
    "62d49c86d9d4becf305093edd65464484dc9ea41c6ff9ae4f536e4a341b171a2 e2fsprogs-1.45.7.tar.xz
     https://mirrors.edge.kernel.org/pub/linux/kernel/people/tytso/e2fsprogs/v1.45.7/e2fsprogs-1.45.7.tar.xz"
    "64f117a4749dd4a1b6c54277f63f6cf1e0eb45d290cbedaf777fbe71b8880885 CaseFolding.txt
     https://www.unicode.org/Public/11.0.0/ucd/CaseFolding.txt"
    "eb115a5de9a32c9ad447d6ea1cddcadb53d47f6cbc2521f3fe0bebb040c39866 DerivedAge.txt
     https://www.unicode.org/Public/11.0.0/ucd/DerivedAge.txt"
    "11c8bd81ecbede4d67c7b5b693a471647d5401956707c639ae053b836cc7f5da DerivedCombiningClass.txt
     https://www.unicode.org/Public/11.0.0/ucd/extracted/DerivedCombiningClass.txt"
    "3406825d64564bf2a37031c36a3e0f99d708aa17595b81f8b539d0f3d1a3923f DerivedCoreProperties.txt
     https://www.unicode.org/Public/11.0.0/ucd/DerivedCoreProperties.txt"
    "c9ffe32e616fa085246644c2351c525788fac363872491185dab7d5ce69fefa9 NormalizationCorrections.txt
     https://www.unicode.org/Public/11.0.0/ucd/NormalizationCorrections.txt"
    "0fdfc17093dd5482f8089cb11dcd936abdba34c4c9c324e5b8a4e5d8f943f6d3 NormalizationTest.txt
     https://www.unicode.org/Public/11.0.0/ucd/NormalizationTest.txt"
    "4997a3196eb79b4d0d6b8384560f6aeb46a062693f0abd5ba736abbff7976099 UnicodeData.txt
     https://www.unicode.org/Public/11.0.0/ucd/UnicodeData.txt"
    "2bd3480bc93e6bff530872b8bc80cbcaa821449f7bf6aaf202fa12fb8c2e6f55 v10.0.1.tar.gz
     https://github.com/NetworkConfiguration/dhcpcd/archive/refs/tags/v10.0.1.tar.gz"
    "92f59ecaa85fe2746ef1830ac01eef84c394a413f7c7326b8d0a5c819e87502f kbd-1.15.tar.xz
     https://mirrors.edge.kernel.org/pub/linux/utils/kbd/kbd-1.15.tar.xz"
    "db36da85ee1a9d8bafb4b041bd4c8c11becba0c43ec446353b67045de1634fda ed-1.4.tar.gz
     https://mirrors.kernel.org/gnu/ed/ed-1.4.tar.gz"
    "515430115b3334c636317503460a0950dff79940aa3259ce2c1aa67c2881d023 bc-1.08.1.tar.xz
     https://mirrors.kernel.org/gnu/bc/bc-1.08.1.tar.xz"
    "0cfbf866bc39c31f25fa0e56af1e56c5e5c92fc1e5d51242ebafef7ea211f3d5 gnulib-8e128e.tar.gz
     https://archive.org/download/live-bootstrap-sources/gnulib-8e128e.tar.gz"
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
    urls=("${words[@]:2}")
    # GNU's own server serves every file kernel.org's GNU mirror does.
    for url in "${words[@]:2}"; do
        case "$url" in
        https://mirrors.kernel.org/gnu/*)
            urls+=("https://ftp.gnu.org/gnu/${url#https://mirrors.kernel.org/gnu/}")
            ;;
        esac
    done
    fetched=no
    for url in "${urls[@]}"; do
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
git -C "$WORK" commit -q -m "live-bootstrap b1ceced7 x86 through CI pass 1"
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
