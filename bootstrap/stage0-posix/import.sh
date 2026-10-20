#!/usr/bin/env bash
# Archive stage0-posix and make the native object that builds it
# (specs/DECISIONS.md §36, §37).
#
# Usage:
#   import.sh <actor id> [kairo args...]
#
# 1. `kairo source fetch https://github.com/oriansj/stage0-posix`
#    archives stage0-posix as a source object, and with it, recursively,
#    every submodule its commits pin (the seeds, M2-Planet, M2libc,
#    mescc-tools, ...), each as its own source, under its real upstream
#    URL. This needs the network.
# 2. The native wrapper -- this directory's kairo.toml and kairo.kaem,
#    which pin every source the AMD64 chain uses at its Release_1.9.1
#    commit -- is committed as SHA-256 history and created as an object.
#
# Extra arguments go before the command in every `kairo` call, e.g.
# `--store <path>`. KAIRO names the kairo binary (default: kairo on
# PATH). Prints the wrapper's object id and the config lines that accept
# the seed and allowlist the engine.
set -euo pipefail
# Failures inside $(...) stop the import too.
shopt -s inherit_errexit

STAGE0_URL=https://github.com/oriansj/stage0-posix
SEEDS_URL=https://github.com/oriansj/bootstrap-seeds
SEEDS_REVISION=git:sha1:cedec6b8066d1db229b6c77d42d120a23c6980ed

if [ $# -lt 1 ]; then
    echo "usage: $0 <actor id> [kairo args...]" >&2
    exit 2
fi
ACTOR=$1
shift
KAIRO_ARGS=("$@")
KAIRO=${KAIRO:-kairo}
HERE=$(cd "$(dirname "$0")" && pwd)

kairo() { "$KAIRO" "${KAIRO_ARGS[@]}" "$@"; }
field() { sed -n "s/^$1 = //p" | head -n 1; }
fail() {
    echo "import.sh: $*" >&2
    exit 1
}

# 1. The sources, recursively. Incomplete submodule archiving fails here.
kairo source fetch "$STAGE0_URL" --actor "$ACTOR" --require-complete >&2

# 2. The native wrapper: SHA-256 history, born in the archive. Fixed
# author and dates, so importing again gives the same revision.
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
export GIT_AUTHOR_NAME=kairo-bootstrap GIT_AUTHOR_EMAIL=bootstrap@kairo.invalid
export GIT_COMMITTER_NAME=kairo-bootstrap GIT_COMMITTER_EMAIL=bootstrap@kairo.invalid
export GIT_AUTHOR_DATE=2026-01-01T00:00:00Z GIT_COMMITTER_DATE=2026-01-01T00:00:00Z
cp "$HERE/kairo.toml" "$HERE/kairo.kaem" "$WORK/"
git -C "$WORK" init -q --initial-branch=main --object-format=sha256
git -C "$WORK" config commit.gpgsign false
git -C "$WORK" add kairo.toml kairo.kaem
git -C "$WORK" commit -q -m "stage0-posix 1.9.1 AMD64 bootstrap"
OID=$(git -C "$WORK" rev-parse HEAD)

WRAPPER=$(kairo object create --actor "$ACTOR" --kind software | field object)
[ -n "$WRAPPER" ] || fail "object create printed no id"
kairo git fetch --object "$WRAPPER" --remote "file://$WORK" --branch main \
    --object-format sha256 >/dev/null
STATEMENT=$(kairo revision create --actor "$ACTOR" --object "$WRAPPER" \
    --revision "git:sha256:$OID" --manifest "$WORK/kairo.toml" | field statement)
[ -n "$STATEMENT" ] || fail "revision create printed no statement"
kairo branch set --actor "$ACTOR" --object "$WRAPPER" --revision "$STATEMENT" >/dev/null

cat <<OUT
wrapper = $WRAPPER

# Add to <store>/config.toml after auditing the seeds (POSIX/AMD64/*-seed
# against their .hex0 sources in $SEEDS_URL), then
#   kairo build object:$WRAPPER --target amd64 --actor $ACTOR
[[runtime.seeds]]
source = "$SEEDS_URL"
revision = "$SEEDS_REVISION"

[[runtime.engines]]
kind = "bubblewrap"
path = "/usr/bin/bwrap"
OUT
