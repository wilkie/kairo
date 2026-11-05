# Builds GNU make 4.2.1 into /kairo/package (packages/make/README.md) for
# the musl target named as the first argument, its first generation,
# with the bootstraps' packages (DECISIONS.md §59).
set -euo pipefail
export PATH=/usr/bin
# The x86 guest runs on a 64-bit kernel, whose machine config.guess would
# name: the triple is given, for x86-64 too.
TRIPLE=$1
# Libraries go in the environment's directory for musl's, where its
# loader looks (DECISIONS.md §60).
case "$TRIPLE" in
i686-*) LIBDIR=/usr/lib/i686-unknown-linux-musl class=1 machine=3 ;;
x86_64-*) LIBDIR=/usr/lib/x86_64-linux-musl class=2 machine=62 ;;
*) echo "build.sh: no library directory for $TRIPLE" >&2; exit 1 ;;
esac
OUT=/kairo/package
SRC=$PWD
BUILD=$SRC/build
# As many jobs as the guest has processors: the output doesn't depend on
# it (DECISIONS.md §52 leaves the machine out of a build key).
JOBS=$(nproc)
mkdir -p "$BUILD"

# The guest is root in its own namespace, where the tarballs' owners
# aren't mapped: the files are the guest's.
unpack() {
    tar -C "$BUILD" --no-same-owner -xf "$SRC/distfiles/$1"
}
VERSION=4.2.1

# make 4.2.1 predates C23, gcc 15's default, and calls functions it
# doesn't declare, which gcc 14 made an error that -fpermissive makes a
# warning again. It has no use for guile here.
unpack make-$VERSION.tar.gz
mkdir "$BUILD/make"
cd "$BUILD/make"
../make-$VERSION/configure --build="$TRIPLE" --host="$TRIPLE" --prefix=/usr \
    --libdir="$LIBDIR" --disable-nls --without-guile \
    CFLAGS="-O2 -std=gnu17 -fpermissive"
make -j"$JOBS"
make install DESTDIR="$OUT/make-$VERSION"

# What builds don't need: documentation, translations, and libtool's
# archives.
cd "$OUT"
rm -rf ./*/usr/share/info ./*/usr/share/man ./*/usr/share/doc ./*/usr/share/locale
find "$OUT" -name '*.la' -delete
find "$OUT" -depth -type d -empty -delete

# Each child is everything under its usr/ (musl's shared part its
# loader's search path in etc/ too), nothing in lib64, and every ELF in
# it the target's code.
for child in *; do
    case "$(ls -A "$child" | tr '\n' ' ')" in
    "usr " | "etc usr ") ;;
    *) echo "build.sh: $child holds not only usr/: $(ls -A "$child")" >&2; exit 1 ;;
    esac
    [ ! -e "$child/usr/lib64" ] || { echo "build.sh: $child/usr/lib64 exists" >&2; exit 1; }
done
find . -type f | while IFS= read -r file; do
    [ "$(od -An -c -N4 "$file" | tr -d ' ')" = 177ELF ] || continue
    [ "$(od -An -tu1 -j4 -N1 "$file" | tr -d ' ')" = $class ] || { echo "$file is the wrong class" >&2; exit 1; }
    [ "$(od -An -tu2 -j18 -N2 "$file" | tr -d ' ')" = $machine ] || { echo "$file is the wrong machine" >&2; exit 1; }
done

# The libraries each child's programs and libraries load, by soname, for
# its requires in kairo.toml (§56).
for child in *; do
    needed=$(find "$child" -type f -exec sh -c 'od -An -c -N4 "$1" | tr -d " " | grep -qx 177ELF' sh {} \; -print |
        { xargs -r readelf -d 2>/dev/null || true; } | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p' | LC_ALL=C sort -u | tr '\n' ' ')
    echo "build.sh: $child needs: ${needed:-nothing}" >&2
done

# It works, installed over what it was built with.
cp -a "$OUT/make-$VERSION/." /
[ "$(make --version | head -1)" = "GNU Make $VERSION" ] || { echo "build.sh: make is $(make --version | head -1)" >&2; exit 1; }
mkdir "$BUILD/check"
printf 'all:\n\t@echo made\n' >"$BUILD/check/Makefile"
[ "$(make -C "$BUILD/check" -s)" = made ] || { echo "build.sh: make made nothing" >&2; exit 1; }
