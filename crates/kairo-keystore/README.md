# kairo-keystore

Local keystore for Kairo signing keys. `FilesystemKeystore` implements
the `Keystore` trait and persists each actor's secret as
`<root>/<actor-id>.json` using the `kairo.key.private.v1` schema.

**MVP only — not production key management.** Secret material is
stored as JSON on disk, protected by file-system permissions (`0700`
on the directory and `0600` on each key file under Unix). Passphrase
encryption, OS-keychain integration, HSM/PKCS11 support, and key
rotation are explicit non-goals for the MVP.

Errors mirror `kairo-store`'s tri-fold model: `Missing` (semantic),
`Corrupt` (fixity), `Unavailable` (operational).

**Position in the dependency stack:** sits above `kairo-core` and
`kairo-identity`. Depended on by `kairo-cli`.

**Read more:** crate-level docs in `src/lib.rs` and
`memory/project_keystore_design.md` for the MVP scope rationale.
