#!/usr/bin/env bash
# Archive stage0-posix and make the native object that builds it
# (specs/DECISIONS.md §36, §37).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# 1. `kairo source fetch https://github.com/oriansj/stage0-posix`
#    archives stage0-posix as a source object, and with it, recursively,
#    every submodule its commits pin (the seeds, M2-Planet, M2libc,
#    mescc-tools, ...), each as its own source, under its real upstream
#    URL. This needs the network. If Savannah can't be reached,
#    mescc-tools is fetched through its GitHub mirror instead, and its
#    observation says so.
# 2. The native wrapper -- this directory's kairo.toml and
#    kairo-<arch>.kaem, which pin every source the AMD64 and x86 chains
#    use at its Release_1.9.1 commit -- is committed as SHA-256 history
#    and created as an object, or, with --wrapper, committed on top of
#    that existing wrapper object's history as its next revision.
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
# Savannah is sometimes unreachable; the maintainer's GitHub mirror
# holds the same history (specs/DECISIONS.md §40).
MIRRORS=(
    --mirror https://git.savannah.nongnu.org/git/mescc-tools=https://github.com/oriansj/mescc-tools
)
SEEDS_REVISION=git:sha1:cedec6b8066d1db229b6c77d42d120a23c6980ed

KAIRO=${KAIRO:-kairo}
HERE=$(cd "$(dirname "$0")" && pwd)

kairo() { "$KAIRO" "${KAIRO_ARGS[@]}" "$@"; }
field() { sed -n "s/^$1 = //p" | head -n 1; }
fail() {
    echo "import.sh: $*" >&2
    exit 1
}
# shellcheck source=../wrapper.sh
. "$HERE/../wrapper.sh"
parse_import_args "$@"

# 1. The sources, recursively. Incomplete submodule archiving fails here.
kairo source fetch "$STAGE0_URL" --actor "$ACTOR" --require-complete "${MIRRORS[@]}" >&2

# 2. The native wrapper: SHA-256 history, born in the archive
# (../wrapper.sh).
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
cp "$HERE/kairo.toml" "$HERE/kairo-amd64.kaem" "$HERE/kairo-x86.kaem" "$WORK/"
commit_wrapper "stage0-posix 1.9.1 AMD64 and x86 bootstrap" kairo.toml kairo-amd64.kaem kairo-x86.kaem
# A release of upstream's 1.9.1, offered by label (DECISIONS.md §61).
tag_release 1.9.1

cat <<OUT
wrapper = $WRAPPER

# Add to <store>/config.toml after auditing the seeds (POSIX/AMD64/*-seed
# and POSIX/x86/*-seed against their .hex0 sources in $SEEDS_URL), then
#   kairo build object:$WRAPPER --target amd64 --actor $ACTOR
#   kairo build object:$WRAPPER --target x86 --actor $ACTOR
[[runtime.seeds]]
source = "$SEEDS_URL"
revision = "$SEEDS_REVISION"

[[runtime.engines]]
kind = "bubblewrap"
path = "/usr/bin/bwrap"
OUT
