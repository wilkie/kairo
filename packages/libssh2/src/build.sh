# Builds libssh2 1.11.1 into /kairo/package (packages/libssh2/README.md) for
# the target named as the first argument, its first release, with
# packages alone (data:build-base, DECISIONS.md §61): one child,
# libssh2-1.11.1.
. ./recipe.sh "$1"
NAME=libssh2
VERSION=1.11.1
CHILD=libssh2-1.11.1

# Its cryptography by OpenSSL, compression by zlib. Without its static
# library, examples, or tests.
gnu "$CHILD" "$NAME-$VERSION" --disable-static --with-crypto=openssl --with-libz \
    --disable-examples-build --disable-tests

finish

# It works, installed over what it was built with.
install_over
cat > scheck.c <<'C'
#include <string.h>
#include <libssh2.h>
int main(void) {
    if (libssh2_init(0) != 0) return 1;
    return strcmp(libssh2_version(0), "1.11.1") != 0;
}
C
gcc -o scheck scheck.c -lssh2
./scheck
