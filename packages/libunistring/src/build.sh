# Builds GNU libunistring 1.5 into /kairo/package (packages/libunistring/README.md) for
# the target named as the first argument, its first release, with
# packages alone (data:build-base, DECISIONS.md §61): one child,
# libunistring-1.5.
. ./recipe.sh "$1"
NAME=libunistring
VERSION=1.5
CHILD=libunistring-1.5

# Without its static library.
gnu "$CHILD" "$NAME-$VERSION" --disable-static

finish

# It works, installed over what it was built with.
install_over
cat > ucheck.c <<'C'
#include <unistr.h>
int main(void) { return u8_mbsnlen((const uint8_t *)"m\xc3\xa4" "de", 5) != 4; }
C
gcc -o ucheck ucheck.c -lunistring
./ucheck
