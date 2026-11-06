# Builds GNU patch 2.8 into /kairo/package (packages/patch/README.md)
# for the target named as the first argument, its first generation,
# with the bootstraps' packages (DECISIONS.md §59): one child,
# patch-2.8.
. ./recipe.sh "$1"
NAME=patch
VERSION=2.8
CHILD=patch-2.8

gnu "$CHILD" "$NAME-$VERSION"

finish

# It works, installed over what it was built with.
install_over
patch --version | head -1 | grep -q " 2.8$"
