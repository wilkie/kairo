# Commit a bootstrap wrapper as SHA-256 history and make it an object's
# revision; sourced by each wrapper's import.sh, which defines kairo(),
# field(), fail(), ACTOR, and WORK (a directory holding the files).
#
#   commit_wrapper <message> <file>...
#
# With WRAPPER unset, creates a new object. With WRAPPER set to an
# object id (import.sh --wrapper), the commit's parent is that object's
# current commit in the Git cache, and the new revision names it, so
# re-importing a changed wrapper extends the object's history instead of
# making another object that offers the same labels. Fixed author and
# dates make the commit depend only on the files and its parent, so an
# unchanged wrapper gives the commit it already has and no new revision.
# Sets WRAPPER and OID.
commit_wrapper() {
    local message=$1
    shift
    export GIT_AUTHOR_NAME=kairo-bootstrap GIT_AUTHOR_EMAIL=bootstrap@kairo.invalid
    export GIT_COMMITTER_NAME=kairo-bootstrap GIT_COMMITTER_EMAIL=bootstrap@kairo.invalid
    export GIT_AUTHOR_DATE=2026-01-01T00:00:00Z GIT_COMMITTER_DATE=2026-01-01T00:00:00Z
    git -C "$WORK" init -q --initial-branch=main --object-format=sha256
    git -C "$WORK" config commit.gpgsign false
    local parent=()
    if [ -n "${WRAPPER:-}" ]; then
        local cache repo previous
        cache=$(kairo git cache status | sed -n 's/^git cache: //p')
        repo=$cache/${WRAPPER:3:2}/${WRAPPER:5:2}/$WRAPPER
        [ -d "$repo" ] || fail "object:$WRAPPER has no history in the Git cache"
        git -C "$WORK" fetch -q --update-head-ok "$repo" "+refs/heads/main:refs/heads/main"
        previous=$(git -C "$WORK" rev-parse refs/heads/main)
        # The index starts empty, so the commit holds exactly these files.
        git -C "$WORK" add "$@"
        if git -C "$WORK" diff --cached --quiet "$previous"; then
            echo "import.sh: object:$WRAPPER already holds this wrapper" >&2
            OID=$previous
            return
        fi
        parent=(--parent "git:sha256:$previous")
    else
        WRAPPER=$(kairo object create --actor "$ACTOR" --kind software | field object)
        [ -n "$WRAPPER" ] || fail "object create printed no id"
        git -C "$WORK" add "$@"
    fi
    git -C "$WORK" commit -q -m "$message"
    OID=$(git -C "$WORK" rev-parse HEAD)
    kairo git fetch --object "$WRAPPER" --remote "file://$WORK" --branch main \
        --object-format sha256 >/dev/null
    local statement
    statement=$(kairo revision create --actor "$ACTOR" --object "$WRAPPER" \
        --revision "git:sha256:$OID" "${parent[@]}" --manifest "$WORK/kairo.toml" |
        field statement)
    [ -n "$statement" ] || fail "revision create printed no statement"
    kairo branch set --actor "$ACTOR" --object "$WRAPPER" --revision "$statement" >/dev/null
}

# import.sh's arguments: <actor id> [--wrapper <object id>] [kairo args...].
# Sets ACTOR, WRAPPER, and KAIRO_ARGS.
parse_import_args() {
    if [ $# -lt 1 ]; then
        echo "usage: import.sh <actor id> [--wrapper <object id>] [kairo args...]" >&2
        exit 2
    fi
    ACTOR=$1
    shift
    WRAPPER=
    if [ "${1:-}" = --wrapper ]; then
        [ $# -ge 2 ] || fail "--wrapper needs an object id"
        WRAPPER=$2
        shift 2
    fi
    KAIRO_ARGS=("$@")
}
