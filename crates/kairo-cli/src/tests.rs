    use super::*;
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use ed25519_dalek::{Signer, SigningKey};
    use kairo_core::canonical::CanonicalEncode;
    use kairo_identity::json::{ActorGenesisJson, PublicKeyJson};
    use kairo_statement::json::{
        ObjectRevisionBodyJson, ObjectRevisionStatementJson, SignatureJson,
    };

    const ACTOR_ID: &str = "zQmTn1mdQDA1ryQZsiqYfRbqj5DGcG8TNvYcRmBrBLAuk5t";
    const OBJECT_ID: &str = "zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk";
    const MANIFEST: &str = r#"
        [kairo]
        schema = 1
        object = "zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk"
        kind = "software"
        name = "Example"

        [content]
        kind = "tree"

        [[provides]]
        provides = "tool:make"
        version = "3.81"

        [[dependencies]]
        kind = "provides"
        provides = "lib:zlib:static"
    "#;

    #[test]
    fn inspect_output_includes_manifest_details() {
        let manifest = ObjectManifest::parse_toml(MANIFEST);
        let output = manifest.map(|manifest| crate::commands::manifest::format_manifest_inspection(&manifest));

        assert!(
            matches!(output, Ok(output) if output.contains("manifest_hash = z")
            && output.contains("kind = software")
            && output.contains("provides tool:make")
            && output.contains("requires lib:zlib:static"))
        );
    }

    #[test]
    fn parses_manifest_hash_command() {
        let cli = Cli::try_parse_from(["kairo", "manifest", "hash", "custom.toml"]);

        assert!(matches!(
            cli,
            Ok(Cli { store: None, keys: None,
                command: Some(Command::Manifest {
                    command: ManifestCommand::Hash { path }
                })
            }) if path.as_os_str() == "custom.toml"
        ));
    }

    #[test]
    fn parses_manifest_inspect_default_path() {
        let cli = Cli::try_parse_from(["kairo", "manifest", "inspect"]);

        assert!(matches!(
            cli,
            Ok(Cli { store: None, keys: None,
                command: Some(Command::Manifest {
                    command: ManifestCommand::Inspect { path }
                })
            }) if path.as_os_str() == "kairo.toml"
        ));
    }

    #[test]
    fn parses_revision_validate_manifest_command() {
        let cli = Cli::try_parse_from([
            "kairo",
            "revision",
            "validate-manifest",
            "--statement",
            "revision.json",
            "--manifest",
            "kairo.toml",
        ]);

        assert!(matches!(
            cli,
            Ok(Cli { store: None, keys: None,
                command: Some(Command::Revision {
                    command: RevisionCommand::ValidateManifest {
                        statement,
                        manifest
                    }
                })
            }) if statement.as_os_str() == "revision.json" && manifest.as_os_str() == "kairo.toml"
        ));
    }

    #[test]
    fn formats_valid_revision_manifest_output() {
        let output = ObjectManifest::parse_toml(MANIFEST)
            .ok()
            .and_then(|manifest| {
                let dto = revision_dto(manifest.manifest_hash().to_string());
                dto.to_statement().ok().map(|statement| {
                    format_revision_manifest_valid(statement.unsigned().body(), &manifest)
                })
            });

        assert!(
            matches!(output, Some(output) if output.contains("valid revision manifest")
            && output.contains("object = z")
            && output.contains("revision = git:sha256:revision")
            && output.contains("manifest_hash = z"))
        );
    }

    #[test]
    fn parses_revision_verify_signature_command() {
        let cli = Cli::try_parse_from([
            "kairo",
            "revision",
            "verify-signature",
            "--statement",
            "revision.json",
            "--public-key",
            "ZmFrZQ==",
        ]);

        assert!(matches!(
            cli,
            Ok(Cli { store: None, keys: None,
                command: Some(Command::Revision {
                    command: RevisionCommand::VerifySignature {
                        statement,
                        public_key: Some(public_key),
                        public_key_file: None
                    }
                })
            }) if statement.as_os_str() == "revision.json" && public_key == "ZmFrZQ=="
        ));
    }

    #[test]
    fn parses_actor_id_command() {
        let cli = Cli::try_parse_from(["kairo", "actor", "id", "--genesis", "actor.json"]);

        assert!(matches!(
            cli,
            Ok(Cli { store: None, keys: None,
                command: Some(Command::Actor {
                    command: ActorCommand::Id { genesis }
                })
            }) if genesis.as_os_str() == "actor.json"
        ));
    }

    #[test]
    fn parses_revision_verify_actor_genesis_command() {
        let cli = Cli::try_parse_from([
            "kairo",
            "revision",
            "verify-actor-genesis",
            "--statement",
            "revision.json",
            "--actor-genesis",
            "actor.json",
        ]);

        assert!(matches!(
            cli,
            Ok(Cli { store: None, keys: None,
                command: Some(Command::Revision {
                    command: RevisionCommand::VerifyActorGenesis {
                        statement,
                        actor_genesis,
                        json: false,
                    }
                })
            }) if statement.as_os_str() == "revision.json" && actor_genesis.as_os_str() == "actor.json"
        ));
    }

    #[test]
    fn verifies_revision_signature_for_output() {
        let statement = signed_revision_statement();
        let public_key = PublicKey::ed25519(signing_key().verifying_key().to_bytes());
        let output = statement.and_then(|statement| {
            statement.verify_signature(&public_key).ok().map(|_| {
                format_revision_signature_valid(statement.unsigned().body(), statement.signature())
            })
        });

        assert!(
            matches!(output, Some(output) if output.contains("valid revision signature")
            && output.contains("signature = valid")
            && output.contains("key_id = z"))
        );
    }

    #[test]
    fn formats_actor_genesis_verified_revision_output() {
        let actor_genesis = actor_genesis_dto().to_body();
        let output = actor_genesis.ok().and_then(|actor_genesis| {
            let statement =
                signed_revision_statement_for_actor(actor_genesis.actor_id().to_string())?;
            let mut resolver = MemoryActorResolver::new();
            resolver.insert(actor_genesis);
            let report = verify_envelope_statement(&statement, &resolver);
            if report.is_cryptographically_valid() {
                Some(format_verification_report(
                    statement.unsigned().body(),
                    &report,
                ))
            } else {
                None
            }
        });

        assert!(
            matches!(output, Some(output) if output.contains("valid revision actor genesis")
            && output.contains("signature = valid")
            && output.contains("actor_resolution = resolved")
            && output.contains("trust = unevaluated")
            && output.contains("actor = z"))
        );
    }

    #[test]
    fn actor_genesis_id_output_is_actor_id() {
        let output = actor_genesis_dto()
            .to_body()
            .map(|actor_genesis| format!("{}\n", actor_genesis.actor_id()));

        assert!(matches!(output, Ok(output) if output.starts_with('z') && output.ends_with('\n')));
    }

    #[test]
    fn reads_inline_public_key_base64() {
        let encoded = STANDARD.encode(signing_key().verifying_key().to_bytes());
        let public_key = read_public_key(Some(encoded), None);

        assert!(
            matches!(public_key, Ok(public_key) if public_key.bytes() == &signing_key().verifying_key().to_bytes())
        );
    }

    fn revision_dto(manifest_hash: String) -> ObjectRevisionStatementJson {
        revision_dto_for_actor(ACTOR_ID.to_owned(), manifest_hash)
    }

    fn revision_dto_for_actor(
        actor_id: String,
        manifest_hash: String,
    ) -> ObjectRevisionStatementJson {
        ObjectRevisionStatementJson {
            statement_type: "ObjectRevision".to_owned(),
            version: 1,
            actor: actor_id.clone(),
            subject: format!("object:{OBJECT_ID}"),
            created_at: "2026-05-01T14:32:07Z".to_owned(),
            body: ObjectRevisionBodyJson {
                object: OBJECT_ID.to_owned(),
                revision: "git:sha256:revision".to_owned(),
                parents: Vec::new(),
                manifest_hash,
                attests_reachable_history: true,
            },
            signature: SignatureJson {
                actor: actor_id,
                key_id: "primary".to_owned(),
                algorithm: "example".to_owned(),
                bytes: "c2lnbmF0dXJl".to_owned(),
            },
        }
    }

    fn signed_revision_statement() -> Option<kairo_statement::SignedStatement<ObjectRevisionBody>> {
        signed_revision_statement_for_actor(ACTOR_ID.to_owned())
    }

    fn signed_revision_statement_for_actor(
        actor_id: String,
    ) -> Option<kairo_statement::SignedStatement<ObjectRevisionBody>> {
        let manifest = ObjectManifest::parse_toml(MANIFEST).ok()?;
        let mut dto = revision_dto_for_actor(actor_id, manifest.manifest_hash().to_string());
        let unsigned = dto.to_statement().ok()?.unsigned().clone();
        let signature = signing_key().sign(&unsigned.canonical_bytes()).to_bytes();
        dto.signature.algorithm = "ed25519".to_owned();
        dto.signature.key_id = PublicKey::ed25519(signing_key().verifying_key().to_bytes())
            .key_id()
            .to_string();
        dto.signature.bytes = STANDARD.encode(signature);
        dto.to_statement().ok()
    }

    fn actor_genesis_dto() -> ActorGenesisJson {
        ActorGenesisJson {
            statement_type: "ActorGenesis".to_owned(),
            version: 1,
            actor_kind: "person".to_owned(),
            initial_key: PublicKeyJson {
                algorithm: "ed25519".to_owned(),
                bytes: STANDARD.encode(signing_key().verifying_key().to_bytes()),
            },
            attestation_keys: vec![PublicKeyJson {
                algorithm: "ed25519".to_owned(),
                bytes: STANDARD
                    .encode(SigningKey::from_bytes(&[200; 32]).verifying_key().to_bytes()),
            }],
            attestation_threshold: 1,
            created_at: "2026-05-01T14:32:07Z".to_owned(),
            nonce: "0909090909090909090909090909090909090909090909090909090909090909".to_owned(),
        }
    }

    fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&[7; 32])
    }

    #[test]
    fn end_to_end_actor_object_revision_against_tempdir() -> Result<(), Box<dyn std::error::Error>>
    {
        let store_dir = tempfile::TempDir::new()?;
        let manifest_dir = tempfile::TempDir::new()?;
        let manifest_path = manifest_dir.path().join("kairo.toml");
        let bare_manifest = r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "Example"

            [content]
            kind = "tree"
        "#;
        std::fs::write(&manifest_path, bare_manifest)?;

        // 1. Create a fresh actor.
        let actor_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Create {
                    kind: "person".to_owned(),
                    attestation_keys: vec![],
                    generate_attestation_keys: 1,
                    attestation_threshold: 1,
                },
            }),
        })?;
        let actor_id = parse_field(&actor_output, "actor = ")?;

        // 2. Create an object lineage.
        let object_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Object {
                command: ObjectSubcommand::Create {
                    actor: actor_id.clone(),
                    kind: "software".to_owned(),
                    initial_revision: Some("git:sha256:abc".to_owned()),
                },
            }),
        })?;
        let object_id = parse_field(&object_output, "object = ")?;

        // 3. Create a signed revision pointing at that object.
        let revision_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Revision {
                command: RevisionCommand::Create {
                    actor: actor_id.clone(),
                    object: object_id.clone(),
                    revision: "git:sha256:def".to_owned(),
                    manifest: manifest_path.clone(),
                    parents: vec!["git:sha256:abc".to_owned()],
                    no_attests_reachable_history: false,
                },
            }),
        })?;
        assert!(revision_output.contains("created revision"));
        assert!(revision_output.contains(&actor_id));
        assert!(revision_output.contains(&object_id));

        // 4. Read back from the store and verify the signature against the
        //    persisted ActorGenesis through the generic verifier.
        use kairo_statement::verify::ActorResolution;
        let store = open_store(&StorePaths {
            store: store_dir.path().to_path_buf(),
            keys: store_dir.path().join("keys"),
        })?;
        let actor_id_typed = ActorId::new(actor_id)?;
        let _genesis = store.get_actor(&actor_id_typed)?;

        // The revision should be readable by its statement id (we don't have
        // direct access to it here, but the parse_field above pinned the
        // round-trip to a successful write).
        let signed =
            first_statement_on_disk(store_dir.path())?.ok_or("no revision statement on disk")?;
        let report = kairo_statement::verify::verify_envelope_statement(&signed, &store);
        assert_eq!(report.actor, ActorResolution::Resolved);
        assert!(report.is_cryptographically_valid());

        Ok(())
    }

    fn first_statement_on_disk(
        store_root: &std::path::Path,
    ) -> Result<Option<SignedStatement<ObjectRevisionBody>>, Box<dyn std::error::Error>> {
        let statements_dir = store_root.join("statements");
        for level1 in std::fs::read_dir(&statements_dir)? {
            let level1 = level1?;
            for level2 in std::fs::read_dir(level1.path())? {
                let level2 = level2?;
                if let Some(entry) = std::fs::read_dir(level2.path())?.next() {
                    let path = entry?.path();
                    let json: ObjectRevisionStatementJson =
                        serde_json::from_slice(&std::fs::read(&path)?)?;
                    let signed = json.to_statement().map_err(|error| error.to_string())?;
                    return Ok(Some(signed));
                }
            }
        }
        Ok(None)
    }

    fn parse_field(text: &str, prefix: &str) -> Result<String, Box<dyn std::error::Error>> {
        text.lines()
            .find_map(|line| line.strip_prefix(prefix).map(str::to_owned))
            .ok_or_else(|| format!("missing field {prefix:?} in {text:?}").into())
    }

    #[test]
    fn parses_actor_import_command() {
        let cli = Cli::try_parse_from(["kairo", "actor", "import", "--genesis", "actor.json"]);

        assert!(matches!(
            cli,
            Ok(Cli {
                store: None,
                keys: None,
                command: Some(Command::Actor {
                    command: ActorCommand::Import { genesis }
                })
            }) if genesis.as_os_str() == "actor.json"
        ));
    }

    #[test]
    fn parses_object_import_command() {
        let cli = Cli::try_parse_from(["kairo", "object", "import", "--statement", "obj.json"]);

        assert!(matches!(
            cli,
            Ok(Cli {
                store: None,
                keys: None,
                command: Some(Command::Object {
                    command: ObjectSubcommand::Import { statement }
                })
            }) if statement.as_os_str() == "obj.json"
        ));
    }

    #[test]
    fn parses_revision_import_command() {
        let cli = Cli::try_parse_from(["kairo", "revision", "import", "--statement", "r.json"]);

        assert!(matches!(
            cli,
            Ok(Cli {
                store: None,
                keys: None,
                command: Some(Command::Revision {
                    command: RevisionCommand::Import { statement }
                })
            }) if statement.as_os_str() == "r.json"
        ));
    }

    #[test]
    fn parses_revision_inspect_command() {
        let cli = Cli::try_parse_from([
            "kairo",
            "revision",
            "inspect",
            "--statement",
            "zQmStatement",
            "--json",
        ]);

        assert!(matches!(
            cli,
            Ok(Cli {
                store: None,
                keys: None,
                command: Some(Command::Revision {
                    command: RevisionCommand::Inspect {
                        statement,
                        json: true,
                    }
                })
            }) if statement == "zQmStatement"
        ));
    }

    #[test]
    fn parses_revision_list_command() {
        let cli = Cli::try_parse_from(["kairo", "revision", "list", "--object", "zQmObject"]);

        assert!(matches!(
            cli,
            Ok(Cli {
                store: None,
                keys: None,
                command: Some(Command::Revision {
                    command: RevisionCommand::List { object }
                })
            }) if object == "zQmObject"
        ));
    }

    #[test]
    fn parses_revision_verify_actor_genesis_with_json() {
        let cli = Cli::try_parse_from([
            "kairo",
            "revision",
            "verify-actor-genesis",
            "--statement",
            "r.json",
            "--actor-genesis",
            "a.json",
            "--json",
        ]);

        assert!(matches!(
            cli,
            Ok(Cli {
                store: None,
                keys: None,
                command: Some(Command::Revision {
                    command: RevisionCommand::VerifyActorGenesis {
                        statement: _,
                        actor_genesis: _,
                        json: true,
                    }
                })
            })
        ));
    }

    #[test]
    fn end_to_end_import_inspect_list() -> Result<(), Box<dyn std::error::Error>> {
        // Drive the create flow into a temp store, then use the import / inspect /
        // list commands to round-trip.
        let store_dir = tempfile::TempDir::new()?;
        let other_dir = tempfile::TempDir::new()?;
        let manifest_dir = tempfile::TempDir::new()?;
        let manifest_path = manifest_dir.path().join("kairo.toml");
        let bare_manifest = r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "Example"

            [content]
            kind = "tree"
        "#;
        std::fs::write(&manifest_path, bare_manifest)?;

        // 1. Create actor + object + revision in store_dir.
        let actor_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Create {
                    kind: "person".to_owned(),
                    attestation_keys: vec![],
                    generate_attestation_keys: 1,
                    attestation_threshold: 1,
                },
            }),
        })?;
        let actor_id = parse_field(&actor_output, "actor = ")?;
        let object_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Object {
                command: ObjectSubcommand::Create {
                    actor: actor_id.clone(),
                    kind: "software".to_owned(),
                    initial_revision: None,
                },
            }),
        })?;
        let object_id = parse_field(&object_output, "object = ")?;
        let revision_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Revision {
                command: RevisionCommand::Create {
                    actor: actor_id.clone(),
                    object: object_id.clone(),
                    revision: "git:sha256:def".to_owned(),
                    manifest: manifest_path.clone(),
                    parents: vec![],
                    no_attests_reachable_history: false,
                },
            }),
        })?;
        let statement_id = parse_field(&revision_output, "statement = ")?;

        // 2. Re-find the on-disk JSONs in store_dir's actors/objects/statements
        //    directories so we can re-import them into a fresh store.
        let actor_json = find_one(&store_dir.path().join("actors"), "json")?;
        let object_json = find_one(&store_dir.path().join("objects"), "json")?;
        let statement_json = find_one(&store_dir.path().join("statements"), "json")?;

        // 3. Import them all into a fresh store via the CLI.
        let imported_actor = run(Cli {
            store: Some(other_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Import {
                    genesis: actor_json,
                },
            }),
        })?;
        assert!(imported_actor.contains("imported actor"));
        assert!(imported_actor.contains(&actor_id));

        let imported_object = run(Cli {
            store: Some(other_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Object {
                command: ObjectSubcommand::Import {
                    statement: object_json,
                },
            }),
        })?;
        assert!(imported_object.contains("imported object genesis"));
        assert!(imported_object.contains(&object_id));

        let imported_revision = run(Cli {
            store: Some(other_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Revision {
                command: RevisionCommand::Import {
                    statement: statement_json,
                },
            }),
        })?;
        assert!(imported_revision.contains("imported revision"));
        assert!(imported_revision.contains(&statement_id));

        // 4. Inspect the revision in the new store.
        let inspect_text = run(Cli {
            store: Some(other_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Revision {
                command: RevisionCommand::Inspect {
                    statement: statement_id.clone(),
                    json: false,
                },
            }),
        })?;
        assert!(inspect_text.contains(&statement_id));
        assert!(inspect_text.contains(&object_id));
        assert!(inspect_text.contains("revision = git:sha256:def"));

        let inspect_json = run(Cli {
            store: Some(other_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Revision {
                command: RevisionCommand::Inspect {
                    statement: statement_id.clone(),
                    json: true,
                },
            }),
        })?;
        let parsed: serde_json::Value = serde_json::from_str(&inspect_json)?;
        assert_eq!(parsed["statement_id"], statement_id);
        assert_eq!(parsed["object"], object_id);
        assert_eq!(parsed["revision"], "git:sha256:def");

        // 5. List revisions for that object.
        let list_text = run(Cli {
            store: Some(other_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Revision {
                command: RevisionCommand::List {
                    object: object_id.clone(),
                },
            }),
        })?;
        assert!(list_text.contains("revisions = 1"));
        assert!(list_text.contains(&statement_id));

        Ok(())
    }

    fn find_one(
        root: &std::path::Path,
        extension: &str,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        for level1 in std::fs::read_dir(root)? {
            let level1 = level1?;
            if !level1.path().is_dir() {
                continue;
            }
            for level2 in std::fs::read_dir(level1.path())? {
                let level2 = level2?;
                if !level2.path().is_dir() {
                    continue;
                }
                if let Some(entry) = std::fs::read_dir(level2.path())?.next() {
                    let path = entry?.path();
                    if path.extension().and_then(|s| s.to_str()) == Some(extension) {
                        return Ok(path);
                    }
                }
            }
        }
        Err(format!("no {extension} file found under {}", root.display()).into())
    }

    #[test]
    fn parses_branch_set_default_name() {
        let cli = Cli::try_parse_from([
            "kairo",
            "branch",
            "set",
            "--actor",
            "zActor",
            "--object",
            "zObject",
            "--revision",
            "zRev",
        ]);

        assert!(matches!(
            cli,
            Ok(Cli {
                store: None,
                keys: None,
                command: Some(Command::Branch {
                    command: BranchCommand::Set { name, .. }
                })
            }) if name == "head"
        ));
    }

    #[test]
    fn parses_branch_set_with_explicit_name() {
        let cli = Cli::try_parse_from([
            "kairo",
            "branch",
            "set",
            "--actor",
            "zActor",
            "--object",
            "zObject",
            "--revision",
            "zRev",
            "--name",
            "release",
        ]);

        assert!(matches!(
            cli,
            Ok(Cli {
                store: None,
                keys: None,
                command: Some(Command::Branch {
                    command: BranchCommand::Set { name, .. }
                })
            }) if name == "release"
        ));
    }

    #[test]
    fn parses_branch_show_defaults_to_head() {
        let cli = Cli::try_parse_from(["kairo", "branch", "show", "--object", "zObject"]);

        assert!(matches!(
            cli,
            Ok(Cli {
                store: None,
                keys: None,
                command: Some(Command::Branch {
                    command: BranchCommand::Show {
                        actor: None,
                        name,
                        json: false,
                        ..
                    }
                })
            }) if name == "head"
        ));
    }

    #[test]
    fn parses_branch_list_command() {
        let cli = Cli::try_parse_from(["kairo", "branch", "list", "--object", "zObject"]);

        assert!(matches!(
            cli,
            Ok(Cli {
                store: None,
                keys: None,
                command: Some(Command::Branch {
                    command: BranchCommand::List { object }
                })
            }) if object == "zObject"
        ));
    }

    #[test]
    fn end_to_end_branch_set_show_list() -> Result<(), Box<dyn std::error::Error>> {
        // Drive create + revision into a temp store, then exercise branch
        // set / show / list and prove that supersession moves the index.
        let store_dir = tempfile::TempDir::new()?;
        let manifest_dir = tempfile::TempDir::new()?;
        let manifest_path = manifest_dir.path().join("kairo.toml");
        std::fs::write(
            &manifest_path,
            r#"[kairo]
schema = 1
kind = "software"
name = "Example"

[content]
kind = "tree"
"#,
        )?;

        let actor_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Create {
                    kind: "person".to_owned(),
                    attestation_keys: vec![],
                    generate_attestation_keys: 1,
                    attestation_threshold: 1,
                },
            }),
        })?;
        let actor_id = parse_field(&actor_output, "actor = ")?;
        let object_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Object {
                command: ObjectSubcommand::Create {
                    actor: actor_id.clone(),
                    kind: "software".to_owned(),
                    initial_revision: None,
                },
            }),
        })?;
        let object_id = parse_field(&object_output, "object = ")?;

        // Two revisions on the same object so we can supersede a branch tip.
        let r1 = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Revision {
                command: RevisionCommand::Create {
                    actor: actor_id.clone(),
                    object: object_id.clone(),
                    revision: "git:sha256:r1".to_owned(),
                    manifest: manifest_path.clone(),
                    parents: vec![],
                    no_attests_reachable_history: false,
                },
            }),
        })?;
        let r1_statement = parse_field(&r1, "statement = ")?;

        // Force a strictly greater created_at by pausing briefly. Timestamp
        // resolution is whole seconds, so wait one full second to guarantee
        // strict supersession.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let r2 = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Revision {
                command: RevisionCommand::Create {
                    actor: actor_id.clone(),
                    object: object_id.clone(),
                    revision: "git:sha256:r2".to_owned(),
                    manifest: manifest_path.clone(),
                    parents: vec!["git:sha256:r1".to_owned()],
                    no_attests_reachable_history: false,
                },
            }),
        })?;
        let r2_statement = parse_field(&r2, "statement = ")?;
        assert_ne!(r1_statement, r2_statement);

        // Set head to r1.
        let set_r1 = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Branch {
                command: BranchCommand::Set {
                    actor: actor_id.clone(),
                    object: object_id.clone(),
                    revision: r1_statement.clone(),
                    name: "head".to_owned(),
                },
            }),
        })?;
        assert!(set_r1.contains("set branch"));
        assert!(set_r1.contains(&r1_statement));

        // Show should currently return r1.
        let show_r1 = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Branch {
                command: BranchCommand::Show {
                    object: object_id.clone(),
                    actor: None,
                    name: "head".to_owned(),
                    json: false,
                },
            }),
        })?;
        assert!(show_r1.contains(&r1_statement));

        // Pause again so the supersession ordering is unambiguous at
        // whole-second granularity.
        std::thread::sleep(std::time::Duration::from_millis(1100));

        // Advance head to r2.
        run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Branch {
                command: BranchCommand::Set {
                    actor: actor_id.clone(),
                    object: object_id.clone(),
                    revision: r2_statement.clone(),
                    name: "head".to_owned(),
                },
            }),
        })?;

        // Show should now return r2.
        let show_r2 = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Branch {
                command: BranchCommand::Show {
                    object: object_id.clone(),
                    actor: None,
                    name: "head".to_owned(),
                    json: true,
                },
            }),
        })?;
        let parsed: serde_json::Value = serde_json::from_str(&show_r2)?;
        assert_eq!(parsed["revision"], r2_statement);

        // List should report exactly one branch tip for the object.
        let list_text = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Branch {
                command: BranchCommand::List {
                    object: object_id.clone(),
                },
            }),
        })?;
        assert!(list_text.contains("branches = 1"));
        assert!(list_text.contains("name=head"));

        Ok(())
    }

    #[test]
    fn branch_set_rejects_revision_for_wrong_object() -> Result<(), Box<dyn std::error::Error>> {
        // Set up two distinct objects and try to point object A's branch at
        // a revision that binds to object B. The branch set command must
        // fail rather than create a dangling pointer.
        let store_dir = tempfile::TempDir::new()?;
        let manifest_dir = tempfile::TempDir::new()?;
        let manifest_path = manifest_dir.path().join("kairo.toml");
        std::fs::write(
            &manifest_path,
            r#"[kairo]
schema = 1
kind = "software"
name = "Example"

[content]
kind = "tree"
"#,
        )?;

        let actor_id = parse_field(
            &run(Cli {
                store: Some(store_dir.path().to_path_buf()),
                keys: None,
                command: Some(Command::Actor {
                    command: ActorCommand::Create {
                        kind: "person".to_owned(),
                        attestation_keys: vec![],
                        generate_attestation_keys: 1,
                        attestation_threshold: 1,
                    },
                }),
            })?,
            "actor = ",
        )?;

        let object_a = parse_field(
            &run(Cli {
                store: Some(store_dir.path().to_path_buf()),
                keys: None,
                command: Some(Command::Object {
                    command: ObjectSubcommand::Create {
                        actor: actor_id.clone(),
                        kind: "software".to_owned(),
                        initial_revision: None,
                    },
                }),
            })?,
            "object = ",
        )?;
        let object_b = parse_field(
            &run(Cli {
                store: Some(store_dir.path().to_path_buf()),
                keys: None,
                command: Some(Command::Object {
                    command: ObjectSubcommand::Create {
                        actor: actor_id.clone(),
                        kind: "software".to_owned(),
                        initial_revision: Some("git:sha256:bootstrap".to_owned()),
                    },
                }),
            })?,
            "object = ",
        )?;
        assert_ne!(object_a, object_b);

        let r_b = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Revision {
                command: RevisionCommand::Create {
                    actor: actor_id.clone(),
                    object: object_b.clone(),
                    revision: "git:sha256:rb".to_owned(),
                    manifest: manifest_path.clone(),
                    parents: vec![],
                    no_attests_reachable_history: false,
                },
            }),
        })?;
        let r_b_statement = parse_field(&r_b, "statement = ")?;

        let result = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Branch {
                command: BranchCommand::Set {
                    actor: actor_id,
                    object: object_a,
                    revision: r_b_statement,
                    name: "head".to_owned(),
                },
            }),
        });

        assert!(matches!(result, Err(CliError::BranchObjectMismatch { .. })));
        Ok(())
    }

    #[test]
    fn parses_snapshot_compute_defaults() {
        let cli = Cli::try_parse_from(["kairo", "snapshot", "compute", "--object", "zObject"]);

        assert!(matches!(
            cli,
            Ok(Cli {
                store: None,
                keys: None,
                command: Some(Command::Snapshot {
                    command: SnapshotCommand::Compute {
                        statement: None,
                        actor: None,
                        name,
                        json: false,
                        ..
                    }
                })
            }) if name == "head"
        ));
    }

    #[test]
    fn parses_snapshot_compute_with_pinned_statement() {
        let cli = Cli::try_parse_from([
            "kairo",
            "snapshot",
            "compute",
            "--object",
            "zObject",
            "--statement",
            "zStatement",
            "--json",
        ]);

        assert!(matches!(
            cli,
            Ok(Cli {
                store: None,
                keys: None,
                command: Some(Command::Snapshot {
                    command: SnapshotCommand::Compute {
                        statement: Some(stmt),
                        json: true,
                        ..
                    }
                })
            }) if stmt == "zStatement"
        ));
    }

    #[test]
    fn snapshot_compute_with_pinned_statement_and_actor_conflicts() {
        // --statement conflicts with --actor and --name (which would
        // otherwise route through branch resolution).
        let cli = Cli::try_parse_from([
            "kairo",
            "snapshot",
            "compute",
            "--object",
            "zObject",
            "--statement",
            "zStatement",
            "--actor",
            "zActor",
        ]);

        assert!(cli.is_err());
    }

    #[test]
    fn end_to_end_snapshot_via_branch() -> Result<(), Box<dyn std::error::Error>> {
        let store_dir = tempfile::TempDir::new()?;
        let manifest_dir = tempfile::TempDir::new()?;
        let manifest_path = manifest_dir.path().join("kairo.toml");
        std::fs::write(
            &manifest_path,
            r#"[kairo]
schema = 1
kind = "software"
name = "Example"

[content]
kind = "tree"
"#,
        )?;

        let actor_id = parse_field(
            &run(Cli {
                store: Some(store_dir.path().to_path_buf()),
                keys: None,
                command: Some(Command::Actor {
                    command: ActorCommand::Create {
                        kind: "person".to_owned(),
                        attestation_keys: vec![],
                        generate_attestation_keys: 1,
                        attestation_threshold: 1,
                    },
                }),
            })?,
            "actor = ",
        )?;
        let object_id = parse_field(
            &run(Cli {
                store: Some(store_dir.path().to_path_buf()),
                keys: None,
                command: Some(Command::Object {
                    command: ObjectSubcommand::Create {
                        actor: actor_id.clone(),
                        kind: "software".to_owned(),
                        initial_revision: None,
                    },
                }),
            })?,
            "object = ",
        )?;
        let revision_statement = parse_field(
            &run(Cli {
                store: Some(store_dir.path().to_path_buf()),
                keys: None,
                command: Some(Command::Revision {
                    command: RevisionCommand::Create {
                        actor: actor_id.clone(),
                        object: object_id.clone(),
                        revision: "git:sha256:def".to_owned(),
                        manifest: manifest_path.clone(),
                        parents: vec![],
                        no_attests_reachable_history: false,
                    },
                }),
            })?,
            "statement = ",
        )?;

        // No branch set yet — snapshot must fail with BranchNotFound.
        let no_branch = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Snapshot {
                command: SnapshotCommand::Compute {
                    object: object_id.clone(),
                    statement: None,
                    actor: None,
                    name: "head".to_owned(),
                    json: false,
                },
            }),
        });
        assert!(matches!(no_branch, Err(CliError::BranchNotFound { .. })));

        // Pinning the statement directly should work without a branch.
        let pinned = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Snapshot {
                command: SnapshotCommand::Compute {
                    object: object_id.clone(),
                    statement: Some(revision_statement.clone()),
                    actor: None,
                    name: "head".to_owned(),
                    json: true,
                },
            }),
        })?;
        let pinned_json: serde_json::Value = serde_json::from_str(&pinned)?;
        assert_eq!(pinned_json["object"], object_id);
        assert_eq!(pinned_json["revision"], "git:sha256:def");
        let pinned_id = pinned_json["snapshot_id"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        assert!(pinned_id.starts_with('z'));

        // Set head to point at the revision so default-resolution works too.
        run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Branch {
                command: BranchCommand::Set {
                    actor: actor_id.clone(),
                    object: object_id.clone(),
                    revision: revision_statement.clone(),
                    name: "head".to_owned(),
                },
            }),
        })?;

        // Default-resolved snapshot should produce the same id as pinning.
        let resolved = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Snapshot {
                command: SnapshotCommand::Compute {
                    object: object_id.clone(),
                    statement: None,
                    actor: None,
                    name: "head".to_owned(),
                    json: true,
                },
            }),
        })?;
        let resolved_json: serde_json::Value = serde_json::from_str(&resolved)?;
        assert_eq!(
            resolved_json["snapshot_id"].as_str(),
            Some(pinned_id.as_str())
        );

        // Human-readable form contains the snapshot id and frontier.
        let human = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Snapshot {
                command: SnapshotCommand::Compute {
                    object: object_id,
                    statement: None,
                    actor: None,
                    name: "head".to_owned(),
                    json: false,
                },
            }),
        })?;
        assert!(human.contains(&pinned_id));
        assert!(human.contains("revision = git:sha256:def"));
        assert!(human.contains("frontier = 1"));
        assert!(human.contains(&revision_statement));

        Ok(())
    }

    /// Built fixture: store dir + manifest dir (held for lifetime),
    /// then actor id, object id, revision statement id, manifest path.
    type VerifyFixture = (
        tempfile::TempDir,
        tempfile::TempDir,
        String,
        String,
        String,
        PathBuf,
    );

    /// Build a temp store with one actor, one object, one signed
    /// revision, and a `head` branch pointing at the revision.
    fn fixture_with_branch() -> Result<VerifyFixture, Box<dyn std::error::Error>> {
        let store_dir = tempfile::TempDir::new()?;
        let manifest_dir = tempfile::TempDir::new()?;
        let manifest_path = manifest_dir.path().join("kairo.toml");
        std::fs::write(
            &manifest_path,
            r#"
                [kairo]
                schema = 1
                kind = "software"
                name = "verify-fixture"

                [content]
                kind = "tree"
            "#,
        )?;

        let actor_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Create {
                    kind: "person".to_owned(),
                    attestation_keys: vec![],
                    generate_attestation_keys: 1,
                    attestation_threshold: 1,
                },
            }),
        })?;
        let actor_id = parse_field(&actor_output, "actor = ")?;

        let object_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Object {
                command: ObjectSubcommand::Create {
                    actor: actor_id.clone(),
                    kind: "software".to_owned(),
                    initial_revision: None,
                },
            }),
        })?;
        let object_id = parse_field(&object_output, "object = ")?;

        let revision_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Revision {
                command: RevisionCommand::Create {
                    actor: actor_id.clone(),
                    object: object_id.clone(),
                    revision: "git:sha256:r1".to_owned(),
                    manifest: manifest_path.clone(),
                    parents: vec![],
                    no_attests_reachable_history: false,
                },
            }),
        })?;
        let revision_statement = parse_field(&revision_output, "statement = ")?;

        run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Branch {
                command: BranchCommand::Set {
                    actor: actor_id.clone(),
                    object: object_id.clone(),
                    revision: revision_statement.clone(),
                    name: "head".to_owned(),
                },
            }),
        })?;

        Ok((
            store_dir,
            manifest_dir,
            actor_id,
            object_id,
            revision_statement,
            manifest_path,
        ))
    }

    #[test]
    fn verify_object_happy_path_with_manifest_is_indeterminate(
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Content-layer check is always Indeterminate today (TODO §11);
        // until then the strongest reachable verdict is INDETERMINATE.
        let (store_dir, _manifest_dir, _actor_id, object_id, _revision_statement, manifest_path) =
            fixture_with_branch()?;

        let output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Verify {
                command: VerifyCommand::Object {
                    object: object_id.clone(),
                    statement: None,
                    actor: None,
                    name: "head".to_owned(),
                    repo: None,
                    no_repo: true,
                    no_cache: false,
                    no_cwd_repo: false,
                    r#as: None,
                    no_as: true,
                    manifest: Some(manifest_path),
                    json: false,
                },
            }),
        })?;
        assert!(output.contains("verify object: INDETERMINATE"));
        assert!(output.contains("signature = valid"));
        assert!(output.contains("manifest_binding = VALID (bound)"));
        assert!(output.contains("content = INDETERMINATE"));
        assert!(output.contains(&object_id));
        Ok(())
    }

    #[test]
    fn verify_object_without_manifest_marks_binding_indeterminate(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (store_dir, _manifest_dir, _actor_id, object_id, _revision_statement, _manifest_path) =
            fixture_with_branch()?;

        let output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Verify {
                command: VerifyCommand::Object {
                    object: object_id,
                    statement: None,
                    actor: None,
                    name: "head".to_owned(),
                    repo: None,
                    no_repo: true,
                    no_cache: false,
                    no_cwd_repo: false,
                    r#as: None,
                    no_as: true,
                    manifest: None,
                    json: false,
                },
            }),
        })?;
        assert!(output.contains("verify object: INDETERMINATE"));
        assert!(output.contains("manifest_binding = INDETERMINATE (no manifest provided)"));
        Ok(())
    }

    #[test]
    fn verify_object_with_pinned_statement_uses_pinned_frontier(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (store_dir, _manifest_dir, _actor_id, object_id, revision_statement, manifest_path) =
            fixture_with_branch()?;

        let output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Verify {
                command: VerifyCommand::Object {
                    object: object_id,
                    statement: Some(revision_statement.clone()),
                    actor: None,
                    name: "head".to_owned(),
                    repo: None,
                    no_repo: true,
                    no_cache: false,
                    no_cwd_repo: false,
                    r#as: None,
                    no_as: true,
                    manifest: Some(manifest_path),
                    json: false,
                },
            }),
        })?;
        assert!(output.contains("frontier: pinned statement="));
        assert!(output.contains(&revision_statement));
        Ok(())
    }

    #[test]
    fn verify_object_with_wrong_manifest_is_invalid_and_errors(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (store_dir, manifest_dir, _actor_id, object_id, _revision_statement, _manifest_path) =
            fixture_with_branch()?;

        let wrong_manifest = manifest_dir.path().join("wrong.toml");
        std::fs::write(
            &wrong_manifest,
            r#"
                [kairo]
                schema = 1
                kind = "software"
                name = "different"

                [content]
                kind = "tree"
            "#,
        )?;

        let result = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Verify {
                command: VerifyCommand::Object {
                    object: object_id,
                    statement: None,
                    actor: None,
                    name: "head".to_owned(),
                    repo: None,
                    no_repo: true,
                    no_cache: false,
                    no_cwd_repo: false,
                    r#as: None,
                    no_as: true,
                    manifest: Some(wrong_manifest),
                    json: false,
                },
            }),
        });
        assert!(matches!(
            result,
            Err(CliError::ObjectVerificationFailed(report))
                if report.contains("INVALID") && report.contains("hash mismatch")
        ));
        Ok(())
    }

    #[test]
    fn verify_object_json_output_is_well_formed() -> Result<(), Box<dyn std::error::Error>> {
        let (store_dir, _manifest_dir, _actor_id, object_id, revision_statement, manifest_path) =
            fixture_with_branch()?;

        let json = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Verify {
                command: VerifyCommand::Object {
                    object: object_id.clone(),
                    statement: None,
                    actor: None,
                    name: "head".to_owned(),
                    repo: None,
                    no_repo: true,
                    no_cache: false,
                    no_cwd_repo: false,
                    r#as: None,
                    no_as: true,
                    manifest: Some(manifest_path),
                    json: true,
                },
            }),
        })?;
        let value: serde_json::Value = serde_json::from_str(&json)?;
        assert_eq!(value["overall"].as_str(), Some("INDETERMINATE"));
        assert_eq!(value["object"].as_str(), Some(object_id.as_str()));
        assert_eq!(value["frontier"]["kind"].as_str(), Some("branch"));
        assert_eq!(
            value["revision"]["statement_id"].as_str(),
            Some(revision_statement.as_str())
        );
        assert_eq!(value["revision"]["signature"].as_str(), Some("valid"));
        assert_eq!(
            value["revision"]["manifest_binding"]["status"].as_str(),
            Some("bound")
        );
        Ok(())
    }

    #[test]
    fn verify_object_branch_not_found_returns_error() -> Result<(), Box<dyn std::error::Error>> {
        // Build a fixture without a branch.
        let store_dir = tempfile::TempDir::new()?;
        let actor_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Create {
                    kind: "person".to_owned(),
                    attestation_keys: vec![],
                    generate_attestation_keys: 1,
                    attestation_threshold: 1,
                },
            }),
        })?;
        let actor_id = parse_field(&actor_output, "actor = ")?;
        let object_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Object {
                command: ObjectSubcommand::Create {
                    actor: actor_id,
                    kind: "software".to_owned(),
                    initial_revision: None,
                },
            }),
        })?;
        let object_id = parse_field(&object_output, "object = ")?;

        let result = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Verify {
                command: VerifyCommand::Object {
                    object: object_id,
                    statement: None,
                    actor: None,
                    name: "head".to_owned(),
                    repo: None,
                    no_repo: true,
                    no_cache: false,
                    no_cwd_repo: false,
                    r#as: None,
                    no_as: true,
                    manifest: None,
                    json: false,
                },
            }),
        });
        assert!(matches!(result, Err(CliError::BranchNotFound { .. })));
        Ok(())
    }

    #[test]
    fn parses_verify_object_command() {
        let cli = Cli::try_parse_from([
            "kairo",
            "verify",
            "object",
            "--object",
            "zQmObject",
            "--manifest",
            "kairo.toml",
            "--json",
        ]);
        assert!(matches!(
            cli,
            Ok(Cli {
                command: Some(Command::Verify {
                    command: VerifyCommand::Object {
                        object,
                        manifest: Some(manifest),
                        json: true,
                        statement: None,
                        actor: None,
                        repo: None,
                        no_repo: false,
                        r#as: None,
                        no_as: false,
                        name,
                        ..
                    }
                }),
                ..
            }) if object == "zQmObject" && manifest.as_os_str() == "kairo.toml" && name == "head"
        ));
    }

    /// Init a Git repo, commit a kairo.toml that matches `manifest_text`,
    /// and return (tempdir, commit_oid).
    fn init_git_repo_with_manifest(
        manifest_text: &str,
    ) -> Result<(tempfile::TempDir, String), Box<dyn std::error::Error>> {
        use std::process::Command as Process;
        let dir = tempfile::TempDir::new()?;
        let run_git = |args: &[&str]| -> Result<(), Box<dyn std::error::Error>> {
            let status = Process::new("git")
                .current_dir(dir.path())
                .args(args)
                .status()?;
            if !status.success() {
                return Err(format!("git {args:?} failed").into());
            }
            Ok(())
        };
        run_git(&["init", "--initial-branch=main", "--quiet"])?;
        run_git(&["config", "user.name", "Kairo Test"])?;
        run_git(&["config", "user.email", "test@kairo.test"])?;
        run_git(&["config", "commit.gpgsign", "false"])?;
        std::fs::write(dir.path().join("kairo.toml"), manifest_text)?;
        run_git(&["add", "kairo.toml"])?;
        run_git(&["commit", "-m", "first", "--quiet"])?;
        let output = Process::new("git")
            .current_dir(dir.path())
            .args(["rev-parse", "HEAD"])
            .output()?;
        if !output.status.success() {
            return Err("rev-parse failed".into());
        }
        let oid = String::from_utf8(output.stdout)?.trim().to_owned();
        Ok((dir, oid))
    }

    #[test]
    fn verify_object_with_real_git_repo_can_reach_valid(
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Build a fixture where the revision's storage commit really
        // exists in a Git repo and its tree's kairo.toml matches what
        // the revision was signed against. With everything available,
        // overall must be VALID.
        let manifest_text = r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "git-fixture"

            [content]
            kind = "tree"
        "#;
        let (git_dir, commit_oid) = init_git_repo_with_manifest(manifest_text)?;
        let store_dir = tempfile::TempDir::new()?;

        // Use the same kairo.toml content the commit holds, so the
        // manifest_hash signed into the revision matches the tree
        // content the verifier reads back.
        let manifest_path = git_dir.path().join("kairo.toml");

        let actor_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Create {
                    kind: "person".to_owned(),
                    attestation_keys: vec![],
                    generate_attestation_keys: 1,
                    attestation_threshold: 1,
                },
            }),
        })?;
        let actor_id = parse_field(&actor_output, "actor = ")?;
        let object_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Object {
                command: ObjectSubcommand::Create {
                    actor: actor_id.clone(),
                    kind: "software".to_owned(),
                    initial_revision: None,
                },
            }),
        })?;
        let object_id = parse_field(&object_output, "object = ")?;

        let revision_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Revision {
                command: RevisionCommand::Create {
                    actor: actor_id.clone(),
                    object: object_id.clone(),
                    revision: format!("git:sha256:{commit_oid}"),
                    manifest: manifest_path,
                    parents: vec![],
                    no_attests_reachable_history: false,
                },
            }),
        })?;
        let revision_statement = parse_field(&revision_output, "statement = ")?;
        run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Branch {
                command: BranchCommand::Set {
                    actor: actor_id,
                    object: object_id.clone(),
                    revision: revision_statement,
                    name: "head".to_owned(),
                },
            }),
        })?;

        // Verify with --repo pointing at the real git repo. No
        // --manifest — the verifier must read kairo.toml from the
        // commit's tree itself.
        let output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Verify {
                command: VerifyCommand::Object {
                    object: object_id,
                    statement: None,
                    actor: None,
                    name: "head".to_owned(),
                    repo: Some(git_dir.path().to_path_buf()),
                    no_repo: false,
                    no_cache: false,
                    no_cwd_repo: false,
                    r#as: None,
                    no_as: true,
                    manifest: None,
                    json: false,
                },
            }),
        })?;
        assert!(
            output.contains("verify object: VALID"),
            "expected VALID, got:\n{output}"
        );
        assert!(output.contains("content = VALID"));
        assert!(output.contains("manifest_binding = VALID (bound)"));
        assert!(output.contains(&format!("manifest_source = git:sha256:{commit_oid}/kairo.toml")));
        // Default precedence: cwd-discovered repo (no cache populated).
        assert!(
            output.contains("commit lookup: repo at "),
            "expected cwd repo lookup, got:\n{output}"
        );
        Ok(())
    }

    /// Build a Git pack containing every commit reachable in `src`,
    /// returning the raw bytes. Mirrors `kairo_git::test_support::
    /// build_pack_from`, duplicated here because that helper is
    /// crate-private.
    fn build_pack_from(src: &std::path::Path) -> Vec<u8> {
        use std::io::Read;
        use std::process::Command as Process;
        use std::process::Stdio;
        let mut child = Process::new("git")
            .arg("-C")
            .arg(src)
            .args(["pack-objects", "--all", "--stdout"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn pack-objects");
        let mut buf = Vec::new();
        child
            .stdout
            .as_mut()
            .expect("stdout")
            .read_to_end(&mut buf)
            .expect("read stdout");
        let status = child.wait().expect("wait");
        assert!(status.success(), "pack-objects failed");
        buf
    }

    /// Populate the managed Git cache under `<store>/git/` with the
    /// commits from `src_repo` and pin `commit_oid` at
    /// `refs/heads/main` for `object_id`. Used by the cache-
    /// integration verify tests below.
    fn populate_cache(
        store_root: &std::path::Path,
        src_repo: &std::path::Path,
        object_id: &str,
        commit_oid: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let pack = build_pack_from(src_repo);
        let cache = kairo_git::GitCache::open(store_root.join("git"))?;
        cache.ingest_pack(&pack)?;
        cache.set_ref(object_id, "refs/heads/main", commit_oid)?;
        Ok(())
    }

    #[test]
    fn verify_object_uses_cache_with_no_cwd_repo() -> Result<(), Box<dyn std::error::Error>> {
        // Cache populated, --no-cwd-repo set so cwd discovery is
        // suppressed even though the source repo's tempdir would
        // otherwise be discoverable. Verify reaches VALID against
        // the cache alone, and the report says so.
        let manifest_text = r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "cache-fixture"

            [content]
            kind = "tree"
        "#;
        let (git_dir, commit_oid) = init_git_repo_with_manifest(manifest_text)?;
        let store_dir = tempfile::TempDir::new()?;
        let manifest_path = git_dir.path().join("kairo.toml");

        let actor_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Create {
                    kind: "person".to_owned(),
                    attestation_keys: vec![],
                    generate_attestation_keys: 1,
                    attestation_threshold: 1,
                },
            }),
        })?;
        let actor_id = parse_field(&actor_output, "actor = ")?;
        let object_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Object {
                command: ObjectSubcommand::Create {
                    actor: actor_id.clone(),
                    kind: "software".to_owned(),
                    initial_revision: None,
                },
            }),
        })?;
        let object_id = parse_field(&object_output, "object = ")?;
        let revision_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Revision {
                command: RevisionCommand::Create {
                    actor: actor_id.clone(),
                    object: object_id.clone(),
                    revision: format!("git:sha256:{commit_oid}"),
                    manifest: manifest_path,
                    parents: vec![],
                    no_attests_reachable_history: false,
                },
            }),
        })?;
        let revision_statement = parse_field(&revision_output, "statement = ")?;
        run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Branch {
                command: BranchCommand::Set {
                    actor: actor_id,
                    object: object_id.clone(),
                    revision: revision_statement,
                    name: "head".to_owned(),
                },
            }),
        })?;

        // Populate cache and pin the commit.
        populate_cache(store_dir.path(), git_dir.path(), &object_id, &commit_oid)?;

        // Verify with --no-cwd-repo: cache is the only source.
        let output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Verify {
                command: VerifyCommand::Object {
                    object: object_id.clone(),
                    statement: None,
                    actor: None,
                    name: "head".to_owned(),
                    repo: None,
                    no_repo: false,
                    no_cache: false,
                    no_cwd_repo: true,
                    r#as: None,
                    no_as: true,
                    manifest: None,
                    json: false,
                },
            }),
        })?;
        assert!(
            output.contains("verify object: VALID"),
            "expected VALID, got:\n{output}"
        );
        assert!(output.contains("content = VALID"));
        assert!(output.contains("manifest_binding = VALID (bound)"));
        assert!(
            output.contains(&format!("commit lookup: cache (object {object_id})")),
            "expected cache lookup, got:\n{output}"
        );
        Ok(())
    }

    #[test]
    fn verify_object_explicit_repo_skips_cache() -> Result<(), Box<dyn std::error::Error>> {
        // Both cache and explicit repo populated. --repo <path>
        // takes precedence over the cache; the report shows the
        // explicit path, not "cache".
        let manifest_text = r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "cache-vs-repo"
        "#;
        let (git_dir, commit_oid) = init_git_repo_with_manifest(manifest_text)?;
        let store_dir = tempfile::TempDir::new()?;
        let manifest_path = git_dir.path().join("kairo.toml");

        let actor_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Create {
                    kind: "person".to_owned(),
                    attestation_keys: vec![],
                    generate_attestation_keys: 1,
                    attestation_threshold: 1,
                },
            }),
        })?;
        let actor_id = parse_field(&actor_output, "actor = ")?;
        let object_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Object {
                command: ObjectSubcommand::Create {
                    actor: actor_id.clone(),
                    kind: "software".to_owned(),
                    initial_revision: None,
                },
            }),
        })?;
        let object_id = parse_field(&object_output, "object = ")?;
        let revision_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Revision {
                command: RevisionCommand::Create {
                    actor: actor_id.clone(),
                    object: object_id.clone(),
                    revision: format!("git:sha256:{commit_oid}"),
                    manifest: manifest_path,
                    parents: vec![],
                    no_attests_reachable_history: false,
                },
            }),
        })?;
        let revision_statement = parse_field(&revision_output, "statement = ")?;
        run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Branch {
                command: BranchCommand::Set {
                    actor: actor_id,
                    object: object_id.clone(),
                    revision: revision_statement,
                    name: "head".to_owned(),
                },
            }),
        })?;

        populate_cache(store_dir.path(), git_dir.path(), &object_id, &commit_oid)?;

        let output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Verify {
                command: VerifyCommand::Object {
                    object: object_id,
                    statement: None,
                    actor: None,
                    name: "head".to_owned(),
                    repo: Some(git_dir.path().to_path_buf()),
                    no_repo: false,
                    no_cache: false,
                    no_cwd_repo: false,
                    r#as: None,
                    no_as: true,
                    manifest: None,
                    json: false,
                },
            }),
        })?;
        assert!(
            output.contains("verify object: VALID"),
            "expected VALID, got:\n{output}"
        );
        assert!(
            output.contains("commit lookup: repo at "),
            "expected explicit repo lookup, got:\n{output}"
        );
        // Crucially, NOT the cache form.
        assert!(
            !output.contains("commit lookup: cache"),
            "explicit --repo must skip cache, got:\n{output}"
        );
        Ok(())
    }

    #[test]
    fn verify_object_no_cwd_repo_with_cache_miss_is_indeterminate(
    ) -> Result<(), Box<dyn std::error::Error>> {
        // No cache populated, --no-cwd-repo set. Content layer
        // must report INDETERMINATE rather than erroring out on
        // "no Git repo discovered".
        let (store_dir, _manifest_dir, _actor_id, object_id, _revision_statement, manifest_path) =
            fixture_with_branch()?;

        let output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Verify {
                command: VerifyCommand::Object {
                    object: object_id,
                    statement: None,
                    actor: None,
                    name: "head".to_owned(),
                    repo: None,
                    no_repo: false,
                    no_cache: false,
                    no_cwd_repo: true,
                    r#as: None,
                    no_as: true,
                    manifest: Some(manifest_path),
                    json: false,
                },
            }),
        })?;
        assert!(
            output.contains("verify object: INDETERMINATE"),
            "expected INDETERMINATE, got:\n{output}"
        );
        assert!(output.contains("content = INDETERMINATE"));
        assert!(
            output.contains("commit lookup: skipped"),
            "expected lookup skipped, got:\n{output}"
        );
        Ok(())
    }

    #[test]
    fn verify_object_with_repo_missing_commit_is_invalid(
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Sign a revision against a commit oid that doesn't exist in
        // the git repo we point --repo at. Content layer must report
        // CommitNotFound, which makes overall INVALID.
        let manifest_text = r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "git-fixture"
        "#;
        let (git_dir, real_oid) = init_git_repo_with_manifest(manifest_text)?;
        let _ = real_oid; // we need a repo with at least one commit, but we'll sign against a different oid
        let store_dir = tempfile::TempDir::new()?;

        // Use the working tree's kairo.toml as the manifest the user
        // signs against.
        let manifest_path = git_dir.path().join("kairo.toml");

        let actor_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Create {
                    kind: "person".to_owned(),
                    attestation_keys: vec![],
                    generate_attestation_keys: 1,
                    attestation_threshold: 1,
                },
            }),
        })?;
        let actor_id = parse_field(&actor_output, "actor = ")?;
        let object_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Object {
                command: ObjectSubcommand::Create {
                    actor: actor_id.clone(),
                    kind: "software".to_owned(),
                    initial_revision: None,
                },
            }),
        })?;
        let object_id = parse_field(&object_output, "object = ")?;
        let revision_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Revision {
                command: RevisionCommand::Create {
                    actor: actor_id.clone(),
                    object: object_id.clone(),
                    revision:
                        "git:sha256:0123456789abcdef0123456789abcdef01234567".to_owned(),
                    manifest: manifest_path.clone(),
                    parents: vec![],
                    no_attests_reachable_history: false,
                },
            }),
        })?;
        let revision_statement = parse_field(&revision_output, "statement = ")?;
        run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Branch {
                command: BranchCommand::Set {
                    actor: actor_id,
                    object: object_id.clone(),
                    revision: revision_statement,
                    name: "head".to_owned(),
                },
            }),
        })?;

        let result = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Verify {
                command: VerifyCommand::Object {
                    object: object_id,
                    statement: None,
                    actor: None,
                    name: "head".to_owned(),
                    repo: Some(git_dir.path().to_path_buf()),
                    no_repo: false,
                    no_cache: false,
                    no_cwd_repo: false,
                    r#as: None,
                    no_as: true,
                    manifest: Some(manifest_path),
                    json: false,
                },
            }),
        });
        assert!(matches!(
            result,
            Err(CliError::ObjectVerificationFailed(report))
                if report.contains("INVALID") && report.contains("commit not in repo")
        ));
        Ok(())
    }

    #[test]
    fn parses_verify_object_with_pinned_statement() {
        let cli = Cli::try_parse_from([
            "kairo",
            "verify",
            "object",
            "--object",
            "zQmObject",
            "--statement",
            "zQmStatement",
        ]);
        assert!(matches!(
            cli,
            Ok(Cli {
                command: Some(Command::Verify {
                    command: VerifyCommand::Object {
                        statement: Some(statement),
                        actor: None,
                        ..
                    }
                }),
                ..
            }) if statement == "zQmStatement"
        ));
    }

    #[test]
    fn end_to_end_trust_grant_show_list() -> Result<(), Box<dyn std::error::Error>> {
        // Create two actors (truster + trusted), grant trust, show
        // and list — confirm the head reflects the grant.
        let store_dir = tempfile::TempDir::new()?;

        let truster_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Create {
                    kind: "person".to_owned(),
                    attestation_keys: vec![],
                    generate_attestation_keys: 1,
                    attestation_threshold: 1,
                },
            }),
        })?;
        let truster_id = parse_field(&truster_output, "actor = ")?;

        let trusted_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Create {
                    kind: "person".to_owned(),
                    attestation_keys: vec![],
                    generate_attestation_keys: 1,
                    attestation_threshold: 1,
                },
            }),
        })?;
        let trusted_id = parse_field(&trusted_output, "actor = ")?;
        assert_ne!(truster_id, trusted_id);

        let grant = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Trust {
                command: TrustCommand::Grant {
                    by: truster_id.clone(),
                    of: trusted_id.clone(),
                    reason: Some("works for me".to_owned()),
                },
            }),
        })?;
        assert!(grant.contains("grant trust"));
        assert!(grant.contains("decision = trusted"));
        assert!(grant.contains("supersedes = (genesis)"));

        let show = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Trust {
                command: TrustCommand::Show {
                    by: truster_id.clone(),
                    of: trusted_id.clone(),
                    json: false,
                },
            }),
        })?;
        assert!(show.contains("decision = trusted"));
        assert!(show.contains("reason = works for me"));

        let list = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Trust {
                command: TrustCommand::List {
                    by: truster_id.clone(),
                },
            }),
        })?;
        assert!(list.contains("opinions = 1"));
        assert!(list.contains(&trusted_id));
        Ok(())
    }

    #[test]
    fn trust_block_then_withdraw_chains_correctly()
    -> Result<(), Box<dyn std::error::Error>> {
        let store_dir = tempfile::TempDir::new()?;

        let truster_id = parse_field(
            &run(Cli {
                store: Some(store_dir.path().to_path_buf()),
                keys: None,
                command: Some(Command::Actor {
                    command: ActorCommand::Create {
                        kind: "person".to_owned(),
                        attestation_keys: vec![],
                        generate_attestation_keys: 1,
                        attestation_threshold: 1,
                    },
                }),
            })?,
            "actor = ",
        )?;
        let trusted_id = parse_field(
            &run(Cli {
                store: Some(store_dir.path().to_path_buf()),
                keys: None,
                command: Some(Command::Actor {
                    command: ActorCommand::Create {
                        kind: "person".to_owned(),
                        attestation_keys: vec![],
                        generate_attestation_keys: 1,
                        attestation_threshold: 1,
                    },
                }),
            })?,
            "actor = ",
        )?;

        let block = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Trust {
                command: TrustCommand::Block {
                    by: truster_id.clone(),
                    of: trusted_id.clone(),
                    reason: None,
                },
            }),
        })?;
        let block_statement = parse_field(&block, "statement = ")?;
        assert!(block.contains("decision = untrusted"));

        // Wait so created_at moves; not strictly required for chain
        // precedence, but keeps history readable.
        std::thread::sleep(std::time::Duration::from_millis(1100));

        let withdraw = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Trust {
                command: TrustCommand::Withdraw {
                    by: truster_id.clone(),
                    of: trusted_id.clone(),
                    reason: None,
                },
            }),
        })?;
        assert!(withdraw.contains("decision = (withdrawn)"));
        assert!(withdraw.contains(&format!("supersedes = {block_statement}")));

        // Show should now report unknown (withdrawal collapsed).
        let show = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Trust {
                command: TrustCommand::Show {
                    by: truster_id.clone(),
                    of: trusted_id.clone(),
                    json: false,
                },
            }),
        })?;
        // The withdrawal is the head; in show output we render the
        // chain leaf's decision literally, which is "unknown".
        assert!(show.contains("decision = unknown"));

        // History should report newest-first: withdraw, then block.
        let history = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Trust {
                command: TrustCommand::History {
                    by: truster_id.clone(),
                    of: trusted_id.clone(),
                    json: false,
                },
            }),
        })?;
        assert!(history.contains("history (newest -> oldest, 2 entries):"));
        assert!(history.contains("kind=withdraw"));
        assert!(history.contains("kind=block"));
        Ok(())
    }

    #[test]
    fn trust_show_unknown_when_no_opinion() -> Result<(), Box<dyn std::error::Error>> {
        let store_dir = tempfile::TempDir::new()?;
        let truster_id = parse_field(
            &run(Cli {
                store: Some(store_dir.path().to_path_buf()),
                keys: None,
                command: Some(Command::Actor {
                    command: ActorCommand::Create {
                        kind: "person".to_owned(),
                        attestation_keys: vec![],
                        generate_attestation_keys: 1,
                        attestation_threshold: 1,
                    },
                }),
            })?,
            "actor = ",
        )?;
        let trusted_id = parse_field(
            &run(Cli {
                store: Some(store_dir.path().to_path_buf()),
                keys: None,
                command: Some(Command::Actor {
                    command: ActorCommand::Create {
                        kind: "person".to_owned(),
                        attestation_keys: vec![],
                        generate_attestation_keys: 1,
                        attestation_threshold: 1,
                    },
                }),
            })?,
            "actor = ",
        )?;

        let show = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Trust {
                command: TrustCommand::Show {
                    by: truster_id,
                    of: trusted_id,
                    json: false,
                },
            }),
        })?;
        assert!(show.contains("decision = unknown"));
        Ok(())
    }

    #[test]
    fn trust_withdraw_without_prior_errors() -> Result<(), Box<dyn std::error::Error>> {
        let store_dir = tempfile::TempDir::new()?;
        let truster_id = parse_field(
            &run(Cli {
                store: Some(store_dir.path().to_path_buf()),
                keys: None,
                command: Some(Command::Actor {
                    command: ActorCommand::Create {
                        kind: "person".to_owned(),
                        attestation_keys: vec![],
                        generate_attestation_keys: 1,
                        attestation_threshold: 1,
                    },
                }),
            })?,
            "actor = ",
        )?;
        let trusted_id = parse_field(
            &run(Cli {
                store: Some(store_dir.path().to_path_buf()),
                keys: None,
                command: Some(Command::Actor {
                    command: ActorCommand::Create {
                        kind: "person".to_owned(),
                        attestation_keys: vec![],
                        generate_attestation_keys: 1,
                        attestation_threshold: 1,
                    },
                }),
            })?,
            "actor = ",
        )?;

        let result = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Trust {
                command: TrustCommand::Withdraw {
                    by: truster_id,
                    of: trusted_id,
                    reason: None,
                },
            }),
        });
        assert!(matches!(
            result,
            Err(CliError::WithdrawWithoutPriorTrust { .. })
        ));
        Ok(())
    }

    #[test]
    fn verify_object_auto_picks_sole_local_actor_for_trust()
    -> Result<(), Box<dyn std::error::Error>> {
        // The fixture creates exactly one local actor (the signer) and
        // does not publish a trust opinion about itself, so the
        // auto-picked truster sees its own statements as Unknown.
        let (store_dir, _manifest_dir, _actor_id, object_id, _revision_statement, manifest_path) =
            fixture_with_branch()?;

        let output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Verify {
                command: VerifyCommand::Object {
                    object: object_id,
                    statement: None,
                    actor: None,
                    name: "head".to_owned(),
                    r#as: None,
                    no_as: false,
                    repo: None,
                    no_repo: true,
                    no_cache: false,
                    no_cwd_repo: false,
                    manifest: Some(manifest_path),
                    json: false,
                },
            }),
        })?;
        assert!(output.contains("trust = unknown"));
        // Trust line includes the truster id when auto-resolved.
        assert!(output.contains("(as zQm"));
        Ok(())
    }

    #[test]
    fn verify_object_with_explicit_as_grants_trusted()
    -> Result<(), Box<dyn std::error::Error>> {
        // Create a separate truster actor, grant trust to the signer,
        // then verify --as <truster> sees Trusted.
        let (store_dir, _manifest_dir, signer_id, object_id, _revision_statement, manifest_path) =
            fixture_with_branch()?;

        // Add a second local actor to act as truster.
        let truster_id = parse_field(
            &run(Cli {
                store: Some(store_dir.path().to_path_buf()),
                keys: None,
                command: Some(Command::Actor {
                    command: ActorCommand::Create {
                        kind: "person".to_owned(),
                        attestation_keys: vec![],
                        generate_attestation_keys: 1,
                        attestation_threshold: 1,
                    },
                }),
            })?,
            "actor = ",
        )?;
        // Grant trust from truster -> signer.
        run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Trust {
                command: TrustCommand::Grant {
                    by: truster_id.clone(),
                    of: signer_id.clone(),
                    reason: None,
                },
            }),
        })?;

        let output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Verify {
                command: VerifyCommand::Object {
                    object: object_id,
                    statement: None,
                    actor: None,
                    name: "head".to_owned(),
                    r#as: Some(truster_id.clone()),
                    no_as: false,
                    repo: None,
                    no_repo: true,
                    no_cache: false,
                    no_cwd_repo: false,
                    manifest: Some(manifest_path),
                    json: false,
                },
            }),
        })?;
        assert!(output.contains("trust = trusted"));
        assert!(output.contains(&format!("(as {truster_id})")));
        Ok(())
    }

    #[test]
    fn verify_object_with_no_as_skips_trust_evaluation()
    -> Result<(), Box<dyn std::error::Error>> {
        let (store_dir, _manifest_dir, _actor_id, object_id, _revision_statement, manifest_path) =
            fixture_with_branch()?;

        let output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Verify {
                command: VerifyCommand::Object {
                    object: object_id,
                    statement: None,
                    actor: None,
                    name: "head".to_owned(),
                    r#as: None,
                    no_as: true,
                    repo: None,
                    no_repo: true,
                    no_cache: false,
                    no_cwd_repo: false,
                    manifest: Some(manifest_path),
                    json: false,
                },
            }),
        })?;
        assert!(output.contains("trust = unevaluated"));
        // No "(as ...)" suffix when no truster was used.
        assert!(!output.contains("(as zQm"));
        Ok(())
    }

    #[test]
    fn verify_object_ambiguous_local_actor_errors()
    -> Result<(), Box<dyn std::error::Error>> {
        // Two local actors, no --as: must error.
        let (store_dir, _manifest_dir, _signer_id, object_id, _revision_statement, manifest_path) =
            fixture_with_branch()?;
        // Add a second actor to make resolution ambiguous.
        run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Create {
                    kind: "person".to_owned(),
                    attestation_keys: vec![],
                    generate_attestation_keys: 1,
                    attestation_threshold: 1,
                },
            }),
        })?;

        let result = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Verify {
                command: VerifyCommand::Object {
                    object: object_id,
                    statement: None,
                    actor: None,
                    name: "head".to_owned(),
                    r#as: None,
                    no_as: false,
                    repo: None,
                    no_repo: true,
                    no_cache: false,
                    no_cwd_repo: false,
                    manifest: Some(manifest_path),
                    json: false,
                },
            }),
        });
        assert!(matches!(
            result,
            Err(CliError::AmbiguousLocalActor { .. })
        ));
        Ok(())
    }

    #[test]
    fn end_to_end_bundle_export_then_import() -> Result<(), Box<dyn std::error::Error>> {
        // Build a populated source store, export a bundle to a tmp
        // dir, then import into a brand-new store and re-resolve the
        // branch tip end-to-end.
        let (src_store_dir, _manifest_dir, actor_id, object_id, revision_statement, _manifest_path) =
            fixture_with_branch()?;

        let bundle_dir = tempfile::TempDir::new()?;
        let bundle_path = bundle_dir.path().join("bundle");

        let export_output = run(Cli {
            store: Some(src_store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Bundle {
                command: BundleCommand::Export {
                    object: object_id.clone(),
                    output: bundle_path.clone(),
                    include_git: false,
                },
            }),
        })?;
        assert!(export_output.contains("export bundle"));
        assert!(export_output.contains(&object_id));

        // Fresh empty store as the import target.
        let dest_store_dir = tempfile::TempDir::new()?;
        let import_output = run(Cli {
            store: Some(dest_store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Bundle {
                command: BundleCommand::Import {
                    input: bundle_path.clone(),
                },
            }),
        })?;
        assert!(import_output.contains("import bundle"));
        assert!(import_output.contains("actors = 1"));

        // Branch resolves at the new store, pointing at the original
        // revision statement.
        let show_output = run(Cli {
            store: Some(dest_store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Branch {
                command: BranchCommand::Show {
                    object: object_id.clone(),
                    actor: Some(actor_id.clone()),
                    name: "head".to_owned(),
                    json: false,
                },
            }),
        })?;
        assert!(show_output.contains(&revision_statement));
        Ok(())
    }

    #[test]
    fn bundle_import_rejects_unknown_directory() -> Result<(), Box<dyn std::error::Error>> {
        let store_dir = tempfile::TempDir::new()?;
        let nowhere = std::path::PathBuf::from("/nonexistent-kairo-bundle-dir-xyz");
        let result = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Bundle {
                command: BundleCommand::Import { input: nowhere },
            }),
        });
        assert!(matches!(result, Err(CliError::Bundle(_))));
        Ok(())
    }

    fn create_local_actor(
        store_dir: &std::path::Path,
    ) -> Result<String, Box<dyn std::error::Error>> {
        parse_field(
            &run(Cli {
                store: Some(store_dir.to_path_buf()),
                keys: None,
                command: Some(Command::Actor {
                    command: ActorCommand::Create {
                        kind: "person".to_owned(),
                        attestation_keys: vec![],
                        generate_attestation_keys: 1,
                        attestation_threshold: 1,
                    },
                }),
            })?,
            "actor = ",
        )
    }

    fn create_local_object(
        store_dir: &std::path::Path,
        actor: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        parse_field(
            &run(Cli {
                store: Some(store_dir.to_path_buf()),
                keys: None,
                command: Some(Command::Object {
                    command: ObjectSubcommand::Create {
                        actor: actor.to_owned(),
                        kind: "software".to_owned(),
                        initial_revision: None,
                    },
                }),
            })?,
            "object = ",
        )
    }

    #[test]
    fn capability_grant_then_list_by_grantor()
    -> Result<(), Box<dyn std::error::Error>> {
        let store_dir = tempfile::TempDir::new()?;
        let grantor = create_local_actor(store_dir.path())?;
        let grantee = create_local_actor(store_dir.path())?;
        let object = create_local_object(store_dir.path(), &grantor)?;

        let grant_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Capability {
                command: CapabilityCommand::Grant {
                    grantor: grantor.clone(),
                    grantee: grantee.clone(),
                    object: object.clone(),
                    kinds: vec!["ObjectVersionTag".to_owned()],
                    delegable: false,
                    expires_at: None,
                    max_delegation_depth: None,
                    key_pinned: None,
                },
            }),
        })?;
        assert!(grant_output.contains("grant capability"));
        assert!(grant_output.contains("supersedes = (genesis)"));
        assert!(grant_output.contains("kinds = [ObjectVersionTag]"));

        let list_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Capability {
                command: CapabilityCommand::List {
                    grantor: Some(grantor.clone()),
                    object: None,
                },
            }),
        })?;
        assert!(list_output.contains("heads = 1"));
        assert!(list_output.contains(&grantee));
        assert!(list_output.contains(&object));
        Ok(())
    }

    #[test]
    fn capability_grant_supersedes_prior_chain_leaf()
    -> Result<(), Box<dyn std::error::Error>> {
        let store_dir = tempfile::TempDir::new()?;
        let grantor = create_local_actor(store_dir.path())?;
        let grantee = create_local_actor(store_dir.path())?;
        let object = create_local_object(store_dir.path(), &grantor)?;

        let first = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Capability {
                command: CapabilityCommand::Grant {
                    grantor: grantor.clone(),
                    grantee: grantee.clone(),
                    object: object.clone(),
                    kinds: vec!["ObjectVersionTag".to_owned()],
                    delegable: false,
                    expires_at: None,
                    max_delegation_depth: None,
                    key_pinned: None,
                },
            }),
        })?;
        let first_id = parse_field(&first, "statement = ")?;

        // Wait so created_at strictly increases.
        std::thread::sleep(std::time::Duration::from_millis(1100));

        let second = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Capability {
                command: CapabilityCommand::Grant {
                    grantor: grantor.clone(),
                    grantee: grantee.clone(),
                    object: object.clone(),
                    kinds: vec![
                        "ObjectVersionTag".to_owned(),
                        "ObjectBranch".to_owned(),
                    ],
                    delegable: true,
                    expires_at: None,
                    max_delegation_depth: None,
                    key_pinned: None,
                },
            }),
        })?;
        assert!(second.contains(&format!("supersedes = {first_id}")));
        assert!(second.contains("delegable = true"));
        assert!(second.contains("kinds = [ObjectBranch,ObjectVersionTag]"));
        Ok(())
    }

    #[test]
    fn capability_revoke_emits_revocation_and_blocks_wrong_grantor()
    -> Result<(), Box<dyn std::error::Error>> {
        let store_dir = tempfile::TempDir::new()?;
        let grantor = create_local_actor(store_dir.path())?;
        let grantee = create_local_actor(store_dir.path())?;
        let intruder = create_local_actor(store_dir.path())?;
        let object = create_local_object(store_dir.path(), &grantor)?;

        let grant = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Capability {
                command: CapabilityCommand::Grant {
                    grantor: grantor.clone(),
                    grantee,
                    object,
                    kinds: vec!["ObjectVersionTag".to_owned()],
                    delegable: false,
                    expires_at: None,
                    max_delegation_depth: None,
                    key_pinned: None,
                },
            }),
        })?;
        let grant_id = parse_field(&grant, "statement = ")?;

        // A different actor cannot revoke someone else's grant.
        let intruder_attempt = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Capability {
                command: CapabilityCommand::Revoke {
                    grantor: intruder,
                    grant: grant_id.clone(),
                    retroactive: false,
                    reason: None,
                },
            }),
        });
        assert!(matches!(
            intruder_attempt,
            Err(CliError::RevokeWrongGrantor { .. })
        ));

        // The original grantor can.
        let revoke = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Capability {
                command: CapabilityCommand::Revoke {
                    grantor,
                    grant: grant_id.clone(),
                    retroactive: true,
                    reason: Some("compromised".to_owned()),
                },
            }),
        })?;
        assert!(revoke.contains("revoke capability"));
        assert!(revoke.contains(&format!("revoked_grant = {grant_id}")));
        assert!(revoke.contains("retroactive = true"));
        Ok(())
    }

    #[test]
    fn capability_list_requires_exactly_one_filter()
    -> Result<(), Box<dyn std::error::Error>> {
        let store_dir = tempfile::TempDir::new()?;
        let neither = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Capability {
                command: CapabilityCommand::List {
                    grantor: None,
                    object: None,
                },
            }),
        });
        assert!(matches!(neither, Err(CliError::CapabilityListExclusive)));
        Ok(())
    }

    #[test]
    fn capability_grant_rejects_empty_kinds()
    -> Result<(), Box<dyn std::error::Error>> {
        let store_dir = tempfile::TempDir::new()?;
        let grantor = create_local_actor(store_dir.path())?;
        let grantee = create_local_actor(store_dir.path())?;
        let object = create_local_object(store_dir.path(), &grantor)?;
        let result = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Capability {
                command: CapabilityCommand::Grant {
                    grantor,
                    grantee,
                    object,
                    kinds: vec![],
                    delegable: false,
                    expires_at: None,
                    max_delegation_depth: None,
                    key_pinned: None,
                },
            }),
        });
        assert!(matches!(result, Err(CliError::CapabilityKindsRequired)));
        Ok(())
    }

    #[test]
    fn capability_list_by_object_includes_grantor_grantee()
    -> Result<(), Box<dyn std::error::Error>> {
        let store_dir = tempfile::TempDir::new()?;
        let grantor = create_local_actor(store_dir.path())?;
        let grantee = create_local_actor(store_dir.path())?;
        let object = create_local_object(store_dir.path(), &grantor)?;
        run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Capability {
                command: CapabilityCommand::Grant {
                    grantor: grantor.clone(),
                    grantee: grantee.clone(),
                    object: object.clone(),
                    kinds: vec!["ObjectVersionTag".to_owned()],
                    delegable: false,
                    expires_at: None,
                    max_delegation_depth: None,
                    key_pinned: None,
                },
            }),
        })?;

        let by_object = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Capability {
                command: CapabilityCommand::List {
                    grantor: None,
                    object: Some(object.clone()),
                },
            }),
        })?;
        assert!(by_object.contains("heads = 1"));
        assert!(by_object.contains(&grantor));
        assert!(by_object.contains(&grantee));
        Ok(())
    }

    // ---- actor key rotation / revocation ----

    #[test]
    fn parses_actor_rotate_key_command() {
        let cli = Cli::try_parse_from(["kairo", "actor", "rotate-key", "--actor", "zActor"]);
        assert!(matches!(
            cli,
            Ok(Cli {
                store: None,
                keys: None,
                command: Some(Command::Actor {
                    command: ActorCommand::RotateKey { actor }
                })
            }) if actor == "zActor"
        ));
    }

    #[test]
    fn parses_actor_revoke_key_command_with_flags() {
        let cli = Cli::try_parse_from([
            "kairo",
            "actor",
            "revoke-key",
            "--actor",
            "zActor",
            "--key",
            "zKey",
            "--retroactive",
            "--reason",
            "lost device",
            "--brick-actor",
        ]);
        assert!(matches!(
            cli,
            Ok(Cli {
                store: None,
                keys: None,
                command: Some(Command::Actor {
                    command: ActorCommand::RevokeKey {
                        actor,
                        key_id,
                        retroactive: true,
                        reason: Some(reason),
                        brick_actor: true,
                    }
                })
            }) if actor == "zActor" && key_id == "zKey" && reason == "lost device"
        ));
    }

    #[test]
    fn parses_actor_key_history_command() {
        let cli = Cli::try_parse_from([
            "kairo",
            "actor",
            "key-history",
            "--actor",
            "zActor",
            "--json",
        ]);
        assert!(matches!(
            cli,
            Ok(Cli {
                store: None,
                keys: None,
                command: Some(Command::Actor {
                    command: ActorCommand::KeyHistory { actor, json: true }
                })
            }) if actor == "zActor"
        ));
    }

    #[test]
    fn end_to_end_rotate_key_persists_chain_and_swaps_keystore()
    -> Result<(), Box<dyn std::error::Error>> {
        let store_dir = tempfile::TempDir::new()?;

        // 1. Create an actor.
        let actor_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Create {
                    kind: "person".to_owned(),
                    attestation_keys: vec![],
                    generate_attestation_keys: 1,
                    attestation_threshold: 1,
                },
            }),
        })?;
        let actor_id = parse_field(&actor_output, "actor = ")?;
        let initial_key_id = parse_field(&actor_output, "key_id = ")?;

        // 2. Rotate the key. Output records both prior + next key_id
        // and `supersedes = (genesis)`.
        let rotate_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::RotateKey {
                    actor: actor_id.clone(),
                },
            }),
        })?;
        assert!(rotate_output.contains("rotated key"));
        assert!(rotate_output.contains("supersedes = (genesis)"));
        let prior_key_id = parse_field(&rotate_output, "prior_key_id = ")?;
        let next_key_id = parse_field(&rotate_output, "next_key_id = ")?;
        assert_eq!(prior_key_id, initial_key_id);
        assert_ne!(next_key_id, initial_key_id);

        // 3. Rotate again — the second rotation must supersede the
        // first (chain continuity).
        let rotate_two = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::RotateKey {
                    actor: actor_id.clone(),
                },
            }),
        })?;
        let rotation_one_statement = parse_field(&rotate_output, "statement = ")?;
        assert!(rotate_two.contains(&format!("supersedes = {rotation_one_statement}")));

        // 4. key-history reflects both rotations.
        let history = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::KeyHistory {
                    actor: actor_id,
                    json: false,
                },
            }),
        })?;
        assert!(history.contains("rotations = 2"));
        assert!(history.contains("revocations = 0"));
        Ok(())
    }

    #[test]
    fn revoke_key_refuses_to_brick_actor_without_flag()
    -> Result<(), Box<dyn std::error::Error>> {
        let store_dir = tempfile::TempDir::new()?;

        let actor_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Create {
                    kind: "person".to_owned(),
                    attestation_keys: vec![],
                    generate_attestation_keys: 1,
                    attestation_threshold: 1,
                },
            }),
        })?;
        let actor_id = parse_field(&actor_output, "actor = ")?;
        let initial_key_id = parse_field(&actor_output, "key_id = ")?;

        // No rotation has happened — initial_key_id is the only
        // active key. Without --brick-actor, revoking it must error.
        let result = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::RevokeKey {
                    actor: actor_id.clone(),
                    key_id: initial_key_id.clone(),
                    retroactive: false,
                    reason: None,
                    brick_actor: false,
                },
            }),
        });
        assert!(matches!(result, Err(CliError::WouldBrickActor { .. })));

        // With --brick-actor it succeeds.
        let result = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::RevokeKey {
                    actor: actor_id,
                    key_id: initial_key_id,
                    retroactive: true,
                    reason: Some("compromised".to_owned()),
                    brick_actor: true,
                },
            }),
        })?;
        assert!(result.contains("revoked key"));
        assert!(result.contains("retroactive = true"));
        assert!(result.contains("reason = compromised"));
        Ok(())
    }

    #[test]
    fn signing_command_after_rotation_uses_new_active_key()
    -> Result<(), Box<dyn std::error::Error>> {
        // After the first rotation, signing commands (here,
        // `object create`) continue to work because the keystore
        // secret is matched against the active key chain rather than
        // against `actor_body.initial_key()`. Regression guard for
        // the require_active_signing_key sweep.
        let store_dir = tempfile::TempDir::new()?;

        let actor_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Create {
                    kind: "person".to_owned(),
                    attestation_keys: vec![],
                    generate_attestation_keys: 1,
                    attestation_threshold: 1,
                },
            }),
        })?;
        let actor_id = parse_field(&actor_output, "actor = ")?;

        run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::RotateKey {
                    actor: actor_id.clone(),
                },
            }),
        })?;

        let object_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Object {
                command: ObjectSubcommand::Create {
                    actor: actor_id,
                    kind: "software".to_owned(),
                    initial_revision: None,
                },
            }),
        })?;
        assert!(object_output.contains("created object"));
        Ok(())
    }

    #[test]
    fn revoke_old_key_after_rotation_does_not_require_brick_flag()
    -> Result<(), Box<dyn std::error::Error>> {
        let store_dir = tempfile::TempDir::new()?;

        let actor_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Create {
                    kind: "person".to_owned(),
                    attestation_keys: vec![],
                    generate_attestation_keys: 1,
                    attestation_threshold: 1,
                },
            }),
        })?;
        let actor_id = parse_field(&actor_output, "actor = ")?;
        let initial_key_id = parse_field(&actor_output, "key_id = ")?;

        // Rotate first so the actor has a fresh active key.
        run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::RotateKey {
                    actor: actor_id.clone(),
                },
            }),
        })?;

        // Now revoke the old genesis key. This should not require
        // --brick-actor because the active key has already moved.
        let result = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::RevokeKey {
                    actor: actor_id,
                    key_id: initial_key_id,
                    retroactive: false,
                    reason: None,
                    brick_actor: false,
                },
            }),
        })?;
        assert!(result.contains("revoked key"));
        Ok(())
    }

    // ---- Phase 2 §14: cold-storage attestation CLI tests ----

    #[test]
    fn actor_create_rejects_no_attestation_source() {
        let store_dir = tempfile::TempDir::new().expect("tempdir");
        let result = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Create {
                    kind: "person".to_owned(),
                    attestation_keys: vec![],
                    generate_attestation_keys: 0,
                    attestation_threshold: 1,
                },
            }),
        });
        assert!(matches!(result, Err(CliError::NoAttestationKeyProvided)));
    }

    #[test]
    fn actor_create_with_operator_presented_attestation_key()
    -> Result<(), Box<dyn std::error::Error>> {
        let store_dir = tempfile::TempDir::new()?;
        // Pre-generate an attestation keypair externally; pass only the
        // public key to the CLI.
        let attestation_seed = [123_u8; 32];
        let attestation_pub = SigningKey::from_bytes(&attestation_seed)
            .verifying_key()
            .to_bytes();
        let attestation_hex: String =
            attestation_pub.iter().map(|b| format!("{b:02x}")).collect();

        let output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Create {
                    kind: "person".to_owned(),
                    attestation_keys: vec![attestation_hex.clone()],
                    generate_attestation_keys: 0,
                    attestation_threshold: 1,
                },
            }),
        })?;
        assert!(output.contains("created actor"));
        assert!(output.contains("attestation_keys = 1"));
        // Operator-presented path does NOT print a seed — Kairo never
        // sees the private half.
        assert!(!output.contains("seed = "));
        Ok(())
    }

    #[test]
    fn actor_create_generate_attestation_key_prints_seed_once()
    -> Result<(), Box<dyn std::error::Error>> {
        let store_dir = tempfile::TempDir::new()?;
        let output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Create {
                    kind: "person".to_owned(),
                    attestation_keys: vec![],
                    generate_attestation_keys: 1,
                    attestation_threshold: 1,
                },
            }),
        })?;
        assert!(output.contains("created actor"));
        assert!(output.contains("attestation_keys = 1"));
        assert!(output.contains("generated_attestation_keys = 1"));
        assert!(output.contains("seed = "));
        assert!(output.contains("pubkey = "));
        Ok(())
    }

    /// End-to-end: create an actor with `--generate-attestation-key`,
    /// pull the seed out of the output, write it to a file, then run
    /// `recover-key sign` to produce an emergency rotation. Confirms
    /// the convenience flow round-trips and the new active key lands
    /// in the keystore.
    #[test]
    fn actor_recover_key_sign_round_trip() -> Result<(), Box<dyn std::error::Error>> {
        let store_dir = tempfile::TempDir::new()?;
        let create_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Create {
                    kind: "person".to_owned(),
                    attestation_keys: vec![],
                    generate_attestation_keys: 1,
                    attestation_threshold: 1,
                },
            }),
        })?;
        let actor_id = parse_field(&create_output, "actor = ")?;
        let initial_key_id = parse_field(&create_output, "key_id = ")?;
        let seed_b64 = parse_field(&create_output, "    seed = ")?;
        let attestation_key_id = parse_field(&create_output, "    attestation_key_id = ")?;

        let seed_path = store_dir.path().join("attestation.seed");
        std::fs::write(&seed_path, seed_b64.as_bytes())?;

        let recover_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::RecoverKey {
                    command: RecoverKeyCommand::Sign {
                        actor: actor_id.clone(),
                        attestation_key_seed: seed_path,
                    },
                },
            }),
        })?;
        assert!(recover_output.contains("recovered active key"));
        let new_key_id = parse_field(&recover_output, "next_key_id = ")?;
        assert_ne!(new_key_id, initial_key_id);
        let logged_attestation_key_id =
            parse_field(&recover_output, "attestation_key_id = ")?;
        assert_eq!(logged_attestation_key_id, attestation_key_id);

        // After recovery, key-history surfaces the new emergency
        // rotation in the rotation chain with surface = attestation,
        // and a routine rotate-key call should now sign with the
        // freshly-rotated active key.
        let history_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::KeyHistory {
                    actor: actor_id.clone(),
                    json: false,
                },
            }),
        })?;
        assert!(history_output.contains("rotations = 1"));
        assert!(history_output.contains("surface = attestation"));
        assert!(history_output.contains(&format!("next_key_id = {new_key_id}")));

        // Confirm the keystore replaced the active signing key by
        // running a routine rotate-key; if the keystore-vs-active-key
        // check fails the call would error.
        let rotate_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::RotateKey {
                    actor: actor_id,
                },
            }),
        })?;
        assert!(rotate_output.contains("rotated key"));
        Ok(())
    }

    /// Pure prepare/import round-trip. The "operator's" cold device
    /// is simulated inline: we know the seed because we generated it
    /// ourselves, and we sign the prepared payload externally instead
    /// of going through the convenience `sign` path.
    #[test]
    fn actor_recover_key_prepare_import_round_trip()
    -> Result<(), Box<dyn std::error::Error>> {
        let store_dir = tempfile::TempDir::new()?;
        let create_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Create {
                    kind: "person".to_owned(),
                    attestation_keys: vec![],
                    generate_attestation_keys: 1,
                    attestation_threshold: 1,
                },
            }),
        })?;
        let actor_id = parse_field(&create_output, "actor = ")?;
        let seed_b64 = parse_field(&create_output, "    seed = ")?;
        let attestation_seed_bytes = STANDARD.decode(&seed_b64)?;
        let attestation_seed: [u8; 32] = attestation_seed_bytes
            .as_slice()
            .try_into()
            .expect("attestation seed is 32 bytes");
        let attestation_signing = SigningKey::from_bytes(&attestation_seed);

        // Operator-managed new active key (we hold the private half
        // externally — it will never enter the keystore in this flow).
        let new_active_seed = [42_u8; 32];
        let new_active_pub = SigningKey::from_bytes(&new_active_seed)
            .verifying_key()
            .to_bytes();
        let new_active_hex: String =
            new_active_pub.iter().map(|b| format!("{b:02x}")).collect();

        let envelope_path = store_dir.path().join("recovery.json");
        let prepare_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::RecoverKey {
                    command: RecoverKeyCommand::Prepare {
                        actor: actor_id.clone(),
                        new_key: new_active_hex,
                        output: envelope_path.clone(),
                    },
                },
            }),
        })?;
        assert!(prepare_output.contains("prepared emergency rotation envelope"));
        let payload_path = payload_path_for(&envelope_path);
        let payload_bytes = std::fs::read(&payload_path)?;

        // Operator signs the payload externally on the cold device.
        let signature_bytes = attestation_signing.sign(&payload_bytes).to_bytes();
        let signature_b64 = STANDARD.encode(signature_bytes);
        let sig_path = store_dir.path().join("recovery.sig");
        std::fs::write(&sig_path, signature_b64.as_bytes())?;

        let submit_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::RecoverKey {
                    command: RecoverKeyCommand::Submit {
                        prepared: envelope_path,
                        signature: Some(sig_path),
                    },
                },
            }),
        })?;
        assert!(submit_output.contains("imported emergency rotation"));

        // Key-history reflects the imported emergency rotation.
        let history_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::KeyHistory {
                    actor: actor_id,
                    json: false,
                },
            }),
        })?;
        assert!(history_output.contains("rotations = 1"));
        assert!(history_output.contains("surface = attestation"));
        Ok(())
    }

    /// Multi-sig recover-key flow with threshold = 2: two cosigners
    /// each append a signature via `co-sign`, then `submit` finalizes
    /// without `--signature`. Validates the (have, need) counter and
    /// the threshold check at submit time.
    #[test]
    fn actor_recover_key_cosign_two_of_two_round_trip()
    -> Result<(), Box<dyn std::error::Error>> {
        let store_dir = tempfile::TempDir::new()?;

        // Two operator-presented attestation keys, threshold = 2.
        let attest_a_seed = [201_u8; 32];
        let attest_b_seed = [202_u8; 32];
        let attest_a_pub = SigningKey::from_bytes(&attest_a_seed)
            .verifying_key()
            .to_bytes();
        let attest_b_pub = SigningKey::from_bytes(&attest_b_seed)
            .verifying_key()
            .to_bytes();
        let attest_a_hex: String = attest_a_pub.iter().map(|b| format!("{b:02x}")).collect();
        let attest_b_hex: String = attest_b_pub.iter().map(|b| format!("{b:02x}")).collect();

        let create_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Create {
                    kind: "person".to_owned(),
                    attestation_keys: vec![attest_a_hex, attest_b_hex],
                    generate_attestation_keys: 0,
                    attestation_threshold: 2,
                },
            }),
        })?;
        let actor_id = parse_field(&create_output, "actor = ")?;

        // Operator-managed new active key.
        let new_active_pub = SigningKey::from_bytes(&[42_u8; 32])
            .verifying_key()
            .to_bytes();
        let new_active_hex: String = new_active_pub.iter().map(|b| format!("{b:02x}")).collect();

        let envelope_path = store_dir.path().join("recovery.json");
        run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::RecoverKey {
                    command: RecoverKeyCommand::Prepare {
                        actor: actor_id.clone(),
                        new_key: new_active_hex,
                        output: envelope_path.clone(),
                    },
                },
            }),
        })?;

        // Each cosigner reads their own seed file.
        let seed_a_path = store_dir.path().join("seed_a.txt");
        let seed_b_path = store_dir.path().join("seed_b.txt");
        std::fs::write(&seed_a_path, STANDARD.encode(attest_a_seed))?;
        std::fs::write(&seed_b_path, STANDARD.encode(attest_b_seed))?;

        // Submit before any signatures fail (below threshold).
        let early_submit = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::RecoverKey {
                    command: RecoverKeyCommand::Submit {
                        prepared: envelope_path.clone(),
                        signature: None,
                    },
                },
            }),
        });
        assert!(early_submit.is_err(), "submit before any sigs must error");

        let cosign_a = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::CoSign {
                    prepared: envelope_path.clone(),
                    actor: actor_id.clone(),
                    attestation_key_seed: seed_a_path.clone(),
                },
            }),
        })?;
        assert!(cosign_a.contains("signatures = 1/2"));

        // Submit at 1/2 must still fail — below threshold.
        let mid_submit = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::RecoverKey {
                    command: RecoverKeyCommand::Submit {
                        prepared: envelope_path.clone(),
                        signature: None,
                    },
                },
            }),
        });
        assert!(mid_submit.is_err(), "submit at 1/2 must error");

        // Re-cosigning with the same seed must refuse.
        let dup = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::CoSign {
                    prepared: envelope_path.clone(),
                    actor: actor_id.clone(),
                    attestation_key_seed: seed_a_path.clone(),
                },
            }),
        });
        assert!(matches!(dup, Err(CliError::CosignDuplicateKeyId { .. })));

        let cosign_b = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::CoSign {
                    prepared: envelope_path.clone(),
                    actor: actor_id.clone(),
                    attestation_key_seed: seed_b_path,
                },
            }),
        })?;
        assert!(cosign_b.contains("signatures = 2/2"));

        let submit_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::RecoverKey {
                    command: RecoverKeyCommand::Submit {
                        prepared: envelope_path,
                        signature: None,
                    },
                },
            }),
        })?;
        assert!(submit_output.contains("imported emergency rotation"));
        Ok(())
    }

    /// `change-attestation-threshold sign` is the single-signer
    /// convenience flow. It only works when the asymmetric authority
    /// rule needs exactly one signature — i.e., `current = 1` and
    /// `to ≤ 1`. Raises always require ≥ 2 sigs and lowers never
    /// happen at current = 1, so practical use is rare; this test
    /// confirms the guard refuses raises and accepts the no-op case.
    #[test]
    fn actor_change_attestation_threshold_sign_refuses_raise_above_one()
    -> Result<(), Box<dyn std::error::Error>> {
        let store_dir = tempfile::TempDir::new()?;
        let attest_a_seed = [201_u8; 32];
        let attest_b_seed = [202_u8; 32];
        let attest_a_pub = SigningKey::from_bytes(&attest_a_seed)
            .verifying_key()
            .to_bytes();
        let attest_b_pub = SigningKey::from_bytes(&attest_b_seed)
            .verifying_key()
            .to_bytes();
        let attest_a_hex: String = attest_a_pub.iter().map(|b| format!("{b:02x}")).collect();
        let attest_b_hex: String = attest_b_pub.iter().map(|b| format!("{b:02x}")).collect();

        let create_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Create {
                    kind: "person".to_owned(),
                    attestation_keys: vec![attest_a_hex, attest_b_hex],
                    generate_attestation_keys: 0,
                    attestation_threshold: 1,
                },
            }),
        })?;
        let actor_id = parse_field(&create_output, "actor = ")?;
        let seed_a_path = store_dir.path().join("seed_a.txt");
        std::fs::write(&seed_a_path, STANDARD.encode(attest_a_seed))?;

        // Raise from 1 → 2 needs max(1, 2) = 2 sigs; sign refuses.
        let refuse_raise = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::ChangeAttestationThreshold {
                    command: ChangeAttestationThresholdCommand::Sign {
                        actor: actor_id.clone(),
                        attestation_key_seed: seed_a_path.clone(),
                        to: 2,
                    },
                },
            }),
        });
        assert!(matches!(
            refuse_raise,
            Err(CliError::ChangeThresholdSignNeedsCosign {
                current_threshold: 1,
                required: 2,
                ..
            })
        ));

        // No-op (current = to = 1) needs current = 1 sig; sign works.
        let sign_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::ChangeAttestationThreshold {
                    command: ChangeAttestationThresholdCommand::Sign {
                        actor: actor_id,
                        attestation_key_seed: seed_a_path,
                        to: 1,
                    },
                },
            }),
        })?;
        assert!(sign_output.contains("changed attestation threshold"));
        assert!(sign_output.contains("new_threshold = 1"));
        Ok(())
    }

    /// `change-attestation-threshold prepare` + `co-sign` × 2 +
    /// `submit` lowers threshold from 2 back to 1. Tests the full
    /// multi-sig flow end-to-end.
    #[test]
    fn actor_change_attestation_threshold_lower_via_cosign_round_trip()
    -> Result<(), Box<dyn std::error::Error>> {
        let store_dir = tempfile::TempDir::new()?;
        let attest_a_seed = [203_u8; 32];
        let attest_b_seed = [204_u8; 32];
        let attest_a_pub = SigningKey::from_bytes(&attest_a_seed)
            .verifying_key()
            .to_bytes();
        let attest_b_pub = SigningKey::from_bytes(&attest_b_seed)
            .verifying_key()
            .to_bytes();
        let attest_a_hex: String = attest_a_pub.iter().map(|b| format!("{b:02x}")).collect();
        let attest_b_hex: String = attest_b_pub.iter().map(|b| format!("{b:02x}")).collect();

        let create_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Create {
                    kind: "person".to_owned(),
                    attestation_keys: vec![attest_a_hex, attest_b_hex],
                    generate_attestation_keys: 0,
                    attestation_threshold: 2,
                },
            }),
        })?;
        let actor_id = parse_field(&create_output, "actor = ")?;

        let seed_a_path = store_dir.path().join("seed_a.txt");
        let seed_b_path = store_dir.path().join("seed_b.txt");
        std::fs::write(&seed_a_path, STANDARD.encode(attest_a_seed))?;
        std::fs::write(&seed_b_path, STANDARD.encode(attest_b_seed))?;

        let envelope_path = store_dir.path().join("threshold.json");
        let prepare_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::ChangeAttestationThreshold {
                    command: ChangeAttestationThresholdCommand::Prepare {
                        actor: actor_id.clone(),
                        to: 1,
                        output: envelope_path.clone(),
                    },
                },
            }),
        })?;
        assert!(prepare_output.contains("required_signatures = 2"));

        run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::CoSign {
                    prepared: envelope_path.clone(),
                    actor: actor_id.clone(),
                    attestation_key_seed: seed_a_path,
                },
            }),
        })?;
        run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::CoSign {
                    prepared: envelope_path.clone(),
                    actor: actor_id.clone(),
                    attestation_key_seed: seed_b_path,
                },
            }),
        })?;

        let submit_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::ChangeAttestationThreshold {
                    command: ChangeAttestationThresholdCommand::Submit {
                        prepared: envelope_path,
                        signature: None,
                    },
                },
            }),
        })?;
        assert!(submit_output.contains("changed attestation threshold"));
        assert!(submit_output.contains("new_threshold = 1"));
        assert!(submit_output.contains("signatures = 2"));

        // key-history reflects the threshold trajectory (1 change
        // from genesis 2 → 1 with quorum_at_event = 2).
        let history = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::KeyHistory {
                    actor: actor_id.clone(),
                    json: false,
                },
            }),
        })?;
        assert!(history.contains("genesis_attestation_threshold = 2"));
        assert!(history.contains("current_attestation_threshold = 1"));
        assert!(history.contains("attestation_threshold_changes = 1"));
        assert!(history.contains("from = 2"));
        assert!(history.contains("to = 1"));
        assert!(history.contains("quorum_at_event = 2"));

        // JSON mode carries the same trajectory under
        // `attestation_threshold_changes`.
        let history_json = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::KeyHistory {
                    actor: actor_id,
                    json: true,
                },
            }),
        })?;
        let parsed: serde_json::Value = serde_json::from_str(&history_json)?;
        assert_eq!(parsed["genesis_attestation_threshold"], 2);
        assert_eq!(parsed["current_attestation_threshold"], 1);
        let changes = parsed["attestation_threshold_changes"]
            .as_array()
            .expect("threshold changes array");
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0]["from"], 2);
        assert_eq!(changes[0]["to"], 1);
        assert_eq!(changes[0]["quorum_at_event"], 2);
        Ok(())
    }

    /// `add-attestation-key sign` with `--generate` ships a new
    /// attestation key signed by the existing one.
    #[test]
    fn actor_add_attestation_key_sign_generate_round_trip()
    -> Result<(), Box<dyn std::error::Error>> {
        let store_dir = tempfile::TempDir::new()?;
        let create_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Create {
                    kind: "person".to_owned(),
                    attestation_keys: vec![],
                    generate_attestation_keys: 1,
                    attestation_threshold: 1,
                },
            }),
        })?;
        let actor_id = parse_field(&create_output, "actor = ")?;
        let seed_b64 = parse_field(&create_output, "    seed = ")?;
        let initial_attestation_key_id =
            parse_field(&create_output, "    attestation_key_id = ")?;
        let seed_path = store_dir.path().join("att1.seed");
        std::fs::write(&seed_path, seed_b64.as_bytes())?;

        let add_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::AddAttestationKey {
                    command: AddAttestationKeyCommand::Sign {
                        actor: actor_id.clone(),
                        signing_attestation_key_seed: seed_path,
                        key: None,
                        generate: true,
                    },
                },
            }),
        })?;
        assert!(add_output.contains("added attestation key"));
        let signing_key_id = parse_field(&add_output, "signing_attestation_key_id = ")?;
        assert_eq!(signing_key_id, initial_attestation_key_id);
        assert!(add_output.contains("new_attestation_key_id = "));
        assert!(add_output.contains("generated_attestation_seed = "));

        let history_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::KeyHistory {
                    actor: actor_id,
                    json: true,
                },
            }),
        })?;
        let history: serde_json::Value = serde_json::from_str(&history_output)?;
        assert_eq!(history["attestation_adds"].as_array().map(Vec::len), Some(1));
        Ok(())
    }

    /// `revoke-attestation-key sign` round-trips: actor starts with
    /// two attestation keys at threshold 1; revoke the genesis-
    /// declared key signed by the same key (self-revocation), then
    /// `add-attestation-key sign` first to keep the set non-empty.
    /// Validates the resulting attestation set + key-history surface.
    #[test]
    fn actor_revoke_attestation_key_sign_round_trip()
    -> Result<(), Box<dyn std::error::Error>> {
        let store_dir = tempfile::TempDir::new()?;

        // Create actor with attestation key A only, threshold 1.
        let attest_a_seed = [201_u8; 32];
        let attest_a_pub = SigningKey::from_bytes(&attest_a_seed)
            .verifying_key()
            .to_bytes();
        let attest_a_hex: String = attest_a_pub.iter().map(|b| format!("{b:02x}")).collect();

        let create_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Create {
                    kind: "person".to_owned(),
                    attestation_keys: vec![attest_a_hex],
                    generate_attestation_keys: 0,
                    attestation_threshold: 1,
                },
            }),
        })?;
        let actor_id = parse_field(&create_output, "actor = ")?;
        let attest_a_id = kairo_identity::PublicKey::ed25519(attest_a_pub)
            .key_id()
            .to_string();

        let seed_a_path = store_dir.path().join("seed_a.txt");
        std::fs::write(&seed_a_path, STANDARD.encode(attest_a_seed))?;

        // First: revoke A while it's the only key — store must
        // refuse (non-empty-set guard).
        let early = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::RevokeAttestationKey {
                    command: RevokeAttestationKeyCommand::Sign {
                        actor: actor_id.clone(),
                        signing_attestation_key_seed: seed_a_path.clone(),
                        revoke_key: attest_a_id.clone(),
                        reason: None,
                    },
                },
            }),
        });
        assert!(early.is_err(), "revoking only key must fail");

        // Add a replacement key B via add-attestation-key sign
        // (--generate so the seed is printed once and we can use it).
        let add_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::AddAttestationKey {
                    command: AddAttestationKeyCommand::Sign {
                        actor: actor_id.clone(),
                        signing_attestation_key_seed: seed_a_path.clone(),
                        key: None,
                        generate: true,
                    },
                },
            }),
        })?;
        let new_b_seed_b64 = parse_field(&add_output, "generated_attestation_seed = ")?;
        let new_b_id = parse_field(&add_output, "new_attestation_key_id = ")?;

        // Now revoke A with self-signature (signing key A revokes
        // itself). The set after = {B}, threshold = 1 → still
        // satisfies the non-empty guard.
        let revoke_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::RevokeAttestationKey {
                    command: RevokeAttestationKeyCommand::Sign {
                        actor: actor_id.clone(),
                        signing_attestation_key_seed: seed_a_path,
                        revoke_key: attest_a_id.clone(),
                        reason: Some("yubikey lost".to_owned()),
                    },
                },
            }),
        })?;
        assert!(revoke_output.contains("revoked attestation key"));
        assert!(revoke_output.contains(&format!("revoked_key = {attest_a_id}")));

        // key-history JSON shows the revocation entry.
        let history = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::KeyHistory {
                    actor: actor_id,
                    json: true,
                },
            }),
        })?;
        let parsed: serde_json::Value = serde_json::from_str(&history)?;
        let revs = parsed["attestation_revocations"]
            .as_array()
            .expect("attestation_revocations array");
        assert_eq!(revs.len(), 1);
        assert_eq!(revs[0]["revoked_key"], attest_a_id);
        // Confirm B is still in the set via the add entry.
        let adds = parsed["attestation_adds"]
            .as_array()
            .expect("attestation_adds array");
        assert_eq!(adds.len(), 1);
        assert_eq!(adds[0]["new_attestation_key_id"], new_b_id);
        // Suppress unused-variable warning while documenting that B
        // was generated and printed.
        assert!(!new_b_seed_b64.is_empty());
        Ok(())
    }

    /// Programmatic walkthrough of `examples/README.md`. Mirrors the
    /// shell-script flow step-for-step so the README doesn't bit-rot
    /// when CLI verbs change. Each `kairo` invocation in the README
    /// corresponds to a `run(Cli {...})` call here; outputs are
    /// parsed and threaded through the same way the operator would.
    ///
    /// Skips on hosts without `git` on PATH (the example tree depends
    /// on a real Git commit being addressable as `git:sha256:<oid>`).
    #[test]
    fn examples_readme_walkthrough_round_trip() -> Result<(), Box<dyn std::error::Error>> {
        // Aliased so we don't shadow the cli `Command` enum in scope.
        use std::process::Command as ProcCommand;

        // The walkthrough binds the example tree as a real Git commit;
        // skip when git is missing rather than failing.
        if ProcCommand::new("git").arg("--version").output().is_err() {
            eprintln!("skipping: git not available on PATH");
            return Ok(());
        }

        // ---- Setup: temp store + git working tree with kairo.toml. ----
        let store_dir = tempfile::TempDir::new()?;
        let work_dir = tempfile::TempDir::new()?;
        let manifest_path = work_dir.path().join("kairo.toml");
        std::fs::write(
            &manifest_path,
            r#"[kairo]
schema = 1
kind = "software"
name = "hello-kairo"
summary = "Minimal example object used in the end-to-end MVP walkthrough."

[content]
kind = "tree"
"#,
        )?;
        let git = |args: &[&str]| -> Result<String, Box<dyn std::error::Error>> {
            let output = ProcCommand::new("git")
                .current_dir(work_dir.path())
                .args(args)
                .output()?;
            if !output.status.success() {
                return Err(format!(
                    "git {args:?} failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                )
                .into());
            }
            Ok(String::from_utf8(output.stdout)?.trim().to_owned())
        };
        git(&["init", "--initial-branch=main", "--quiet"])?;
        git(&["config", "user.name", "Kairo Test"])?;
        git(&["config", "user.email", "test@kairo.test"])?;
        git(&["config", "commit.gpgsign", "false"])?;
        git(&["add", "kairo.toml"])?;
        git(&["commit", "-m", "init", "--quiet"])?;
        let commit = git(&["rev-parse", "HEAD"])?;
        let revision_ref = format!("git:sha256:{commit}");

        // ---- README step 1: actor create. ----
        let actor_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Create {
                    kind: "person".to_owned(),
                    attestation_keys: vec![],
                    generate_attestation_keys: 1,
                    attestation_threshold: 1,
                },
            }),
        })?;
        let actor_id = parse_field(&actor_output, "actor = ")?;

        // ---- README step 2: object create. ----
        let object_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Object {
                command: ObjectSubcommand::Create {
                    actor: actor_id.clone(),
                    kind: "software".to_owned(),
                    initial_revision: Some(revision_ref.clone()),
                },
            }),
        })?;
        let object_id = parse_field(&object_output, "object = ")?;

        // ---- README step 3: revision create. ----
        let revision_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Revision {
                command: RevisionCommand::Create {
                    actor: actor_id.clone(),
                    object: object_id.clone(),
                    revision: revision_ref.clone(),
                    manifest: manifest_path.clone(),
                    parents: vec![],
                    no_attests_reachable_history: false,
                },
            }),
        })?;
        let statement_id = parse_field(&revision_output, "statement = ")?;

        // ---- README step 4: manifest inspect. ----
        let inspect_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Manifest {
                command: ManifestCommand::Inspect {
                    path: manifest_path.clone(),
                },
            }),
        })?;
        assert!(inspect_output.contains("kind = software"));
        assert!(inspect_output.contains("name = hello-kairo"));

        // ---- README step 5: revision inspect + list. ----
        let revision_inspect = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Revision {
                command: RevisionCommand::Inspect {
                    statement: statement_id.clone(),
                    json: false,
                },
            }),
        })?;
        assert!(revision_inspect.contains(&statement_id));
        let revision_list = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Revision {
                command: RevisionCommand::List {
                    object: object_id.clone(),
                },
            }),
        })?;
        assert!(revision_list.contains(&statement_id));

        // ---- README step 6a: branch set/show/list. ----
        let branch_set = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Branch {
                command: BranchCommand::Set {
                    actor: actor_id.clone(),
                    object: object_id.clone(),
                    revision: statement_id.clone(),
                    name: "head".to_owned(),
                },
            }),
        })?;
        assert!(branch_set.contains("name = head"));
        let branch_show = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Branch {
                command: BranchCommand::Show {
                    object: object_id.clone(),
                    actor: None,
                    name: "head".to_owned(),
                    json: false,
                },
            }),
        })?;
        assert!(branch_show.contains(&statement_id));
        let branch_list = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Branch {
                command: BranchCommand::List {
                    object: object_id.clone(),
                },
            }),
        })?;
        assert!(branch_list.contains(&actor_id));

        // ---- README step 6b: tag bind/show/list. ----
        run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Tag {
                command: TagCommand::Bind {
                    actor: actor_id.clone(),
                    object: object_id.clone(),
                    version: "1.0.0".to_owned(),
                    revision: statement_id.clone(),
                },
            }),
        })?;
        let tag_show = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Tag {
                command: TagCommand::Show {
                    object: object_id.clone(),
                    actor: None,
                    version: "1.0.0".to_owned(),
                    json: false,
                },
            }),
        })?;
        assert!(tag_show.contains(&statement_id));
        let tag_list = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Tag {
                command: TagCommand::List {
                    object: object_id.clone(),
                },
            }),
        })?;
        assert!(tag_list.contains("1.0.0"));

        // ---- README step 6c: snapshot compute (text + json). ----
        let snapshot_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Snapshot {
                command: SnapshotCommand::Compute {
                    object: object_id.clone(),
                    actor: None,
                    name: "head".to_owned(),
                    statement: None,
                    json: false,
                },
            }),
        })?;
        assert!(snapshot_output.contains("snapshot = "));
        let snapshot_json = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Snapshot {
                command: SnapshotCommand::Compute {
                    object: object_id.clone(),
                    actor: None,
                    name: "head".to_owned(),
                    statement: None,
                    json: true,
                },
            }),
        })?;
        let snapshot_parsed: serde_json::Value = serde_json::from_str(&snapshot_json)?;
        assert!(snapshot_parsed["snapshot_id"].as_str().is_some());

        // ---- README step 7: verify object end-to-end. ----
        // Pass --repo explicitly since the test process cwd is not the
        // example tree. With one actor in the keystore, --as is
        // auto-picked, so trust starts at `unknown` (no opinion yet).
        let verify_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Verify {
                command: VerifyCommand::Object {
                    object: object_id.clone(),
                    statement: None,
                    actor: None,
                    name: "head".to_owned(),
                    r#as: None,
                    no_as: false,
                    repo: Some(work_dir.path().to_path_buf()),
                    no_repo: false,
                    no_cache: false,
                    no_cwd_repo: false,
                    manifest: Some(manifest_path.clone()),
                    json: false,
                },
            }),
        })?;
        assert!(verify_output.contains("verify object: VALID"));
        assert!(verify_output.contains("signature = valid"));
        assert!(verify_output.contains("trust = unknown"));

        // ---- README step 8: trust grant + show + list. ----
        run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Trust {
                command: TrustCommand::Grant {
                    by: actor_id.clone(),
                    of: actor_id.clone(),
                    reason: Some("self-trust".to_owned()),
                },
            }),
        })?;
        let trust_show = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Trust {
                command: TrustCommand::Show {
                    by: actor_id.clone(),
                    of: actor_id.clone(),
                    json: false,
                },
            }),
        })?;
        assert!(trust_show.contains("trusted"));
        run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Trust {
                command: TrustCommand::List {
                    by: actor_id.clone(),
                },
            }),
        })?;

        // ---- README: re-verify (trust now `trusted`). ----
        let reverify_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Verify {
                command: VerifyCommand::Object {
                    object: object_id.clone(),
                    statement: None,
                    actor: None,
                    name: "head".to_owned(),
                    r#as: None,
                    no_as: false,
                    repo: Some(work_dir.path().to_path_buf()),
                    no_repo: false,
                    no_cache: false,
                    no_cwd_repo: false,
                    manifest: Some(manifest_path.clone()),
                    json: false,
                },
            }),
        })?;
        assert!(reverify_output.contains("verify object: VALID"));
        assert!(reverify_output.contains("trust = trusted"));

        // ---- README step 9: round-trip via per-record import. ----
        let actor_file = shard_path(store_dir.path(), "actors", &actor_id);
        let object_file = shard_path(store_dir.path(), "objects", &object_id);
        let statement_file = shard_path(store_dir.path(), "statements", &statement_id);
        assert!(actor_file.exists());
        assert!(object_file.exists());
        assert!(statement_file.exists());

        let fresh_store = tempfile::TempDir::new()?;
        run(Cli {
            store: Some(fresh_store.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Import {
                    genesis: actor_file,
                },
            }),
        })?;
        run(Cli {
            store: Some(fresh_store.path().to_path_buf()),
            keys: None,
            command: Some(Command::Object {
                command: ObjectSubcommand::Import {
                    statement: object_file,
                },
            }),
        })?;
        run(Cli {
            store: Some(fresh_store.path().to_path_buf()),
            keys: None,
            command: Some(Command::Revision {
                command: RevisionCommand::Import {
                    statement: statement_file,
                },
            }),
        })?;

        // ---- README step 10: bundle export + import + branch show. ----
        let bundle_dir = tempfile::TempDir::new()?;
        let bundle_root = bundle_dir.path().join("object-bundle");
        run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Bundle {
                command: BundleCommand::Export {
                    object: object_id.clone(),
                    output: bundle_root.clone(),
                    include_git: false,
                },
            }),
        })?;
        let bundled_store = tempfile::TempDir::new()?;
        run(Cli {
            store: Some(bundled_store.path().to_path_buf()),
            keys: None,
            command: Some(Command::Bundle {
                command: BundleCommand::Import {
                    input: bundle_root.clone(),
                },
            }),
        })?;
        let bundled_branch_show = run(Cli {
            store: Some(bundled_store.path().to_path_buf()),
            keys: None,
            command: Some(Command::Branch {
                command: BranchCommand::Show {
                    object: object_id.clone(),
                    actor: None,
                    name: "head".to_owned(),
                    json: false,
                },
            }),
        })?;
        assert!(bundled_branch_show.contains(&statement_id));

        Ok(())
    }

    /// Compute the on-disk shard path for an ID under
    /// `<root>/<type_dir>/<XX>/<YY>/<id>.json`. Mirrors the layout
    /// `kairo-store::shard::shard_path` writes.
    fn shard_path(root: &std::path::Path, type_dir: &str, id: &str) -> std::path::PathBuf {
        root.join(type_dir)
            .join(&id[3..5])
            .join(&id[5..7])
            .join(format!("{id}.json"))
    }

    #[test]
    fn kairo_git_fetch_lands_ref_in_cache() -> Result<(), Box<dyn std::error::Error>> {
        let manifest_text = r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "git-fetch-fixture"
        "#;
        let (src_dir, head_oid) = init_git_repo_with_manifest(manifest_text)?;
        let store_dir = tempfile::TempDir::new()?;
        let url = format!("file://{}", src_dir.path().display());

        let output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Git {
                command: GitCommand::Fetch {
                    object: OBJECT_ID.to_owned(),
                    remote: url.clone(),
                    branch: "main".to_owned(),
                },
            }),
        })?;
        assert!(output.contains("fetched"));
        assert!(output.contains(&format!("object = {OBJECT_ID}")));
        assert!(output.contains(&format!("remote = {url}")));
        assert!(output.contains("ref = refs/heads/main"));
        assert!(output.contains(&format!("oid = {head_oid}")));

        // The fetched ref is reachable in the per-object cache repo.
        let object_repo = kairo_git::object_repo_path(&store_dir.path().join("git"), OBJECT_ID)?;
        let probe = std::process::Command::new("git")
            .arg("-C")
            .arg(&object_repo)
            .args(["rev-parse", "--verify", "refs/heads/main"])
            .output()?;
        assert!(probe.status.success());
        assert_eq!(
            String::from_utf8_lossy(&probe.stdout).trim(),
            head_oid
        );
        Ok(())
    }

    #[test]
    fn kairo_git_fetch_strips_refs_heads_prefix() -> Result<(), Box<dyn std::error::Error>> {
        // Users who copy a fully-qualified ref out of `git branch -a`
        // get the same outcome as the bare `--branch main` form.
        let manifest_text = r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "branch-prefix-fixture"
        "#;
        let (src_dir, head_oid) = init_git_repo_with_manifest(manifest_text)?;
        let store_dir = tempfile::TempDir::new()?;
        let url = format!("file://{}", src_dir.path().display());

        let output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Git {
                command: GitCommand::Fetch {
                    object: OBJECT_ID.to_owned(),
                    remote: url,
                    branch: "refs/heads/main".to_owned(),
                },
            }),
        })?;
        assert!(output.contains("ref = refs/heads/main"));
        assert!(output.contains(&format!("oid = {head_oid}")));
        Ok(())
    }

    #[test]
    fn kairo_git_fetch_unknown_branch_errors() -> Result<(), Box<dyn std::error::Error>> {
        let manifest_text = r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "missing-branch-fixture"
        "#;
        let (src_dir, _head_oid) = init_git_repo_with_manifest(manifest_text)?;
        let store_dir = tempfile::TempDir::new()?;
        let url = format!("file://{}", src_dir.path().display());

        let result = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Git {
                command: GitCommand::Fetch {
                    object: OBJECT_ID.to_owned(),
                    remote: url,
                    branch: "no-such-branch".to_owned(),
                },
            }),
        });
        assert!(result.is_err(), "fetch must fail for missing branch");
        Ok(())
    }

    #[test]
    fn kairo_git_cache_status_empty_cache() -> Result<(), Box<dyn std::error::Error>> {
        // Status against a never-used cache root: prints the path,
        // pool not initialized, zero objects. Does not error and
        // does not require git on PATH for this code path (we
        // never invoke git when the cache root is absent).
        let store_dir = tempfile::TempDir::new()?;
        let output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Git {
                command: GitCommand::Cache {
                    command: GitCacheCommand::Status,
                },
            }),
        })?;
        assert!(output.contains("git cache:"));
        assert!(output.contains("pool: not initialized"));
        assert!(output.contains("objects: 0"));
        Ok(())
    }

    #[test]
    fn kairo_git_cache_status_after_fetch() -> Result<(), Box<dyn std::error::Error>> {
        // After a fetch, status reports the per-object repo and its
        // pinned ref.
        let manifest_text = r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "cache-status-fixture"
        "#;
        let (src_dir, head_oid) = init_git_repo_with_manifest(manifest_text)?;
        let store_dir = tempfile::TempDir::new()?;
        let url = format!("file://{}", src_dir.path().display());

        run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Git {
                command: GitCommand::Fetch {
                    object: OBJECT_ID.to_owned(),
                    remote: url,
                    branch: "main".to_owned(),
                },
            }),
        })?;

        let output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Git {
                command: GitCommand::Cache {
                    command: GitCacheCommand::Status,
                },
            }),
        })?;
        assert!(output.contains("pool: initialized"), "expected initialized pool, got:\n{output}");
        assert!(output.contains("objects: 1"));
        assert!(output.contains(OBJECT_ID));
        assert!(output.contains(&format!("refs/heads/main = {head_oid}")));
        Ok(())
    }

    #[test]
    fn parses_kairo_git_fetch_command() {
        let cli = Cli::try_parse_from([
            "kairo",
            "git",
            "fetch",
            "--object",
            "zQmObject",
            "--remote",
            "https://example.test/repo.git",
            "--branch",
            "main",
        ]);
        assert!(matches!(
            cli,
            Ok(Cli {
                command: Some(Command::Git {
                    command: GitCommand::Fetch { object, remote, branch },
                }),
                ..
            }) if object == "zQmObject"
                && remote == "https://example.test/repo.git"
                && branch == "main"
        ));
    }

    #[test]
    fn parses_kairo_git_cache_status_command() {
        let cli = Cli::try_parse_from(["kairo", "git", "cache", "status"]);
        assert!(matches!(
            cli,
            Ok(Cli {
                command: Some(Command::Git {
                    command: GitCommand::Cache {
                        command: GitCacheCommand::Status,
                    },
                }),
                ..
            })
        ));
    }

    #[test]
    fn kairo_bundle_export_include_git_writes_pack_and_flips_flag(
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Populate the cache, then export with --include-git. The
        // bundle directory must contain `git/<object-id>.pack`, and
        // the manifest must say `git_history.included = true`.
        let manifest_text = r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "include-git-fixture"

            [content]
            kind = "tree"
        "#;
        let (git_dir, commit_oid) = init_git_repo_with_manifest(manifest_text)?;
        let store_dir = tempfile::TempDir::new()?;
        let manifest_path = git_dir.path().join("kairo.toml");

        let actor_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Create {
                    kind: "person".to_owned(),
                    attestation_keys: vec![],
                    generate_attestation_keys: 1,
                    attestation_threshold: 1,
                },
            }),
        })?;
        let actor_id = parse_field(&actor_output, "actor = ")?;
        let object_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Object {
                command: ObjectSubcommand::Create {
                    actor: actor_id.clone(),
                    kind: "software".to_owned(),
                    initial_revision: None,
                },
            }),
        })?;
        let object_id = parse_field(&object_output, "object = ")?;
        let revision_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Revision {
                command: RevisionCommand::Create {
                    actor: actor_id.clone(),
                    object: object_id.clone(),
                    revision: format!("git:sha256:{commit_oid}"),
                    manifest: manifest_path,
                    parents: vec![],
                    no_attests_reachable_history: false,
                },
            }),
        })?;
        let revision_statement = parse_field(&revision_output, "statement = ")?;
        run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Branch {
                command: BranchCommand::Set {
                    actor: actor_id,
                    object: object_id.clone(),
                    revision: revision_statement,
                    name: "head".to_owned(),
                },
            }),
        })?;

        // Populate the cache by fetching from the source repo.
        let url = format!("file://{}", git_dir.path().display());
        run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Git {
                command: GitCommand::Fetch {
                    object: object_id.clone(),
                    remote: url,
                    branch: "main".to_owned(),
                },
            }),
        })?;

        // Export with --include-git.
        let bundle_dir = tempfile::TempDir::new()?;
        let bundle_path = bundle_dir.path().join("bundle");
        let export_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Bundle {
                command: BundleCommand::Export {
                    object: object_id.clone(),
                    output: bundle_path.clone(),
                    include_git: true,
                },
            }),
        })?;
        assert!(export_output.contains("git_history_included = true"));

        // The pack file must exist in the bundle directory.
        let pack_path = bundle_path
            .join("git")
            .join(format!("{object_id}.pack"));
        assert!(pack_path.is_file(), "expected pack at {}", pack_path.display());
        let pack_bytes = std::fs::read(&pack_path)?;
        assert!(!pack_bytes.is_empty(), "pack must not be empty");

        // The manifest must record included = true.
        let manifest_str =
            std::fs::read_to_string(bundle_path.join("manifest.json"))?;
        assert!(
            manifest_str.contains("\"included\": true"),
            "manifest must have included = true: {manifest_str}"
        );
        Ok(())
    }

    #[test]
    fn kairo_bundle_roundtrip_with_git_packs_verifies_without_cwd_repo(
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Export a bundle with --include-git from store A, import
        // into a fresh store B (a different `<store>` directory),
        // then run `kairo verify object --no-cwd-repo` against B.
        // Federation precondition: B reaches VALID without any
        // external Git repo or working tree.
        let manifest_text = r#"
            [kairo]
            schema = 1
            kind = "software"
            name = "roundtrip-fixture"

            [content]
            kind = "tree"
        "#;
        let (git_dir, commit_oid) = init_git_repo_with_manifest(manifest_text)?;

        // ---- Source store A: create actor, object, revision, branch.
        let store_a = tempfile::TempDir::new()?;
        let manifest_path = git_dir.path().join("kairo.toml");

        let actor_output = run(Cli {
            store: Some(store_a.path().to_path_buf()),
            keys: None,
            command: Some(Command::Actor {
                command: ActorCommand::Create {
                    kind: "person".to_owned(),
                    attestation_keys: vec![],
                    generate_attestation_keys: 1,
                    attestation_threshold: 1,
                },
            }),
        })?;
        let actor_id = parse_field(&actor_output, "actor = ")?;
        let object_output = run(Cli {
            store: Some(store_a.path().to_path_buf()),
            keys: None,
            command: Some(Command::Object {
                command: ObjectSubcommand::Create {
                    actor: actor_id.clone(),
                    kind: "software".to_owned(),
                    initial_revision: None,
                },
            }),
        })?;
        let object_id = parse_field(&object_output, "object = ")?;
        let revision_output = run(Cli {
            store: Some(store_a.path().to_path_buf()),
            keys: None,
            command: Some(Command::Revision {
                command: RevisionCommand::Create {
                    actor: actor_id.clone(),
                    object: object_id.clone(),
                    revision: format!("git:sha256:{commit_oid}"),
                    manifest: manifest_path,
                    parents: vec![],
                    no_attests_reachable_history: false,
                },
            }),
        })?;
        let revision_statement = parse_field(&revision_output, "statement = ")?;
        run(Cli {
            store: Some(store_a.path().to_path_buf()),
            keys: None,
            command: Some(Command::Branch {
                command: BranchCommand::Set {
                    actor: actor_id,
                    object: object_id.clone(),
                    revision: revision_statement,
                    name: "head".to_owned(),
                },
            }),
        })?;

        // ---- Populate store A's cache via fetch.
        let url = format!("file://{}", git_dir.path().display());
        run(Cli {
            store: Some(store_a.path().to_path_buf()),
            keys: None,
            command: Some(Command::Git {
                command: GitCommand::Fetch {
                    object: object_id.clone(),
                    remote: url,
                    branch: "main".to_owned(),
                },
            }),
        })?;

        // ---- Export with --include-git.
        let bundle_dir = tempfile::TempDir::new()?;
        let bundle_path = bundle_dir.path().join("bundle");
        run(Cli {
            store: Some(store_a.path().to_path_buf()),
            keys: None,
            command: Some(Command::Bundle {
                command: BundleCommand::Export {
                    object: object_id.clone(),
                    output: bundle_path.clone(),
                    include_git: true,
                },
            }),
        })?;

        // ---- Recipient store B: import the bundle.
        let store_b = tempfile::TempDir::new()?;
        let import_output = run(Cli {
            store: Some(store_b.path().to_path_buf()),
            keys: None,
            command: Some(Command::Bundle {
                command: BundleCommand::Import {
                    input: bundle_path.clone(),
                },
            }),
        })?;
        assert!(import_output.contains("git_packs = 1"));
        assert!(import_output.contains("git_refs_pinned = 1"));

        // ---- Verify against B with --no-cwd-repo: cache is the
        //      only Git source. Must reach VALID.
        let verify_output = run(Cli {
            store: Some(store_b.path().to_path_buf()),
            keys: None,
            command: Some(Command::Verify {
                command: VerifyCommand::Object {
                    object: object_id.clone(),
                    statement: None,
                    actor: None,
                    name: "head".to_owned(),
                    repo: None,
                    no_repo: false,
                    no_cache: false,
                    no_cwd_repo: true,
                    r#as: None,
                    no_as: true,
                    manifest: None,
                    json: false,
                },
            }),
        })?;
        assert!(
            verify_output.contains("verify object: VALID"),
            "expected VALID after bundle import, got:\n{verify_output}"
        );
        assert!(verify_output.contains("content = VALID"));
        assert!(
            verify_output.contains(&format!("commit lookup: cache (object {object_id})")),
            "expected cache lookup, got:\n{verify_output}"
        );

        // ---- B's cache status reflects the imported pack +
        //      pinned ref.
        let status_output = run(Cli {
            store: Some(store_b.path().to_path_buf()),
            keys: None,
            command: Some(Command::Git {
                command: GitCommand::Cache {
                    command: GitCacheCommand::Status,
                },
            }),
        })?;
        assert!(status_output.contains("pool: initialized"));
        assert!(status_output.contains("objects: 1"));
        assert!(status_output.contains(&object_id));
        assert!(
            status_output.contains(&format!(
                "refs/kairo/imported/{commit_oid} = {commit_oid}"
            )),
            "expected pinned imported ref, got:\n{status_output}"
        );
        Ok(())
    }

    #[test]
    fn kairo_bundle_import_without_git_data_skips_cache_open(
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Sanity: a bundle without `git_history.included` must
        // import without touching the cache. `git_packs = 0` and
        // `git_refs_pinned = 0` in the output. The store-only path
        // should not require git on PATH for this code (already
        // tested in earlier verify-object tests for the cache-miss
        // path; this assertion is a regression guard).
        let (store_a, _manifest_dir, _actor_id, object_id, _revision_statement, _manifest_path) =
            fixture_with_branch()?;
        let bundle_dir = tempfile::TempDir::new()?;
        let bundle_path = bundle_dir.path().join("bundle");

        run(Cli {
            store: Some(store_a.path().to_path_buf()),
            keys: None,
            command: Some(Command::Bundle {
                command: BundleCommand::Export {
                    object: object_id.clone(),
                    output: bundle_path.clone(),
                    include_git: false,
                },
            }),
        })?;

        let store_b = tempfile::TempDir::new()?;
        let import_output = run(Cli {
            store: Some(store_b.path().to_path_buf()),
            keys: None,
            command: Some(Command::Bundle {
                command: BundleCommand::Import {
                    input: bundle_path,
                },
            }),
        })?;
        assert!(import_output.contains("git_packs = 0"));
        assert!(import_output.contains("git_refs_pinned = 0"));
        Ok(())
    }

    #[test]
    fn kairo_bundle_export_default_leaves_git_history_excluded(
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Sanity: without --include-git, no `git/` subdir is
        // written and the manifest still says included = false.
        let (store_dir, _manifest_dir, _actor_id, object_id, _revision_statement, _manifest_path) =
            fixture_with_branch()?;
        let bundle_dir = tempfile::TempDir::new()?;
        let bundle_path = bundle_dir.path().join("bundle");

        let export_output = run(Cli {
            store: Some(store_dir.path().to_path_buf()),
            keys: None,
            command: Some(Command::Bundle {
                command: BundleCommand::Export {
                    object: object_id.clone(),
                    output: bundle_path.clone(),
                    include_git: false,
                },
            }),
        })?;
        assert!(export_output.contains("git_history_included = false"));
        assert!(
            !bundle_path.join("git").exists(),
            "no git/ subdir without --include-git"
        );
        let manifest_str =
            std::fs::read_to_string(bundle_path.join("manifest.json"))?;
        assert!(
            manifest_str.contains("\"included\": false"),
            "manifest must have included = false: {manifest_str}"
        );
        Ok(())
    }

    #[test]
    fn kairo_git_fetch_default_branch_is_main() {
        let cli = Cli::try_parse_from([
            "kairo",
            "git",
            "fetch",
            "--object",
            "zQmObject",
            "--remote",
            "https://example.test/repo.git",
        ]);
        assert!(matches!(
            cli,
            Ok(Cli {
                command: Some(Command::Git {
                    command: GitCommand::Fetch { branch, .. },
                }),
                ..
            }) if branch == "main"
        ));
    }
