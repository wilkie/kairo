# Builds GNU libidn2 2.3.8 into /kairo/package (packages/libidn2/README.md) for
# the target named as the first argument, its first release, with
# packages alone (data:build-base, DECISIONS.md §61): one child,
# libidn2-2.3.8.
. ./recipe.sh "$1"
NAME=libidn2
VERSION=2.3.8
CHILD=libidn2-2.3.8

# With the libunistring package, not its own copy. Without its static
# library.
gnu "$CHILD" "$NAME-$VERSION" --disable-static --without-included-libunistring

finish

# It works, installed over what it was built with.
install_over
cat > icheck.c <<'C'
#include <stdlib.h>
#include <string.h>
#include <idn2.h>
int main(void) {
    char *ascii;
    if (idn2_to_ascii_8z("b\xc3\xbc" "cher.example", &ascii, IDN2_NONTRANSITIONAL) != IDN2_OK) return 1;
    return strcmp(ascii, "xn--bcher-kva.example") != 0;
}
C
gcc -o icheck icheck.c -lidn2
./icheck
