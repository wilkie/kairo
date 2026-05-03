# kairo-identity

Actor identity primitives: `ActorGenesisBody` (the unsigned, content-
addressed actor record), `ActorKind`, `PublicKey` / `SecretSigningKey`
(ed25519 in MVP), the `ActorResolver` trait, and signature
verification helpers shared by `kairo-statement::verify`.

`ActorGenesis` is intentionally unsigned — it is self-attesting via
content-addressing (the body's canonical bytes derive the `ActorId`),
and possession of the private key is enforced on every later signed
statement instead.

**Position in the dependency stack:** sits directly above `kairo-core`.
Depended on by `kairo-statement`, `kairo-store`, `kairo-keystore`, and
`kairo-cli`.

**Read more:** crate-level docs in `src/lib.rs`,
`specs/ACTORS.md` (especially §6.1.1 on the genesis-signature
asymmetry), and `schemas/canonical/actor-genesis-v1.md`.
