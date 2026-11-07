# Builds SQLite 3.54.1 into /kairo/package (packages/sqlite/README.md) for
# the target named as the first argument, its first release, with
# packages alone (data:build-base, DECISIONS.md §61): one child,
# sqlite-3.54.1.
. ./recipe.sh "$1"
NAME=sqlite
VERSION=3.54.1
CHILD=sqlite-3.54.1

# The amalgamation, configured by autosetup, which builds its own Tcl,
# jimsh, to run. With the extensions Debian and Alpine enable (FTS3,
# FTS4, FTS5, R*Tree; JSON is built in), and its shell without line
# editing, for which no package is built yet. Without its static
# library. Its library's soname is libsqlite3.so.0, as it has always
# been and as distributions keep it; autosetup sets none unless told.
unpack "sqlite-autoconf-3540100"
cd "$BUILD/sqlite-autoconf-3540100"
./configure --build="$TRIPLE" --host="$TRIPLE" --prefix=/usr --libdir="$LIBDIR" \
    --all --fts3 --disable-readline --disable-static --soname=legacy CFLAGS=-O2
make -j"$JOBS"
make install DESTDIR="$OUT/$CHILD"

finish

# It works, installed over what it was built with.
install_over
readelf -d "$LIBDIR/libsqlite3.so" | grep -q "(SONAME).*\[libsqlite3\.so\.0\]"
[ "$(sqlite3 :memory: "select sqlite_version()")" = 3.54.1 ]
[ "$(sqlite3 :memory: "create virtual table t using fts5(x); insert into t values ('made it'); select x from t where t match 'made'")" = "made it" ]
