# What every live-bootstrap pass after the first ships
# (specs/DECISIONS.md §49), as pass 1's package.sh does: the installed
# /usr and /etc (ca-certificates installs into /etc/ssl) and every
# package so far, so a later pass can go on from here; the state that
# pass needs; and each package this pass made extracted into a
# directory of its own, the root of its child object. Earlier passes'
# packages stay their children.
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
