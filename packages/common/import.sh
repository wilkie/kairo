# Sourced by a package's import.sh (packages/common/README.md): archive
# its sources, commit its manifest and its recipe, every file of src/
# with recipe.sh beside them, as its object or that object's next revision, and tag the
# revision with the release's version (DECISIONS.md §59, §61).
#
# The package's import.sh sets DISTFILES ("<sha256> <name> <url>..."
# entries) and HERE, then calls
#
#   import_package "$@" <message> <release version>
#
# with its own arguments: <actor id> [--wrapper <object id>] [kairo
# args...]. Extra arguments go before the command in every `kairo` call,
# e.g. `--store <path>`. KAIRO names the kairo binary (default: kairo on
# PATH). Prints the object's id.
KAIRO=${KAIRO:-kairo}
kairo() { "$KAIRO" "${KAIRO_ARGS[@]}" "$@"; }
field() { sed -n "s/^$1 = //p" | head -n 1; }
fail() {
    echo "import.sh: $*" >&2
    exit 1
}
# shellcheck source=../../bootstrap/wrapper.sh
. "$HERE/../../bootstrap/wrapper.sh"

import_package() {
    local release=${*: -1} message=${*: -2:1}
    parse_import_args "${@:1:$#-2}"
    fetch_distfiles
    WORK=$(mktemp -d)
    trap 'rm -rf "$WORK"' EXIT
    cp -r "$HERE/kairo.toml" "$HERE/src" "$WORK/"
    cp "$HERE/../common/recipe.sh" "$WORK/src/"
    # shellcheck disable=SC2046 # one path per line, none with spaces
    commit_wrapper "$message" kairo.toml $(cd "$WORK" && find src -type f | LC_ALL=C sort)
    tag_release "$release"
    cat <<OUT
object = $WRAPPER

# With what it builds with imported:
#   kairo build object:$WRAPPER --target x86 --actor $ACTOR
#   kairo build object:$WRAPPER --target x86-64 --actor $ACTOR
OUT
}
