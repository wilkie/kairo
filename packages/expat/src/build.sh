# Builds Expat 2.10.0 into /kairo/package (packages/expat/README.md) for
# the target named as the first argument, its first release, with
# packages alone (data:build-base, DECISIONS.md §61): one child,
# expat-2.10.0.
. ./recipe.sh "$1"
NAME=expat
VERSION=2.10.0
CHILD=expat-2.10.0

# Without its static library, its examples, its tests, or the manual
# page, which it would need docbook2man for.
gnu "$CHILD" "$NAME-$VERSION" --disable-static --without-docbook --without-examples --without-tests

finish

# It works, installed over what it was built with.
install_over
printf '<a>made</a>' >made.xml
xmlwf made.xml
cat > xcheck.c <<'C'
#include <string.h>
#include <expat.h>
static char text[16];
static void chars(void *data, const XML_Char *s, int len) { strncat(text, s, len); }
int main(void) {
    XML_Parser p = XML_ParserCreate(NULL);
    XML_SetCharacterDataHandler(p, chars);
    if (XML_Parse(p, "<a>made</a>", 11, 1) != XML_STATUS_OK) return 1;
    XML_ParserFree(p);
    return strcmp(text, "made") != 0 || XML_MAJOR_VERSION != 2 || XML_MINOR_VERSION != 10;
}
C
gcc -o xcheck xcheck.c -lexpat
./xcheck
