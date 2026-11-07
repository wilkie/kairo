# Builds mpdecimal 4.0.1 into /kairo/package (packages/mpdecimal/README.md) for
# the target named as the first argument, its first release, with
# packages alone (data:build-base, DECISIONS.md §61): one child,
# mpdecimal-4.0.1.
. ./recipe.sh "$1"
NAME=mpdecimal
VERSION=4.0.1
CHILD=mpdecimal-4.0.1

# Its configure is autoconf's, but it builds no libtool and takes no
# triple. libmpdec alone: without its C++ library, its static library,
# or its documentation.
unpack "$NAME-$VERSION"
cd "$BUILD/$NAME-$VERSION"
./configure --prefix=/usr --libdir="$LIBDIR" --disable-cxx --disable-static --disable-doc
make -j"$JOBS"
make install DESTDIR="$OUT/$CHILD"

finish

# It works, installed over what it was built with.
install_over
cat > mcheck.c <<'C'
#include <string.h>
#include <mpdecimal.h>
int main(void) {
    mpd_context_t ctx;
    uint32_t status = 0;
    mpd_init(&ctx, 5);
    mpd_t *one = mpd_qnew(), *three = mpd_qnew(), *third = mpd_qnew();
    mpd_qset_string(one, "1", &ctx, &status);
    mpd_qset_string(three, "3", &ctx, &status);
    mpd_qdiv(third, one, three, &ctx, &status);
    char *s = mpd_to_sci(third, 0);
    return strcmp(s, "0.33333") != 0 || strcmp(mpd_version(), "4.0.1") != 0;
}
C
gcc -o mcheck mcheck.c -lmpdec
./mcheck
