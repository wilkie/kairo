# Builds gcc 15.2.0 into /kairo/package (packages/gcc/README.md), with C
# and C++, for the musl target named as the first argument. It is a
# round of the base's gcc 15.2.0 after the first, built with the round
# before it, an older generation of the same object (DECISIONS.md §59,
# §64), and it bootstraps: that gcc builds its first stage, which builds
# the second, which builds the third, the last two compared, so what it
# is doesn't depend on the gcc it was built with. The compiler is one
# child, every file of it but the programs in /usr/bin under a directory
# named by its version, so it sits beside another gcc; each shared
# runtime library is a child of its own, in the library directory (§56,
# §60).
. ./recipe.sh "$1"
VERSION=15.2.0
COMPILER=$OUT/gcc-$VERSION

# gcc, and gmp, mpfr, and mpc in its tree. On x86-64, the multilib
# directory for the OS, which it adds to every library path, is the
# library directory itself, not lib64 (§60).
unpack gcc-$VERSION
for library in gmp-6.2.1 mpfr-4.1.0 mpc-1.2.1; do
    unpack "$library"
    mv "$BUILD/$library" "$BUILD/gcc-$VERSION/${library%-*}"
done
sed -i '/m64=/s/=\.\.\/lib64.*$/=./' "$BUILD/gcc-$VERSION/gcc/config/i386/t-linux64"
# musl provides the stack protector, but its __stack_chk_fail_local,
# which i386 code built with -fstack-protector calls, is hidden in libc:
# each program needs its own. gcc's libssp_nonshared.a defines it, and
# where the C library provides the protector the link adds that archive
# alone, as Alpine's gcc does; an x86-64 program never pulls it in.
sed -i 's/"|fstack-protector-strong|fstack-protector-explicit:}"/"|fstack-protector-strong|fstack-protector-explicit:-lssp_nonshared}"/' \
    "$BUILD/gcc-$VERSION/gcc/gcc.cc"
grep -q 'explicit:-lssp_nonshared}' "$BUILD/gcc-$VERSION/gcc/gcc.cc"

# The runtime libraries, libstdc++ and libgcc_s among them, go in gcc's
# own versioned directory, with the C++ headers, so they never take the
# place of another gcc's. libcc1, gdb's plugin, would go in the library
# directory under no version, and isn't built. libsanitizer doesn't
# build against musl, so it isn't built. libssp is, for
# libssp_nonshared.a alone, which is kept: musl has the rest of the
# stack protector, so libssp's libraries and headers are removed.
# Multiarch, the Debian triple in its library paths and what
# -print-multiarch reports, is off: left to itself, gcc turns it on
# when the machine it is built on has /usr/lib/libc.so, and the x86
# bootstraps' roots do, so it would report glibc's i386-linux-gnu for a
# musl target and send Python's configure, CMake, and the rest looking
# for libraries where none are.
mkdir "$BUILD/gcc"
cd "$BUILD/gcc"
../gcc-$VERSION/configure --build="$TRIPLE" --host="$TRIPLE" --target="$TRIPLE" \
    --prefix=/usr --libdir="$LIBDIR" --enable-languages=c,c++ --disable-nls \
    --disable-multilib --enable-bootstrap --enable-shared --enable-threads=posix \
    --enable-version-specific-runtime-libs --disable-libcc1 \
    --disable-libsanitizer --disable-multiarch
make -j"$JOBS" MAKEINFO=true
make install DESTDIR="$COMPILER" MAKEINFO=true
# libgcc puts libgcc_s in the versioned directory's multilib directory
# for the OS, where that isn't the directory itself, which no version
# names: it is moved into the versioned directory.
GCCLIB=$COMPILER$LIBDIR/gcc/$TRIPLE/$VERSION
unversioned=$COMPILER$LIBDIR/gcc/$TRIPLE/lib
if [ -d "$unversioned" ]; then
    mv "$unversioned"/* "$GCCLIB/"
    rmdir "$unversioned"
fi
rm -rf "$GCCLIB"/libssp.* "$GCCLIB"/include/ssp
[ -f "$GCCLIB/libssp_nonshared.a" ] || { echo "build.sh: no libssp_nonshared.a" >&2; exit 1; }

# Each shared runtime library, its file and the soname's link, is
# copied out to the library directory, where the loader finds it, as a
# child of its own: a program links against the gcc that builds it, and
# runs with the newest of the library any gcc's child offers.
for library in stdc++ gcc_s gomp atomic itm quadmath; do
    mkdir -p "$OUT/lib$library-$VERSION$LIBDIR"
    cp -a "$GCCLIB"/lib$library.so.[0-9]* "$OUT/lib$library-$VERSION$LIBDIR/"
    rm -f "$OUT/lib$library-$VERSION$LIBDIR"/*-gdb.py
done

finish

# All of the compiler but its programs is under a directory named by its
# version.
stray=$(cd "$COMPILER" && find usr ! -type d ! -path 'usr/bin/*' ! -path "*/$VERSION/*" ! -path "*/gcc-$VERSION/*")
if [ -n "$stray" ]; then
    echo "build.sh: outside gcc's versioned directories:" >&2
    echo "$stray" >&2
    exit 1
fi

# It works, the compiler installed over what it was built with, as a
# consumer would have it: C, static and dynamic, and C++, static and
# dynamic, the dynamic C++ program run against the newest libstdc++ its
# environment has and against this gcc's own.
cp -a --remove-destination "$COMPILER/." /
cd "$BUILD"
[ "$(gcc -dumpfullversion)" = $VERSION ] || { echo "build.sh: gcc is $(gcc -dumpfullversion)" >&2; exit 1; }
[ -z "$(gcc -print-multiarch)" ] || { echo "build.sh: gcc reports multiarch $(gcc -print-multiarch)" >&2; exit 1; }
printf '#include <stdio.h>\nint main(void) { puts("hello"); return 0; }\n' >hello.c
printf '#include <iostream>\nint main() { std::cout << "hello" << std::endl; }\n' >hello.cc
gcc -o hello-dynamic hello.c
gcc -static -o hello-static hello.c
gcc -fPIE -pie -fstack-protector-all -o hello-protected hello.c
g++ -o hello-cc hello.cc
g++ -static -o hello-cc-static hello.cc
for program in hello-dynamic hello-static hello-protected hello-cc hello-cc-static; do
    [ "$(./$program)" = hello ] || { echo "build.sh: $program failed" >&2; exit 1; }
done
[ "$(LD_LIBRARY_PATH=$OUT/libstdc++-$VERSION$LIBDIR:$OUT/libgcc_s-$VERSION$LIBDIR ./hello-cc)" = hello ] ||
    { echo "build.sh: hello-cc failed with gcc $VERSION's libstdc++" >&2; exit 1; }
