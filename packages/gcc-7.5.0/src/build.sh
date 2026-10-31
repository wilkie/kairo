# Builds gcc 7.5.0 into /kairo/package (packages/gcc-7.5.0/README.md),
# with C and C++, for the musl target named as the first argument, on
# the root it is given. It bootstraps: built by the root's gcc, then by
# itself twice, the last two compared, so what it is doesn't depend on
# the root's gcc. Every file but the programs in /usr/bin is under a
# directory named by its version, so it sits beside another gcc.
set -euo pipefail
export PATH=/usr/bin
# The root has no texinfo, and the manuals aren't shipped; gcc's
# top-level configure puts its own in place of a missing makeinfo, so it
# is given on make's command line.
export MAKEINFO=true
# The x86 guest runs on a 64-bit kernel, whose machine config.guess would
# name: the triple is given, for x86-64 too.
TRIPLE=$1
VERSION=7.5.0
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

# gcc, and gmp, mpfr, and mpc in its tree. Its libraries go in lib, as
# musl's loader looks there, not lib64.
unpack gcc-$VERSION.tar.xz
for library in gmp-6.2.1.tar.xz mpfr-4.1.0.tar.xz mpc-1.2.1.tar.gz; do
    unpack "$library"
    directory=${library%.tar.*}
    mv "$BUILD/$directory" "$BUILD/gcc-$VERSION/${directory%-*}"
done
sed -i '/m64=/s/lib64/lib/' "$BUILD/gcc-$VERSION/gcc/config/i386/t-linux64"
# musl defines NULL as nullptr for C++11 and later, which gcc 7, built
# by itself as C++14, can't initialize a bool with; gcc 8 initializes
# it with false.
sed -i 's/bool error_p = NULL;/bool error_p = false;/' "$BUILD/gcc-$VERSION/gcc/config/i386/i386.c"
grep -q 'bool error_p = false;' "$BUILD/gcc-$VERSION/gcc/config/i386/i386.c"

# The root's gcc 15 builds the first stage only. gcc 7's C predates
# C23, gcc 15's default, and calls functions it doesn't declare, which
# gcc 14 made an error that -fpermissive makes a warning again; its C++
# increments a bool, which C++17, g++ 15's default, removed.
#
# The runtime libraries, libstdc++ and libgcc_s among them, go in gcc's
# own versioned directory, with the C++ headers, so they never take the
# place of another gcc's. libcc1, gdb's plugin, would go in /usr/lib, and
# isn't built. The libraries that don't build against musl aren't:
# libsanitizer, libmpx, and libcilkrts; musl has its own stack
# protector, so libssp isn't either.
mkdir "$BUILD/gcc"
cd "$BUILD/gcc"
../gcc-$VERSION/configure --build="$TRIPLE" --host="$TRIPLE" --target="$TRIPLE" \
    --prefix=/usr --enable-languages=c,c++ --disable-nls --disable-multilib \
    --enable-bootstrap --enable-shared --enable-threads=posix \
    --enable-version-specific-runtime-libs --disable-libcc1 \
    --disable-libsanitizer --disable-libmpx --disable-libcilkrts \
    --disable-libssp \
    CC="gcc -std=gnu17 -fpermissive" CXX="g++ -std=gnu++98"
make -j"$JOBS" MAKEINFO=true
make install DESTDIR="$OUT" MAKEINFO=true
# libgcc puts libgcc_s in the versioned directory's multilib directory
# for the OS, ../lib, which no version names: it is moved into the
# versioned directory, which the driver searches when it links.
unversioned=$OUT/usr/lib/gcc/$TRIPLE/lib
if [ -d "$unversioned" ]; then
    mv "$unversioned"/* "$OUT/usr/lib/gcc/$TRIPLE/$VERSION/"
    rmdir "$unversioned"
fi

# What a compiler for building with doesn't need: documentation and
# libtool's archives.
rm -rf "$OUT/usr/share/info" "$OUT/usr/share/man"
find "$OUT" -name '*.la' -delete

# Every file is under /usr, and all but its programs under a directory
# named by its version.
cd "$OUT"
[ "$(ls -A)" = usr ] || { echo "build.sh: not only usr/: $(ls -A)" >&2; exit 1; }
stray=$(find usr ! -type d ! -path 'usr/bin/*' ! -path "*/$VERSION/*" ! -path "*/gcc-$VERSION/*")
if [ -n "$stray" ]; then
    echo "build.sh: outside gcc's versioned directories:" >&2
    echo "$stray" >&2
    exit 1
fi

# Every program is the target's code: an ELF of its class for its
# machine.
case "$TRIPLE" in
i686-*) class=1 machine=3 ;;
x86_64-*) class=2 machine=62 ;;
*) echo "build.sh: no ELF class for $TRIPLE" >&2; exit 1 ;;
esac
for file in usr/bin/* usr/libexec/gcc/"$TRIPLE"/$VERSION/cc1*; do
    [ -f "$file" ] && [ ! -L "$file" ] || continue
    [ "$(od -An -c -N4 "$file" | tr -d ' ')" = 177ELF ] || continue
    [ "$(od -An -tu1 -j4 -N1 "$file" | tr -d ' ')" = $class ] || { echo "$file is the wrong class" >&2; exit 1; }
    [ "$(od -An -tu2 -j18 -N2 "$file" | tr -d ' ')" = $machine ] || { echo "$file is the wrong machine" >&2; exit 1; }
done

# It works, installed over the root as a consumer would have it: C,
# static and dynamic, and C++, dynamic, run against the root's newer
# libstdc++, and static.
cp -a "$OUT/." /
cd "$BUILD"
[ "$(gcc -dumpfullversion)" = $VERSION ] || { echo "build.sh: gcc is $(gcc -dumpfullversion)" >&2; exit 1; }
printf '#include <stdio.h>\nint main(void) { puts("hello"); return 0; }\n' > hello.c
printf '#include <iostream>\nint main() { std::cout << "hello" << std::endl; }\n' > hello.cc
gcc -o hello-dynamic hello.c
gcc -static -o hello-static hello.c
g++ -o hello-cc hello.cc
g++ -static -o hello-cc-static hello.cc
for program in hello-dynamic hello-static hello-cc hello-cc-static; do
    [ "$(./$program)" = hello ] || { echo "build.sh: $program failed" >&2; exit 1; }
done
