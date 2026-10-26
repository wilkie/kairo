#!/usr/bin/env python3
"""Print the wrapper's [[build.targets.children]] entries.

Reads live-bootstrap package file names (`bash-5.2.15_0.tar.bz2`), one per
line, in the order the build makes them; prints one child per package,
rooted at the directory start.kaem extracts it into and installed at /
(specs/DECISIONS.md §41 sub-decision 6), at the version the package
holds (§44). The last revision of each name and version provides that
name's label, at the child's version; earlier revisions, which later
steps rebuilt, are children without labels. A musl package is three
children, or two, by what linking needs of it (§51; split-musl.sh).

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
    "linux-headers": ["lib:linux:headers"],
    "autoconf-archive": ["data:autoconf-archive"],
    # A perl module perl's own build uses.
    "perl-Devel-Tokenizer-C": ["lib:perl:Devel-Tokenizer-C"],
    "openssl": ["lib:openssl"],
    "ca-certificates": ["data:ca-certificates"],
    "libffi": ["lib:ffi"],
    "gc": ["lib:gc"],
    "libunistring": ["lib:unistring"],
    # A perl module texinfo's build uses.
    "perl-Text-CSV": ["lib:perl:Text-CSV"],
}
# What a package needs beside it wherever it is installed
# (specs/DECISIONS.md §46): gcc runs binutils and compiles against
# musl's headers and the kernel's, and links against one of musl's
# static or shared libc, which whatever builds with it chooses itself
# (§51). gcc confers nothing: what is built with it needs a libc at run
# time only if it chose the shared one, which confers itself.
REQUIRES = {
    "gcc": [
        "tool:binutils",
        "lib:musl:headers",
        ("lib:musl:static", "lib:musl:shared"),
        "lib:linux:headers",
    ],
}
# More labels a package holds, by its name or its name and version:
# programs that go by other names, and what one version adds.
ALSO = {
    "heirloom-devtools": ["tool:yacc", "tool:lex"],
    "diffutils": ["tool:diff"],
    "findutils": ["tool:find"],
    "openssl": ["tool:openssl"],
    "guile": ["lib:guile"],
    "gettext": ["lib:intl"],
    "texinfo": ["tool:makeinfo"],
    # 4.7.4 is the first gcc live-bootstrap builds with C++; its
    # libstdc++ is static only, so it confers nothing.
    "gcc-4.7.4": ["tool:g++", "lib:stdc++:static"],
}
# The musl packages that hold a shared libc, as built: the earlier
# ones are built static only. A wrong guess fails the build, since the
# child's root isn't made.
MUSL_SHARED = {"musl-1.2.5_2"}


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


def requirement(entry):
    if isinstance(entry, tuple):
        alternatives = ", ".join(f'{{ provides = "{label}" }}' for label in entry)
        return f"{{ one_of = [{alternatives}] }}"
    return f'{{ provides = "{entry}" }}'


def child(name, root, summary, version, requires, provides):
    """One [[build.targets.children]] entry; `provides` is None for a
    child without labels."""
    print("[[build.targets.children]]")
    # Child names are lowercase (child-genesis-v1); the root is the
    # package's own spelling.
    print(f'name = "{name.lower()}"')
    print(f'root = "{root}"')
    print(f'summary = "{summary}"')
    print('tags = ["live-bootstrap"]')
    print(f'version = "{version}"')
    if requires:
        print(f"requires = [{', '.join(requirement(entry) for entry in requires)}]")
    if provides is not None:
        print(f"provides = [{', '.join(provides)}]")
    print()


def main():
    packages = [parse(line.strip()) for line in sys.stdin if line.strip()]
    last = {}
    for package, name, version, revision in packages:
        if revision >= last.get((name, version), -1):
            last[(name, version)] = revision
    for package, name, version, revision in packages:
        labelled = last[(name, version)] == revision
        if name == "musl":
            parts = ["headers", "static"] + (["shared"] if package in MUSL_SHARED else [])
            for part in parts:
                provides = None
                requires = []
                if labelled:
                    label = f"lib:musl:{part}"
                    provides = [f'{{ provides = "{label}" }}']
                    if part == "shared":
                        # Linking against it means needing it, at this
                        # version or a later 1.x; its loader is reached
                        # through /lib (§50).
                        provides = [
                            f'{{ provides = "{label}", confers = '
                            f'[{{ provides = "{label}", version = "^{version}" }}] }}'
                        ]
                if part == "shared":
                    requires = ["data:linux-filesystem:merged-usr"]
                child(
                    f"{package}-{part}",
                    f"{package}-{part}",
                    f"musl {version}'s {part} part, as live-bootstrap builds it (package {package}).",
                    version,
                    requires,
                    provides,
                )
            continue
        labels = (
            LIBRARIES.get(name, [f"tool:{name}"])
            + ALSO.get(name, [])
            + ALSO.get(f"{name}-{version}", [])
        )
        child(
            package,
            package,
            f"{name} {version}, as live-bootstrap builds it (package {package}).",
            version,
            REQUIRES.get(name, []),
            [f'{{ provides = "{label}" }}' for label in labels] if labelled else None,
        )


main()
