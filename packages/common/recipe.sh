# Sourced by a package's build.sh, beside which its import.sh puts it
# (packages/common/README.md): what every package's recipe shares.
#
#   . ./recipe.sh "$1"     the target's triple
#
# Sets TRIPLE; LIBDIR, the environment's directory for musl's libraries
# (DECISIONS.md §60); KERNEL_ARCH, as the kernel names the target; the
# ELF class and machine its code has; OUT, SRC, BUILD, and JOBS. The
# x86 guest runs on a 64-bit kernel, whose machine config.guess would
# name: the triple is given, for x86-64 too.
set -euo pipefail
export PATH=/usr/bin
TRIPLE=$1
case "$TRIPLE" in
i686-*) LIBDIR=/usr/lib/i686-unknown-linux-musl KERNEL_ARCH=i386 class=1 machine=3 ;;
x86_64-*) LIBDIR=/usr/lib/x86_64-linux-musl KERNEL_ARCH=x86_64 class=2 machine=62 ;;
*) echo "recipe.sh: no library directory for $TRIPLE" >&2; exit 1 ;;
esac
OUT=/kairo/package
SRC=$PWD
BUILD=$SRC/build
# As many jobs as the guest has processors: the output doesn't depend on
# it (DECISIONS.md §52 leaves the machine out of a build key).
JOBS=$(nproc)
mkdir -p "$BUILD"
# configure scripts that refuse to run as root otherwise: the guest is
# root only in its own namespace.
export FORCE_UNSAFE_CONFIGURE=1

# The guest is root in its own namespace, where the tarballs' owners
# aren't mapped: the files are the guest's. A tarball is named by the
# package and version alone.
unpack() {
    local tarball
    tarball=$(cd "$SRC/distfiles" && echo "$1".tar.*)
    tar -C "$BUILD" --no-same-owner -xf "$SRC/distfiles/$tarball"
}

# A GNU package, configured for the target, built, and installed into
# the child directory named first, under /usr, its libraries in the
# library directory. Older packages predate C23, gcc 15's default, and
# some call functions they don't declare, which gcc 14 made an error
# that -fpermissive makes a warning again.
#
#   gnu <child> <package-version> [configure arguments...]
gnu() {
    local child=$1 package=$2
    shift 2
    unpack "$package"
    mkdir "$BUILD/$package-build"
    cd "$BUILD/$package-build"
    "../$package/configure" --build="$TRIPLE" --host="$TRIPLE" --prefix=/usr \
        --libdir="$LIBDIR" --disable-nls CFLAGS="-O2 -std=gnu17 -fpermissive" "$@"
    make -j"$JOBS" MAKEINFO=true
    make install DESTDIR="$OUT/$child" MAKEINFO=true
}

# What builds don't need is removed: documentation, translations, and
# libtool's archives. Then every child is checked: everything under its
# usr/ (musl's shared part its loader's search path in etc/ too),
# nothing in lib64, and every ELF in it the target's code. And what each
# child's programs and libraries load is reported, by soname, for its
# requires in kairo.toml (DECISIONS.md §56).
finish() {
    cd "$OUT"
    rm -rf ./*/usr/share/info ./*/usr/share/man ./*/usr/share/doc ./*/usr/share/locale
    find "$OUT" -name '*.la' -delete
    find "$OUT" -depth -type d -empty -delete
    local child file needed
    for child in *; do
        case "$(ls -A "$child" | tr '\n' ' ')" in
        "usr " | "etc usr ") ;;
        *) echo "recipe.sh: $child holds not only usr/: $(ls -A "$child")" >&2; exit 1 ;;
        esac
        [ ! -e "$child/usr/lib64" ] || { echo "recipe.sh: $child/usr/lib64 exists" >&2; exit 1; }
    done
    find . -type f | while IFS= read -r file; do
        [ "$(od -An -c -N4 "$file" | tr -d ' ')" = 177ELF ] || continue
        [ "$(od -An -tu1 -j4 -N1 "$file" | tr -d ' ')" = $class ] || { echo "$file is the wrong class" >&2; exit 1; }
        [ "$(od -An -tu2 -j18 -N2 "$file" | tr -d ' ')" = $machine ] || { echo "$file is the wrong machine" >&2; exit 1; }
    done
    for child in *; do
        needed=$(find "$child" -type f -exec sh -c 'od -An -c -N4 "$1" | tr -d " " | grep -qx 177ELF' sh {} \; -print |
            { xargs -r readelf -d 2>/dev/null || true; } | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p' | LC_ALL=C sort -u | tr '\n' ' ')
        echo "build.sh: $child needs: ${needed:-nothing}" >&2
    done
}

# Each child installed over what it was built with, to check it works as
# a consumer would have it.
install_over() {
    local child
    for child in "$OUT"/*; do
        cp -a --remove-destination "$child/." /
    done
    cd "$BUILD"
}
