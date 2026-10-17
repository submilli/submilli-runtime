use super::*;
use sha2::{Digest, Sha256};
use submilli_durable::filesystems::workspace_id;
use submilli_durable::object::ObjectId;
use submilli_durable::recovery::WorkspaceLayout;

use crate::objects::{DurableObjectConfig, ObjectStoreError};
use std::collections::BTreeSet;
use std::io::{Cursor, Read};
use submilli_durable::object::Retention;
use submilli_shared::secret_store::SecretCipher;
mod boundaries;
pub(crate) mod process;

struct Fixture {
    root: tempfile::TempDir,
    database: Arc<ServerDatabase>,
    objects: DurableObjects,
    run: String,
    filesystem: DurableFilesystem,
}

impl Fixture {
    async fn new() -> Self {
        Self::with_maximum_object_bytes(4 << 30).await
    }

    async fn with_maximum_object_bytes(maximum_object_bytes: u64) -> Self {
        Self::with_limits(submilli_durable::publications::ObjectLimits {
            maximum_object_bytes,
            ..Default::default()
        })
        .await
    }

    async fn with_limits(limits: submilli_durable::publications::ObjectLimits) -> Self {
        Self::with_filesystem_limit(limits, None).await
    }

    async fn with_filesystem_limit(
        limits: submilli_durable::publications::ObjectLimits,
        maximum_bytes: Option<u64>,
    ) -> Self {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("objects")).unwrap();
        std::fs::create_dir(root.path().join("ephemeral")).unwrap();
        let database = Arc::new(
            ServerDatabase::open(&root.path().join("state.db"))
                .await
                .unwrap(),
        );
        let objects = DurableObjects::open(
            DurableObjectConfig {
                directory: root.path().join("objects"),
                limits,
                ..Default::default()
            },
            database.clone(),
            Arc::new(SecretCipher::from_key([7; 32])),
        )
        .await
        .unwrap();
        let run = uuid::Uuid::new_v4().to_string();
        let id = run.clone();
        database.transaction(move |connection| Box::pin(async move {
            sqlx::query("INSERT INTO runs(run_id,reservation_id,session_id,fingerprint,owner_generation,admission_order,created_at_unix_ms,state) VALUES(?,?,'session','request','startup',1,0,'running')")
                .bind(&id).bind(id).execute(connection).await?;
            Ok(())
        })).await.unwrap();
        let filesystem = if let Some(maximum_bytes) = maximum_bytes {
            let directory = root.path().join("workspace");
            std::fs::create_dir(&directory).unwrap();
            let vfs = Vfs::external(directory)
                .unwrap()
                .with_size_limit(maximum_bytes);
            DurableFilesystem::open(vfs, &run, "owner", database.clone(), objects.clone())
                .await
                .unwrap()
        } else {
            DurableFilesystem::retained_ephemeral(
                &root.path().join("ephemeral"),
                &run,
                "owner",
                database.clone(),
                objects.clone(),
            )
            .await
            .unwrap()
        };
        Self {
            root,
            database,
            objects,
            run,
            filesystem,
        }
    }

    async fn capture(&self, bytes: &[u8]) -> Content {
        let id = DurableObjects::new_id().unwrap();
        self.objects
            .reserve(
                id.clone(),
                "owner".into(),
                Sha256::digest(bytes).into(),
                bytes.len() as u64,
            )
            .await
            .unwrap();
        let prepared = self
            .objects
            .publish(&id, Cursor::new(bytes.to_vec()))
            .await
            .unwrap();
        let reference = self
            .objects
            .bind(
                &prepared,
                BTreeSet::from([Retention::Run(self.run.clone())]),
            )
            .await
            .unwrap();
        Content {
            id: reference.id.as_str().into(),
            bytes: reference.content.bytes,
            sha256: reference.content.sha256,
        }
    }

    async fn terminal(&self) {
        let id = self.run.clone();
        self.database
            .transaction(move |connection| {
                Box::pin(async move {
                    sqlx::query(
                        "UPDATE runs SET state='finished',completed_at_unix_ms=1 WHERE run_id=?",
                    )
                    .bind(id)
                    .execute(connection)
                    .await?;
                    Ok(())
                })
            })
            .await
            .unwrap();
    }
}

async fn publication_without_receipt(
    filesystem: &DurableFilesystem,
    effect: &str,
    request: Mutation,
) {
    let gate = Arc::new(
        filesystem
            .test_objects()
            .test_gate()
            .clone()
            .lock_owned()
            .await,
    );
    filesystem
        .test_journal()
        .admit(filesystem.id(), effect, &request)
        .await
        .unwrap();
    let workspace = filesystem.test_workspace().clone();
    let mut source = filesystem.test_content(&request).unwrap();
    let effect = effect.to_owned();
    let intent = blocking(gate.clone(), move || {
        workspace.prepare(&effect, request, &mut source)
    })
    .await
    .unwrap();
    filesystem
        .test_journal()
        .prepared(filesystem.id(), &intent)
        .await
        .unwrap();
    let workspace = filesystem.test_workspace().clone();
    let mut source = filesystem.test_content(intent.mutation()).unwrap();
    blocking(gate, move || workspace.apply(&intent, &mut source))
        .await
        .unwrap();
}

#[tokio::test]
async fn sqlite_receipt_suppresses_completed_mutation_after_path_reuse() {
    let fixture = Fixture::new().await;
    let content = fixture.capture(b"original").await;
    let effect = uuid::Uuid::new_v4().to_string();
    let request = Mutation::Write {
        path: "/file".into(),
        content,
    };
    fixture
        .filesystem
        .execute(&effect, request.clone())
        .await
        .unwrap();
    let replacement = fixture.capture(b"newer").await;
    fixture
        .filesystem
        .execute(
            &uuid::Uuid::new_v4().to_string(),
            Mutation::Write {
                path: "/file".into(),
                content: replacement,
            },
        )
        .await
        .unwrap();
    let restored = DurableFilesystem::resume(
        fixture.filesystem.id(),
        "owner",
        fixture.database.clone(),
        fixture.objects.clone(),
    )
    .await
    .unwrap();
    restored.execute(&effect, request.clone()).await.unwrap();
    assert_eq!(
        std::fs::read(restored.vfs().root().join("file")).unwrap(),
        b"newer"
    );
    assert!(
        restored
            .execute(
                &effect,
                Mutation::Remove {
                    path: "/file".into()
                }
            )
            .await
            .is_err()
    );
    assert!(restored.recover().await.unwrap().is_none());
}

#[tokio::test]
async fn append_publication_gap_recovers_once_and_blocks_other_mutations() {
    let fixture = Fixture::new().await;
    let initial = fixture.capture(b"before").await;
    fixture
        .filesystem
        .execute(
            &uuid::Uuid::new_v4().to_string(),
            Mutation::Write {
                path: "/file".into(),
                content: initial,
            },
        )
        .await
        .unwrap();
    let content = fixture.capture(b" suffix").await;
    let effect = uuid::Uuid::new_v4().to_string();
    let request = Mutation::Append {
        path: "/file".into(),
        content,
    };
    publication_without_receipt(&fixture.filesystem, &effect, request.clone()).await;
    assert!(
        fixture
            .filesystem
            .execute(
                &uuid::Uuid::new_v4().to_string(),
                Mutation::Remove {
                    path: "/file".into()
                }
            )
            .await
            .is_err()
    );
    let restored = DurableFilesystem::resume(
        fixture.filesystem.id(),
        "owner",
        fixture.database.clone(),
        fixture.objects.clone(),
    )
    .await
    .unwrap();
    restored.recover().await.unwrap().unwrap();
    restored.execute(&effect, request).await.unwrap();
    assert_eq!(
        std::fs::read(restored.vfs().root().join("file")).unwrap(),
        b"before suffix"
    );
}

#[tokio::test]
async fn rejected_preflight_is_replayed_even_when_the_path_later_exists() {
    let fixture = Fixture::new().await;
    let effect = uuid::Uuid::new_v4().to_string();
    let request = Mutation::Remove {
        path: "/missing".into(),
    };
    assert!(matches!(
        fixture.filesystem.execute(&effect, request.clone()).await,
        Err(FilesystemError::Rejected(_))
    ));
    let content = fixture.capture(b"later").await;
    fixture
        .filesystem
        .execute(
            &uuid::Uuid::new_v4().to_string(),
            Mutation::Write {
                path: "/missing".into(),
                content,
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        fixture.filesystem.execute(&effect, request).await,
        Err(FilesystemError::Rejected(_))
    ));
    assert_eq!(
        std::fs::read(fixture.filesystem.vfs().root().join("missing")).unwrap(),
        b"later"
    );
}

#[tokio::test]
async fn ephemeral_drop_retains_workspace_and_only_terminal_retirement_removes_it() {
    let fixture = Fixture::new().await;
    let root = fixture.filesystem.vfs().root().to_owned();
    let id = fixture.filesystem.id().to_owned();
    assert!(fixture.filesystem.retire("owner").await.is_err());
    let content = fixture.capture(b"captured").await;
    let object_id = ObjectId::parse(content.id.clone()).unwrap();
    fixture
        .filesystem
        .execute(
            &uuid::Uuid::new_v4().to_string(),
            Mutation::Write {
                path: "/file".into(),
                content,
            },
        )
        .await
        .unwrap();
    fixture
        .objects
        .release(&object_id, &Retention::Run(fixture.run.clone()))
        .await
        .unwrap();
    assert!(fixture.objects.collect(&object_id).await.is_err());
    fixture.terminal().await;
    fixture.filesystem.retire("owner").await.unwrap();
    assert!(!root.exists());
    fixture.objects.collect(&object_id).await.unwrap();
    DurableFilesystem::recover_cleanup(
        &id,
        "owner",
        fixture.database.clone(),
        fixture.objects.clone(),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn stored_paths_and_payloads_are_sealed_and_foreign_databases_are_rejected() {
    let fixture = Fixture::new().await;
    let secret = b"filesystem-plaintext-canary-94c2";
    let content = fixture.capture(secret).await;
    fixture
        .filesystem
        .execute(
            &uuid::Uuid::new_v4().to_string(),
            Mutation::Write {
                path: "/private-path-canary".into(),
                content,
            },
        )
        .await
        .unwrap();
    for name in ["state.db", "state.db-wal"] {
        let bytes = std::fs::read(fixture.root.path().join(name)).unwrap();
        assert!(!bytes.windows(secret.len()).any(|window| window == secret));
        assert!(
            !bytes
                .windows(19)
                .any(|window| window == b"private-path-canary")
        );
    }
    let foreign = Fixture::new().await;
    assert!(matches!(
        DurableFilesystem::resume(
            fixture.filesystem.id(),
            "owner",
            fixture.database.clone(),
            foreign.objects.clone()
        )
        .await,
        Err(FilesystemError::Configuration(_))
    ));
}

#[tokio::test]
async fn terminal_cleanup_handles_cancelled_bootstrap_and_missing_cwd() {
    let fixture = Fixture::new().await;
    let id = fixture.filesystem.id().to_owned();
    let cwd = fixture.filesystem.vfs().root().join("cwd");
    std::fs::create_dir(&cwd).unwrap();
    let layout =
        WorkspaceLayout::capture(&fixture.filesystem.vfs().clone().with_cwd("/cwd").unwrap())
            .unwrap();
    let stored = fixture.filesystem.test_journal().clone();
    // Store the layout as it would have been admitted, then simulate deletion
    // of cwd before the cleanup's final root unlink.
    let bytes = fixture
        .objects
        .sealer()
        .seal(
            &format!("layout:{id}"),
            &serde_json::to_vec(&layout).unwrap(),
        )
        .unwrap();
    let sql_id = id.clone();
    fixture
        .database
        .transaction(move |connection| {
            Box::pin(async move {
                sqlx::query("UPDATE durable_workspaces SET layout=? WHERE workspace_id=?")
                    .bind(bytes)
                    .bind(sql_id)
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .await
        .unwrap();
    fixture.terminal().await;
    stored.retire(&id, "owner").await.unwrap();
    std::fs::remove_dir(cwd).unwrap();
    DurableFilesystem::recover_cleanup(
        &id,
        "owner",
        fixture.database.clone(),
        fixture.objects.clone(),
    )
    .await
    .unwrap();
    assert!(!fixture.filesystem.vfs().root().exists());

    let bootstrap = Fixture::new().await;
    let id = bootstrap.filesystem.id().to_owned();
    // No public contents are possible before registration of the full layout.
    bootstrap.filesystem.test_workspace().retire().unwrap();
    std::fs::remove_dir(
        bootstrap
            .filesystem
            .vfs()
            .root()
            .join(".git-submilli-recovery"),
    )
    .unwrap();
    let sql_id = id.clone();
    bootstrap.database.transaction(move |connection| Box::pin(async move {
        sqlx::query("UPDATE durable_workspaces SET state='preparing',identity=NULL,layout=NULL WHERE workspace_id=?").bind(sql_id).execute(connection).await?;
        Ok(())
    })).await.unwrap();
    bootstrap.terminal().await;
    DurableFilesystem::recover_cleanup(
        &id,
        "owner",
        bootstrap.database.clone(),
        bootstrap.objects.clone(),
    )
    .await
    .unwrap();
    assert!(!bootstrap.filesystem.vfs().root().exists());
    assert_eq!(
        bootstrap
            .filesystem
            .test_journal()
            .workspace(&id, "owner")
            .await
            .unwrap()
            .state,
        "gone"
    );
}

#[tokio::test]
async fn terminal_runs_refuse_new_effects_and_wrong_owner_cannot_fence_cleanup() {
    let fixture = Fixture::new().await;
    fixture.terminal().await;
    assert!(fixture.filesystem.retire("wrong-owner").await.is_err());
    assert_eq!(
        fixture
            .filesystem
            .test_journal()
            .workspace(fixture.filesystem.id(), "owner")
            .await
            .unwrap()
            .state,
        "active"
    );
    assert!(
        fixture
            .filesystem
            .execute(
                &uuid::Uuid::new_v4().to_string(),
                Mutation::CreateDirectory {
                    path: "/late".into()
                }
            )
            .await
            .is_err()
    );
    assert!(!fixture.filesystem.vfs().root().join("late").exists());
}

#[tokio::test]
async fn writer_chunks_survive_pause_and_duplicate_close_never_republishes() {
    let fixture = Fixture::new().await;
    let resource = uuid::Uuid::new_v4().to_string();
    let chunk = uuid::Uuid::new_v4().to_string();
    let close = uuid::Uuid::new_v4().to_string();
    fixture
        .filesystem
        .open_writer(&resource, "/out")
        .await
        .unwrap();
    let first = fixture.capture(b"first").await;
    fixture
        .filesystem
        .write_chunk(&resource, &chunk, first.clone())
        .await
        .unwrap();
    let resumed = DurableFilesystem::resume(
        fixture.filesystem.id(),
        "owner",
        fixture.database.clone(),
        fixture.objects.clone(),
    )
    .await
    .unwrap();
    resumed.write_chunk(&resource, &chunk, first).await.unwrap();
    let second = fixture.capture(b" second").await;
    resumed
        .write_chunk(&resource, &uuid::Uuid::new_v4().to_string(), second)
        .await
        .unwrap();
    resumed
        .test_journal()
        .closing_writer(
            resumed.id(),
            &resource,
            &close,
            resumed.test_objects().maximum_bytes(),
        )
        .await
        .unwrap();
    assert!(
        resumed
            .execute(
                &uuid::Uuid::new_v4().to_string(),
                Mutation::Remove {
                    path: "/out".into()
                }
            )
            .await
            .is_err()
    );
    resumed.recover().await.unwrap();
    assert_eq!(
        std::fs::read(resumed.vfs().root().join("out")).unwrap(),
        b"first second"
    );
    let newer = fixture.capture(b"newer").await;
    resumed
        .execute(
            &uuid::Uuid::new_v4().to_string(),
            Mutation::Write {
                path: "/out".into(),
                content: newer,
            },
        )
        .await
        .unwrap();
    resumed.close_writer(&resource, &close).await.unwrap();
    assert_eq!(
        std::fs::read(resumed.vfs().root().join("out")).unwrap(),
        b"newer"
    );
    fixture.terminal().await;
    fixture.filesystem.retire("owner").await.unwrap();
}

#[tokio::test]
async fn reader_checkpoint_restores_original_inode_and_cleanup_waits_for_native_reader() {
    let fixture = Fixture::new().await;
    let content = fixture.capture(b"original").await;
    fixture
        .filesystem
        .execute(
            &uuid::Uuid::new_v4().to_string(),
            Mutation::Write {
                path: "/file".into(),
                content,
            },
        )
        .await
        .unwrap();
    let mut reader = fixture
        .filesystem
        .open_reader(&uuid::Uuid::new_v4().to_string(), "/file")
        .await
        .unwrap();
    let mut prefix = [0; 3];
    reader.read_exact(&mut prefix).unwrap();
    let position = reader.position().unwrap();
    drop(reader);
    let content = fixture.capture(b"replacement").await;
    fixture
        .filesystem
        .execute(
            &uuid::Uuid::new_v4().to_string(),
            Mutation::Write {
                path: "/file".into(),
                content,
            },
        )
        .await
        .unwrap();
    let mut reader = fixture.filesystem.restore_reader(position).await.unwrap();
    let mut rest = String::new();
    reader.read_to_string(&mut rest).unwrap();
    assert_eq!(rest, "ginal");
    fixture.terminal().await;
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(20),
            fixture.filesystem.retire("owner")
        )
        .await
        .is_err()
    );
    assert!(fixture.filesystem.vfs().root().exists());
    drop(reader);
    fixture.filesystem.retire("owner").await.unwrap();
}

#[tokio::test]
async fn compound_plan_continues_a_committed_prefix_and_replays_a_failure() {
    let fixture = Fixture::new().await;
    let content = fixture.capture(b"child").await;
    let plan = uuid::Uuid::new_v4().to_string();
    let steps = vec![
        Mutation::CreateDirectory {
            path: "/tree".into(),
        },
        Mutation::Write {
            path: "/tree/child".into(),
            content,
        },
        Mutation::Remove {
            path: "/missing".into(),
        },
    ];
    fixture
        .filesystem
        .test_journal()
        .plan(fixture.filesystem.id(), &plan, &steps)
        .await
        .unwrap();
    let gate = Arc::new(fixture.objects.test_gate().clone().lock_owned().await);
    let effect = workspace_id(&format!("plan:{plan}:0"), fixture.filesystem.id());
    fixture
        .filesystem
        .test_execute_step(gate.clone(), &effect, steps[0].clone(), Some(&plan), false)
        .await
        .unwrap();
    drop(gate);
    assert!(
        fixture
            .filesystem
            .execute(
                &uuid::Uuid::new_v4().to_string(),
                Mutation::Remove {
                    path: "/tree".into()
                }
            )
            .await
            .is_err()
    );
    let restored = DurableFilesystem::resume(
        fixture.filesystem.id(),
        "owner",
        fixture.database.clone(),
        fixture.objects.clone(),
    )
    .await
    .unwrap();
    assert!(matches!(
        restored.recover().await,
        Err(FilesystemError::Rejected(_))
    ));
    assert_eq!(
        std::fs::read(restored.vfs().root().join("tree/child")).unwrap(),
        b"child"
    );
    std::fs::write(restored.vfs().root().join("missing"), b"newer").unwrap();
    assert!(matches!(
        restored.execute_plan(&plan, steps).await,
        Err(FilesystemError::Rejected(_))
    ));
    assert!(restored.vfs().root().join("missing").exists());
    fixture.terminal().await;
    restored.retire("owner").await.unwrap();
}

#[tokio::test]
async fn directory_snapshot_restores_order_after_directory_changes() {
    let fixture = Fixture::new().await;
    std::fs::create_dir(fixture.filesystem.vfs().root().join("tree")).unwrap();
    std::fs::write(fixture.filesystem.vfs().root().join("tree/a"), b"a").unwrap();
    std::fs::write(fixture.filesystem.vfs().root().join("tree/b"), b"b").unwrap();
    let resource = uuid::Uuid::new_v4().to_string();
    let mut listing = fixture
        .filesystem
        .open_directory(&resource, "/tree", true, 0)
        .await
        .unwrap();
    assert_eq!(listing.next().unwrap().path, "/tree/a");
    let offset = listing.position();
    drop(listing);
    std::fs::remove_file(fixture.filesystem.vfs().root().join("tree/b")).unwrap();
    std::fs::write(fixture.filesystem.vfs().root().join("tree/c"), b"c").unwrap();
    let resumed = DurableFilesystem::resume(
        fixture.filesystem.id(),
        "owner",
        fixture.database.clone(),
        fixture.objects.clone(),
    )
    .await
    .unwrap();
    let mut listing = resumed
        .open_directory(&resource, "/tree", true, offset)
        .await
        .unwrap();
    assert_eq!(listing.next().unwrap().path, "/tree/b");
    assert!(listing.next().is_none());
    assert!(
        resumed
            .open_directory(&resource, "/tree", false, 0)
            .await
            .is_err()
    );
    fixture.terminal().await;
    resumed.retire("owner").await.unwrap();
    fixture.objects.recover().await.unwrap();
}

#[tokio::test]
async fn prepared_git_publication_recovers_the_same_commit() {
    fn git(directory: &std::path::Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .current_dir(directory)
            .args(args)
            .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z")
            .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().into()
    }
    let fixture = Fixture::new().await;
    let staged = tempfile::tempdir().unwrap();
    git(staged.path(), &["init", "--quiet", "-b", "main"]);
    git(
        staged.path(),
        &[
            "-c",
            "user.name=Durability Test",
            "-c",
            "user.email=durability@example.invalid",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "prepared",
        ],
    );
    let commit = git(staged.path(), &["rev-parse", "HEAD"]);
    let target = fixture.filesystem.vfs().root().join("repo");
    std::fs::create_dir(&target).unwrap();
    git(&target, &["init", "--quiet", "-b", "main"]);
    let mut steps = Vec::new();
    for prefix in std::fs::read_dir(staged.path().join(".git/objects")).unwrap() {
        let prefix = prefix.unwrap();
        let name = prefix.file_name().into_string().unwrap();
        if name.len() != 2 {
            continue;
        }
        steps.push(Mutation::GitDirectory {
            repository: "/repo".into(),
            path: format!("/repo/.git/objects/{name}"),
        });
        for object in std::fs::read_dir(prefix.path()).unwrap() {
            let object = object.unwrap();
            let content = fixture
                .capture(&std::fs::read(object.path()).unwrap())
                .await;
            steps.push(Mutation::GitWrite {
                executable: false,
                repository: "/repo".into(),
                path: format!(
                    "/repo/.git/objects/{name}/{}",
                    object.file_name().to_str().unwrap()
                ),
                content,
            });
        }
    }
    let content = fixture.capture(format!("{commit}\n").as_bytes()).await;
    steps.push(Mutation::GitWrite {
        executable: false,
        repository: "/repo".into(),
        path: "/repo/.git/refs/heads/main".into(),
        content,
    });
    let plan = uuid::Uuid::new_v4().to_string();
    fixture
        .filesystem
        .test_journal()
        .plan(fixture.filesystem.id(), &plan, &steps)
        .await
        .unwrap();
    let gate = Arc::new(fixture.objects.test_gate().clone().lock_owned().await);
    for (index, step) in steps.iter().take(2).enumerate() {
        let effect = workspace_id(&format!("plan:{plan}:{index}"), fixture.filesystem.id());
        fixture
            .filesystem
            .test_execute_step(gate.clone(), &effect, step.clone(), Some(&plan), true)
            .await
            .unwrap();
    }
    drop(gate);
    let resumed = DurableFilesystem::resume(
        fixture.filesystem.id(),
        "owner",
        fixture.database.clone(),
        fixture.objects.clone(),
    )
    .await
    .unwrap();
    resumed.recover().await.unwrap();
    assert_eq!(git(&target, &["rev-parse", "HEAD"]), commit);
    resumed
        .publish_git_plan(&plan, "/repo", steps)
        .await
        .unwrap();
    assert_eq!(git(&target, &["rev-parse", "HEAD"]), commit);
    assert!(
        resumed
            .execute(
                &uuid::Uuid::new_v4().to_string(),
                Mutation::GitRemove {
                    repository: "/repo".into(),
                    path: "/outside".into()
                }
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn filesystem_uses_the_shared_run_effect_identity_and_payload() {
    use submilli_durable::effects::{self, EffectKey};

    let fixture = Fixture::new().await;
    let content = fixture.capture(b"shared effect bytes").await;
    fixture
        .filesystem
        .execute(
            "bb54bf79-dcbc-4c79-9483-7a498bf08f0a",
            Mutation::Write {
                path: "/shared".into(),
                content,
            },
        )
        .await
        .unwrap();
    let run = fixture.run.clone();
    let workspace = fixture.filesystem.id().to_owned();
    let mut unit = fixture.objects.units(fixture.database.clone())()
        .await
        .unwrap();
    let key = EffectKey {
        run: &run,
        effect: "bb54bf79-dcbc-4c79-9483-7a498bf08f0a",
        operation: "filesystem.mutation",
        version: 1,
    };
    let record = effects::load(unit.as_mut(), &key).await.unwrap().unwrap();
    let request: serde_json::Value = serde_json::from_slice(&record.request).unwrap();
    assert_eq!(request["workspace"], workspace);
    let intent =
        submilli_durable::recovery::Intent::decode(record.recovery_payload.as_ref().unwrap())
            .unwrap();
    assert_eq!(intent.effect(), "bb54bf79-dcbc-4c79-9483-7a498bf08f0a");
    assert!(record.outcome.is_some());
    let foreign = EffectKey {
        operation: "other.operation",
        ..key
    };
    assert!(
        effects::admit(unit.as_mut(), &foreign, &record.request)
            .await
            .is_err()
    );
    drop(unit);
    let duplicate_table: bool = fixture.database.read(|connection| Box::pin(async move {
        Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='durable_filesystem_effects')").fetch_one(connection).await?)
    })).await.unwrap();
    assert!(!duplicate_table);
}

async fn blocking<T: Send + 'static>(
    guard: Arc<tokio::sync::OwnedMutexGuard<()>>,
    operation: impl FnOnce() -> Result<T, RecoveryError> + Send + 'static,
) -> Result<T, FilesystemError> {
    tokio::task::spawn_blocking(move || {
        let _guard = guard;
        operation()
    })
    .await
    .map_err(|e| FilesystemError::Worker(e.to_string()))?
    .map_err(Into::into)
}

#[tokio::test]
async fn interpreter_uses_durable_backend_and_replay_does_not_repeat_append() {
    let fixture = Fixture::new().await;
    let source = r#"import { appendText } from "submilli:fs";
        function main(): void { appendText("/port.txt", "once"); }"#;
    run_backend(
        &fixture,
        source,
        Some("00000000-0000-4000-8000-000000000031"),
    )
    .await
    .unwrap();
    run_backend(
        &fixture,
        source,
        Some("00000000-0000-4000-8000-000000000031"),
    )
    .await
    .unwrap();
    assert_eq!(
        std::fs::read(fixture.filesystem.vfs().root().join("port.txt")).unwrap(),
        b"once"
    );
    let factory = fixture.objects.units(fixture.database.clone());
    let mut unit = factory().await.unwrap();
    assert_eq!(unit.effect_count(&fixture.run).await.unwrap(), 1);
    let effect = unit
        .get_effect(&fixture.run, "00000000-0000-4000-8000-000000000031")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(effect.state(), "done");
    drop(unit);
    let error = run_backend(&fixture, source, None).await.unwrap_err();
    assert!(
        error.to_string().contains("stable operation identity"),
        "{error}"
    );
    assert_eq!(
        std::fs::read(fixture.filesystem.vfs().root().join("port.txt")).unwrap(),
        b"once"
    );
}

#[tokio::test]
async fn code_tools_remain_native_with_a_durable_filesystem_backend() {
    let fixture = Fixture::new().await;
    std::fs::write(fixture.filesystem.vfs().root().join("note.ts"), b"before\n").unwrap();
    let source = r#"import { edit, read, glob } from "submilli:code";
        function main(): void {
            assert(edit("/note.ts", "before", "after").changed);
            assert(read("/note.ts").lines[0].text === "after");
            assert(glob("/*.ts").entries.length === 1);
        }"#;
    run_backend(&fixture, source, None).await.unwrap();
    assert_eq!(
        std::fs::read(fixture.filesystem.vfs().root().join("note.ts")).unwrap(),
        b"after\n"
    );
    let mut unit = fixture.objects.begin().await.unwrap();
    assert_eq!(unit.effect_count(&fixture.run).await.unwrap(), 0);
}

#[tokio::test]
async fn durable_backend_refuses_unprepared_recursive_mutations() {
    let fixture = Fixture::new().await;
    let source = r#"import { mkdir } from "submilli:fs";
        function main(): void { mkdir("/unprepared/child", true); }"#;
    assert!(
        run_backend(
            &fixture,
            source,
            Some("00000000-0000-4000-8000-000000000032")
        )
        .await
        .is_err()
    );
    assert!(std::fs::read(fixture.filesystem.vfs().root().join("unprepared/child")).is_err());
    let mut unit = fixture.objects.begin().await.unwrap();
    assert_eq!(unit.effect_count(&fixture.run).await.unwrap(), 0);
}

#[tokio::test]
async fn failed_effect_admission_rolls_back_its_object_retention() {
    let fixture = Fixture::new().await;
    let content = fixture.capture(b"atomic").await;
    fixture.database.transaction(|connection| Box::pin(async move {
        sqlx::query("CREATE TRIGGER refuse_effect BEFORE INSERT ON durable_effects BEGIN SELECT RAISE(FAIL, 'injected admission failure'); END")
            .execute(connection).await?;
        Ok(())
    })).await.unwrap();
    assert!(
        fixture
            .filesystem
            .execute(
                "00000000-0000-4000-8000-000000000033",
                Mutation::Write {
                    path: "/atomic".into(),
                    content: content.clone()
                }
            )
            .await
            .is_err()
    );
    let mut unit = fixture.objects.begin().await.unwrap();
    let object = unit
        .get_object(&ObjectId::parse(content.id).unwrap())
        .await
        .unwrap()
        .unwrap();
    assert!(
        !object
            .retention()
            .contains(&Retention::Workspace(fixture.filesystem.id().into()))
    );
    assert_eq!(unit.effect_count(&fixture.run).await.unwrap(), 0);
    drop(unit);
    assert!(std::fs::read(fixture.filesystem.vfs().root().join("atomic")).is_err());
}

async fn run_backend(
    fixture: &Fixture,
    source: &str,
    operation: Option<&str>,
) -> wasmtime::Result<Option<String>> {
    run_backend_fueled(fixture, source, operation, None).await
}

async fn run_backend_fueled(
    fixture: &Fixture,
    source: &str,
    operation: Option<&str>,
    fuel: Option<u64>,
) -> wasmtime::Result<Option<String>> {
    use interpreter::runtime::{
        RuntimeConfig, StoreData, dispatch_main_async, install_runtime_async,
    };
    let compiled =
        interpreter::compile_script(source, "backend.ts", interpreter::FileId(0), &[], &[])
            .unwrap();
    let config = RuntimeConfig::default();
    let engine = config.engine().unwrap();
    let mut data = StoreData::with_vfs_and_cap(fixture.filesystem.vfs().clone(), 64 << 20);
    data.filesystem = Arc::new(fixture.filesystem.0.clone());
    data.filesystem_invocation.operation = operation.map(str::to_owned);
    let mut store = config.store(&engine, data).unwrap();
    let module = wasmtime::Module::new(&engine, &compiled.wasm).unwrap();
    let mut linker = wasmtime::Linker::<StoreData>::new(&engine);
    install_runtime_async(&mut linker, &mut store)
        .await
        .unwrap();
    let instance = linker.instantiate_async(&mut store, &module).await.unwrap();
    if let Some(fuel) = fuel {
        store.set_fuel(fuel).unwrap();
    }
    dispatch_main_async(&mut store, &instance).await
}

#[tokio::test]
async fn durable_refusals_remain_catchable_in_guest_code() {
    let fixture = Fixture::new().await;
    std::fs::write(fixture.filesystem.vfs().root().join("existing"), b"keep").unwrap();
    let source = r#"import { mkdir } from "submilli:fs";
        function main(): void {
            let caught = false;
            try { mkdir("/existing", false); } catch (error: Error) { caught = true; }
            assert(caught, "a recorded preflight refusal is catchable");
        }"#;
    run_backend(
        &fixture,
        source,
        Some("00000000-0000-4000-8000-000000000041"),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn durable_listing_keeps_native_sizes_and_consecutive_subtrees() {
    let fixture = Fixture::new().await;
    let root = fixture.filesystem.vfs().root();
    std::fs::create_dir(root.join("a")).unwrap();
    std::fs::write(root.join("a/z"), "child").unwrap();
    std::fs::write(root.join("a.txt"), "sibling").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("a.txt", root.join("link")).unwrap();
    let source = r#"import { list } from "submilli:fs";
        function main(): void {
            let sequence = "";
            for (const entry of list("/", true)) {
                if (entry.kind !== "file") { assert(entry.size === 0, "non-file size"); }
                sequence += entry.name + ",";
            }
            assert(sequence.indexOf("a,z,") >= 0, "directory subtree stays consecutive");
        }"#;
    run_backend(
        &fixture,
        source,
        Some("00000000-0000-4000-8000-000000000042"),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn durable_append_and_eager_listing_admit_their_own_fuel() {
    let fixture = Fixture::new().await;
    let root = fixture.filesystem.vfs().root();
    let original = vec![b'x'; 64 * 1024];
    std::fs::write(root.join("large"), &original).unwrap();
    let append = r#"import { appendText } from "submilli:fs";
        function main(): void { appendText("/large", ""); }"#;
    let error = run_backend_fueled(
        &fixture,
        append,
        Some("00000000-0000-4000-8000-000000000043"),
        Some(10_000),
    )
    .await
    .unwrap_err();
    assert!(error.is::<wasmtime::Trap>(), "{error}");
    assert_eq!(std::fs::read(root.join("large")).unwrap(), original);
    // The admitted identity is still resumable after fuel is replenished.
    run_backend(
        &fixture,
        append,
        Some("00000000-0000-4000-8000-000000000043"),
    )
    .await
    .unwrap();
    for index in 0..100 {
        std::fs::write(root.join(format!("entry-{index}")), "").unwrap();
    }
    let listing = r#"import { list } from "submilli:fs";
        function main(): void { list("/", false); }"#;
    let error = run_backend_fueled(
        &fixture,
        listing,
        Some("00000000-0000-4000-8000-000000000044"),
        Some(10_000),
    )
    .await
    .unwrap_err();
    assert!(error.is::<wasmtime::Trap>(), "{error}");
}

#[tokio::test]
async fn payload_capture_holds_retirement_exclusion_at_every_transaction() {
    use interpreter::runtime::filesystem::{Filesystem, Invocation};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    let fixture = Fixture::new().await;
    let active = Arc::new(AtomicBool::new(false));
    let checked = Arc::new(AtomicUsize::new(0));
    let gate = fixture.objects.test_gate().clone();
    let delegate = fixture.objects.units(fixture.database.clone());
    let factory: submilli_durable::UnitFactory = Arc::new({
        let active = active.clone();
        let checked = checked.clone();
        move || {
            if active.load(Ordering::SeqCst) {
                assert!(
                    gate.try_lock().is_err(),
                    "retirement must not interleave with capture admission or publication"
                );
                checked.fetch_add(1, Ordering::SeqCst);
            }
            delegate()
        }
    });
    let filesystem = submilli_durable::filesystems::DurableFilesystem::open(
        fixture.filesystem.vfs().clone(),
        &fixture.run,
        "owner",
        factory,
        fixture.objects.inner().clone(),
    )
    .await
    .unwrap();
    active.store(true, Ordering::SeqCst);
    filesystem
        .write(
            &Invocation {
                operation: Some("00000000-0000-4000-8000-000000000045".into()),
                ..Default::default()
            },
            "/captured",
            b"retained".to_vec(),
            false,
        )
        .await
        .unwrap();
    assert!(checked.load(Ordering::SeqCst) > 1);
}

#[tokio::test]
async fn durable_quota_scan_stops_before_mutation_and_resumes_with_fuel() {
    let fixture = Fixture::with_filesystem_limit(Default::default(), Some(1 << 20)).await;
    let root = fixture.filesystem.vfs().root();
    for index in 0..100 {
        std::fs::write(root.join(format!("entry-{index}")), "").unwrap();
    }
    let source = r#"import { mkdir } from "submilli:fs";
        function main(): void { mkdir("/created", false); }"#;
    let operation = Some("00000000-0000-4000-8000-000000000045");
    let error = run_backend_fueled(&fixture, source, operation, Some(10_000))
        .await
        .unwrap_err();
    assert!(error.is::<wasmtime::Trap>(), "{error}");
    assert!(!root.join("created").exists());
    run_backend(&fixture, source, operation).await.unwrap();
    assert!(root.join("created").is_dir());
}

#[cfg(unix)]
#[tokio::test]
async fn durable_backend_protects_artifacts_at_every_guest_access_boundary() {
    use interpreter::runtime::filesystem::{Filesystem, Invocation, NativeFilesystem};
    let fixture = Fixture::new().await;
    let backend = &fixture.filesystem.0;
    let root = fixture.filesystem.vfs().root();
    let private = ".git-submilli-recovery";
    std::fs::write(root.join(private).join("secret"), b"private").unwrap();
    std::fs::write(root.join("public"), b"public").unwrap();
    std::os::unix::fs::symlink(private, root.join("alias")).unwrap();
    for path in [
        "/.git-submilli-recovery/secret",
        "/alias/secret",
        "/alias/missing/child",
    ] {
        assert!(Filesystem::metadata(backend, path, true).await.is_err());
        assert!(Filesystem::metadata(backend, path, false).await.is_err());
        assert!(Filesystem::read(backend, path, 0, 100).await.is_err());
        let invocation = Invocation {
            operation: Some(uuid::Uuid::new_v4().to_string()),
            ..Default::default()
        };
        assert!(
            Filesystem::open_reader(backend, &invocation, path)
                .await
                .is_err()
        );
        let invocation = Invocation {
            operation: Some(uuid::Uuid::new_v4().to_string()),
            ..Default::default()
        };
        assert!(
            Filesystem::write(backend, &invocation, path, b"overwrite".to_vec(), false)
                .await
                .is_err()
        );
    }
    for path in ["/.git-submilli-recovery", "/alias"] {
        let invocation = Invocation {
            operation: Some(uuid::Uuid::new_v4().to_string()),
            ..Default::default()
        };
        assert!(
            Filesystem::open_directory(backend, &invocation, path, true)
                .await
                .is_err()
        );
    }
    let invocation = Invocation {
        operation: Some(uuid::Uuid::new_v4().to_string()),
        ..Default::default()
    };
    let mut listing = Filesystem::open_directory(backend, &invocation, "/", true)
        .await
        .unwrap();
    while let Some(entry) = listing.next_entry() {
        assert!(!entry.path.contains(private));
    }
    assert_eq!(
        Filesystem::read(backend, "/public", 0, 100).await.unwrap(),
        b"public"
    );
    assert_eq!(
        std::fs::read(root.join(private).join("secret")).unwrap(),
        b"private"
    );

    // Preflight cost estimation must not inspect protected contents either.
    std::fs::write(root.join(private).join("large"), vec![0; 1024 * 1024]).unwrap();
    let invocation = Invocation {
        operation: Some(uuid::Uuid::new_v4().to_string()),
        work: Some(Arc::new(interpreter::runtime::fuel::WorkBudget::new(
            10_000,
        ))),
    };
    let error = Filesystem::mkdir(backend, &invocation, "/alias/large", false)
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            interpreter::runtime::filesystem::FilesystemError::Rejected(_)
        ),
        "{error}"
    );

    // Backend policy must not leak into the interpreter's ordinary implementation.
    // Production callers must keep retained volumes assigned to their durable backend.
    let native = NativeFilesystem::new(Vfs::external(root.to_owned()).unwrap());
    assert_eq!(
        Filesystem::read(&native, "/alias/secret", 0, 100)
            .await
            .unwrap(),
        b"private"
    );
    let mut listing = Filesystem::open_directory(&native, &Invocation::default(), "/", false)
        .await
        .unwrap();
    let mut found = false;
    while let Some(entry) = listing.next_entry() {
        found |= entry.name == private;
    }
    assert!(found);
}
