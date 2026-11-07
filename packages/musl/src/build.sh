# Builds musl 1.2.5 into /kairo/package (packages/musl/README.md) for
# the target named as the first argument, a round of the base after the
# first, with the round before it (DECISIONS.md §64): its headers,
# static, and shared parts, each a child (§51).
. ./recipe.sh "$1"
NAME=musl
VERSION=1.2.5
case "$TRIPLE" in
i686-*) ARCH=i386 ;;
x86_64-*) ARCH=x86_64 ;;
esac
PACKAGE=$OUT/$NAME-$VERSION

# In the library directory, with its loader in /usr/lib, reached as /lib
# through the merged-/usr links, a relative link to its libc.so, and the
# file that has the loader search the library directory alone (§60). As
# live-bootstrap's, ldd is a link to the loader, and the startup files
# are linked in /usr/lib too: a gcc running from its build tree, as one
# bootstrapping does, looks for them only there and in /lib.
unpack "$NAME-$VERSION"
cd "$BUILD/$NAME-$VERSION"
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

finish

# It works, installed over what it was built with: a program links
# against it, static and dynamic, and runs.
install_over
printf '#include <stdio.h>\nint main(void) { puts("hello"); return 0; }\n' >hello.c
gcc -o hello-dynamic hello.c
gcc -static -o hello-static hello.c
for program in hello-dynamic hello-static; do
    [ "$(./$program)" = hello ] || { echo "build.sh: $program failed" >&2; exit 1; }
done
