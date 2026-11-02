# What every live-bootstrap pass after the first ships
# (specs/DECISIONS.md §49), as pass 1's package.sh does: the installed
# /usr and /etc (ca-certificates installs into /etc/ssl) and every
# package so far, so a later pass can go on from here; the state that
# pass needs; and each package this pass made extracted into a
# directory of its own, the root of its child object, musl's and gcc's
# split into parts (§51, §58). Earlier passes' packages stay their
# children. It reports what each child's programs and libraries load,
# for the children's requires (§56).
set -eu -o pipefail
PATH=/usr/bin
cp -a /usr /kairo/package/usr
cp -a /etc /kairo/package/etc
cp -a /repo /kairo/package/repo
mkdir /kairo/package/state
cp /steps/env /steps/bootstrap.cfg /state/script-generator /kairo/package/state/
for archive in /repo/*.tar.bz2; do
    if grep -qFx "$(basename "$archive")" /external/previous-packages; then
        continue
    fi
    package=$(basename "$archive" .tar.bz2)
    mkdir "/kairo/package/$package"
    bzip2 -dc "$archive" | tar -C "/kairo/package/$package" --no-same-owner -xf -
done
# Each musl package as its headers, static, and shared parts (§51).
. /kairo/src/split-musl.sh
for dir in /kairo/package/musl-*; do
    [ -d "$dir" ] || continue
    split_musl "$dir"
done
# Each gcc package's shared runtime libraries as children of their own
# (§58).
. /kairo/src/split-gcc.sh
for dir in /kairo/package/gcc-*; do
    [ -d "$dir" ] || continue
    split_gcc "$dir"
done
# The libraries each child's programs and libraries load, and those it
# holds, by soname (§56): children.py turns what it loads and doesn't
# hold into its requires.
cd /kairo/package
for child in *; do
    case "$child" in usr | etc | repo | state) continue ;; esac
    needed=$(find "$child" -type f -exec sh -c 'od -An -c -N4 "$1" | tr -d " " | grep -qx 177ELF' sh {} \; -print |
        { xargs -r readelf -d 2>/dev/null || true; } | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p' | LC_ALL=C sort -u | tr '\n' ' ')
    echo "package.sh: $child needs: ${needed:-nothing}" >&2
    sonames=$(find "$child" -type f -name 'lib*.so*' -exec sh -c 'od -An -c -N4 "$1" | tr -d " " | grep -qx 177ELF' sh {} \; -print |
        { xargs -r readelf -d 2>/dev/null || true; } | sed -n 's/.*(SONAME).*\[\(.*\)\]/\1/p' | LC_ALL=C sort -u | tr '\n' ' ')
    echo "package.sh: $child holds: ${sonames:-nothing}" >&2
done
