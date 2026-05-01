# Kairo examples

Self-contained example objects and walkthroughs. Each example is structured to
work against a temporary store, so nothing in `~/.kairo/` is required and
nothing pollutes the user's real store.

## `objects/hello-kairo/`

A minimal object lineage that demonstrates the MVP end-to-end flow:

1. Create an actor (keypair + ActorGenesis).
2. Create an object lineage (ObjectGenesis signed by that actor).
3. Create a signed object revision pointing at the object's current state.
4. Verify the revision against the actor's genesis through the generic
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
# → prints `statement = zQm…`

# 4. Inspect the manifest hash bound by the revision.
kairo manifest inspect

# 5. Verify the most recent revision statement against the actor's genesis.
#    (For now this command takes file paths; the upcoming store-backed
#    `kairo verify` will resolve them automatically.)
STATEMENT_FILE="$(find "$KAIRO_STORE/statements" -name '*.json' | head -n 1)"
ACTOR_FILE="$(find "$KAIRO_STORE/actors" -name '*.json' | head -n 1)"
kairo revision verify-actor-genesis \
  --statement "$STATEMENT_FILE" \
  --actor-genesis "$ACTOR_FILE"
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
