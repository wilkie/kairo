# bootstrap-base

What a typical C or C++ package builds with, as `../build-base/` has
it, but offered **only by the bootstraps**: the environment a package's
first generation builds with (`specs/DECISIONS.md` §59).

**Why it exists.** A package offers the bootstrap's labels at the
bootstrap's versions (`packages/make` offers `tool:make` 4.2.1, as stage
3's child does) and, published later, wins the tie (§48). Through
`build-base`, binutils' first build would then take the make package,
whose build takes the binutils package: a cycle. Breaking it by
whichever was asked for first would make a plan depend on its caller.
So a first generation names its bootstrap.

**How.** It brings `build-base`'s labels (§57), each with `offered_by`
the bootstraps' objects (§59): a label takes only an offer of one of
them or of their children. They are named by their ids in the archive's
canonical store, which other stores receive by bundle or federation, as
their ids don't change:

| Object | What |
|---|---|
| `zQmbn8aYLLJJkzCj8wcmTCHUhycMNozG3vWNXxnDrQRziAQ` | live-bootstrap pass 1, to zlib (`bootstrap/live-bootstrap/`) |
| `zQmRcSie6cMMJSCkJv9tJnkuF1HMSpwqzJQNjJiSLJmAFXu` | pass 2, the perl chain |
| `zQmRJS223L2J8WGQKQZwCjQh5r1NbMiLmNyQf1vc3QtM3eR` | pass 3, to gcc 4.7.4 |
| `zQmULyVeK4Tmvq3sNAh69bdXwUeKXYUbP27cSva9QgJUA4X` | pass 4, python, guile, and autogen |
| `zQmUUoeihepsPtywUtHvfu2s6PFV6fFw3YQg8AALvcSo35V` | pass 5, to gcc 10.5.0 |
| `zQmNRiLHtCnvdDer5i5GRqPxzwjDoioZW7oxVSeudCKnNBk` | pass 6, to gcc 15.2.0 |
| `zQmPZfbmnu1hg7oj5E26zhYHuVur4PNAdNUi4STxE3CNdqS` | the 64-bit bootstrap's stage 3 (`bootstrap/amd64/native-root/`) |

Labels resolve per environment, so the same object brings
live-bootstrap's packages in `linux/x86` and the 64-bit root's in
`linux/x86-64`. A package's later releases build with `build-base`,
where they find the packages and older releases of themselves.

| File | What it is |
|---|---|
| `kairo.toml` | `data:bootstrap-base` 1.0.0, and the dependencies it brings. |
| `import.sh` | Creates the object with `../../bootstrap/wrapper.sh` and tags it 1.0.0. |
