#!/usr/bin/env bash
# Archive readline's sources, make its object or its next revision, and
# tag it 8.3.6 (packages/readline/README.md, packages/common/import.sh).
#
# Usage:
#   import.sh <actor id> [--wrapper <object id>] [kairo args...]
#
# What it builds with must be imported: packages/build-base/ and the
# packages it names, and ncurses.
# Prints the object's id.
set -euo pipefail
shopt -s inherit_errexit

# sha256 name url...
DISTFILES=(
    "fe5383204467828cd495ee8d1d3c037a7eba1389c22bc6a041f627976f9061cc readline-8.3.tar.gz
     https://ftp.gnu.org/gnu/readline/readline-8.3.tar.gz
     https://mirrors.kernel.org/gnu/readline/readline-8.3.tar.gz"
    "21f0a03106dbe697337cd25c70eb0edbaa2bdb6d595b45f83285cdd35bac84de readline83-001
     https://ftp.gnu.org/gnu/readline/readline-8.3-patches/readline83-001
     https://mirrors.kernel.org/gnu/readline/readline-8.3-patches/readline83-001"
    "e27364396ba9f6debf7cbaaf1a669e2b2854241ae07f7eca74ca8a8ba0c97472 readline83-002
     https://ftp.gnu.org/gnu/readline/readline-8.3-patches/readline83-002
     https://mirrors.kernel.org/gnu/readline/readline-8.3-patches/readline83-002"
    "72dee13601ce38f6746eb15239999a7c56f8e1ff5eb1ec8153a1f213e4acdb29 readline83-003
     https://ftp.gnu.org/gnu/readline/readline-8.3-patches/readline83-003
     https://mirrors.kernel.org/gnu/readline/readline-8.3-patches/readline83-003"
    "1f189c4566aa35950964647b9ec3be1a821e42b86048129c63eb7f9d4d2f5a74 readline83-004
     https://ftp.gnu.org/gnu/readline/readline-8.3-patches/readline83-004
     https://mirrors.kernel.org/gnu/readline/readline-8.3-patches/readline83-004"
    "5481adbe161c9f6ba972c74db9e18c7aa351d3df3035cc4a673a1aa06247d1c6 readline83-005
     https://ftp.gnu.org/gnu/readline/readline-8.3-patches/readline83-005
     https://mirrors.kernel.org/gnu/readline/readline-8.3-patches/readline83-005"
    "4ebd261a608287796e171af0ea9af5a1e3122e46672accda2484b6698c47c1a4 readline83-006
     https://ftp.gnu.org/gnu/readline/readline-8.3-patches/readline83-006
     https://mirrors.kernel.org/gnu/readline/readline-8.3-patches/readline83-006"
)

HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../common/import.sh
. "$HERE/../common/import.sh"
import_package "$@" "readline 8.3.6 for x86 and x86-64" 8.3.6
