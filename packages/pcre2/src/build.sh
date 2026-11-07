# Builds PCRE2 10.49 into /kairo/package (packages/pcre2/README.md) for
# the target named as the first argument, its first release, with
# packages alone (data:build-base, DECISIONS.md §61): one child,
# pcre2-10.49.
. ./recipe.sh "$1"
NAME=pcre2
VERSION=10.49
CHILD=pcre2-10.49

# Each width's library, with the just-in-time compiler, as Debian and
# Alpine build it. Without its static libraries.
gnu "$CHILD" "$NAME-$VERSION" --enable-pcre2-16 --enable-pcre2-32 --enable-jit --disable-static

finish

# It works, installed over what it was built with.
install_over
[ "$(echo "made 42 times" | pcre2grep -o '\d+(?= times)')" = 42 ]
# The just-in-time compiler is built in and can run here.
config=$(pcre2test -C)
grep -qx "  Just-in-time compiler support" <<<"$config"
grep -qx "    Can allocate executable memory: Yes" <<<"$config"
