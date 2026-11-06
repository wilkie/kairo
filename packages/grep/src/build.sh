# Builds GNU grep 3.12 into /kairo/package (packages/grep/README.md)
# for the target named as the first argument, its first generation,
# with the bootstraps' packages (DECISIONS.md §59): one child,
# grep-3.12.
. ./recipe.sh "$1"
NAME=grep
VERSION=3.12
CHILD=grep-3.12

gnu "$CHILD" "$NAME-$VERSION"

finish

# It works, installed over what it was built with.
install_over
[ "$(printf "a\nb\n" | grep -c b)" = 1 ]
grep --version | head -1 | grep -q " 3.12$"
