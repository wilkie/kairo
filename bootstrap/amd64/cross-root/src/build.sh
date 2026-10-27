# Cross-builds a minimal x86_64 root into /kairo/package
# (bootstrap/amd64/cross-root/README.md), on live-bootstrap's x86 root with
# stage 1's x86_64-linux-musl toolchain: the kernel headers, musl,
# binutils, gcc, then the userland, each configured to run on x86_64 and
# installed under /usr.
set -euo pipefail
export PATH=/usr/bin
HOST=x86_64-linux-musl
BUILD_TRIPLE=i686-pc-linux-musl
OUT=/kairo/package
SRC=$PWD
BUILD=$SRC/build
# As many jobs as the guest has processors: the output doesn't depend on
# it (DECISIONS.md §52 leaves the machine out of a build key).
JOBS=$(nproc)
mkdir -p "$BUILD"

# The guest is root in its own namespace, where the tarballs' owners
# aren't mapped: the files are the guest's.
# A package's tarball is named by the package alone.
unpack() {
    local tarball
    tarball=$(cd "$SRC/distfiles" && echo "$1".tar.*)
    tar -C "$BUILD" --no-same-owner -xf "$SRC/distfiles/$tarball"
}

# A GNU package, configured to run on x86_64, built, and installed
# under /usr. The older ones predate C23, gcc 15's default, in what they
# build to run on x86_64 and in the helpers they build to run here, and
# some call functions they don't declare, which gcc 14 made an error
# that -fpermissive makes a warning again.
gnu() {
    local package=$1
    shift
    unpack "$package"
    mkdir "$BUILD/$package-build"
    cd "$BUILD/$package-build"
    "../$package/configure" --build="$BUILD_TRIPLE" --host="$HOST" \
        --prefix=/usr --disable-nls CFLAGS="-O2 -std=gnu17 -fpermissive" \
        CC_FOR_BUILD="gcc -std=gnu17 -fpermissive" "$@"
    make -j"$JOBS"
    make install DESTDIR="$OUT"
}

# 1. The kernel's headers for x86_64. Its host helpers predate C23.
unpack linux-4.14.336
cd "$BUILD/linux-4.14.336"
make ARCH=x86_64 HOSTCC="gcc -std=gnu11" INSTALL_HDR_PATH="$OUT/usr" headers_install

# 2. musl, with its loader in /usr/lib, reached as /lib through the
# merged-/usr links.
unpack musl-1.2.5
cd "$BUILD/musl-1.2.5"
./configure --prefix=/usr --syslibdir=/usr/lib --target="$HOST" \
    CROSS_COMPILE="$HOST-" --disable-wrapper
make -j"$JOBS"
make install DESTDIR="$OUT"

# 3. binutils. gprofng doesn't compile under C23, as in stage 1.
unpack binutils-2.41
mkdir "$BUILD/binutils"
cd "$BUILD/binutils"
../binutils-2.41/configure --build="$BUILD_TRIPLE" --host="$HOST" \
    --target="$HOST" --prefix=/usr --disable-nls --disable-werror \
    --disable-multilib --enable-deterministic-archives --disable-gprofng
make -j"$JOBS"
make install DESTDIR="$OUT"

# 4. gcc, with C and C++, and gmp, mpfr, and mpc built in its tree. Its
# libraries go in /usr/lib, beside musl's, not lib64.
unpack gcc-15.2.0
for library in gmp-6.2.1 mpfr-4.1.0 mpc-1.2.1; do
    unpack "$library"
    mv "$BUILD/$library" "$BUILD/gcc-15.2.0/${library%-*}"
done
sed -i '/m64=/s/lib64/lib/' "$BUILD/gcc-15.2.0/gcc/config/i386/t-linux64"
mkdir "$BUILD/gcc"
cd "$BUILD/gcc"
../gcc-15.2.0/configure --build="$BUILD_TRIPLE" --host="$HOST" \
    --target="$HOST" --prefix=/usr --enable-languages=c,c++ \
    --disable-nls --disable-multilib --disable-bootstrap \
    --disable-libsanitizer --disable-libssp --enable-shared \
    --enable-threads=posix
make -j"$JOBS"
make install DESTDIR="$OUT"
ln -s gcc "$OUT/usr/bin/cc"

# 5. The userland.
gnu bash-5.2.15 --without-bash-malloc bash_cv_strtold_broken=no
ln -s bash "$OUT/usr/bin/sh"
gnu coreutils-9.4 --enable-no-install-program=kill,uptime \
    gl_cv_macro_MB_CUR_MAX_good=y
gnu make-4.2.1 --without-guile
gnu sed-4.8
gnu grep-3.12
gnu gawk-5.3.0
gnu tar-1.34
gnu xz-5.6.4 --disable-doc
gnu gzip-1.13
gnu diffutils-3.10
gnu patch-2.8
gnu m4-1.4.19
gnu findutils-4.10.0

# bzip2 has a Makefile alone, whose install makes links by $PREFIX; its
# programs and library are copied in, the links made relative.
unpack bzip2-1.0.8
cd "$BUILD/bzip2-1.0.8"
make -j"$JOBS" CC="$HOST-gcc" AR="$HOST-ar" RANLIB="$HOST-ranlib" \
    CFLAGS="-O2 -D_FILE_OFFSET_BITS=64" bzip2 bzip2recover libbz2.a
install -m 755 bzip2 bzip2recover "$OUT/usr/bin/"
install -m 644 libbz2.a "$OUT/usr/lib/"
install -m 644 bzlib.h "$OUT/usr/include/"
ln -s bzip2 "$OUT/usr/bin/bunzip2"
ln -s bzip2 "$OUT/usr/bin/bzcat"

# What a root that only builds the next one doesn't need: documentation,
# translations, and libtool's archives.
rm -rf "$OUT/usr/share/info" "$OUT/usr/share/man" "$OUT/usr/share/doc" \
    "$OUT/usr/share/locale"
find "$OUT" -name '*.la' -delete

# Everything is under /usr, and nothing in lib64.
cd "$OUT"
[ "$(ls -A)" = usr ] || { echo "build.sh: not only usr/: $(ls -A)" >&2; exit 1; }
if [ -e usr/lib64 ]; then
    echo "build.sh: usr/lib64 exists:" >&2
    find usr/lib64 >&2
    exit 1
fi

# Every program is x86_64 code: an ELF64 (class 2) for machine 62.
for file in usr/bin/* usr/libexec/gcc/"$HOST"/15.2.0/cc1*; do
    [ -f "$file" ] && [ ! -L "$file" ] || continue
    [ "$(od -An -c -N4 "$file" | tr -d ' ')" = 177ELF ] || continue
    [ "$(od -An -tu1 -j4 -N1 "$file" | tr -d ' ')" = 2 ] || { echo "$file isn't ELF64" >&2; exit 1; }
    [ "$(od -An -tu2 -j18 -N2 "$file" | tr -d ' ')" = 62 ] || { echo "$file isn't x86_64" >&2; exit 1; }
done
