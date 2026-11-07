# Builds python 3.11.1 into /kairo/package (packages/python/README.md) for the
# target named as the first argument, its first release, again, with
# OpenSSL and SQLite, with packages alone (data:build-base, DECISIONS.md §61): one child, python-3.11.1.
. ./recipe.sh "$1"
NAME=python
VERSION=3.11.1
CHILD=python-3.11.1

# Without pip, which it would fetch. OpenSSL's headers are in
# /usr/include and its libraries in the library directory, which the
# linker is given. SOURCE_DATE_EPOCH, which Kairo
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
    --enable-shared --with-ensurepip=no --with-system-ffi --with-openssl=/usr \
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
# ssl and hashlib with the OpenSSL package, sqlite3 with the SQLite one,
# and the certificate authorities found where OpenSSL looks.
python3 -c 'import ssl, sys; sys.exit(not ssl.OPENSSL_VERSION.startswith("OpenSSL 3.5.9 "))'
python3 -c 'import _hashlib, hashlib; hashlib.sha3_256(b"made"); hashlib.pbkdf2_hmac("sha256", b"made", b"salt", 1)'
python3 -c 'import ssl, sys; sys.exit(ssl.create_default_context().cert_store_stats()["x509_ca"] < 100)'
python3 -c 'import sqlite3, sys; sys.exit(sqlite3.sqlite_version != "3.54.1")'
python3 -c 'import sys; sys.exit(sys.version_info[:3] != (3, 11, 1))'
