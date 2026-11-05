# Builds musl 1.2.5 into /kairo/package (packages/musl/README.md) for the
# target named as the first argument, its first generation, with the
# bootstraps' packages (DECISIONS.md §59): its headers, static, and
# shared parts, each a child (§51).
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
VERSION=1.2.5
case "$TRIPLE" in
i686-*) ARCH=i386 ;;
x86_64-*) ARCH=x86_64 ;;
esac
PACKAGE=$OUT/musl-$VERSION

# In the library directory, with its loader in /usr/lib, reached as /lib
# through the merged-/usr links, a relative link to its libc.so, and the
# file that has the loader search the library directory alone (§60). As
# live-bootstrap's, ldd is a link to the loader, and the startup files
# are linked in /usr/lib too: a gcc running from its build tree, as one
# bootstrapping does, looks for them only there and in /lib.
unpack musl-$VERSION.tar.gz
cd "$BUILD/musl-$VERSION"
./configure --prefix=/usr --libdir="$LIBDIR" --syslibdir=/usr/lib --disable-wrapper
make -j"$JOBS"
make install DESTDIR="$PACKAGE"
ln -sf "${LIBDIR#/usr/lib/}/libc.so" "$PACKAGE/usr/lib/ld-musl-$ARCH.so.1"
mkdir -p "$PACKAGE/usr/bin" "$PACKAGE/etc"
ln -sf ../lib/ld-musl-$ARCH.so.1 "$PACKAGE/usr/bin/ldd"
printf '%s\n' "$LIBDIR" >"$PACKAGE/etc/ld-musl-$ARCH.path"
for file in crt1.o crti.o crtn.o Scrt1.o rcrt1.o; do
    ln -sf "${LIBDIR#/usr/lib/}/$file" "$PACKAGE/usr/lib/$file"
done

# Its headers, static, and shared parts, as live-bootstrap's are (§51):
# the headers, the startup files, and the stub archives, needed to link
# either way; libc.a; and libc.so, the loader's link, ldd, and the
# loader's search path.
cd "$PACKAGE"
find . ! -type d | sed 's|^\./||' | LC_ALL=C sort | while IFS= read -r path; do
    case "$path" in
    etc/* | usr/bin/ldd | usr/lib/ld-musl-* | */libc.so) part=shared ;;
    */libc.a) part=static ;;
    *) part=headers ;;
    esac
    mkdir -p "$PACKAGE-$part/$(dirname "$path")"
    mv "$path" "$PACKAGE-$part/$path"
done
cd "$OUT"
rm -r "$PACKAGE"

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

# It works, installed over what it was built with: a program links
# against it, static and dynamic, and runs.
for part in headers static shared; do
    cp -a --remove-destination "$PACKAGE-$part/." /
done
cd "$BUILD"
printf '#include <stdio.h>\nint main(void) { puts("hello"); return 0; }\n' >hello.c
gcc -o hello-dynamic hello.c
gcc -static -o hello-static hello.c
for program in hello-dynamic hello-static; do
    [ "$(./$program)" = hello ] || { echo "build.sh: $program failed" >&2; exit 1; }
done
