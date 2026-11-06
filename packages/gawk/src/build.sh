# Builds GNU awk 5.3.0 into /kairo/package (packages/gawk/README.md)
# for the target named as the first argument, its first generation,
# with the bootstraps' packages (DECISIONS.md §59): one child,
# gawk-5.3.0.
. ./recipe.sh "$1"
NAME=gawk
VERSION=5.3.0
CHILD=gawk-5.3.0

gnu "$CHILD" "$NAME-$VERSION"

finish

# It works, installed over what it was built with.
install_over
[ "$(echo 2 3 | awk '{ print $1 * $2 }')" = 6 ]
gawk --version | head -1 | grep -q "5.3.0"
