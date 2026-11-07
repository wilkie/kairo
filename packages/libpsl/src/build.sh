# Builds libpsl 0.23.3 into /kairo/package (packages/libpsl/README.md) for
# the target named as the first argument, its first release, with
# packages alone (data:build-base, DECISIONS.md §61): one child,
# libpsl-0.23.3.
. ./recipe.sh "$1"
NAME=libpsl
VERSION=0.23.3
CHILD=libpsl-0.23.3

# The list its release carries, of 2025-08-01, compiled in by its
# psl-make-dafsa, which Python runs; names converted at run time by
# libidn2. Without its static library.
gnu "$CHILD" "$NAME-$VERSION" --disable-static --enable-runtime=libidn2 --enable-builtin

finish

# It works, installed over what it was built with.
install_over
[ "$(psl --is-public-suffix co.uk example.co.uk)" = "co.uk: 1
example.co.uk: 0" ]
psl --version | grep -q "^psl 0\.23\.3"
