# Builds Meson 1.12.1 into /kairo/package (packages/meson/README.md) for
# the target named as the first argument, its first release, with
# packages alone (data:build-base, DECISIONS.md §61): one child,
# meson-1.12.1.
. ./recipe.sh "$1"
NAME=meson
VERSION=1.12.1
CHILD=meson-1.12.1

# Meson as one file, a zip of its modules Python runs, as its own
# packaging/create_zipapp.py makes it but with its entries in order and
# their times the source's, so it is the same each time: Python 3.11's
# zipapp takes them in the order the directory lists them. Python and
# Ninja, which it runs, are what it requires.
unpack "$NAME-$VERSION"
cd "$BUILD/$NAME-$VERSION"
mkdir -p "$OUT/$CHILD/usr/bin"
python3 - "$OUT/$CHILD/usr/bin/meson" <<'PY'
import os, pathlib, sys, zipfile
out = sys.argv[1]
entries = [("__main__.py", pathlib.Path("meson.py"))] + sorted(
    (path.as_posix(), path) for path in pathlib.Path("mesonbuild").rglob("*") if path.is_file())
with open(out, "wb") as f:
    f.write(b"#!/usr/bin/python3\n")
    with zipfile.ZipFile(f, "w", zipfile.ZIP_DEFLATED) as z:
        for name, path in entries:
            info = zipfile.ZipInfo(name, zipfile.ZipInfo.from_file(path).date_time)
            info.external_attr = 0o644 << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, path.read_bytes())
os.chmod(out, 0o755)
PY

finish

# It works, installed over what it was built with.
install_over
[ "$(meson --version)" = 1.12.1 ]
# A project that finds a library by pkg-config, built with Ninja.
mkdir mcheck
cd mcheck
cat > meson.build <<'MB'
project('made', 'c')
executable('made', 'made.c', dependencies: dependency('libpkgconf'))
MB
cat > made.c <<'C'
#include <libpkgconf/libpkgconf.h>
int main(void) { return 0; }
C
meson setup build
meson compile -C build
./build/made
