# Builds GNU diffutils 3.10 into /kairo/package
# (packages/diffutils/README.md) for the target named as the first
# argument, its second round, with the base's first round (DECISIONS.md
# §64): one child, diffutils-3.10.
. ./recipe.sh "$1"
NAME=diffutils
VERSION=3.10
CHILD=diffutils-3.10

gnu "$CHILD" "$NAME-$VERSION"

finish

# It works, installed over what it was built with.
install_over
printf "a\n" >one; printf "b\n" >two
! diff one two >/dev/null
diff --version | head -1 | grep -q " 3.10$"
