#!/usr/bin/env bash
#
# Time `kairo build --plan-only` for each <object>:<target> given, planned
# in full and reused from its record (`specs/DECISIONS.md` §62), with
# a release build of kairo, against a store, the best of several runs:
# what planning costs, to watch for regressions as the archive grows
# (`specs/DECISIONS.md` §52, §59, §61). Prints each plan's time and how
# many requirements, runtime entries, and builds first it has, so a
# change in time can be told from a change in the plan.
#
# Usage:
#   scripts/plan-timing.sh <store> <actor id> <object>:<target>... [--runs N]
#
# KAIRO names the kairo binary (default: target/release/kairo, built
# here first if it isn't there).
set -euo pipefail
shopt -s inherit_errexit

[ $# -ge 3 ] || { echo "usage: plan-timing.sh <store> <actor id> <object>:<target>... [--runs N]" >&2; exit 2; }
STORE=$1
ACTOR=$2
shift 2
RUNS=3
PLANS=()
while [ $# -gt 0 ]; do
    case "$1" in
    --runs) RUNS=$2; shift 2 ;;
    *) PLANS+=("$1"); shift ;;
    esac
done
HERE=$(cd "$(dirname "$0")/.." && pwd)
KAIRO=${KAIRO:-$HERE/target/release/kairo}
if [ ! -x "$KAIRO" ]; then
    cargo build --release -p kairo-cli --manifest-path "$HERE/Cargo.toml" >&2
fi

# The best of $RUNS runs of a plan, with any extra flags given.
best_of() {
    local object=$1 target=$2 best= start end took
    shift 2
    for _ in $(seq "$RUNS"); do
        start=$(date +%s.%N)
        plan=$("$KAIRO" --store "$STORE" build "object:$object" --target "$target" \
            --actor "$ACTOR" --plan-only "$@")
        end=$(date +%s.%N)
        took=$(echo "$end - $start" | bc)
        if [ -z "$best" ] || [ "$(echo "$took < $best" | bc)" = 1 ]; then
            best=$took
        fi
    done
    echo "$best"
}

# Each plan twice: planned in full (`--replan`), and reused from what
# that recorded (DECISIONS.md §62).
for entry in "${PLANS[@]}"; do
    object=${entry%%:*}
    target=${entry#*:}
    planned=$(best_of "$object" "$target" --replan)
    reused=$(best_of "$object" "$target")
    plan=$("$KAIRO" --store "$STORE" build "object:$object" --target "$target" \
        --actor "$ACTOR" --plan-only)
    count() { sed -n "s/^$1 = //p" <<<"$plan" | head -n 1; }
    printf '%s %s: planned %.2f s, reused %.2f s (requirements %s, runtime %s, builds first %s)\n' \
        "$object" "$target" "$planned" "$reused" "$(count requirements)" \
        "$(count runtime)" "$(count builds_first)"
done
