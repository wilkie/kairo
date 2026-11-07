# Builds ncurses 6.6 into /kairo/package (packages/ncurses/README.md) for
# the target named as the first argument, its first release, with
# packages alone (data:build-base, DECISIONS.md §61): one child,
# ncurses-6.6.
. ./recipe.sh "$1"
NAME=ncurses
VERSION=6.6
CHILD=ncurses-6.6

# Wide characters alone, as every distribution now builds it: the
# libraries are libncursesw and its companions, their headers in
# /usr/include itself. The names without the w, which older configure
# scripts link with, are links to them, as LFS makes them. The terminfo
# database is in /usr/share/terminfo, read there alone. Without its
# static libraries, its C++ binding, Ada, and its tests.
unpack "$NAME-$VERSION"
mkdir "$BUILD/$NAME-build"
cd "$BUILD/$NAME-build"
"../$NAME-$VERSION/configure" --build="$TRIPLE" --host="$TRIPLE" --prefix=/usr \
    --libdir="$LIBDIR" --with-shared --without-normal --without-debug \
    --without-cxx-binding --without-ada --without-tests --without-manpages \
    --enable-widec --enable-overwrite --disable-stripping --enable-pc-files \
    --with-pkg-config-libdir="$LIBDIR/pkgconfig" \
    --with-terminfo-dirs=/usr/share/terminfo --with-default-terminfo-dir=/usr/share/terminfo
make -j"$JOBS"
make install DESTDIR="$OUT/$CHILD"
for lib in ncurses form panel menu; do
    ln -s "lib${lib}w.so" "$OUT/$CHILD$LIBDIR/lib$lib.so"
    [ ! -e "$OUT/$CHILD$LIBDIR/pkgconfig/${lib}w.pc" ] || ln -s "${lib}w.pc" "$OUT/$CHILD$LIBDIR/pkgconfig/$lib.pc"
done
ln -s libncursesw.so "$OUT/$CHILD$LIBDIR/libcurses.so"

finish

# It works, installed over what it was built with.
install_over
infocmp xterm | grep -q "colors#8"
[ "$(tput -T vt100 cols)" = 80 ]
cat > ccheck.c <<'C'
#include <curses.h>
#include <term.h>
int main(void) {
    int err;
    if (setupterm("xterm", 1, &err) != OK) return 1;
    return tigetnum("colors") != 8;
}
C
gcc -o ccheck ccheck.c -lncurses
./ccheck
