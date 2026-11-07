# Builds GNU dbm 1.26 into /kairo/package (packages/gdbm/README.md) for
# the target named as the first argument, its first release, with
# packages alone (data:build-base, DECISIONS.md §61): one child,
# gdbm-1.26.
. ./recipe.sh "$1"
NAME=gdbm
VERSION=1.26
CHILD=gdbm-1.26

# With libgdbm_compat, the ndbm interface, which Python's dbm.ndbm
# uses; gdbmtool without line editing. Without its static libraries.
gnu "$CHILD" "$NAME-$VERSION" --enable-libgdbm-compat --without-readline --disable-static

finish

# It works, installed over what it was built with.
install_over
cat > gcheck.c <<'C'
#include <stdlib.h>
#include <string.h>
#include <gdbm.h>
int main(void) {
    GDBM_FILE db = gdbm_open("check.db", 0, GDBM_NEWDB, 0600, NULL);
    datum key = { "a", 1 }, value = { "made", 5 }, found;
    if (!db || gdbm_store(db, key, value, GDBM_REPLACE) != 0) return 1;
    found = gdbm_fetch(db, key);
    gdbm_close(db);
    return !found.dptr || strcmp(found.dptr, "made") != 0;
}
C
gcc -o gcheck gcheck.c -lgdbm
./gcheck
