# Builds Python 3.15.2 into /kairo/package (packages/python/README.md) for
# the target named as the first argument, a release after its first, with
# packages alone (data:build-base, DECISIONS.md §61): one child,
# python-3.15.2.
. ./recipe.sh "$1"
NAME=python
VERSION=3.15.2
CHILD=python-3.15.2

# With pip, which ensurepip installs from the wheel Python's source
# bundles, fetching nothing. Every library it builds against is a
# package's: expat and mpdecimal too, whose copies it would otherwise
# build in. OpenSSL's headers are in /usr/include and the libraries in
# the library directory, which the linker is given. SOURCE_DATE_EPOCH,
# which Kairo sets, makes its bytecode, pip's too, checked by hash, not
# by time. Its Makefile records a python found installed, to regenerate
# files with; none is used, so it records the one it falls back to.
# Its standard library and compiled modules stay in /usr/lib/python3.15,
# where it looks for them, whatever its libdir; its own shared library,
# with pkg-config's files, moves to the library directory (§60), as
# Debian's multiarch python has it.
unpack "Python-$VERSION"
mkdir "$BUILD/python"
cd "$BUILD/python"
"../Python-$VERSION/configure" --build="$TRIPLE" --host="$TRIPLE" --prefix=/usr \
    --enable-shared --with-ensurepip=install --with-system-expat --with-system-libmpdec \
    --with-openssl=/usr \
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
# curses with ncurses and its terminfo, readline with GNU readline, dbm
# with gdbm both ways, and uuid with libuuid.
TERM=xterm python3 -c 'import curses, sys; curses.setupterm(); sys.exit(curses.tigetnum("colors") != 8)'
python3 -c 'import readline, sys; sys.exit(readline._READLINE_LIBRARY_VERSION != "8.3")'
python3 -c 'import dbm.gnu, dbm.ndbm, sys; d = dbm.gnu.open("check", "n"); d[b"a"] = b"made"; sys.exit(d[b"a"] != b"made")'
python3 -c 'import _uuid, uuid, sys; sys.exit(len(str(uuid.uuid1())) != 36)'
# expat, mpdecimal, and zstd, the packages'.
python3 -c 'import pyexpat, sys; sys.exit(pyexpat.EXPAT_VERSION != "expat_2.10.0")'
python3 -c 'import decimal, sys; sys.exit(decimal.__libmpdec_version__ != "4.0.1" or str(decimal.Decimal(1) / 3)[:5] != "0.333")'
python3 -c 'import sys; from compression import zstd; sys.exit(zstd.decompress(zstd.compress(b"made")) != b"made" or zstd.zstd_version_info[:3] != (1, 5, 7))'
python3 -c 'import sys; sys.exit(sys.version_info[:3] != (3, 15, 2))'
# pip, run by the installed python, and in a new environment, as venv
# installs it from the same wheel.
[ "$(head -n 1 /usr/bin/pip3)" = "#!/usr/bin/python3.15" ]
pip3 --version | grep -q "^pip 26\.2\.2 from /usr/lib/python3\.15/site-packages/pip "
python3 -m venv venv
venv/bin/python -m pip --version | grep -q "^pip 26\.2\.2 "
