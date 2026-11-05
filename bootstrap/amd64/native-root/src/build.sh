# Builds a minimal x86_64 root natively into /kairo/package
# (bootstrap/amd64/native-root/README.md), on the root before it: the
# kernel headers, binutils, gcc bootstrapping itself, musl, the
# userland, zlib, libffi, perl, and python. Each package is installed
# into a directory of its own in the artifact, a child object's root,
# and into /, so each after it is built with it: all but the first few
# are built by this build's own gcc. gcc's runtime libraries are
# children apart from the compiler.
#
# With --fixed-point, the root it builds on is this build's children,
# as another build made them, and it fails unless it makes them again,
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
# Libraries go in the directory named by the target, so that another C
# library's can sit beside them (DECISIONS.md §60), and musl's loader
# searches it alone, as it does in the root this builds on.
LIBDIR=/usr/lib/x86_64-linux-musl

# Every file under a directory's usr/: a link by its target, anything
# else by whether it is executable and its contents. (An artifact keeps
# the whole mode, DECISIONS.md §54; stage 4's artifact being stage 3's
# compares that.)
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

# Copies what a package installed into $STAGE into the child directory
# named as the argument and into /, without its documentation,
# translations, or libtool's archives, which a root that builds what
# comes next doesn't need. A file at / may be running, as bash or the
# loader are, so it is replaced, never written over. No two children
# hold one path: a consumer's overlay would show only one (§41).
OWNERS=$SRC/owners
: > "$OWNERS"
ship() {
    local child=$1
    rm -rf "$STAGE/usr/share/info" "$STAGE/usr/share/man" \
        "$STAGE/usr/share/doc" "$STAGE/usr/share/locale"
    find "$STAGE" -name '*.la' -delete
    (cd "$STAGE" && find . ! -type d) | sed 's|^\./||' | LC_ALL=C sort > "$SRC/shipping"
    if ! awk -F '\t' -v child="$child" '
        FILENAME == ARGV[1] { owner[$2] = $1; next }
        ($0 in owner) && owner[$0] != child { print $0 " is " owner[$0] "\x27s too"; clash = 1 }
        END { exit clash }' "$OWNERS" "$SRC/shipping" >&2; then
        echo "build.sh: $child ships what another child does" >&2
        exit 1
    fi
    sed "s|^|$child\t|" "$SRC/shipping" >> "$OWNERS"
    mkdir -p "$OUT/$child"
    cp -a "$STAGE/." "$OUT/$child/"
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
    "../$package/configure" --prefix=/usr --libdir="$LIBDIR" --disable-nls \
        CFLAGS="-O2 -std=gnu17 -fpermissive" "$@"
    make -j"$JOBS"
    make install DESTDIR="$STAGE"
    ship "$package"
}

# 1. The kernel's headers. Its host helpers predate C23.
unpack linux-4.14.336
cd "$BUILD/linux-4.14.336"
make HOSTCC="gcc -std=gnu11" INSTALL_HDR_PATH="$STAGE/usr" headers_install
ship linux-headers-4.14.336

# 2. binutils. gprofng doesn't compile under C23, as in stage 1.
unpack binutils-2.41
mkdir "$BUILD/binutils"
cd "$BUILD/binutils"
../binutils-2.41/configure --prefix=/usr --libdir="$LIBDIR" --disable-nls --disable-werror \
    --disable-multilib --enable-deterministic-archives --disable-gprofng
make -j"$JOBS" MAKEINFO=true
make install DESTDIR="$STAGE" MAKEINFO=true
ship binutils-2.41

# 3. gcc, with C and C++, and gmp, mpfr, and mpc built in its tree. It
# bootstraps: built by the root's gcc, then by itself, then by itself
# again, the last two compared. Its runtime libraries and C++ headers go
# in its own versioned directory, where it links against them, so that
# another gcc can sit beside it (DECISIONS.md §55); their shared
# libraries are children of their own (§56). libcc1, gdb's plugin, would
# go in the library directory under no version, and isn't built. Its
# library directory is the target's (§60); the multilib directory for
# the OS, which it would add to every library path, is the directory
# itself, not lib64.
unpack gcc-15.2.0
for library in gmp-6.2.1 mpfr-4.1.0 mpc-1.2.1; do
    unpack "$library"
    mv "$BUILD/$library" "$BUILD/gcc-15.2.0/${library%-*}"
done
sed -i '/m64=/s/=\.\.\/lib64.*$/=./' "$BUILD/gcc-15.2.0/gcc/config/i386/t-linux64"
grep -q '^MULTILIB_OSDIRNAMES = m64=\.$' "$BUILD/gcc-15.2.0/gcc/config/i386/t-linux64"
mkdir "$BUILD/gcc"
cd "$BUILD/gcc"
../gcc-15.2.0/configure --prefix=/usr --libdir="$LIBDIR" --enable-languages=c,c++ \
    --disable-nls --disable-multilib --enable-bootstrap \
    --disable-libsanitizer --disable-libssp --enable-shared \
    --enable-threads=posix --enable-version-specific-runtime-libs \
    --disable-libcc1
# Once, a later stage's configure found nothing declared, and the
# stage didn't compile; the same build passed when run again. If it
# fails, what configure saw is shown, since the build's tree is gone
# afterwards.
make -j"$JOBS" MAKEINFO=true || {
    for check in 'whether strsignal is declared' 'for rlim_t' 'whether the C++ compiler works' 'whether the C compiler works'; do
        echo "build.sh: config.log at '$check':" >&2
        grep -n -m1 -A40 "checking $check" gcc/config.log >&2 || true
    done
    exit 1
}
make install DESTDIR="$STAGE" MAKEINFO=true
ln -sf gcc "$STAGE/usr/bin/cc"
GCCLIB=$STAGE$LIBDIR/gcc/x86_64-pc-linux-musl/15.2.0
# Each shared runtime library, its file and the soname's link, is
# copied out to the library directory, where the loader finds it, as a
# child of its own: a program links against the gcc that builds it, and
# runs with the newest of the library any gcc's child offers (§56).
RUNTIME=$SRC/runtime
for library in stdc++ gcc_s gomp atomic itm quadmath; do
    mkdir -p "$RUNTIME/lib$library-15.2.0$LIBDIR"
    cp -a "$GCCLIB"/lib$library.so.[0-9]* "$RUNTIME/lib$library-15.2.0$LIBDIR/"
    rm -f "$RUNTIME/lib$library-15.2.0$LIBDIR"/*-gdb.py
done
ship gcc-15.2.0
for library in stdc++ gcc_s gomp atomic itm quadmath; do
    mv "$RUNTIME/lib$library-15.2.0" "$STAGE"
    ship lib$library-15.2.0
done

# 4. musl, in the library directory, with its loader in /usr/lib,
# reached as /lib through the merged-/usr links, a relative link to its
# libc.so, and the file that has the loader search the library
# directory. It comes after gcc, as what links libc.so with libgcc, so
# that libc.so is this build's gcc's work, whatever built the root.
unpack musl-1.2.5
cd "$BUILD/musl-1.2.5"
./configure --prefix=/usr --libdir="$LIBDIR" --syslibdir=/usr/lib --disable-wrapper
make -j"$JOBS"
make install DESTDIR="$STAGE"
ln -sf "${LIBDIR#/usr/lib/}/libc.so" "$STAGE/usr/lib/ld-musl-x86_64.so.1"
# Its startup files are linked in /usr/lib too, as live-bootstrap's are:
# a gcc running from its build tree, as one bootstrapping does, looks
# for them only there and in /lib.
for file in crt1.o crti.o crtn.o Scrt1.o rcrt1.o; do
    ln -sf "${LIBDIR#/usr/lib/}/$file" "$STAGE/usr/lib/$file"
done
mkdir -p "$STAGE/etc"
printf '%s\n' "$LIBDIR" >"$STAGE/etc/ld-musl-x86_64.path"
ship musl-1.2.5

# 5. The userland, built by that gcc.
gnu bash-5.2.15 --without-bash-malloc
mkdir -p "$STAGE/usr/bin"
ln -sf bash "$STAGE/usr/bin/sh"
ship bash-5.2.15
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
mkdir -p "$STAGE/usr/bin" "$STAGE$LIBDIR" "$STAGE/usr/include"
install -m 755 bzip2 bzip2recover "$STAGE/usr/bin/"
install -m 644 libbz2.a "$STAGE$LIBDIR/"
install -m 755 libbz2.so.1.0.8 "$STAGE$LIBDIR/"
ln -s libbz2.so.1.0.8 "$STAGE$LIBDIR/libbz2.so.1.0"
ln -s libbz2.so.1.0.8 "$STAGE$LIBDIR/libbz2.so"
install -m 644 bzlib.h "$STAGE/usr/include/"
ln -s bzip2 "$STAGE/usr/bin/bunzip2"
ln -s bzip2 "$STAGE/usr/bin/bzcat"
ship bzip2-1.0.8

# 6. The libraries python builds against.
unpack zlib-1.3.1
cd "$BUILD/zlib-1.3.1"
./configure --prefix=/usr --libdir="$LIBDIR"
make -j"$JOBS"
make install DESTDIR="$STAGE"
ship zlib-1.3.1
gnu libffi-3.5.2 --disable-docs

# 7. perl. Configure records the machine's kernel version and the time
# it ran, and any perl already installed; all are fixed here, so the
# root doesn't depend on them. The time, in a setting and in config.sh's
# header, and the installed perl it sets whatever it is given, so they
# are changed in config.sh after, and the files made from config.sh
# made again (Configure -S).
# The libraries it looks for are in the library directory (§60).
# It finds functions, eaccess among them, that musl declares only for
# _GNU_SOURCE, as Alpine's build defines. With no man directories in
# the root, it would install its manuals at the artifact's top; they go
# where the rest of the documentation does, and are removed with it.
unpack perl-5.42.0
cd "$BUILD/perl-5.42.0"
sh Configure -des -Dprefix=/usr -Dvendorprefix=/usr -Duseshrplib \
    -Dcc=gcc -Accflags=-D_GNU_SOURCE -Dlibpth="$LIBDIR" \
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
ship perl-5.42.0

# 8. python, without pip, which it would fetch. SOURCE_DATE_EPOCH, which
# Kairo sets, makes its bytecode checked by hash, not by time. Its
# Makefile records a python found installed, to regenerate files with;
# none is used, so it records the one it falls back to.
# Its standard library and compiled modules stay in /usr/lib/python3.11,
# where it looks for them, whatever its libdir; its modules' build is
# told where the libraries they load are, and its own shared library,
# with pkg-config's files, moves to the library directory (§60), as
# Debian's multiarch python has it.
unpack Python-3.11.1
mkdir "$BUILD/python"
cd "$BUILD/python"
../Python-3.11.1/configure --prefix=/usr --enable-shared \
    --with-ensurepip=no --with-system-ffi PYTHON_FOR_REGEN=python3 \
    LDFLAGS="-L$LIBDIR"
make -j"$JOBS"
make install DESTDIR="$STAGE"
mkdir -p "$STAGE$LIBDIR"
mv "$STAGE"/usr/lib/libpython3*.so* "$STAGE/usr/lib/pkgconfig" "$STAGE$LIBDIR/"
ship python-3.11.1

# musl as its headers, static, and shared parts, as live-bootstrap's are
# (§51): the headers, the startup files, and the stub archives, needed
# to link either way; libc.a; and libc.so, the loader's link, and ldd.
cd "$OUT/musl-1.2.5"
find . ! -type d | sed 's|^\./||' | LC_ALL=C sort | while IFS= read -r path; do
    case "$path" in
    etc/* | usr/bin/ldd | usr/lib/ld-musl-* | */libc.so) part=shared ;;
    */libc.a) part=static ;;
    *) part=headers ;;
    esac
    mkdir -p "$OUT/musl-1.2.5-$part/$(dirname "$path")"
    mv "$path" "$OUT/musl-1.2.5-$part/$path"
done
cd "$OUT"
rm -r musl-1.2.5

# Each child is everything under its usr/, musl's shared part its
# loader's search path in etc/ too, and nothing in lib64.
for child in *; do
    expected=usr
    [ "$child" = musl-1.2.5-shared ] && expected=$'etc\nusr'
    [ "$(ls -A "$child")" = "$expected" ] || { echo "build.sh: $child holds not only usr/: $(ls -A "$child")" >&2; exit 1; }
    if [ -e "$child/usr/lib64" ]; then
        echo "build.sh: $child/usr/lib64 exists:" >&2
        find "$child/usr/lib64" >&2
        exit 1
    fi
done

# The libraries each child's programs and libraries load, by soname, for
# its requires in kairo.toml.
for child in *; do
    needed=$(find "$child" -type f -exec sh -c 'od -An -c -N4 "$1" | tr -d " " | grep -qx 177ELF' sh {} \; -print |
        { xargs -r readelf -d 2>/dev/null || true; } | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p' | LC_ALL=C sort -u | tr '\n' ' ')
    echo "build.sh: $child needs: ${needed:-nothing}" >&2
done

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

# The fixed point: what this build made is the root it ran on, its
# children together.
if [ "$FIXED_POINT" = yes ]; then
    mkdir "$SRC/made-root"
    for child in "$OUT"/*; do
        cp -a "$child/." "$SRC/made-root/"
    done
    listing "$SRC/made-root" > "$SRC/made"
    if ! diff "$SRC/expected" "$SRC/made" >&2; then
        # How each file whose contents differ differs: a text file line by
        # line, any other by the strings in it.
        diff "$SRC/expected" "$SRC/made" | sed -n 's/^> \(usr\/[^ ]*\) [x-] .*/\1/p' |
            while IFS= read -r file; do
                [ -f "$SRC/before/${file#usr/}" ] || continue
                if grep -Iq . "$SRC/made-root/$file"; then
                    diff -u "$SRC/before/${file#usr/}" "$SRC/made-root/$file" | head -40 >&2 || true
                else
                    diff -u <(strings -n 6 "$SRC/before/${file#usr/}") \
                        <(strings -n 6 "$SRC/made-root/$file") | head -60 >&2 || true
                fi
            done
        echo "build.sh: not a fixed point: the lines above differ" >&2
        exit 1
    fi
fi
