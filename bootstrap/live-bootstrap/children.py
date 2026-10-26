#!/usr/bin/env python3
"""Print the wrapper's [[build.targets.children]] entries.

Reads live-bootstrap package file names (`bash-5.2.15_0.tar.bz2`), one per
line, in the order the build makes them; prints one child per package,
rooted at the directory start.kaem extracts it into and installed at /
(specs/DECISIONS.md §41 sub-decision 6), at the version the package
holds (§44). The last revision of each name and version provides that
name's label, at the child's version; earlier revisions, which later
steps rebuilt, are children without labels.

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
    # A perl module perl's own build uses.
    "perl-Devel-Tokenizer-C": ["lib:perl:Devel-Tokenizer-C"],
    "openssl": ["lib:openssl"],
    "ca-certificates": ["data:ca-certificates"],
}
# What a package needs beside it wherever it is installed, and what
# using one of its labels confers on whatever is built with it
# (specs/DECISIONS.md §46): gcc runs binutils and compiles against musl
# and the kernel headers, and what it builds links against musl.
REQUIRES = {
    "gcc": ["tool:binutils", "lib:musl", "lib:linux:headers"],
}
CONFERS = {
    "tool:gcc": ["lib:musl"],
    "tool:g++": ["lib:musl", "lib:stdc++"],
    "lib:musl": ["lib:musl"],
    "lib:stdc++": ["lib:stdc++"],
}
# More labels a package holds, by its name or its name and version:
# programs that go by other names, and what one version adds.
ALSO = {
    "heirloom-devtools": ["tool:yacc", "tool:lex"],
    "diffutils": ["tool:diff"],
    "findutils": ["tool:find"],
    "openssl": ["tool:openssl"],
    # 4.7.4 is the first gcc live-bootstrap builds with C++.
    "gcc-4.7.4": ["tool:g++", "lib:stdc++"],
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
        # Child names are lowercase (child-genesis-v1); the root is the
        # package's own spelling.
        print(f'name = "{package.lower()}"')
        print(f'root = "{package}"')
        print(f'summary = "{name} {version}, as live-bootstrap builds it (package {package})."')
        print('tags = ["live-bootstrap"]')
        print(f'version = "{version}"')
        if name in REQUIRES:
            requires = ", ".join(f'{{ provides = "{label}" }}' for label in REQUIRES[name])
            print(f"requires = [{requires}]")
        if last[(name, version)] == revision:
            labels = (
                LIBRARIES.get(name, [f"tool:{name}"])
                + ALSO.get(name, [])
                + ALSO.get(f"{name}-{version}", [])
            )
            entries = []
            for label in labels:
                conferred = ", ".join(f'{{ provides = "{c}" }}' for c in CONFERS.get(label, []))
                entries.append(
                    f'{{ provides = "{label}", confers = [{conferred}] }}'
                    if conferred
                    else f'{{ provides = "{label}" }}'
                )
            print(f"provides = [{', '.join(entries)}]")
        print()


main()
