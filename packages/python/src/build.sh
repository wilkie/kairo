# Builds python 3.11.1 into /kairo/package (packages/python/README.md) for the
# target named as the first argument, its first release, with packages
# alone (data:build-base, DECISIONS.md §61): one child, python-3.11.1.
. ./recipe.sh "$1"
NAME=python
VERSION=3.11.1
CHILD=python-3.11.1

# Without pip, which it would fetch. SOURCE_DATE_EPOCH, which Kairo
# sets, makes its bytecode checked by hash, not by time. Its Makefile
# records a python found installed, to regenerate files with; none is
# used, so it records the one it falls back to.
# Its standard library and compiled modules stay in /usr/lib/python3.11,
# where it looks for them, whatever its libdir; its modules' build is
# told where the libraries they load are, and its own shared library,
# with pkg-config's files, moves to the library directory (§60), as
# Debian's multiarch python has it.
unpack "Python-$VERSION"
mkdir "$BUILD/python"
cd "$BUILD/python"
"../Python-$VERSION/configure" --build="$TRIPLE" --host="$TRIPLE" --prefix=/usr \
    --enable-shared --with-ensurepip=no --with-system-ffi \
    PYTHON_FOR_REGEN=python3 LDFLAGS="-L$LIBDIR"
make -j"$JOBS"
make install DESTDIR="$OUT/$CHILD"
mkdir -p "$OUT/$CHILD$LIBDIR"
mv "$OUT/$CHILD"/usr/lib/libpython3*.so* "$OUT/$CHILD/usr/lib/pkgconfig" "$OUT/$CHILD$LIBDIR/"

finish

# It works, installed over what it was built with.
install_over
[ "$(python3 -c 'print("made")')" = made ]
python3 -c 'import bz2, ctypes, lzma, zlib'
python3 -c 'import sys; sys.exit(sys.version_info[:3] != (3, 11, 1))'
