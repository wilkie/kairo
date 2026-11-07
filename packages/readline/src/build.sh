# Builds GNU readline 8.3.6 into /kairo/package (packages/readline/README.md) for
# the target named as the first argument, its first release, with
# packages alone (data:build-base, DECISIONS.md §61): one child,
# readline-8.3.6.
. ./recipe.sh "$1"
NAME=readline
VERSION=8.3.6
CHILD=readline-8.3.6

# readline 8.3 and its patches, 1 to 6, as GNU publishes them: 8.3.6,
# as GNU names 8.2 with its thirteen, 8.2.13. Its shared libraries link
# ncurses, so a program that uses readline needn't name it, as LFS and
# Alpine build them. Without its static libraries.
unpack "readline-8.3"
cd "$BUILD/readline-8.3"
for n in 1 2 3 4 5 6; do
    patch -p0 < "$SRC/distfiles/readline83-00$n"
done
cd "$BUILD"
mkdir readline-build
cd readline-build
../readline-8.3/configure --build="$TRIPLE" --host="$TRIPLE" --prefix=/usr \
    --libdir="$LIBDIR" --with-curses --disable-static --disable-install-examples
make -j"$JOBS" SHLIB_LIBS=-lncursesw
make install DESTDIR="$OUT/$CHILD" SHLIB_LIBS=-lncursesw

finish

# It works, installed over what it was built with.
install_over
cat > rcheck.c <<'C'
#include <stdio.h>
#include <string.h>
#include <readline/readline.h>
int main(void) {
    return strcmp(rl_library_version, "8.3") != 0 || rl_readline_version != 0x0803;
}
C
gcc -o rcheck rcheck.c -lreadline
./rcheck
