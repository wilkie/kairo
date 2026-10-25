# What the build ships (specs/DECISIONS.md §41 sub-decisions 2 and 6),
# run at the end by start.kaem under the bash and tools the build made:
# the installed /usr; the checked packages, as built; the state a later
# pass starts from; and each package extracted into a directory of its
# own, the root of its child object.
set -eu -o pipefail
PATH=/usr/bin
cp -a /usr /kairo/package/usr
cp -a /external/repo /kairo/package/repo
# What a later pass needs to go on from here that the packages don't
# hold (DECISIONS.md §49): the env and bootstrap.cfg the improve steps
# edited, and the script generator the seed built and checked.
mkdir /kairo/package/state
cp /steps/env /steps/bootstrap.cfg /script-generator /kairo/package/state/
for archive in /external/repo/*.tar.bz2; do
    package=$(basename "$archive" .tar.bz2)
    mkdir "/kairo/package/$package"
    bzip2 -dc "$archive" | tar -C "/kairo/package/$package" --no-same-owner -xf -
done
