# Builds GNU tar 1.34 into /kairo/package (packages/tar/README.md) for
# the target named as the first argument, its second round, with the
# base's first round (DECISIONS.md §64): one child, tar-1.34.
. ./recipe.sh "$1"
NAME=tar
VERSION=1.34
CHILD=tar-1.34

gnu "$CHILD" "$NAME-$VERSION"

finish

# It works, installed over what it was built with.
install_over
tar --version | head -1 | grep -q " 1.34$"
mkdir t; echo x >t/f; tar -cf t.tar t; [ "$(tar -tf t.tar | head -1)" = t/ ]
