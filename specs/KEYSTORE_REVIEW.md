# Keystore security review

A focused review of `kairo-keystore`. Audits the actual MVP code path
that holds secret signing material on disk; calls out concrete
findings and tracks each one to a severity, a current mitigation (or
absence of one), and a follow-on action.

## 1. Scope

In scope:
- `crates/kairo-keystore/src/lib.rs` — `FilesystemKeystore` and the
  `Keystore` trait.
- `crates/kairo-keystore/src/json.rs` — `kairo.key.private.v1` schema
  (`PrivateKeyJson`), cross-reference validation on load.
- `kairo-identity::SecretSigningKey` insofar as it affects keystore
  behavior (in-memory representation, drop semantics).

Out of scope:
- Network exposure, OS keychain integration, HSM/PKCS11, hardware
  tokens — all explicit MVP non-goals.
- The signing-bytes path itself (already covered by the threat model
  in `THREAT_MODEL.md` §5).
- Backup, multi-machine sync — none is offered today.

## 2. Threat model assumptions

The keystore is a single-user, local CLI persistence layer. The
defended threats are:

- **Defended.** Casual file-system reads by a different user on the
  same host (file mode `0600`, dir mode `0700`).
- **Defended (best-effort).** Crash mid-write leaving a corrupt key
  file (atomic-rename pattern).
- **Defended.** Tampering with the on-disk JSON to swap secrets
  between actors or substitute a different key — fixity check
  re-derives the public `KeyId` from the seed and refuses on
  mismatch.
- **Defended.** Schema confusion across versions — schema string is
  validated on load.

The keystore explicitly does **not** defend against:

- Root / kernel-level attackers on the same host.
- Forensic recovery from disk after deletion (no shred-on-delete).
- Memory-dump recovery (no zeroize-on-drop, see finding §3.5).
- An attacker with write access to the keystore directory (there are
  no signatures over the on-disk shape; tampering is detected only
  via the cross-reference checks in §2.3).

Anyone with write access to `~/.kairo/keys/` can replace the actor's
secret outright, then sign arbitrary statements as that actor. The
mitigation is operational — keep the keystore on a system where only
the operator has write access — and the design choice is to defer
encrypted-at-rest passphrase wrapping to post-MVP.

## 3. Findings

Severity scale matches the threat model: **High** = exploitable by an
attacker without prior privilege escalation; **Medium** = requires
specific co-tenancy or timing conditions; **Low** = defense-in-depth
gap with no realistic single-user exploit; **Info** = design note.

### 3.1 `[Medium]` Permission-set race after `atomic_write`

**Where.** `lib.rs::put_signing_key` / `replace_signing_key`:

```rust
atomic_write(&path, &bytes)?;
set_file_permissions(&path)?;   // chmod 0600 — happens AFTER create
```

`atomic_write` calls `fs::write(&tmp, bytes)` (creates the tmp file
with the default umask, typically `0022` → mode `0644`), then
`fs::rename(&tmp, path)`. The chmod to `0600` happens only *after*
the rename. There is a small window during which the file exists
with default permissions, readable by other users on the host.

For tmp: the tmp file inherits umask perms briefly during write.

For dir creation: `set_dir_permissions` likewise runs after
`fs::create_dir_all`, leaving a window where the `keys/` dir is mode
`0755` (or whatever the umask gave it). A directory listing is
visible to other users in that window even if no individual key file
yet is.

**Severity.** Medium on multi-user hosts (bounded race window for
cross-user reads). Low on single-user systems.

**Mitigation.** Open the tmp file with `O_CREAT | O_EXCL` and pass
mode `0600` directly via `std::os::unix::fs::OpenOptionsExt::mode`,
so the file is born with the right permissions. Do the same for
`create_dir_all` by manually walking and `mkdir(2)`-ing each
component with mode `0700`. Either approach closes the race.

**Action.** Track as a follow-on; not exploitable in the
single-user MVP target environment but worth fixing before the
daemon ships (which would broaden the attacker model).

### 3.2 `[Low]` `put_signing_key` TOCTOU on existence check

**Where.** `lib.rs::put_signing_key`:

```rust
if path.exists() {
    return Err(KeystoreError::Corrupt {
        reason: CorruptReason::AlreadyExists, ...
    });
}
// ... atomic_write later
```

Between `path.exists()` returning `false` and the eventual
`fs::rename` of the tmp file, a concurrent writer could create the
key file. The rename clobbers the concurrent writer silently.

**Severity.** Low. The contract of `put_signing_key` is "create new,
refuse to overwrite"; race conditions are invisible to a single
operator using one CLI process at a time. Becomes meaningful only in
a multi-process scenario (CLI + daemon, or two CLI invocations).

**Mitigation.** Open the tmp file with `O_CREAT | O_EXCL` and
*rename without overwrite*. The standard library's `fs::rename`
overwrites the destination on Unix; the `renameat2` syscall with
`RENAME_NOREPLACE` (Linux) or `linkat` then `unlink` (portable) gives
the no-overwrite semantics atomically. Alternatively, a per-actor
file lock (covered by PHASE_2 §6) makes the existence check race-free
without atomic-rename gymnastics.

**Action.** Roll into PHASE_2 §6 multi-process safety / file locks.

### 3.3 `[Low]` `replace_signing_key` likewise TOCTOU

**Where.** `lib.rs::replace_signing_key`:

```rust
if !path.exists() {
    return Err(KeystoreError::Missing);
}
// ... atomic_write later
```

Between the existence check and the actual write, the key file could
be removed (concurrent operator) or the wrong key file could be
present (concurrent writer). The rename then either errors with
`ENOENT` parented (which we'd surface as `Unavailable`) or clobbers
unexpected content.

**Severity.** Low, same reasoning as §3.2.

**Mitigation.** Same as §3.2 — file locks under PHASE_2 §6.

### 3.4 `[Low]` No `fsync` before rename

**Where.** `lib.rs::atomic_write`:

```rust
fs::write(&tmp, bytes)?;
fs::rename(&tmp, path)?;
```

`fs::write` doesn't fsync the file before returning, and we don't
fsync the parent directory after rename. On a power loss between the
write returning and the next checkpoint, the rename can be visible
in the directory entry but the file's data blocks may not be on
disk, leaving a zero-length or torn file. Modern filesystems (ext4,
APFS, NTFS) make this rare in practice but it is permitted by the
POSIX contract.

**Severity.** Low. A keystore corrupt from torn-write would surface
on next read as `KeystoreError::Corrupt` rather than silent data
loss; the operator can re-import. Actor-genesis re-derivation is
the recovery path. Newly-generated keys *not yet exported anywhere*
would be unrecoverably lost — but that's true of any storage layer
without WAL.

**Mitigation.** After `fs::write`, call `File::sync_all()` on the
tmp file handle; after `fs::rename`, open the parent directory and
fsync it. The POSIX rename atomicity guarantee covers visibility, not
durability — both syncs are needed.

**Action.** Acceptable for MVP; revisit when the daemon ships
(durability matters more once the daemon batches writes).

### 3.5 `[Medium]` `SecretSigningKey` not zeroized on drop

**Where.** `kairo-identity::SecretSigningKey`:

```rust
#[derive(Clone, PartialEq, Eq)]
pub struct SecretSigningKey {
    algorithm: SignatureAlgorithm,
    seed: [u8; 32],
}
```

The seed is held as a plain `[u8; 32]`. On drop, the bytes are not
overwritten. They linger in freed memory until the allocator reuses
the page. A core dump, a process-memory exfiltration bug elsewhere
in the workspace, or a subsequent allocator reuse leaking through a
debugger session could recover the seed.

The `Debug` impl correctly redacts the seed. `seed_bytes()` is
documented as "sensitive" — but only as a comment, not a
compile-time guarantee.

In addition, the keystore copies the seed through several
intermediate allocations on each load:

- `fs::read(&path)` → `Vec<u8>` containing the JSON (which contains
  the base64-encoded seed).
- `serde_json::from_slice` parses, allocating string fragments.
- `STANDARD.decode(&self.secret_key)` → `Vec<u8>` containing the
  raw seed.
- `<[u8; 32]>::try_from(...)` copies into the array.

None of those intermediate `Vec<u8>` / `String` allocations are
zeroized.

**Severity.** Medium. Single-user laptop with no other adversarial
processes running, the practical exposure is small. In a daemon
context, or anywhere a memory leak is exploitable, this matters more.

**Mitigation.** Add the `zeroize` crate as a dependency and:
- Implement `Drop` for `SecretSigningKey` that zeroes `seed`. (Mark
  with `Zeroize` derive.)
- Use `Zeroizing<Vec<u8>>` for the intermediate base64-decoded buffer
  in `PrivateKeyJson::to_secret`.
- Optionally do the same for the JSON-parse buffer, though the seed
  is base64-encoded there so its plaintext recovery requires base64
  decode.

The `ed25519-dalek` `SigningKey` also internally holds the secret;
that crate has a `zeroize` feature flag we'd want to enable.

**Action.** Defer to a focused "zeroize + memory hygiene" pass; it's
a cross-crate change (kairo-identity, kairo-keystore, anywhere
seed_bytes round-trips). Track separately from the keystore review.

### 3.6 `[Info]` Plaintext-at-rest is intentional MVP scope

**Where.** `kairo.key.private.v1` JSON, `secret_key` field
(base64-encoded raw seed).

The seed is stored in plaintext, protected only by file permissions.
This is documented at the top of `lib.rs` as MVP-only.

**Severity.** Info — design choice, not a defect. The threat model
calls this out explicitly.

**Mitigation.** Out-of-band: let users encrypt their `~/.kairo/keys/`
on full-disk-encrypted systems, or run on a machine with disk-level
encryption. In-band: passphrase-wrapped key encryption is the
documented future work.

**Action.** Track as PHASE_3+ work. When implemented, the schema bumps
to `kairo.key.private.v2` and the v1 reader stays for compatibility
during migration.

### 3.7 `[Low]` Stale `.tmp` files not cleaned up

**Where.** `lib.rs::atomic_write` writes to
`<parent>/.<file_name>.tmp` and renames over the destination. If the
process crashes after the write but before the rename, the tmp file
is left behind. Subsequent operations don't clean it up — they
overwrite via `fs::write` (which truncates).

The tmp file inherits whatever mode the umask gave it (see §3.1) and
contains the secret seed in plaintext.

**Severity.** Low. The chance of a crash between `fs::write` and
`fs::rename` is small, and the leaked file would have whatever umask
permissions (typically `0644`, readable by other users on the host).

**Mitigation.** On `FilesystemKeystore::open`, sweep the root for
`.*.tmp` files and remove them. Belt-and-braces: open the tmp file
with `O_CREAT | O_EXCL | mode 0600` so even a leaked tmp file is
not world-readable.

**Action.** Worthwhile cleanup; small. Roll into the §3.1 mode-bits
fix.

### 3.8 `[Info]` ActorId-based filename safe by construction

**Where.** `lib.rs::path_for`:

```rust
self.root.join(format!("{actor_id}{JSON_SUFFIX}"))
```

`ActorId` values are content-addressed — `validate_id_payload` in
`kairo-core` enforces a `z`-prefix base58btc multihash with a fixed
SHA-256 digest length. The base58 alphabet excludes `/`, `\`, `..`,
`\0`, and other path-injection characters. There's no path traversal
risk from interpolating an `ActorId` into a filename.

**Severity.** Info — design property worth documenting so future
changes don't quietly break it.

**Mitigation.** Hold the line on `ActorId` validation. Any new ID
format that drops the alphabet check would need its own filename
scrubbing. (See `kairo-store` shard layout for a richer example
that splits ID prefixes into directory components — same property
applies.)

### 3.9 `[Info]` Symlinks followed by `fs::read`

**Where.** `lib.rs::get_signing_key` calls `fs::read(&path)`, which
follows symlinks. An attacker with write access to the keystore
directory could replace `<actor>.json` with a symlink to a different
file (e.g. `/etc/passwd`).

**Severity.** Info. An attacker with write access to the keystore
directory has more direct attacks available (overwrite the key
outright). Symlink redirection only matters if write-without-replace
is the attacker's only primitive — implausible.

**Mitigation.** None needed. Documenting the property so that future
code paths (e.g. a daemon running as a privileged user) don't
inherit this assumption blindly.

### 3.10 `[Info]` Cross-reference fixity is the only tamper detection

**Where.** `json.rs::to_secret` re-derives the public `KeyId` from
the on-disk seed and refuses if it doesn't match the stored
`key_id` field. It also checks the `actor_id` field matches the
requested actor. There's no signature over the JSON shape itself.

**Severity.** Info. Tamper detection is structural: an attacker who
swaps both the seed *and* the key_id field can substitute their own
key without detection (the keystore happily loads and uses it). The
defense isn't "the file isn't tampered" — it's "if the file is
tampered to point at a *wrong* key, the keystore catches it."

**Mitigation.** A MAC over the file contents (HMAC-SHA256 with a
keystore-master key) would close this gap, but introduces the
master-key-rotation problem. Current design accepts the risk
because file permissions are the primary defense; tamper detection
is defense-in-depth on top of that.

## 4. Strengths

For balance, the items the review found done correctly:

- **Cross-reference validation.** `to_secret` re-derives the public
  key id and refuses on mismatch — this catches the most common
  tamper patterns (swap a single field) and surfaces them as
  `CorruptReason::KeyIdMismatch` / `ActorIdMismatch`.
- **Schema versioning at rest.** `kairo.key.private.v1` is checked
  on load; unknown schemas fail with `SchemaMismatch` rather than
  best-effort parsing. Forward-compatible.
- **Algorithm whitelist.** `to_secret` accepts only `ed25519`
  today; any other algorithm string is `UnsupportedAlgorithm`.
  Adding a new algorithm is a deliberate code change, not a
  side-effect of accepting an unknown wire value.
- **`Debug` redacts the seed.** Logging an actor's `SecretSigningKey`
  in error messages won't leak the key.
- **Clear error taxonomy.** `Missing` (semantic) vs `Corrupt`
  (fixity failure) vs `Unavailable` (transient I/O) lets callers
  distinguish "no such key" from "key file is broken" — important
  for the recovery flow where a missing keystore entry triggers
  attestation-key recovery rather than fatal error.
- **No persistent process state.** Each operation opens, reads or
  writes, and closes. There's no in-memory cache of secrets across
  CLI invocations to leak.

## 5. Summary and follow-on actions

| ID  | Severity | Action                                                            |
|-----|----------|-------------------------------------------------------------------|
| 3.1 | Medium   | Open tmp + dirs with explicit mode `0600`/`0700` to close race    |
| 3.2 | Low      | Roll into PHASE_2 §6 multi-process file locks                     |
| 3.3 | Low      | Roll into PHASE_2 §6 multi-process file locks                     |
| 3.4 | Low      | Add `sync_all` + parent-dir fsync; revisit when daemon ships      |
| 3.5 | Medium   | Track separately as "zeroize + memory hygiene" cross-crate pass   |
| 3.6 | Info     | Out of MVP scope; passphrase encryption is documented future work |
| 3.7 | Low      | Sweep stale `.tmp` files on `open`; pair with §3.1                |
| 3.8 | Info     | No action; document the property                                  |
| 3.9 | Info     | No action; document the property                                  |
| 3.10| Info     | No action; defense-in-depth gap accepted by design                |

The headline takeaways:

1. The fixity story is solid — cross-reference checks and schema
   versioning catch every common tamper pattern with structured
   errors.
2. The two findings worth fixing relatively soon are §3.1 (mode-bits
   race) and §3.5 (zeroize-on-drop). Neither is exploitable in the
   single-user MVP scenario; both should land before the keystore
   sees daemon traffic.
3. Multi-process safety (§3.2, §3.3) is already tracked in
   PHASE_2 §6.
4. Encrypted-at-rest is intentionally deferred and documented.

This review doesn't change any code. It exists to ground future
decisions and to make the deferred-by-design choices explicit.
