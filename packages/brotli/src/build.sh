# Builds Brotli 1.2.1 into /kairo/package (packages/brotli/README.md) for
# the target named as the first argument, its first release, with
# packages alone (data:build-base, DECISIONS.md §61): one child,
# brotli-1.2.1.
. ./recipe.sh "$1"
NAME=brotli
VERSION=1.2.1
CHILD=brotli-1.2.1

# Its source as GitHub archives the release's tag, which Alpine's
# checksum matches, built by CMake with Ninja, its libraries in the
# library directory. Without its tests.
unpack "$NAME-$VERSION"
cd "$BUILD/$NAME-$VERSION"
cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/usr \
    -DCMAKE_INSTALL_LIBDIR="$LIBDIR" -DBUILD_SHARED_LIBS=ON -DBROTLI_DISABLE_TESTS=ON
cmake --build build
DESTDIR="$OUT/$CHILD" cmake --install build

finish

# It works, installed over what it was built with.
install_over
[ "$(echo made | brotli | brotli -d)" = made ]
brotli --version | grep -q "^brotli 1\.2\.1$"
