#!/usr/bin/env python3
"""Print the wrapper's [[build.targets.children]] entries.

Reads live-bootstrap package file names (`bash-5.2.15_0.tar.bz2`), one per
line, in the order the build makes them; prints one child per package,
rooted at the directory start.kaem extracts it into and installed at /
(specs/DECISIONS.md §41 sub-decision 6), at the version the package
holds (§44). The last revision of each name and version provides that
name's label, at the child's version; earlier revisions, which later
steps rebuilt, are children without labels. A musl package is three
children, or two, by what linking needs of it (§51; split-musl.sh); a
gcc with shared runtime libraries is its compiler and a child for each
library (§58; split-gcc.sh).

Given the report package.sh writes to the build's stderr, each child
requires what its programs and libraries load and it doesn't hold
(§56), by the label of the child that holds it.

    kairo build ... | sed -n 's|^  repo/\\([^ ]*\\) .*|\\1|p' | ./children.py [report]
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
    "libmd": ["lib:md"],
    "libbsd": ["lib:bsd"],
    "libarchive": ["lib:archive"],
}
# What a package needs beside it wherever it is installed
# (specs/DECISIONS.md §46): gcc runs binutils and compiles against
# musl's headers and the kernel's, and links against one of musl's
# static or shared libc, which whatever builds with it chooses itself
# (§51). gcc confers nothing: what is built with it needs a libc at run
# time only if it chose the shared one, which confers itself.
# What every compiler and binutils here produces code for (§53): a
# triple without its vendor field, the one spelling labels use. A
# native tool offers its plain label too; live-bootstrap's are native.
TRIPLE = "i686-linux-musl"
TARGETED = ["tool:gcc", "tool:g++", "tool:binutils"]
REQUIRES = {
    "gcc": [
        f"tool:binutils:{TRIPLE}",
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
    "gawk": ["tool:awk"],
    "opendoas": ["tool:doas"],
    "libarchive": ["tool:bsdtar"],
    # 4.7.4 is the first gcc live-bootstrap builds with C++; its
    # libstdc++ is static only, so it confers nothing.
    "gcc-4.7.4": ["tool:g++", "lib:stdc++:static"],
    # 10.5.0 also builds its runtime libraries shared, which are children
    # of their own (GCC_SHARED).
    "gcc-10.5.0": ["tool:g++", "lib:stdc++:static"],
    "gcc-15.2.0": ["tool:g++", "lib:stdc++:static"],
}
# Shared libraries confer themselves on what links against them, as a
# range from the version linked (§46, §51): musl keeps its ABI across a
# major version, and libstdc++ and libgcc_s have kept theirs since gcc
# 3.4, so a later one runs what an earlier one linked.
SELF_CONFERS = {
    "lib:musl:shared": "^{version}",
    "lib:stdc++:shared": ">={version}",
    "lib:gcc:shared": ">={version}",
    "lib:gomp:shared": ">={version}",
    "lib:atomic:shared": ">={version}",
    "lib:itm:shared": ">={version}",
    "lib:quadmath:shared": ">={version}",
}

# The gcc packages built with shared runtime libraries, each of which
# split-gcc.sh moves into a child of its own, by the library's name and
# its label. A wrong guess fails the build, since the child's root isn't
# made.
GCC_SHARED = {"gcc-10.5.0_0", "gcc-15.2.0_0"}
GCC_RUNTIME = [
    ("stdc++", "libstdc++"),
    ("gcc_s", "libgcc_s"),
    ("gomp", "libgomp"),
    ("atomic", "libatomic"),
    ("itm", "libitm"),
    ("quadmath", "libquadmath"),
]

# The label of the child holding each shared library a child may load,
# by soname (§56). A soname loaded but neither here nor held by the
# child stops the generation: a requirement isn't guessed.
SONAMES = {
    "libc.so": "lib:musl:shared",
    "libstdc++.so.6": "lib:stdc++:shared",
    "libgcc_s.so.1": "lib:gcc:shared",
    "libgomp.so.1": "lib:gomp:shared",
    "libatomic.so.1": "lib:atomic:shared",
    "libitm.so.1": "lib:itm:shared",
    "libquadmath.so.0": "lib:quadmath:shared",
    "libmd.so.0": "lib:md",
    "libbsd.so.0": "lib:bsd",
}


def read_report(path):
    """What each child loads and holds, by soname, from package.sh's
    lines in a build's stderr."""
    needs, holds = {}, {}
    with open(path) as report:
        for line in report:
            match = re.match(r"package\.sh: (\S+) (needs|holds): (.*)$", line.rstrip("\n"))
            if not match:
                continue
            child, kind, sonames = match.groups()
            names = [] if sonames.strip() == "nothing" else sonames.split()
            (needs if kind == "needs" else holds)[child] = names
    return needs, holds


def loaded(child, needs, holds):
    """The labels of what `child` loads and doesn't hold."""
    labels = []
    for soname in needs.get(child, []):
        if soname in holds.get(child, []):
            continue
        if soname not in SONAMES:
            sys.exit(f"{child} loads {soname}, which no label here names")
        if SONAMES[soname] not in labels:
            labels.append(SONAMES[soname])
    return labels
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


def provide(label, version):
    """A provides entry, conferring itself when the label does."""
    if label in SELF_CONFERS:
        conferred = SELF_CONFERS[label].format(version=version)
        return (
            f'{{ provides = "{label}", confers = '
            f'[{{ provides = "{label}", version = "{conferred}" }}] }}'
        )
    return f'{{ provides = "{label}" }}'


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
    needs, holds = read_report(sys.argv[1]) if len(sys.argv) > 1 else ({}, {})
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
                    provides = [provide(f"lib:musl:{part}", version)]
                # Its loader is reached through /lib (§50).
                if part == "shared":
                    requires = ["data:linux-filesystem:merged-usr"]
                requires += loaded(f"{package}-{part}", needs, holds)
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
        labels += [f"{label}:{TRIPLE}" for label in TARGETED if label in labels]
        requires = list(REQUIRES.get(name, []))
        requires += [label for label in loaded(package, needs, holds) if label not in requires]
        summary = f"{name} {version}, as live-bootstrap builds it (package {package})."
        if package in GCC_SHARED:
            summary = (
                f"gcc {version}, as live-bootstrap builds it (package {package}), "
                "without its shared runtime libraries, which are children of their own."
            )
        child(
            package,
            package,
            summary,
            version,
            requires,
            [provide(label, version) for label in labels] if labelled else None,
        )
        # Its shared runtime libraries, each a child (§58).
        if package in GCC_SHARED:
            for library, what in GCC_RUNTIME:
                part = f"{package}-lib{library}"
                child(
                    part,
                    part,
                    f"gcc {version}'s shared {what}, as live-bootstrap builds it (package {package}).",
                    version,
                    loaded(part, needs, holds),
                    [provide(f"lib:{library.replace('_s', '')}:shared", version)] if labelled else None,
                )


main()
