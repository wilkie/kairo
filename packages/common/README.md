# common

What every package's recipe and import script share. It is no object
of its own: each package's `import.sh` copies `recipe.sh` into the
package's tree beside its `build.sh`, so every object holds all of its
recipe, and changing `recipe.sh` changes each package's next revision.

| File | What it is |
|---|---|
| `recipe.sh` | Sourced by a package's `build.sh` with the target's triple; a step that fails is named, with its line. Sets the environment's library directory (`specs/DECISIONS.md` §60), the kernel's name for the architecture, and the ELF class and machine its code has. `unpack` extracts a tarball; `gnu` configures a GNU package for the target, its libraries in the library directory, and installs it into a child's directory; `finish` removes documentation, translations, and libtool's archives, checks every child (under `usr/` or `etc/`, nothing in `lib64`, every ELF the target's code), and reports what each child loads, for its `requires` (§56); `install_over` installs the children over what the package was built with, to check it works. |
| `import.sh` | Sourced by a package's `import.sh`: `import_package` archives the package's distfiles, commits its manifest and every file of its `src/`, `build.sh` and `recipe.sh` among them, as its object or that object's next revision (`../../bootstrap/wrapper.sh`), and tags the revision with the release's version (§59, §61). |
