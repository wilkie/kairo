# Builds GNU findutils 4.10.0 into /kairo/package (packages/findutils/README.md)
# for the target named as the first argument, its first generation,
# with the bootstraps' packages (DECISIONS.md §59): one child,
# findutils-4.10.0.
. ./recipe.sh "$1"
NAME=findutils
VERSION=4.10.0
CHILD=findutils-4.10.0

gnu "$CHILD" "$NAME-$VERSION"

finish

# It works, installed over what it was built with.
install_over
find --version | head -1 | grep -q " 4.10.0$"
[ "$(find /usr/bin/find -type f)" = /usr/bin/find ]
