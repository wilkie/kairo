# Builds GNU binutils 2.41 into /kairo/package
# (packages/binutils/README.md) for the musl target named as the first
# argument, its second round, with the base's first round (DECISIONS.md
# §64): one child, binutils-2.41.
. ./recipe.sh "$1"
NAME=binutils
VERSION=2.41
CHILD=binutils-2.41

# For its own machine. gprofng, a profiler for native programs, doesn't
# compile under gcc 15's default C23. Archives are made deterministic.
gnu "$CHILD" "$NAME-$VERSION" --target="$TRIPLE" --disable-werror \
    --disable-multilib --enable-deterministic-archives --disable-gprofng

finish

# It works, installed over what it was built with: gcc assembles and
# links with it, and it names its version.
install_over
ld --version | head -1 | grep -q " $VERSION\$" || { echo "build.sh: ld is $(ld --version | head -1)" >&2; exit 1; }
printf '#include <stdio.h>\nint main(void) { puts("hello"); return 0; }\n' >hello.c
gcc -o hello hello.c
[ "$(./hello)" = hello ]
