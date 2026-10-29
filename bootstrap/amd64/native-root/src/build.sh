# Builds a minimal x86_64 root natively into /kairo/package
# (bootstrap/amd64/native-root/README.md), on the root before it: the
# kernel headers, binutils, gcc bootstrapping itself, musl, the
# userland, zlib, libffi, perl, and python. Each package is installed
# into the artifact and into /, so each after it is built with it: all
# but the first few are built by this build's own gcc.
#
# With --fixed-point, the root it builds on is this build's own, as
# another build made it, and it fails unless it makes that root again,
# file for file.
set -euo pipefail
export PATH=/usr/bin
# configure scripts that refuse to run as root otherwise: the guest is
# root only in its own namespace.
export FORCE_UNSAFE_CONFIGURE=1
# The root has no texinfo, and its documentation isn't shipped: a
# manual a package thinks out of date isn't made. binutils' and gcc's
# top-level configure put their own in place of a missing makeinfo, so
# they are given it on make's command line too.
export MAKEINFO=true
OUT=/kairo/package
SRC=$PWD
BUILD=$SRC/build
STAGE=$SRC/stage
# As many jobs as the guest has processors: the output doesn't depend on
# it (DECISIONS.md §52 leaves the machine out of a build key).
JOBS=$(nproc)
mkdir -p "$BUILD"

# Every file under a directory's usr/: a link by its target, anything
# else by whether it is executable, all an artifact keeps of its mode,
# and its contents.
listing() {
    cd "$1"
    find usr ! -type d | LC_ALL=C sort | while IFS= read -r file; do
        if [ -L "$file" ]; then
            echo "$file -> $(readlink "$file")"
        elif [ -x "$file" ]; then
            echo "$file x $(sha256sum < "$file" | cut -d' ' -f1)"
        else
            echo "$file - $(sha256sum < "$file" | cut -d' ' -f1)"
        fi
    done
}

FIXED_POINT=no
if [ "${1:-}" = --fixed-point ]; then
    FIXED_POINT=yes
    # The root this build runs on, before it installs over it: listed,
    # and kept, to show how a file that differs differs.
    listing / > "$SRC/expected"
    cp -a /usr "$SRC/before"
    cd "$SRC"
fi

# The guest is root in its own namespace, where the tarballs' owners
# aren't mapped: the files are the guest's. A package's tarball is
# named by the package alone.
unpack() {
    local tarball
    tarball=$(cd "$SRC/distfiles" && echo "$1".tar.*)
    tar -C "$BUILD" --no-same-owner -xf "$SRC/distfiles/$tarball"
}

# Copies what a package installed into $STAGE into the artifact and
# into /. A file at / may be running, as bash or the loader are, so it
# is replaced, never written over.
ship() {
    cp -a "$STAGE/." "$OUT/"
    cp -a --remove-destination "$STAGE/." /
    rm -rf "$STAGE"
}

# A GNU package, configured, built, and installed under /usr. The older
# ones predate C23, gcc 15's default, and some call functions they
# don't declare, which gcc 14 made an error that -fpermissive makes a
# warning again.
gnu() {
    local package=$1
    shift
    unpack "$package"
    mkdir "$BUILD/$package-build"
    cd "$BUILD/$package-build"
    "../$package/configure" --prefix=/usr --disable-nls \
        CFLAGS="-O2 -std=gnu17 -fpermissive" "$@"
    make -j"$JOBS"
    make install DESTDIR="$STAGE"
    ship
}

# 1. The kernel's headers. Its host helpers predate C23.
unpack linux-4.14.336
cd "$BUILD/linux-4.14.336"
make HOSTCC="gcc -std=gnu11" INSTALL_HDR_PATH="$STAGE/usr" headers_install
ship

# 2. binutils. gprofng doesn't compile under C23, as in stage 1.
unpack binutils-2.41
mkdir "$BUILD/binutils"
cd "$BUILD/binutils"
../binutils-2.41/configure --prefix=/usr --disable-nls --disable-werror \
    --disable-multilib --enable-deterministic-archives --disable-gprofng
make -j"$JOBS" MAKEINFO=true
make install DESTDIR="$STAGE" MAKEINFO=true
ship

# 3. gcc, with C and C++, and gmp, mpfr, and mpc built in its tree. It
# bootstraps: built by the root's gcc, then by itself, then by itself
# again, the last two compared. Its libraries go in /usr/lib, beside
# musl's, not lib64.
unpack gcc-15.2.0
for library in gmp-6.2.1 mpfr-4.1.0 mpc-1.2.1; do
    unpack "$library"
    mv "$BUILD/$library" "$BUILD/gcc-15.2.0/${library%-*}"
done
sed -i '/m64=/s/lib64/lib/' "$BUILD/gcc-15.2.0/gcc/config/i386/t-linux64"
mkdir "$BUILD/gcc"
cd "$BUILD/gcc"
../gcc-15.2.0/configure --prefix=/usr --enable-languages=c,c++ \
    --disable-nls --disable-multilib --enable-bootstrap \
    --disable-libsanitizer --disable-libssp --enable-shared \
    --enable-threads=posix
make -j"$JOBS" MAKEINFO=true
make install DESTDIR="$STAGE" MAKEINFO=true
ln -sf gcc "$STAGE/usr/bin/cc"
ship

# 4. musl, with its loader in /usr/lib, reached as /lib through the
# merged-/usr links. It comes after gcc, as what links libc.so with
# libgcc, so that libc.so is this build's gcc's work, whatever built
# the root.
unpack musl-1.2.5
cd "$BUILD/musl-1.2.5"
./configure --prefix=/usr --syslibdir=/usr/lib --disable-wrapper
make -j"$JOBS"
make install DESTDIR="$STAGE"
ship

# 5. The userland, built by that gcc.
gnu bash-5.2.15 --without-bash-malloc
mkdir -p "$STAGE/usr/bin"
ln -sf bash "$STAGE/usr/bin/sh"
ship
gnu coreutils-9.4 --enable-no-install-program=kill,uptime
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

# bzip2 has Makefiles alone, whose install makes links by $PREFIX; its
# programs and libraries, static and shared, are copied in, the links
# made relative.
unpack bzip2-1.0.8
cd "$BUILD/bzip2-1.0.8"
make -j"$JOBS" -f Makefile-libbz2_so CFLAGS="-O2 -fPIC -D_FILE_OFFSET_BITS=64"
make clean
make -j"$JOBS" CFLAGS="-O2 -D_FILE_OFFSET_BITS=64" bzip2 bzip2recover libbz2.a
mkdir -p "$STAGE/usr/bin" "$STAGE/usr/lib" "$STAGE/usr/include"
install -m 755 bzip2 bzip2recover "$STAGE/usr/bin/"
install -m 644 libbz2.a "$STAGE/usr/lib/"
install -m 755 libbz2.so.1.0.8 "$STAGE/usr/lib/"
ln -s libbz2.so.1.0.8 "$STAGE/usr/lib/libbz2.so.1.0"
ln -s libbz2.so.1.0.8 "$STAGE/usr/lib/libbz2.so"
install -m 644 bzlib.h "$STAGE/usr/include/"
ln -s bzip2 "$STAGE/usr/bin/bunzip2"
ln -s bzip2 "$STAGE/usr/bin/bzcat"
ship

# 6. The libraries python builds against.
unpack zlib-1.3.1
cd "$BUILD/zlib-1.3.1"
./configure --prefix=/usr
make -j"$JOBS"
make install DESTDIR="$STAGE"
ship
gnu libffi-3.5.2 --disable-docs

# 7. perl. Configure records the machine's kernel version and the time
# it ran, and any perl already installed; all are fixed here, so the
# root doesn't depend on them. The time, in a setting and in config.sh's
# header, and the installed perl it sets whatever it is given, so they
# are changed in config.sh after, and the files made from config.sh
# made again (Configure -S).
# It finds functions, eaccess among them, that musl declares only for
# _GNU_SOURCE, as Alpine's build defines. With no man directories in
# the root, it would install its manuals at the artifact's top; they go
# where the rest of the documentation does, and are removed with it.
unpack perl-5.42.0
cd "$BUILD/perl-5.42.0"
sh Configure -des -Dprefix=/usr -Dvendorprefix=/usr -Duseshrplib \
    -Dcc=gcc -Accflags=-D_GNU_SOURCE \
    -Dman1dir=/usr/share/man/man1 -Dman3dir=/usr/share/man/man3 \
    -Dman1ext=1 -Dman3ext=3pm \
    -Dmyhostname=kairo -Dmydomain=.invalid -Dcf_by=kairo \
    -Dcf_email=root@kairo.invalid -Dperladmin=root@kairo.invalid \
    -Dosvers=4.14 -Dmyuname="linux kairo 4.14 x86_64"
sed -i -e "s/^cf_time=.*/cf_time='Tue Jan  1 00:00:00 UTC 1980'/" \
    -e "s/^# Configuration time: .*/# Configuration time: Tue Jan  1 00:00:00 UTC 1980/" \
    -e "s/^perl5=.*/perl5=''/" config.sh
sh Configure -S
make -j"$JOBS"
make install DESTDIR="$STAGE"
ship

# 8. python, without pip, which it would fetch. SOURCE_DATE_EPOCH, which
# Kairo sets, makes its bytecode checked by hash, not by time. Its
# Makefile records a python found installed, to regenerate files with;
# none is used, so it records the one it falls back to.
unpack Python-3.11.1
mkdir "$BUILD/python"
cd "$BUILD/python"
../Python-3.11.1/configure --prefix=/usr --enable-shared \
    --with-ensurepip=no --with-system-ffi PYTHON_FOR_REGEN=python3
make -j"$JOBS"
make install DESTDIR="$STAGE"
ship

# What a root that builds what comes next doesn't need: documentation,
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

# It works: C, static and dynamic, and C++ compile and run, and perl
# and python run, with the modules that need the libraries above.
cd "$BUILD"
printf '#include <stdio.h>\nint main(void) { puts("hello"); return 0; }\n' > hello.c
printf '#include <iostream>\nint main() { std::cout << "hello" << std::endl; }\n' > hello.cc
gcc -o hello-dynamic hello.c
gcc -static -o hello-static hello.c
g++ -o hello-cc hello.cc
for program in hello-dynamic hello-static hello-cc; do
    [ "$(./$program)" = hello ] || { echo "build.sh: $program failed" >&2; exit 1; }
done
perl -MPOSIX -e 'exit 0'
python3 -c 'import bz2, ctypes, lzma, zlib'

# The fixed point: what this build made is the root it ran on.
if [ "$FIXED_POINT" = yes ]; then
    listing "$OUT" > "$SRC/made"
    if ! diff "$SRC/expected" "$SRC/made" >&2; then
        # How each text file whose contents differ differs.
        diff "$SRC/expected" "$SRC/made" | sed -n 's/^> \(usr\/[^ ]*\) [x-] .*/\1/p' |
            while IFS= read -r file; do
                if [ -f "$SRC/before/${file#usr/}" ] && grep -Iq . "$OUT/$file"; then
                    diff -u "$SRC/before/${file#usr/}" "$OUT/$file" | head -40 >&2 || true
                fi
            done
        echo "build.sh: not a fixed point: the lines above differ" >&2
        exit 1
    fi
fi
