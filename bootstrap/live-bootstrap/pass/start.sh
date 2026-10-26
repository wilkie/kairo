# The start of every live-bootstrap pass after the first
# (specs/DECISIONS.md §49), run by the bash the previous pass built:
#
#   start.sh <the previous pass's last line in the generated script>
#
# The previous pass's whole root is installed at /: its /usr and /etc,
# its packages in /repo, and in /state what its steps left outside the
# packages. This puts the root back as the previous pass's steps left
# it, has upstream's script generator write the scripts for this pass's
# manifest (upstream's, truncated later than the previous pass's), and
# runs the generated script from the step after the previous pass's
# last; then package.sh. Each pass's import.sh puts this file and
# package.sh in its wrapper.
set -euo pipefail
PATH=/usr/bin

# Where the previous pass stopped, e.g. `build zlib-1.3.1 pass1.sh`.
PREVIOUS_LAST=$1

# The packages, where the steps keep them; those made before, so
# package.sh ships this pass's own as children.
ln -s /repo /external/repo
ls /repo >/external/previous-packages
# The env and bootstrap.cfg as the improve steps left them.
cp /state/env /state/bootstrap.cfg /steps/
# The rest of the root outside the packages, as the improve steps made
# it: merged_usr's links from / (its /usr/sbin link is in /usr already,
# and it makes that one without `|| true`), and populate_device_nodes
# and finalize_fhs, which check before they make.
for d in bin lib sbin; do
    ln -s "usr/${d}" "/${d}"
done
(
    # Upstream's scripts run under `set -e` alone.
    set +u +o pipefail
    cd /steps
    . ./bootstrap.cfg
    . ./env
    . ./helpers.sh
    . ./improve/populate_device_nodes.sh
    . ./improve/finalize_fhs.sh
)

# Upstream's scripts for this pass's manifest, which goes over
# upstream's; the generator rewrites bootstrap.cfg, so the previous
# pass's goes back after it.
cp /kairo/src/steps/manifest /steps/manifest
/state/script-generator /steps/manifest
cp /state/bootstrap.cfg /steps/bootstrap.cfg
mapfile -t scripts < <(grep -lFx "$PREVIOUS_LAST" /steps/*.sh)
if [ "${#scripts[@]}" -ne 1 ]; then
    echo "start.sh: '$PREVIOUS_LAST' is in ${#scripts[@]} generated scripts, not 1" >&2
    exit 1
fi
# Its header (through sourcing helpers.sh), then what follows the
# previous pass's last step.
awk -v last="$PREVIOUS_LAST" '
    !begun { print }
    $0 == ". ./helpers.sh" { begun = 1 }
    resumed { print }
    begun && $0 == last { resumed = 1 }
' "${scripts[0]}" >/steps/continue.sh
# With upstream's environment, as rootfs.py runs it (`env -i`, PATH alone),
# not the guest's: musl takes an unset LANG as C.UTF-8, and an explicit
# LANG=C as ASCII, which python's codec generation, for one, can't write.
env -i PATH=/usr/bin bash /steps/continue.sh

bash /kairo/src/package.sh
