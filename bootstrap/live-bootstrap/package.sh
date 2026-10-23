# What the build ships (specs/DECISIONS.md §41 sub-decisions 2 and 6),
# run at the end by start.kaem under the bash and tools the build made:
# the installed /usr; the checked packages, as built; and each package
# extracted into a directory of its own, the root of its child object.
set -eu -o pipefail
PATH=/usr/bin
cp -a /usr /kairo/package/usr
cp -a /external/repo /kairo/package/repo
for archive in /external/repo/*.tar.bz2; do
    package=$(basename "$archive" .tar.bz2)
    mkdir "/kairo/package/$package"
    bzip2 -dc "$archive" | tar -C "/kairo/package/$package" --no-same-owner -xf -
done
