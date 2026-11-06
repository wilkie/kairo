# Builds bzip2 and libbz2 1.0.8 into /kairo/package (packages/bzip2/README.md)
# for the target named as the first argument, its first generation,
# with the bootstraps' packages (DECISIONS.md §59): one child,
# bzip2-1.0.8.
. ./recipe.sh "$1"
NAME=bzip2
VERSION=1.0.8
CHILD=bzip2-1.0.8

# bzip2 has Makefiles alone, whose install makes links by $PREFIX; its
# programs and libraries, static and shared, are copied in, the links
# made relative.
unpack "$NAME-$VERSION"
cd "$BUILD/$NAME-$VERSION"
make -j"$JOBS" -f Makefile-libbz2_so CFLAGS="-O2 -fPIC -D_FILE_OFFSET_BITS=64"
make clean
make -j"$JOBS" CFLAGS="-O2 -D_FILE_OFFSET_BITS=64" bzip2 bzip2recover libbz2.a
PACKAGE=$OUT/$CHILD
mkdir -p "$PACKAGE/usr/bin" "$PACKAGE$LIBDIR" "$PACKAGE/usr/include"
install -m 755 bzip2 bzip2recover "$PACKAGE/usr/bin/"
install -m 644 libbz2.a "$PACKAGE$LIBDIR/"
install -m 755 libbz2.so.1.0.8 "$PACKAGE$LIBDIR/"
ln -s libbz2.so.1.0.8 "$PACKAGE$LIBDIR/libbz2.so.1.0"
ln -s libbz2.so.1.0.8 "$PACKAGE$LIBDIR/libbz2.so"
install -m 644 bzlib.h "$PACKAGE/usr/include/"
ln -s bzip2 "$PACKAGE/usr/bin/bunzip2"
ln -s bzip2 "$PACKAGE/usr/bin/bzcat"

finish

# It works, installed over what it was built with.
install_over
[ "$(echo made | bzip2 | bzip2 -d)" = made ]
