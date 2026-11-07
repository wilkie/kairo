# Builds Ninja 1.13.2 into /kairo/package (packages/ninja/README.md) for
# the target named as the first argument, its first release, with
# packages alone (data:build-base, DECISIONS.md §61): one child,
# ninja-1.13.2.
. ./recipe.sh "$1"
NAME=ninja
VERSION=1.13.2
CHILD=ninja-1.13.2

# Its source as GitHub archives the release's tag, which Alpine's
# checksum matches. configure.py builds it with g++, without a ninja to
# build with (--bootstrap).
unpack "$NAME-$VERSION"
cd "$BUILD/$NAME-$VERSION"
CXX=g++ python3 configure.py --bootstrap
install -D -m 755 ninja "$OUT/$CHILD/usr/bin/ninja"

finish

# It works, installed over what it was built with.
install_over
[ "$(ninja --version)" = 1.13.2 ]
mkdir nlcheck
cd nlcheck
printf 'rule copy\n  command = cp $in $out\nbuild made: copy source\n' >build.ninja
echo made >source
ninja
[ "$(cat made)" = made ]
