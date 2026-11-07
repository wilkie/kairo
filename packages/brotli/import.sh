#!/usr/bin/env bash
# Archive brotli's sources, make its object or its next revision, and
# tag it 1.2.1 (packages/brotli/README.md, packages/common/import.sh).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# What it builds with must be imported: packages/build-base/ and the
# packages it names, and cmake and ninja.
# Prints the object's id.
set -euo pipefail
shopt -s inherit_errexit

# sha256 name url...
DISTFILES=(
    "98d35d11a78dc453aebb7d10c3748f3f73ff78c8267c0dc697b8631f67bf7e8f brotli-1.2.1.tar.gz
     https://github.com/google/brotli/archive/refs/tags/v1.2.1.tar.gz"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "brotli 1.2.1 for x86 and x86-64" 1.2.1
