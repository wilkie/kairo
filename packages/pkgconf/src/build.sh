# Builds pkgconf 3.0.10 into /kairo/package (packages/pkgconf/README.md) for
# the target named as the first argument, its first release, with
# packages alone (data:build-base, DECISIONS.md §61): one child,
# pkgconf-3.0.10.
. ./recipe.sh "$1"
NAME=pkgconf
VERSION=3.0.10
CHILD=pkgconf-3.0.10

# It looks for .pc files in the library directory, where every
# package's libraries put theirs (§60), then /usr/share/pkgconfig, and
# leaves out the flags that name the library directory and /usr/include,
# which the compiler searches anyway. pkg-config is a link to it, as
# Alpine and Debian have it. Without its static library.
gnu "$CHILD" "$NAME-$VERSION" --disable-static \
    --with-pkg-config-dir="$LIBDIR/pkgconfig:/usr/share/pkgconfig" \
    --with-system-libdir="$LIBDIR" --with-system-includedir=/usr/include
ln -s pkgconf "$OUT/$CHILD/usr/bin/pkg-config"

finish

# It works, installed over what it was built with.
install_over
[ "$(pkg-config --modversion libpkgconf)" = 3.0.10 ]
[ "$(pkg-config --libs libpkgconf)" = "-lpkgconf" ]
[ "$(pkg-config --cflags libpkgconf)" = "-I/usr/include/pkgconf" ]
