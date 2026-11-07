# Builds GNU sed 4.8 into /kairo/package (packages/sed/README.md) for
# the target named as the first argument, its second round, with the
# base's first round (DECISIONS.md §64): one child, sed-4.8.
. ./recipe.sh "$1"
NAME=sed
VERSION=4.8
CHILD=sed-4.8

gnu "$CHILD" "$NAME-$VERSION"

finish

# It works, installed over what it was built with.
install_over
[ "$(echo hello | sed s/hello/made/)" = made ]
sed --version | head -1 | grep -q " 4.8$"
