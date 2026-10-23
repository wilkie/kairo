#!/usr/bin/env python3
"""Print the wrapper's [[build.targets.children]] entries.

Reads live-bootstrap package file names (`bash-5.2.15_0.tar.bz2`), one per
line, in the order the build makes them; prints one child per package,
rooted at the directory start.kaem extracts it into and installed at /
(specs/DECISIONS.md §41 sub-decision 6). The last revision of each name
and version provides that name's label, with the version; earlier
revisions, which later steps rebuilt, are children without labels.

    kairo build ... | sed -n 's|^  repo/\\([^ ]*\\) .*|\\1|p' | ./children.py
"""

import re
import sys

# Libraries, by their label; everything else is a tool by its name.
LIBRARIES = {
    "gmp": ["lib:gmp"],
    "mpfr": ["lib:mpfr"],
    "mpc": ["lib:mpc"],
    "zlib": ["lib:zlib"],
    "musl": ["lib:musl"],
    "linux-headers": ["lib:linux:headers"],
    "autoconf-archive": ["data:autoconf-archive"],
}
# Packages whose programs go by other names.
ALSO = {
    "heirloom-devtools": ["tool:yacc", "tool:lex"],
    "diffutils": ["tool:diff"],
    "findutils": ["tool:find"],
}


def parse(file_name):
    package = file_name.removesuffix(".tar.bz2")
    # perl5.004-05 and perl5.005-03 are perl's own spellings.
    special = re.fullmatch(r"perl(5\.00[45])-(\d+)_(\d+)", package)
    if special:
        return package, "perl", f"{special[1]}_{special[2]}", int(special[3])
    match = re.fullmatch(r"(.+?)-(\d[^_]*)_(\d+)", package)
    if not match:
        sys.exit(f"{file_name}: not <name>-<version>_<revision>.tar.bz2")
    return package, match[1], match[2], int(match[3])


def main():
    packages = [parse(line.strip()) for line in sys.stdin if line.strip()]
    last = {}
    for package, name, version, revision in packages:
        if revision >= last.get((name, version), -1):
            last[(name, version)] = revision
    for package, name, version, revision in packages:
        print("[[build.targets.children]]")
        print(f'name = "{package}"')
        print(f'root = "{package}"')
        print(f'summary = "{name} {version}, as live-bootstrap builds it (package {package})."')
        print('tags = ["live-bootstrap"]')
        if last[(name, version)] == revision:
            labels = LIBRARIES.get(name, [f"tool:{name}"]) + ALSO.get(name, [])
            provides = ", ".join(
                f'{{ provides = "{label}", version = "{version}" }}' for label in labels
            )
            print(f"provides = [{provides}]")
        print()


main()
