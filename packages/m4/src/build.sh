# Builds GNU m4 1.4.19 into /kairo/package (packages/m4/README.md) for
# the target named as the first argument, a round of the base after the
# first, with the round before it (DECISIONS.md §64): one child,
# m4-1.4.19.
. ./recipe.sh "$1"
NAME=m4
VERSION=1.4.19
CHILD=m4-1.4.19

gnu "$CHILD" "$NAME-$VERSION"

finish

# It works, installed over what it was built with.
install_over
[ "$(echo "define(x, made)x" | m4)" = made ]
