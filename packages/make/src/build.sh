# Builds GNU make 4.2.1 into /kairo/package (packages/make/README.md) for
# the target named as the first argument, its first generation, with the
# bootstraps' packages (DECISIONS.md §59): one child, make-4.2.1.
. ./recipe.sh "$1"
NAME=make
VERSION=4.2.1
CHILD=make-4.2.1

# It has no use for guile here.
gnu "$CHILD" "$NAME-$VERSION" --without-guile

finish

# It works, installed over what it was built with.
install_over
[ "$(make --version | head -1)" = "GNU Make $VERSION" ] || { echo "build.sh: make is $(make --version | head -1)" >&2; exit 1; }
mkdir check
printf 'all:\n\t@echo made\n' >check/Makefile
[ "$(make -C check -s)" = made ]
