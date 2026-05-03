# Kairo examples

Self-contained example objects and walkthroughs. Each example is structured to
work against a temporary store, so nothing in `~/.kairo/` is required and
nothing pollutes the user's real store.

## `objects/hello-kairo/`

A minimal object lineage that demonstrates the MVP end-to-end flow:

1. Create an actor (keypair + ActorGenesis).
2. Create an object lineage (ObjectGenesis signed by that actor).
3. Create a signed object revision pointing at the object's current state.
4. Set the actor's `head` branch to that revision so it can be resolved by
   name later.
5. Bind a semver version tag to that revision.
6. Compute a `SnapshotId` for the object's effective state.
7. Run end-to-end `kairo verify object` against the local store + Git repo.
8. Publish a first-person trust opinion and observe it in the verify report.

### Run the walkthrough

```sh
# Use a throwaway store rooted under /tmp so this example is reproducible.
export KAIRO_STORE="$(mktemp -d)/kairo-store"
echo "store at: $KAIRO_STORE"

cd examples/objects/hello-kairo

# The example tree is committed to a tiny throwaway Git repo so the
# revision-id (`git:sha256:<commit>`) refers to a real commit. The
# verifier will read kairo.toml back from the commit's tree and check
# the manifest binding end-to-end.
git init --quiet
git add kairo.toml
git -c user.name=Kairo -c user.email=test@kairo.test commit -m "init" --quiet
COMMIT="$(git rev-parse HEAD)"

# 1. Generate a fresh person actor.
kairo actor create --kind person
# → prints `actor = zQm…` (record this)
ACTOR=zQm...

# 2. Bind the example tree as a new object lineage.
kairo object create --actor "$ACTOR" --kind software \
  --initial-revision "git:sha256:$COMMIT"
# → prints `object = zQm…`
OBJECT=zQm...

# 3. Sign a revision claim that binds the storage commit to that object.
kairo revision create \
  --actor "$ACTOR" \
  --object "$OBJECT" \
  --revision "git:sha256:$COMMIT"
# → prints `statement = zQm…` (record this)
STATEMENT=zQm...

# 4. Inspect the manifest hash bound by the revision.
kairo manifest inspect

# 5. Inspect and list the revision through the store.
kairo revision inspect --statement "$STATEMENT"
kairo revision list --object "$OBJECT"

# 6a. Set head to point at the new revision. revision create is a low-level
#     primitive and does not advance any branch on its own; pointer moves
#     are always explicit.
kairo branch set --actor "$ACTOR" --object "$OBJECT" --revision "$STATEMENT"
# → prints `set branch ... name = head`
kairo branch show --object "$OBJECT"
kairo branch list --object "$OBJECT"

# 6b. Bind a semver version tag to the same revision. Tags are independent
#     of branches; consumers that need stable references should pin to the
#     resolved StatementId, not the version string.
kairo tag bind --actor "$ACTOR" --object "$OBJECT" \
  --version 1.0.0 --revision "$STATEMENT"
kairo tag show --object "$OBJECT" --version 1.0.0
kairo tag list --object "$OBJECT"

# 6c. Compute the SnapshotId for the object's effective state. By default
#     this follows the creator-actor's "head" branch; --statement <id>
#     pins the frontier directly.
kairo snapshot compute --object "$OBJECT"
# → prints `snapshot = zQm…`
kairo snapshot compute --object "$OBJECT" --json

# 7. Verify the object end-to-end: genesis fixity, revision signature,
#    actor resolution, object consistency, manifest binding, and content
#    layer (commit found + parents agree). With one local actor in the
#    keystore, --as is auto-picked, so trust resolves too.
kairo verify object --object "$OBJECT"
# → prints `verify object: VALID` plus the per-dimension breakdown,
#   including `trust = unknown (as $ACTOR)` since no opinion exists yet.
kairo verify object --object "$OBJECT" --json

# 8. Publish a first-person trust opinion. With one local actor we are
#    both the truster and the signer of the revision, so this expresses
#    "I trust myself," which evaluate_trust then surfaces in verify.
kairo trust grant --by "$ACTOR" --of "$ACTOR" --reason "self-trust"
kairo trust show --by "$ACTOR" --of "$ACTOR"
kairo trust list --by "$ACTOR"

# Re-run verification: trust now reports `trusted` instead of `unknown`.
kairo verify object --object "$OBJECT"

# 9. (Optional) Re-import the records into a fresh store to demonstrate
#    fixity round-trips: the imported statement_id and object_id are
#    derived from the canonical bytes of the parsed body, and must match
#    what was originally created.
STATEMENT_FILE="$(find "$KAIRO_STORE/statements" -name "$STATEMENT.json")"
ACTOR_FILE="$(find "$KAIRO_STORE/actors" -name "$ACTOR.json")"
OBJECT_FILE="$(find "$KAIRO_STORE/objects" -name "$OBJECT.json")"

export KAIRO_STORE_FRESH="$(mktemp -d)/kairo-store"
kairo --store "$KAIRO_STORE_FRESH" actor import --genesis "$ACTOR_FILE"
kairo --store "$KAIRO_STORE_FRESH" object import --statement "$OBJECT_FILE"
kairo --store "$KAIRO_STORE_FRESH" revision import --statement "$STATEMENT_FILE"

# 10. (Optional) Export everything the local store knows about the
#     object as a portable directory bundle, then import it into yet
#     another fresh store with a single command. Bundles cover what
#     the per-record `actor/object/revision import` does, plus the
#     object's branches, version tags, signing actors, and referenced
#     blobs — every record fixity-checked on import.
BUNDLE_DIR="$(mktemp -d)/object-bundle"
kairo bundle export --object "$OBJECT" --output "$BUNDLE_DIR"
ls "$BUNDLE_DIR"
# → manifest.json  actors/  objects/  statements/  blobs/

export KAIRO_STORE_BUNDLED="$(mktemp -d)/kairo-store"
kairo --store "$KAIRO_STORE_BUNDLED" bundle import --input "$BUNDLE_DIR"

# Branch resolution works in the bundled store — no per-record
# wiring needed.
kairo --store "$KAIRO_STORE_BUNDLED" branch show --object "$OBJECT"

# The bundle declares which Git commits its statements reference but
# does NOT carry the Git history itself in MVP. To reach VALID in the
# bundled store you'd need the same Git repo (or its commits) reachable
# from the bundled-store cwd. A future bundle version will optionally
# include a Git pack and populate ~/.kairo/git/ on import.
cat "$BUNDLE_DIR/manifest.json" | sed -n '/git_history/,$p' | head -n 6
```

### What this demonstrates

- **Content-addressed identity.** ActorId, ObjectId, and StatementId are
  derived from the canonical bytes of their respective records, not
  assigned by any registry.
- **Per-actor pointers.** Branches and version tags are mutable per-actor
  pointers; their statements are signed and the resolver picks the chain
  leaf (with timestamp tiebreak only on forks). Snapshot identity is
  over the resolved frontier, so two callers following different actors'
  branches/tags can land at different snapshots — but each snapshot is
  independently verifiable.
- **End-to-end verification.** `kairo verify object` rolls up six
  independent checks — genesis fixity, signature, actor resolution,
  object consistency, manifest binding, and Git content layer — into a
  single `VALID` / `INDETERMINATE` / `INVALID` verdict.
- **First-person trust.** Trust is parameterized by *who* is asking;
  `--as <truster>` (auto-picked from the keystore when there is one local
  actor) resolves the truster's `ActorTrust` chain leaf into
  `trusted | untrusted | unknown`. Trust is informational — it never
  changes the cryptographic verdict.
- **Layout on disk.** The store at `$KAIRO_STORE/` is sharded two levels
  deep by `<XX>/<YY>` of each ID; the keystore lives at `<store>/keys/`
  (override with `--keys`) with key files at mode `0600` on Unix.

See `specs/STORE.md` §4 for the full MVP layout and `specs/STATEMENTS.md`
§6 for the verification model.
