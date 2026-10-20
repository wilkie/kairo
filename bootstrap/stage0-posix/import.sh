#!/usr/bin/env bash
# Import stage0-posix Release_1.9.1 for AMD64 as two Kairo objects
# (specs/DECISIONS.md §36): the seed, and the sources that build from it.
#
# Usage:
#   import.sh <stage0-posix checkout> <actor id> [kairo args...]
#
# The checkout must be the pinned release with its submodules:
#   git clone --branch Release_1.9.1 --recurse-submodules \
#     https://github.com/oriansj/stage0-posix.git
#
# Extra arguments go before the command in every `kairo` call, e.g.
# `--store <path>`. KAIRO names the kairo binary (default: kairo on
# PATH). Prints the two object ids, then the config lines that accept
# the seed and allowlist the engine.
set -euo pipefail
# Failures inside $(...) stop the import too.
shopt -s inherit_errexit

STAGE0_COMMIT=45d90f5955b6907dc6cdea9ebafce558359edcd3
SEEDS_COMMIT=cedec6b8066d1db229b6c77d42d120a23c6980ed

if [ $# -lt 2 ]; then
    echo "usage: $0 <stage0-posix checkout> <actor id> [kairo args...]" >&2
    exit 2
fi
CHECKOUT=$(cd "$1" && pwd)
ACTOR=$2
shift 2
KAIRO_ARGS=("$@")
KAIRO=${KAIRO:-kairo}
HERE=$(cd "$(dirname "$0")" && pwd)

kairo() { "$KAIRO" "${KAIRO_ARGS[@]}" "$@"; }
field() { sed -n "s/^$1 = //p" | head -n 1; }
fail() {
    echo "import.sh: $*" >&2
    exit 1
}

# The checkout is the pinned release, seeds included.
got=$(git -C "$CHECKOUT" rev-parse HEAD)
[ "$got" = "$STAGE0_COMMIT" ] || fail "checkout is at $got, expected Release_1.9.1 ($STAGE0_COMMIT)"
got=$(git -C "$CHECKOUT/bootstrap-seeds" rev-parse HEAD)
[ "$got" = "$SEEDS_COMMIT" ] || fail "bootstrap-seeds is at $got, expected $SEEDS_COMMIT"

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

# Fixed author and dates, so importing the same release again gives the
# same commits, and so the same revisions.
export GIT_AUTHOR_NAME=kairo-bootstrap GIT_AUTHOR_EMAIL=bootstrap@kairo.invalid
export GIT_COMMITTER_NAME=kairo-bootstrap GIT_COMMITTER_EMAIL=bootstrap@kairo.invalid
export GIT_AUTHOR_DATE=2026-01-01T00:00:00Z GIT_COMMITTER_DATE=2026-01-01T00:00:00Z

# copy_tree <from> <to>: the files of a checkout, without Git metadata
# (a submodule's .git is a file).
copy_tree() {
    mkdir -p "$2"
    cp -a "$1/." "$2/"
    find "$2" -name .git -prune -exec rm -rf {} +
}

# create <dir> <message>: commit <dir> and make an object whose head is
# that tree; prints the object id.
create() {
    # SHA-256, the default for new objects' history.
    git -C "$1" init -q --initial-branch=main --object-format=sha256
    git -C "$1" config commit.gpgsign false
    # Upstream's .gitignore files ignore the directories the chain
    # writes into; the tree must be the checkout's files exactly.
    git -C "$1" add -A --force
    git -C "$1" commit -q -m "$2"
    local oid object statement
    oid=$(git -C "$1" rev-parse HEAD)
    object=$(kairo object create --actor "$ACTOR" --kind software | field object)
    [ -n "$object" ] || fail "object create printed no id"
    kairo git fetch --object "$object" --remote "file://$1" --branch main \
        --object-format sha256 >/dev/null
    statement=$(kairo revision create --actor "$ACTOR" --object "$object" \
        --revision "git:sha256:$oid" --manifest "$1/kairo.toml" | field statement)
    [ -n "$statement" ] || fail "revision create printed no statement"
    kairo branch set --actor "$ACTOR" --object "$object" --revision "$statement" >/dev/null
    echo "$object"
}

# 1. The seed: bootstrap-seeds unchanged, plus its kairo.toml.
copy_tree "$CHECKOUT/bootstrap-seeds" "$WORK/seeds"
cp "$HERE/bootstrap-seeds.kairo.toml" "$WORK/seeds/kairo.toml"
SEED=$(create "$WORK/seeds" "bootstrap-seeds $SEEDS_COMMIT")
SEED_SNAPSHOT=$(kairo snapshot compute --object "$SEED" | field snapshot)
[ -n "$SEED_SNAPSHOT" ] || fail "snapshot compute printed no snapshot"

# 2. The sources: submodules flattened in, bootstrap-seeds/ a link to
#    the seed's canonical path in the contract, plus kairo.kaem.
copy_tree "$CHECKOUT" "$WORK/stage0"
rm -rf "$WORK/stage0/bootstrap-seeds"
ln -s "/kairo/content/$SEED_SNAPSHOT" "$WORK/stage0/bootstrap-seeds"
cp "$HERE/kairo.kaem" "$WORK/stage0/kairo.kaem"
cp "$HERE/stage0-posix.kairo.toml" "$WORK/stage0/kairo.toml"
SOURCES=$(create "$WORK/stage0" "stage0-posix Release_1.9.1 ($STAGE0_COMMIT), flattened")

cat <<OUT
seed = $SEED
seed_snapshot = $SEED_SNAPSHOT
sources = $SOURCES

# Add to <store>/config.toml after auditing the seed, then
#   kairo build object:$SOURCES --target amd64 --actor $ACTOR
[[runtime.seeds]]
object = "$SEED"

[[runtime.engines]]
kind = "bubblewrap"
path = "/usr/bin/bwrap"
OUT
