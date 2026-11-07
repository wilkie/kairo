# Builds zlib 1.3.1 into /kairo/package (packages/zlib/README.md) for the
# target named as the first argument, its first release, with packages
# alone (data:build-base, DECISIONS.md §61): one child, zlib-1.3.1.
. ./recipe.sh "$1"
NAME=zlib
VERSION=1.3.1
CHILD=zlib-1.3.1

# zlib's configure is its own, not autoconf's: it takes no triple, and
# builds for what gcc targets.
unpack "$NAME-$VERSION"
cd "$BUILD/$NAME-$VERSION"
./configure --prefix=/usr --libdir="$LIBDIR"
make -j"$JOBS"
make install DESTDIR="$OUT/$CHILD"

finish

# It works, installed over what it was built with.
install_over
cat > zcheck.c <<'C'
#include <string.h>
#include <zlib.h>
int main(void) {
    unsigned char packed[64], unpacked[16];
    uLongf p = sizeof packed, u = sizeof unpacked;
    if (compress(packed, &p, (const Bytef *)"made", 5) != Z_OK) return 1;
    if (uncompress(unpacked, &u, packed, p) != Z_OK) return 1;
    return strcmp((char *)unpacked, "made") != 0;
}
C
gcc -o zcheck zcheck.c -lz
./zcheck
