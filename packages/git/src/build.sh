# Builds Git 2.56.0 into /kairo/package (packages/git/README.md) for
# the target named as the first argument, its first release, with
# packages alone (data:build-base, DECISIONS.md §61): one child,
# git-2.56.0.
. ./recipe.sh "$1"
NAME=git
VERSION=2.56.0
CHILD=git-2.56.0

# Its Makefile alone, compiled by gcc, as the base has no cc. HTTP and
# HTTPS remotes by libcurl, pushing over WebDAV with expat, and grep -P
# with PCRE2. With git's own regular expressions, as musl's lack what it
# asks of them, as Alpine builds it. Left out for now: its Perl parts
# (send-email, svn), its Python one (p4), Tcl/Tk (gitk, git gui),
# translations, and its Rust part, optional until Git 3.0, as no
# package has a Rust compiler yet. Its built-in commands are the one git program, not a
# link each; what are links are symbolic.
unpack "$NAME-$VERSION"
cd "$BUILD/$NAME-$VERSION"
GIT_MAKE=(CC=gcc prefix=/usr gitexecdir=/usr/libexec/git-core CFLAGS=-O2
    NO_PERL=YesPlease NO_PYTHON=YesPlease NO_TCLTK=YesPlease NO_GETTEXT=YesPlease
    NO_RUST=YesPlease NO_REGEX=YesPlease USE_LIBPCRE2=YesPlease
    SKIP_DASHED_BUILT_INS=YesPlease INSTALL_SYMLINKS=YesPlease)
make -j"$JOBS" "${GIT_MAKE[@]}" all
make "${GIT_MAKE[@]}" install DESTDIR="$OUT/$CHILD"

finish

# It works, installed over what it was built with.
install_over
git --version | grep -q "^git version 2\.56\.0$"
export HOME=$PWD GIT_AUTHOR_NAME=made GIT_AUTHOR_EMAIL=made@example GIT_COMMITTER_NAME=made GIT_COMMITTER_EMAIL=made@example
git init -q -b main repo
cd repo
echo "made 42 times" >file
git add file
git commit -q -m made
[ "$(git grep -P -o '\d+(?= times)' | cut -d: -f2)" = 42 ]
cd ..
git clone -q "file://$PWD/repo" copy
[ "$(git -C copy log --format=%s)" = made ]
# HTTPS reaches libcurl: nothing listens on the port, and curl says so.
! git ls-remote https://localhost:1/repo 2>https.err
grep -q "unable to access .https://localhost:1/repo/.: Failed to connect to localhost" https.err || { cat https.err >&2; false; }
