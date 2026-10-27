# Builds the x86_64-linux-musl cross toolchain into /kairo/package
# (bootstrap/amd64/cross/README.md), on live-bootstrap's x86 root: binutils,
# the kernel headers, a C-only gcc to build musl with, musl, then the full
# gcc. Each part is installed into / too, for the parts after it to use.
set -euo pipefail
export PATH=/usr/bin
TARGET=x86_64-linux-musl
SYSROOT=/usr/$TARGET
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

# 1. binutils. gprofng, a profiler for native programs, doesn't compile
# under gcc 15's default C23, and a cross toolchain has no use for it.
unpack binutils-2.41.tar.xz
mkdir "$BUILD/binutils"
cd "$BUILD/binutils"
../binutils-2.41/configure --prefix=/usr --target="$TARGET" \
    --with-sysroot="$SYSROOT" --disable-nls --disable-werror \
    --disable-multilib --enable-deterministic-archives --disable-gprofng
make -j"$JOBS"
make install DESTDIR="$OUT"
make install

# 2. The kernel's headers for x86_64, into the sysroot. Its host
# helpers predate C23, gcc 15's default.
unpack linux-4.14.336.tar.xz
cd "$BUILD/linux-4.14.336"
make ARCH=x86_64 HOSTCC="gcc -std=gnu11" INSTALL_HDR_PATH="$OUT$SYSROOT" headers_install
make ARCH=x86_64 HOSTCC="gcc -std=gnu11" INSTALL_HDR_PATH="$SYSROOT" headers_install

# 3. A C-only gcc, with no libc yet, to build musl with; not shipped.
# Its libraries go in the sysroot's lib, beside musl's, not lib64: musl's
# loader doesn't search lib64, and what this gcc builds for stage 2's
# root installs where this gcc says.
unpack gcc-15.2.0.tar.xz
sed -i '/m64=/s/lib64/lib/' "$BUILD/gcc-15.2.0/gcc/config/i386/t-linux64"
gcc_configure() {
    ../gcc-15.2.0/configure --prefix=/usr --target="$TARGET" \
        --with-sysroot="$SYSROOT" --with-native-system-header-dir=/include \
        --disable-nls --disable-multilib --disable-libsanitizer \
        --disable-libssp "$@"
}
mkdir "$BUILD/gcc-1"
cd "$BUILD/gcc-1"
gcc_configure --enable-languages=c --disable-shared --disable-threads \
    --disable-libatomic --disable-libgomp --disable-libquadmath \
    --disable-libstdcxx --with-newlib --without-headers
make -j"$JOBS" all-gcc all-target-libgcc
make install-gcc install-target-libgcc

# 4. musl for x86_64, into the sysroot.
unpack musl-1.2.5.tar.gz
cd "$BUILD/musl-1.2.5"
./configure --prefix=/ --target="$TARGET" CC="$TARGET-gcc" --disable-wrapper
make -j"$JOBS"
make install DESTDIR="$SYSROOT"
make install DESTDIR="$OUT$SYSROOT"

# 5. The whole gcc, C and C++, against that musl.
mkdir "$BUILD/gcc-2"
cd "$BUILD/gcc-2"
gcc_configure --enable-languages=c,c++ --enable-shared --enable-threads=posix
make -j"$JOBS"
make install DESTDIR="$OUT"
make install

# What isn't the toolchain's own: documentation, translations, and the
# few files the native gcc and binutils install at the same paths.
rm -rf "$OUT/usr/share" "$OUT/usr/lib/bfd-plugins"
find "$OUT/usr/lib" -maxdepth 1 -name 'libcc1*' -delete
# Every path left is named by the target, so the toolchain installs
# beside the x86 root's own without touching it.
stray=$(cd "$OUT" && find . ! -type d | sed 's|^\./||' | grep -v \
    -e "^usr/$TARGET/" -e "^usr/bin/$TARGET-" -e "^usr/lib/gcc/$TARGET/" \
    -e "^usr/libexec/gcc/$TARGET/" || true)
if [ -n "$stray" ]; then
    echo "build.sh: paths not named by $TARGET:" >&2
    echo "$stray" >&2
    exit 1
fi

# It makes x86_64 code: an ELF64 (class 2) for machine 62, static and
# dynamic, C and C++.
check() {
    local file=$1
    [ "$(od -An -tu1 -j4 -N1 "$file" | tr -d ' ')" = 2 ] || { echo "$file isn't ELF64" >&2; exit 1; }
    [ "$(od -An -tu2 -j18 -N2 "$file" | tr -d ' ')" = 62 ] || { echo "$file isn't x86_64" >&2; exit 1; }
}
cd "$BUILD"
printf '#include <stdio.h>\nint main(void) { puts("hello"); return 0; }\n' > hello.c
printf '#include <iostream>\nint main() { std::cout << "hello\\n"; }\n' > hello.cc
"$TARGET-gcc" -static -o hello-static hello.c
"$TARGET-gcc" -o hello-dynamic hello.c
"$TARGET-g++" -o hello-cc hello.cc
for file in hello-static hello-dynamic hello-cc; do
    check "$file"
done
