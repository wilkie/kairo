# Pass 2's start (specs/DECISIONS.md §49), run by the bash pass 1 built.
# Pass 1's whole root is installed at /: its /usr, its packages in
# /repo, and in /state what its steps left outside the packages. This
# puts the root back as pass 1's steps left it, has upstream's script
# generator write the scripts for this pass's manifest (upstream's,
# truncated later than pass 1's), and runs the generated script from
# the step after pass 1's last; then package.sh.
set -euo pipefail
PATH=/usr/bin

# Where pass 1 stopped: the line its last step is in the generated script.
PREVIOUS_LAST='build zlib-1.3.1 pass1.sh'

# The packages, where the steps keep them; those pass 1 made, so
# package.sh ships this pass's own as children.
ln -s /repo /external/repo
ls /repo >/external/previous-packages
# The env and bootstrap.cfg as pass 1's improve steps left them.
cp /state/env /state/bootstrap.cfg /steps/
# The rest of the root outside the packages, as pass 1's improve steps
# made it: merged_usr's links from / (its /usr/sbin link is in /usr
# already, and it makes that one without `|| true`), and
# populate_device_nodes and finalize_fhs, which check before they make.
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
# upstream's; the generator rewrites bootstrap.cfg, so pass 1's goes
# back after it.
cp /kairo/src/steps/manifest /steps/manifest
/state/script-generator /steps/manifest
cp /state/bootstrap.cfg /steps/bootstrap.cfg
mapfile -t scripts < <(grep -lFx "$PREVIOUS_LAST" /steps/*.sh)
if [ "${#scripts[@]}" -ne 1 ]; then
    echo "start.sh: '$PREVIOUS_LAST' is in ${#scripts[@]} generated scripts, not 1" >&2
    exit 1
fi
# Its header (through sourcing helpers.sh), then what follows pass 1's
# last step.
awk -v last="$PREVIOUS_LAST" '
    !begun { print }
    $0 == ". ./helpers.sh" { begun = 1 }
    resumed { print }
    begun && $0 == last { resumed = 1 }
' "${scripts[0]}" >/steps/continue.sh
bash /steps/continue.sh

bash /kairo/src/package.sh
