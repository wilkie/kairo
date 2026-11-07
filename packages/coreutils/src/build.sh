# Builds GNU coreutils 9.4 into /kairo/package
# (packages/coreutils/README.md) for the target named as the first
# argument, its second round, with the base's first round (DECISIONS.md
# §64): one child, coreutils-9.4.
. ./recipe.sh "$1"
NAME=coreutils
VERSION=9.4
CHILD=coreutils-9.4

gnu "$CHILD" "$NAME-$VERSION" --enable-no-install-program=kill,uptime

finish

# It works, installed over what it was built with.
install_over
ls --version | head -1 | grep -q " 9.4$" || { echo "build.sh: ls is $(ls --version | head -1)" >&2; exit 1; }
[ "$(echo b a | tr " " "\n" | sort | head -1)" = a ]
