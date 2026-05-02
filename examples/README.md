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
5. Compute a `SnapshotId` for the object's effective state.
6. Verify the revision against the actor's genesis through the generic
   verifier.

### Run the walkthrough

```sh
# Use a throwaway store rooted under /tmp so this example is reproducible.
export KAIRO_STORE="$(mktemp -d)/kairo-store"
echo "store at: $KAIRO_STORE"

# 1. Generate a fresh person actor.
kairo actor create --kind person
# → prints `actor = zQm…` (record this)
ACTOR=zQm...

# 2. Bind the example tree as a new object lineage.
cd examples/objects/hello-kairo
kairo object create --actor "$ACTOR" --kind software --initial-revision git:sha256:0000000000000000000000000000000000000000000000000000000000000001
# → prints `object = zQm…`
OBJECT=zQm...

# 3. Sign a revision claim that binds a storage commit to that object.
kairo revision create \
  --actor "$ACTOR" \
  --object "$OBJECT" \
  --revision git:sha256:0000000000000000000000000000000000000000000000000000000000000001 \
  --parent  git:sha256:0000000000000000000000000000000000000000000000000000000000000000
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

# 6b. Compute the SnapshotId for object's effective state. By default this
#     follows the creator-actor's "head" branch; --statement <id> pins the
#     frontier directly.
kairo snapshot compute --object "$OBJECT"
# → prints `snapshot = zQm…`
kairo snapshot compute --object "$OBJECT" --json

# 7. Verify the most recent revision statement against the actor's genesis.
#    (For now this command takes file paths; the upcoming store-backed
#    `kairo verify` will resolve them automatically.)
STATEMENT_FILE="$(find "$KAIRO_STORE/statements" -name '*.json' | head -n 1)"
ACTOR_FILE="$(find "$KAIRO_STORE/actors" -name '*.json' | head -n 1)"
kairo revision verify-actor-genesis \
  --statement "$STATEMENT_FILE" \
  --actor-genesis "$ACTOR_FILE"

# 7b. (Optional) Verify with --json for machine-readable output.
kairo revision verify-actor-genesis \
  --statement "$STATEMENT_FILE" \
  --actor-genesis "$ACTOR_FILE" --json

# 8. (Optional) Re-import the records into a fresh store to demonstrate
#    fixity round-trips: the imported statement_id and object_id are
#    derived from the canonical bytes of the parsed body, and must match
#    what was originally created.
export KAIRO_STORE_FRESH="$(mktemp -d)/kairo-store"
kairo --store "$KAIRO_STORE_FRESH" actor import --genesis "$ACTOR_FILE"
kairo --store "$KAIRO_STORE_FRESH" object import --statement \
  "$(find "$KAIRO_STORE/objects" -name '*.json' | head -n 1)"
kairo --store "$KAIRO_STORE_FRESH" revision import --statement "$STATEMENT_FILE"
```

### What this demonstrates

- Identity is content-addressed: the ActorId, ObjectId, and StatementId are
  all derived from the canonical bytes of their respective records, not
  assigned by any registry.
- The keystore lives at `<store>/keys/` (override with `--keys`); the secret
  key file is mode `0600` on Unix.
- The store is sharded two levels deep by `<XX>/<YY>` of each ID, so look
  for files at e.g. `actors/zQ/m…/zQm…json`.
- Verification reports three independent dimensions — signature status,
  actor resolution, and trust evaluation. Trust is `unevaluated` until the
  trust crate lands; see `specs/ACTORS.md` §6.2.
