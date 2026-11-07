# Builds util-linux's libuuid 2.42.4 into /kairo/package (packages/util-linux/README.md) for
# the target named as the first argument, its first release, with
# packages alone (data:build-base, DECISIONS.md §61): one child,
# libuuid-2.42.4.
. ./recipe.sh "$1"
NAME=util-linux
VERSION=2.42.4
CHILD=libuuid-2.42.4

# util-linux is one object, its parts children of their own: this
# revision builds libuuid alone, as libuuid-<version>. Its programs and
# other libraries can be later children of the same object.
gnu "$CHILD" "$NAME-$VERSION" --disable-all-programs --enable-libuuid --disable-static \
    --disable-bash-completion --without-python --without-systemd --without-udev

finish

# It works, installed over what it was built with.
install_over
cat > ucheck.c <<'C'
#include <string.h>
#include <uuid/uuid.h>
int main(void) {
    uuid_t made, back;
    char text[37];
    uuid_generate(made);
    uuid_unparse(made, text);
    return strlen(text) != 36 || uuid_parse(text, back) != 0 || uuid_compare(made, back) != 0;
}
C
gcc -o ucheck ucheck.c -luuid
./ucheck
