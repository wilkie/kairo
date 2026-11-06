# Builds GNU gzip 1.13 into /kairo/package (packages/gzip/README.md)
# for the target named as the first argument, its first generation,
# with the bootstraps' packages (DECISIONS.md §59): one child,
# gzip-1.13.
. ./recipe.sh "$1"
NAME=gzip
VERSION=1.13
CHILD=gzip-1.13

gnu "$CHILD" "$NAME-$VERSION"

finish

# It works, installed over what it was built with.
install_over
[ "$(echo made | gzip | gzip -d)" = made ]
