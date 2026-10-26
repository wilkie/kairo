# Sourced by package.sh (this pass's and every later pass's,
# specs/DECISIONS.md §51): lays out a musl package extracted in <dir> as
# the roots of its children, by what linking needs of it.
#
#   split_musl <dir>
#
# <dir>-headers: the headers, the startup files and their links, and
# the empty stub archives (libm.a and the rest), needed to link either
# way; <dir>-static: libc.a; <dir>-shared, when the package has one:
# libc.so, the loader link, ldd, and etc/ld-musl-*.path. Then <dir> is
# removed.
split_musl() {
    local dir=$1 path part
    (cd "$dir" && find . -type f -o -type l) | sort | while read -r path; do
        path=${path#./}
        case "$path" in
        etc/* | usr/bin/ldd | usr/lib/ld-musl-* | */libc.so) part=shared ;;
        */libc.a) part=static ;;
        *) part=headers ;;
        esac
        mkdir -p "$dir-$part/$(dirname "$path")"
        mv "$dir/$path" "$dir-$part/$path"
    done
    rm -r "$dir"
}
