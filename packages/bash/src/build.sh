# Builds GNU bash 5.2.15 into /kairo/package (packages/bash/README.md)
# for the target named as the first argument, its second round, with the
# base's first round (DECISIONS.md §64): one child, bash-5.2.15.
. ./recipe.sh "$1"
NAME=bash
VERSION=5.2.15
CHILD=bash-5.2.15

gnu "$CHILD" "$NAME-$VERSION" --without-bash-malloc
ln -s bash "$OUT/$CHILD/usr/bin/sh"

finish

# It works, installed over what it was built with.
install_over
[ "$(bash -c 'echo "${BASH_VERSINFO[0]}.${BASH_VERSINFO[1]}.${BASH_VERSINFO[2]}"')" = 5.2.15 ] || { echo "build.sh: bash is $(bash --version | head -1)" >&2; exit 1; }
[ "$(sh -c 'echo works')" = works ]
