# Builds Zstandard 1.5.7 into /kairo/package (packages/zstd/README.md) for
# the target named as the first argument, its first release, with
# packages alone (data:build-base, DECISIONS.md §61): one child,
# zstd-1.5.7.
. ./recipe.sh "$1"
NAME=zstd
VERSION=1.5.7
CHILD=zstd-1.5.7

# Its Makefiles alone. Its programs would read gzip, xz, and lz4 when
# their libraries are found, and xz's is in what it builds with; they
# are told none, so what they are doesn't depend on what is there. The
# compiler is gcc, as the base has no cc.
# Without its static library.
unpack "$NAME-$VERSION"
cd "$BUILD/$NAME-$VERSION"
ZSTD_MAKE=(CC=gcc PREFIX=/usr LIBDIR="$LIBDIR" HAVE_ZLIB=0 HAVE_LZMA=0 HAVE_LZ4=0)
make -j"$JOBS" "${ZSTD_MAKE[@]}"
make install DESTDIR="$OUT/$CHILD" "${ZSTD_MAKE[@]}"
rm "$OUT/$CHILD$LIBDIR/libzstd.a"

finish

# It works, installed over what it was built with.
install_over
zstd --version | grep -q "v1.5.7,"
[ "$(echo made | zstd | zstd -d)" = made ]
