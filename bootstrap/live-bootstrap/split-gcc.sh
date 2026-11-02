# Sourced by every later pass's package.sh (specs/DECISIONS.md §56,
# §58): lays out a gcc package extracted in <dir> so that each of its
# shared runtime libraries is a child of its own.
#
#   split_gcc <dir>
#
# For each runtime library, the shared file and its soname's link move
# to <dir>-lib<name>, laid out as in <dir>: libstdc++, libgcc_s,
# libgomp, libatomic, libitm, and libquadmath. What links against them
# stays: the unversioned names (libstdc++.so, and libgcc_s.so, a linker
# script), the static archives, the headers, and the debugger's scripts.
# A gcc built without shared libraries, as live-bootstrap's 4.x are, has
# none to move, and is left as it is. One that has some must have all,
# or the children named for it would be missing.
split_gcc() {
    local dir=$1 library path found
    found=$(cd "$dir" && find . -type f -name 'libstdc++.so.[0-9]*' ! -name '*-gdb.py')
    [ -n "$found" ] || return 0
    for library in stdc++ gcc_s gomp atomic itm quadmath; do
        found=no
        while IFS= read -r path; do
            path=${path#./}
            mkdir -p "$dir-lib$library/$(dirname "$path")"
            mv "$dir/$path" "$dir-lib$library/$path"
            found=yes
        done < <(cd "$dir" && find . \( -type f -o -type l \) -name "lib$library.so.[0-9]*" ! -name '*-gdb.py' | LC_ALL=C sort)
        if [ "$found" = no ]; then
            echo "split-gcc.sh: $dir has no shared lib$library" >&2
            return 1
        fi
    done
}
