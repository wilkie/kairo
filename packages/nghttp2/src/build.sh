# Builds libnghttp2 1.71.0 into /kairo/package (packages/nghttp2/README.md) for
# the target named as the first argument, its first release, with
# packages alone (data:build-base, DECISIONS.md §61): one child,
# nghttp2-1.71.0.
. ./recipe.sh "$1"
NAME=nghttp2
VERSION=1.71.0
CHILD=nghttp2-1.71.0

# The library alone, as curl uses it; its applications would need
# libev and more. Without its static library.
gnu "$CHILD" "$NAME-$VERSION" --enable-lib-only --disable-static

finish

# It works, installed over what it was built with.
install_over
cat > ncheck.c <<'C'
#include <string.h>
#include <nghttp2/nghttp2.h>
int main(void) { return strcmp(nghttp2_version(0)->version_str, "1.71.0") != 0; }
C
gcc -o ncheck ncheck.c -lnghttp2
./ncheck
