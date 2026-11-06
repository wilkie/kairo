# Builds XZ Utils 5.6.4 into /kairo/package (packages/xz/README.md)
# for the target named as the first argument, its first generation,
# with the bootstraps' packages (DECISIONS.md §59): one child,
# xz-5.6.4.
. ./recipe.sh "$1"
NAME=xz
VERSION=5.6.4
CHILD=xz-5.6.4

gnu "$CHILD" "$NAME-$VERSION" --disable-doc

finish

# It works, installed over what it was built with.
install_over
[ "$(echo made | xz | xz -d)" = made ]
