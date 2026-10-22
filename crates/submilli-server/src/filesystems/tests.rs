use super::*;
use interpreter::runtime::durable::{EffectId, RunId};
use interpreter::runtime::filesystem::Filesystem;

use submilli_durable::unit_of_work::Outcome;
use submilli_shared::secret_store::SecretCipher;
mod boundaries;
pub(crate) mod process;
mod resources;

struct Fixture {
    root: tempfile::TempDir,
    database: Arc<ServerDatabase>,
    cipher: Arc<SecretCipher>,
    run: RunId,
    filesystem: DurableFilesystem,
}

impl Fixture {
    async fn new() -> Self {
        Self::with_filesystem_limit(None).await
    }
    async fn with_filesystem_limit(maximum_bytes: Option<u64>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("workspace");
        std::fs::create_dir(&directory).unwrap();
        let database = Arc::new(
            ServerDatabase::open(&root.path().join("state.db"))
                .await
                .unwrap(),
        );
        let cipher = Arc::new(SecretCipher::from_key([7; 32]));
        let run: RunId = uuid::Uuid::new_v4().to_string().into();
        let mut vfs = Vfs::external(directory).unwrap();
        if let Some(limit) = maximum_bytes {
            vfs = vfs.with_size_limit(limit);
        }
        let filesystem = open(vfs, &run, database.clone(), cipher.clone())
            .await
            .unwrap();
        Self {
            root,
            database,
            cipher,
            run,
            filesystem,
        }
    }

    fn units(&self) -> submilli_durable::UnitFactory {
        crate::adapters::unit_of_work::durable::units(self.database.clone(), self.cipher.clone())
    }

    async fn assert_no_pending_work(&self) {
        let mut unit = (self.units())().await.unwrap();
        assert!(
            unit.pending_journal_entry(&self.run)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            unit.closing_writer(self.filesystem.run_id())
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            unit.pending_resource(self.filesystem.run_id())
                .await
                .unwrap()
                .is_none()
        );
    }

    async fn input(&self, bytes: &[u8]) -> Vec<u8> {
        bytes.to_vec()
    }
}

async fn publication_without_receipt(fixture: &Fixture, effect: &EffectId, request: TestCall) {
    fail_operation_updates(fixture, "NEW.status='done'").await;
    assert!(fixture.filesystem.call(effect, request).await.is_err());
    clear_operation_failure(fixture).await;
}
async fn fail_operation_updates(fixture: &Fixture, condition: &'static str) {
    fixture.database.transaction(move |connection| Box::pin(async move {
        let statement = format!("CREATE TRIGGER refuse_operation_update BEFORE UPDATE ON durable_filesystem_operations WHEN {condition} BEGIN SELECT RAISE(FAIL, 'injected update failure'); END");
        sqlx::query(sqlx::AssertSqlSafe(statement.as_str())).execute(connection).await?;
        Ok(())
    })).await.unwrap();
}
async fn clear_operation_failure(fixture: &Fixture) {
    fixture
        .database
        .transaction(|connection| {
            Box::pin(async move {
                sqlx::query("DROP TRIGGER refuse_operation_update")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .await
        .unwrap();
}
async fn reserve_close(fixture: &Fixture, resource: &EffectId, effect: &EffectId) {
    let mut unit = fixture.units()().await.unwrap();
    let mut writer = unit
        .get_writer(&fixture.run, resource)
        .await
        .unwrap()
        .unwrap();
    writer.state = "closing".into();
    writer.close_effect = Some(effect.clone());
    unit.save_writer(&fixture.run, writer).await.unwrap();
    unit.commit().await.unwrap();
}
#[tokio::test]
async fn sqlite_receipt_suppresses_completed_mutation_after_path_reuse() {
    let fixture = Fixture::new().await;
    let content = fixture.input(b"original").await;
    let effect: EffectId = uuid::Uuid::new_v4().to_string().into();
    let request = TestCall::Append {
        path: "/file".into(),
        content,
    };
    fixture
        .filesystem
        .call(&effect, request.clone())
        .await
        .unwrap();
    let replacement = fixture.input(b"newer").await;
    fixture
        .filesystem
        .call(
            &uuid::Uuid::new_v4().to_string().into(),
            TestCall::Write {
                path: "/file".into(),
                content: replacement,
            },
        )
        .await
        .unwrap();
    let restored = fixture
        .filesystem
        .reopen(
            fixture.filesystem.vfs().clone(),
            fixture.filesystem.run_id(),
            fixture.units(),
        )
        .await
        .unwrap();
    restored.call(&effect, request.clone()).await.unwrap();
    assert_eq!(
        std::fs::read(restored.vfs().root().join("file")).unwrap(),
        b"newer"
    );
    assert!(
        restored
            .call(
                &effect,
                TestCall::Remove {
                    path: "/file".into()
                }
            )
            .await
            .is_err()
    );
    fixture.assert_no_pending_work().await;
}

#[tokio::test]
async fn append_publication_gap_recovers_once_and_blocks_other_mutations() {
    let fixture = Fixture::new().await;
    let initial = fixture.input(b"before").await;
    fixture
        .filesystem
        .call(
            &uuid::Uuid::new_v4().to_string().into(),
            TestCall::Write {
                path: "/file".into(),
                content: initial,
            },
        )
        .await
        .unwrap();
    let content = fixture.input(b" suffix").await;
    let effect: EffectId = uuid::Uuid::new_v4().to_string().into();
    let request = TestCall::Append {
        path: "/file".into(),
        content,
    };
    publication_without_receipt(&fixture, &effect, request.clone()).await;
    let journal_entry = {
        let mut unit = fixture.units()().await.unwrap();
        unit.get_journal_entry(&fixture.run, &effect)
            .await
            .unwrap()
            .unwrap()
    };
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&journal_entry.state.unwrap()).unwrap(),
        serde_json::json!({"Prepared": {"offset": 6}})
    );
    assert!(
        fixture
            .filesystem
            .call(
                &uuid::Uuid::new_v4().to_string().into(),
                TestCall::Remove {
                    path: "/file".into()
                }
            )
            .await
            .is_err()
    );
    let restored = fixture
        .filesystem
        .reopen(
            fixture.filesystem.vfs().clone(),
            fixture.filesystem.run_id(),
            fixture.units(),
        )
        .await
        .unwrap();
    assert!(
        restored
            .call(
                &effect,
                TestCall::Append {
                    path: "/other".into(),
                    content: fixture.input(b" suffix").await,
                },
            )
            .await
            .is_err()
    );
    assert!(!restored.vfs().root().join("other").exists());
    restored.call(&effect, request.clone()).await.unwrap();
    restored.call(&effect, request).await.unwrap();
    assert_eq!(
        std::fs::read(restored.vfs().root().join("file")).unwrap(),
        b"before suffix"
    );
}

#[tokio::test]
async fn rejected_preflight_is_replayed_even_when_the_path_later_exists() {
    let fixture = Fixture::new().await;
    let effect: EffectId = uuid::Uuid::new_v4().to_string().into();
    let request = TestCall::Remove {
        path: "/missing".into(),
    };
    assert!(matches!(
        fixture.filesystem.call(&effect, request.clone()).await,
        Err(FilesystemError::Rejected(_))
    ));
    let content = fixture.input(b"later").await;
    fixture
        .filesystem
        .call(
            &uuid::Uuid::new_v4().to_string().into(),
            TestCall::Write {
                path: "/missing".into(),
                content,
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        fixture.filesystem.call(&effect, request).await,
        Err(FilesystemError::Rejected(_))
    ));
    assert_eq!(
        std::fs::read(fixture.filesystem.vfs().root().join("missing")).unwrap(),
        b"later"
    );
}

#[tokio::test]
async fn writer_chunks_survive_pause_and_duplicate_close_never_republishes() {
    let fixture = Fixture::new().await;
    let resource: EffectId = uuid::Uuid::new_v4().to_string().into();
    let chunk: EffectId = uuid::Uuid::new_v4().to_string().into();
    let close: EffectId = uuid::Uuid::new_v4().to_string().into();
    fixture
        .filesystem
        .operation(None)
        .open_writer(&resource, "/out")
        .await
        .unwrap();
    let first = fixture.input(b"first").await;
    fixture
        .filesystem
        .operation(None)
        .write_chunk(&resource, &chunk, first.clone())
        .await
        .unwrap();
    let resumed = fixture
        .filesystem
        .reopen(
            fixture.filesystem.vfs().clone(),
            fixture.filesystem.run_id(),
            fixture.units(),
        )
        .await
        .unwrap();
    resumed
        .operation(None)
        .write_chunk(&resource, &chunk, first)
        .await
        .unwrap();
    let second = fixture.input(b" second").await;
    resumed
        .operation(None)
        .write_chunk(&resource, &uuid::Uuid::new_v4().to_string().into(), second)
        .await
        .unwrap();
    reserve_close(&fixture, &resource, &close).await;
    assert!(
        resumed
            .call(
                &uuid::Uuid::new_v4().to_string().into(),
                TestCall::Remove {
                    path: "/out".into()
                }
            )
            .await
            .is_err()
    );
    let resumed = fixture
        .filesystem
        .reopen(
            fixture.filesystem.vfs().clone(),
            fixture.filesystem.run_id(),
            fixture.units(),
        )
        .await
        .unwrap();
    assert!(!resumed.vfs().root().join("out").exists());
    resumed
        .operation(None)
        .close_writer(&resource, &close)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(resumed.vfs().root().join("out")).unwrap(),
        b"first second"
    );
    let newer = fixture.input(b"newer").await;
    resumed
        .call(
            &uuid::Uuid::new_v4().to_string().into(),
            TestCall::Write {
                path: "/out".into(),
                content: newer,
            },
        )
        .await
        .unwrap();
    resumed
        .operation(None)
        .close_writer(&resource, &close)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(resumed.vfs().root().join("out")).unwrap(),
        b"newer"
    );
}

fn byte_options() -> interpreter::runtime::filesystem::ReaderOptions {
    interpreter::runtime::filesystem::ReaderOptions::Bytes { chunk_size: 3 }
}
fn resource_effect_id() -> EffectId {
    EffectId::stub().unwrap()
}
fn read_bytes(output: interpreter::runtime::filesystem::ReadOutput) -> Option<Vec<u8>> {
    match output.item {
        Some(interpreter::runtime::filesystem::ReadItem::Bytes(bytes)) => Some(bytes),
        None => None,
        _ => panic!("expected bytes"),
    }
}

#[tokio::test]
async fn reader_reopen_refuses_a_replaced_path() {
    use interpreter::runtime::filesystem::Filesystem;
    let fixture = Fixture::new().await;
    let root = fixture.filesystem.vfs().root();
    std::fs::write(root.join("file"), b"original").unwrap();
    let mut reader = fixture
        .filesystem
        .operation(None)
        .open_reader(
            &uuid::Uuid::new_v4().to_string().into(),
            "/file",
            byte_options(),
        )
        .await
        .unwrap();
    let descriptor = recorded_resource(&reader);
    let limits = interpreter::runtime::limits::TenantLimits::new(1 << 20);
    assert_eq!(
        read_bytes(
            Filesystem::next_reader(
                &fixture.filesystem,
                &resource_effect_id(),
                None,
                &mut reader,
                &limits
            )
            .await
            .unwrap()
        )
        .unwrap(),
        b"ori"
    );
    drop(reader);
    std::fs::rename(root.join("file"), root.join("old")).unwrap();
    std::fs::write(root.join("file"), b"replacement").unwrap();
    let mut restored = descriptor;
    assert!(
        Filesystem::next_reader(
            &fixture.filesystem,
            &resource_effect_id(),
            None,
            &mut restored,
            &limits
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn directory_snapshot_restores_order_after_directory_changes() {
    let fixture = Fixture::new().await;
    std::fs::create_dir(fixture.filesystem.vfs().root().join("tree")).unwrap();
    std::fs::write(fixture.filesystem.vfs().root().join("tree/a"), b"a").unwrap();
    std::fs::write(fixture.filesystem.vfs().root().join("tree/b"), b"b").unwrap();
    let resource: EffectId = uuid::Uuid::new_v4().to_string().into();
    let mut listing = fixture
        .filesystem
        .listing(&resource, "/tree", true)
        .await
        .unwrap();
    let limits = interpreter::runtime::limits::TenantLimits::new(64 << 20);
    assert_eq!(
        Filesystem::next_directory(
            &fixture.filesystem,
            &resource_effect_id(),
            None,
            &mut listing,
            &limits
        )
        .await
        .unwrap()
        .unwrap()
        .path,
        "/tree/a"
    );
    drop(listing);
    std::fs::remove_file(fixture.filesystem.vfs().root().join("tree/b")).unwrap();
    std::fs::write(fixture.filesystem.vfs().root().join("tree/c"), b"c").unwrap();
    let resumed = fixture
        .filesystem
        .reopen(
            fixture.filesystem.vfs().clone(),
            fixture.filesystem.run_id(),
            fixture.units(),
        )
        .await
        .unwrap();
    let mut listing = resumed.listing(&resource, "/tree", true).await.unwrap();
    assert_eq!(
        Filesystem::next_directory(&resumed, &resource_effect_id(), None, &mut listing, &limits)
            .await
            .unwrap()
            .unwrap()
            .path,
        "/tree/b"
    );
    assert!(
        Filesystem::next_directory(&resumed, &resource_effect_id(), None, &mut listing, &limits)
            .await
            .unwrap()
            .is_none()
    );
    assert!(resumed.listing(&resource, "/tree", false).await.is_err());
}

#[tokio::test]
async fn interpreter_uses_durable_backend_and_replay_does_not_repeat_append() {
    let fixture = Fixture::new().await;
    let source = r#"import { appendText } from "submilli:fs";
        function main(): void { appendText("/port.txt", "once"); }"#;
    run_backend(&fixture, source).await.unwrap();
    let operation = sole_operation_id(&fixture).await;
    let effect_id: EffectId = operation.clone();
    interpreter::runtime::filesystem::Filesystem::append(
        &fixture.filesystem,
        &effect_id,
        None,
        "/port.txt",
        b"once".to_vec(),
    )
    .await
    .unwrap();
    assert_eq!(
        std::fs::read(fixture.filesystem.vfs().root().join("port.txt")).unwrap(),
        b"once"
    );
    let factory = fixture.units();
    let mut unit = factory().await.unwrap();
    assert_eq!(unit.journal_entry_count(&fixture.run).await.unwrap(), 1);
    let effect = unit
        .get_journal_entry(&fixture.run, &operation)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(effect.status().unwrap(), "done");
    drop(unit);
    run_backend(&fixture, source).await.unwrap();
    assert_eq!(
        std::fs::read(fixture.filesystem.vfs().root().join("port.txt")).unwrap(),
        b"onceonce"
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
    run_backend(&fixture, source).await.unwrap();
    assert_eq!(
        std::fs::read(fixture.filesystem.vfs().root().join("note.ts")).unwrap(),
        b"after\n"
    );
    let mut unit = fixture.units()().await.unwrap();
    assert_eq!(unit.journal_entry_count(&fixture.run).await.unwrap(), 0);
}

#[tokio::test]
async fn durable_backend_refuses_unprepared_recursive_mutations() {
    let fixture = Fixture::new().await;
    let source = r#"import { mkdir } from "submilli:fs";
        function main(): void { mkdir("/unprepared/child", true); }"#;
    assert!(run_backend(&fixture, source).await.is_err());
    assert!(std::fs::read(fixture.filesystem.vfs().root().join("unprepared/child")).is_err());
    let mut unit = fixture.units()().await.unwrap();
    assert_eq!(unit.journal_entry_count(&fixture.run).await.unwrap(), 0);
}

async fn sole_operation_id(fixture: &Fixture) -> EffectId {
    let run_id = fixture.run.clone();
    let ids: Vec<String> = fixture
        .database
        .read(move |connection| {
            Box::pin(async move {
                Ok(sqlx::query_scalar(
                    "SELECT operation_id FROM durable_filesystem_operations WHERE scope_id=?",
                )
                .bind(run_id.as_str())
                .fetch_all(connection)
                .await?)
            })
        })
        .await
        .unwrap();
    assert_eq!(ids.len(), 1);
    ids.into_iter().next().unwrap().into()
}

async fn run_backend(fixture: &Fixture, source: &str) -> wasmtime::Result<Option<String>> {
    run_backend_fueled(fixture, source, None).await
}

async fn run_backend_fueled(
    fixture: &Fixture,
    source: &str,
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
    let mut data = StoreData::with_vfs_and_cap(
        fixture.filesystem.run_id().clone(),
        fixture.filesystem.vfs().clone(),
        64 << 20,
    );
    data.filesystem = Arc::new(fixture.filesystem.clone());
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
    run_backend(&fixture, source).await.unwrap();
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
    run_backend(&fixture, source).await.unwrap();
}

#[tokio::test]
async fn durable_append_and_eager_listing_admit_their_own_fuel() {
    let fixture = Fixture::new().await;
    let root = fixture.filesystem.vfs().root();
    let original = vec![b'x'; 64 * 1024];
    std::fs::write(root.join("large"), &original).unwrap();
    let append = r#"import { appendText } from "submilli:fs";
        function main(): void { appendText("/large", ""); }"#;
    // Empty append no longer hashes the large existing file.
    run_backend_fueled(&fixture, append, Some(10_000))
        .await
        .unwrap();
    assert_eq!(std::fs::read(root.join("large")).unwrap(), original);
    let effect_id: EffectId = uuid::Uuid::new_v4().to_string().into();
    let work = Arc::new(interpreter::runtime::fuel::WorkBudget::new(1_000));
    let error = fixture
        .filesystem
        .operation(Some(work))
        .append_bytes(&effect_id, "/large", vec![b'y'; 64 * 1024])
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        submilli_durable::FilesystemError::Recovery(submilli_durable::RecoveryError::Fuel(_))
    ));
    assert_eq!(std::fs::read(root.join("large")).unwrap(), original);
    fixture
        .filesystem
        .operation(None)
        .append_bytes(&effect_id, "/large", vec![b'y'; 64 * 1024])
        .await
        .unwrap();
    assert_eq!(
        std::fs::metadata(root.join("large")).unwrap().len(),
        128 * 1024
    );
    for index in 0..100 {
        std::fs::write(root.join(format!("entry-{index}")), "").unwrap();
    }
    let listing = r#"import { list } from "submilli:fs";
        function main(): void { list("/", false); }"#;
    let error = run_backend_fueled(&fixture, listing, Some(10_000))
        .await
        .unwrap_err();
    assert!(error.is::<wasmtime::Trap>(), "{error}");
}

#[tokio::test]
async fn durable_mutations_do_not_scan_unrelated_files_for_quota() {
    let fixture = Fixture::with_filesystem_limit(Some(1 << 20)).await;
    let root = fixture.filesystem.vfs().root();
    for index in 0..100 {
        std::fs::write(root.join(format!("entry-{index}")), "").unwrap();
    }
    let source = r#"import { mkdir } from "submilli:fs";
        function main(): void { mkdir("/created", false); }"#;
    run_backend_fueled(&fixture, source, Some(10_000))
        .await
        .unwrap();
    assert!(root.join("created").is_dir());
    assert_eq!(fixture.filesystem.vfs().quota().unwrap().used(), 0);
}

#[cfg(unix)]
#[tokio::test]
async fn durable_backend_protects_artifacts_at_every_guest_access_boundary() {
    use interpreter::runtime::default_filesystem::DefaultFilesystem;
    use interpreter::runtime::filesystem::Filesystem;
    let fixture = Fixture::new().await;
    let backend = &fixture.filesystem;
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
        assert!(
            Filesystem::metadata(backend, &resource_effect_id(), None, path, true)
                .await
                .is_err()
        );
        assert!(
            Filesystem::metadata(backend, &resource_effect_id(), None, path, false)
                .await
                .is_err()
        );
        assert!(
            Filesystem::read(backend, &resource_effect_id(), None, path, 0, 100)
                .await
                .is_err()
        );
        let effect_id: EffectId = uuid::Uuid::new_v4().to_string().into();
        assert!(
            Filesystem::open_reader(backend, &effect_id, None, path, byte_options())
                .await
                .is_err()
        );
        let effect_id: EffectId = uuid::Uuid::new_v4().to_string().into();
        assert!(
            Filesystem::write(backend, &effect_id, None, path, b"overwrite".to_vec())
                .await
                .is_err()
        );
    }
    for path in ["/.git-submilli-recovery", "/alias"] {
        let effect_id: EffectId = uuid::Uuid::new_v4().to_string().into();
        assert!(
            Filesystem::open_directory(
                backend,
                &effect_id,
                None,
                path,
                true,
                &interpreter::runtime::limits::TenantLimits::new(64 << 20)
            )
            .await
            .is_err()
        );
    }
    let effect_id: EffectId = uuid::Uuid::new_v4().to_string().into();
    let mut listing = Filesystem::open_directory(
        backend,
        &effect_id,
        None,
        "/",
        true,
        &interpreter::runtime::limits::TenantLimits::new(64 << 20),
    )
    .await
    .unwrap();
    while let Some(entry) = Filesystem::next_directory(
        backend,
        &resource_effect_id(),
        None,
        &mut listing,
        &interpreter::runtime::limits::TenantLimits::new(64 << 20),
    )
    .await
    .unwrap()
    {
        assert!(!entry.path.contains(private));
    }
    assert_eq!(
        Filesystem::read(backend, &resource_effect_id(), None, "/public", 0, 100)
            .await
            .unwrap(),
        b"public"
    );
    assert_eq!(
        std::fs::read(root.join(private).join("secret")).unwrap(),
        b"private"
    );

    // Preflight cost estimation must not inspect protected contents either.
    std::fs::write(root.join(private).join("large"), vec![0; 1024 * 1024]).unwrap();
    let effect_id: EffectId = uuid::Uuid::new_v4().to_string().into();
    let work_budget = Some(Arc::new(interpreter::runtime::fuel::WorkBudget::new(
        10_000,
    )));
    let error = Filesystem::mkdir(backend, &effect_id, work_budget, "/alias/large", false)
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
    let native = DefaultFilesystem::new(Vfs::external(root.to_owned()).unwrap(), "run".into());
    assert_eq!(
        Filesystem::read(
            &native,
            &resource_effect_id(),
            None,
            "/alias/secret",
            0,
            100
        )
        .await
        .unwrap(),
        b"private"
    );
    let mut listing = Filesystem::open_directory(
        &native,
        &EffectId::stub().unwrap(),
        None,
        "/",
        false,
        &interpreter::runtime::limits::TenantLimits::new(64 << 20),
    )
    .await
    .unwrap();
    let mut found = false;
    while let Some(entry) = Filesystem::next_directory(
        &native,
        &resource_effect_id(),
        None,
        &mut listing,
        &interpreter::runtime::limits::TenantLimits::new(64 << 20),
    )
    .await
    .unwrap()
    {
        found |= entry.name == private;
    }
    assert!(found);
}

// A compact test-case matrix; production operations have no dispatcher.
#[derive(Clone)]
enum TestCall {
    Write { path: String, content: Vec<u8> },
    Append { path: String, content: Vec<u8> },
    Remove { path: String },
    CreateDirectory { path: String },
}
#[async_trait::async_trait]
trait CallTestCase {
    async fn call(&self, id: &EffectId, call: TestCall) -> Result<Outcome, FilesystemError>;
    async fn listing(
        &self,
        id: &EffectId,
        path: &str,
        recursive: bool,
    ) -> Result<
        interpreter::runtime::filesystem::DirectoryReader,
        interpreter::runtime::filesystem::FilesystemError,
    >;
}
#[async_trait::async_trait]
impl CallTestCase for DurableFilesystem {
    async fn listing(
        &self,
        id: &EffectId,
        path: &str,
        recursive: bool,
    ) -> Result<
        interpreter::runtime::filesystem::DirectoryReader,
        interpreter::runtime::filesystem::FilesystemError,
    > {
        let effect_id: EffectId = id.clone();
        interpreter::runtime::filesystem::Filesystem::open_directory(
            self,
            &effect_id,
            None,
            path,
            recursive,
            &interpreter::runtime::limits::TenantLimits::new(64 << 20),
        )
        .await
    }

    async fn call(&self, id: &EffectId, call: TestCall) -> Result<Outcome, FilesystemError> {
        match call {
            TestCall::Write { path, content } => {
                self.operation(None).write_bytes(id, &path, content).await
            }
            TestCall::Append { path, content } => {
                self.operation(None).append_bytes(id, &path, content).await
            }
            TestCall::Remove { path } => self.operation(None).remove_path(id, &path).await,
            TestCall::CreateDirectory { path } => {
                self.operation(None).create_directory(id, &path).await
            }
        }
    }
}

#[tokio::test]
async fn pending_append_requires_replay_bytes_and_rejects_changed_input() {
    let fixture = Fixture::new().await;
    let effect: EffectId = uuid::Uuid::new_v4().to_string().into();
    fail_operation_updates(&fixture, "NEW.status='pending'").await;
    assert!(
        fixture
            .filesystem
            .operation(None)
            .append_bytes(&effect, "/output", b"original input payload".to_vec())
            .await
            .is_err()
    );
    clear_operation_failure(&fixture).await;
    let restarted = fixture
        .filesystem
        .reopen(
            fixture.filesystem.vfs().clone(),
            &fixture.run,
            fixture.units(),
        )
        .await
        .unwrap();
    assert!(!restarted.vfs().root().join("output").exists());
    assert!(
        restarted
            .operation(None)
            .append_bytes(&effect, "/output", b"changed input payload".to_vec())
            .await
            .is_err()
    );
    assert!(!restarted.vfs().root().join("output").exists());
    restarted
        .operation(None)
        .append_bytes(&effect, "/output", b"original input payload".to_vec())
        .await
        .unwrap();
    std::fs::write(restarted.vfs().root().join("output"), b"later").unwrap();
    restarted
        .operation(None)
        .append_bytes(&effect, "/output", b"original input payload".to_vec())
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(restarted.vfs().root().join("output")).unwrap(),
        b"later"
    );
    let mut unit = fixture.units()().await.unwrap();
    let record = unit
        .get_journal_entry(&fixture.run, &effect)
        .await
        .unwrap()
        .unwrap();
    for bytes in [&record.request, record.state.as_ref().unwrap()] {
        assert!(!String::from_utf8_lossy(bytes).contains("original input payload"));
    }
    assert_eq!(
        record.request.len(),
        64,
        "filesystem configuration and request digests only"
    );
    drop(unit);
    let tables: Vec<String> = fixture
        .database
        .read(|connection| {
            Box::pin(async move {
                Ok(sqlx::query_scalar(
                    "SELECT name FROM sqlite_master WHERE type='table' AND name LIKE '%object%'",
                )
                .fetch_all(connection)
                .await?)
            })
        })
        .await
        .unwrap();
    assert!(
        tables.is_empty(),
        "filesystem storage must not create object tables: {tables:?}"
    );
}

#[tokio::test]
async fn directory_outputs_larger_than_metadata_limit_are_stored_in_sqlite() {
    use interpreter::runtime::{filesystem::Filesystem, fuel};
    let fixture = Fixture::new().await;
    for i in 0..1100 {
        std::fs::write(
            fixture
                .filesystem
                .vfs()
                .root()
                .join(format!("entry-{i:04}-{}", "x".repeat(60))),
            b"",
        )
        .unwrap();
    }
    let resource: EffectId = uuid::Uuid::new_v4().to_string().into();
    fixture
        .filesystem
        .listing(&resource, "/", false)
        .await
        .unwrap();
    let mut unit = fixture.units()().await.unwrap();
    let entries = unit
        .get_listing(&fixture.run, &resource)
        .await
        .unwrap()
        .unwrap()
        .content
        .unwrap();
    drop(unit);
    assert!(serde_json::to_vec(&entries).unwrap().len() > submilli_durable::codec::MAX_RECORD);
    std::fs::write(fixture.filesystem.vfs().root().join("later"), b"").unwrap();
    fixture
        .filesystem
        .listing(&resource, "/", false)
        .await
        .unwrap();
    let mut unit = fixture.units()().await.unwrap();
    let replay = unit
        .get_listing(&fixture.run, &resource)
        .await
        .unwrap()
        .unwrap()
        .content
        .unwrap();
    drop(unit);
    assert_eq!(entries, replay);
    // Replay must pay for long path strings, even when the committed observation
    // exceeds the remaining budget. Return it and settle; do not lose the result.
    let work = Arc::new(fuel::WorkBudget::new(1));
    let effect_id: EffectId = resource;
    Filesystem::open_directory(
        &fixture.filesystem,
        &effect_id,
        Some(work.clone()),
        "/",
        false,
        &interpreter::runtime::limits::TenantLimits::new(64 << 20),
    )
    .await
    .unwrap();
    let path_bytes = entries.iter().map(|entry| entry.path.len() as u64).sum();
    assert!(work.spent() >= fuel::PARSE.cost(path_bytes) + fuel::HASH.cost(path_bytes));
}

#[tokio::test]
async fn writer_rejects_changed_chunks_and_corrupt_staging_before_publication() {
    let fixture = Fixture::new().await;
    let resource: EffectId = uuid::Uuid::new_v4().to_string().into();
    let chunk: EffectId = uuid::Uuid::new_v4().to_string().into();
    fixture
        .filesystem
        .operation(None)
        .open_writer(&resource, "/output")
        .await
        .unwrap();
    fixture
        .filesystem
        .operation(None)
        .write_chunk(&resource, &chunk, b"original".to_vec())
        .await
        .unwrap();
    assert!(
        fixture
            .filesystem
            .operation(None)
            .write_chunk(&resource, &chunk, b"changed".to_vec())
            .await
            .is_err()
    );
    let staged = fixture
        .filesystem
        .vfs()
        .root()
        .join(".git-submilli-recovery")
        .join(fixture.filesystem.run_id().as_str())
        .join(format!("writer-{resource}/output"));
    std::fs::write(&staged, b"corrupt!").unwrap();
    assert!(
        fixture
            .filesystem
            .operation(None)
            .close_writer(&resource, &uuid::Uuid::new_v4().to_string().into())
            .await
            .is_err()
    );
    assert!(!fixture.filesystem.vfs().root().join("output").exists());
}

#[tokio::test]
async fn failed_append_admission_does_not_stage_input_or_mutate_public_files() {
    let fixture = Fixture::new().await;
    fixture.database.transaction(|connection| Box::pin(async move {
        sqlx::query("CREATE TRIGGER refuse_effect BEFORE INSERT ON durable_filesystem_operations BEGIN SELECT RAISE(FAIL, 'injected admission failure'); END").execute(connection).await?;
        Ok(())
    })).await.unwrap();
    let effect: EffectId = uuid::Uuid::new_v4().to_string().into();
    assert!(
        fixture
            .filesystem
            .operation(None)
            .append_bytes(&effect, "/output", b"private input".to_vec())
            .await
            .is_err()
    );
    assert!(!fixture.filesystem.vfs().root().join("output").exists());
    assert!(
        !fixture
            .filesystem
            .vfs()
            .root()
            .join(".git-submilli-recovery")
            .join(fixture.filesystem.run_id().as_str())
            .join(effect.as_str())
            .exists()
    );
}

#[tokio::test]
async fn writer_close_records_target_refusals_and_replays_them() {
    let fixture = Fixture::new().await;
    let root = fixture.filesystem.vfs().root();
    std::fs::create_dir(root.join("parent")).unwrap();
    let resource: EffectId = uuid::Uuid::new_v4().to_string().into();
    fixture
        .filesystem
        .operation(None)
        .open_writer(&resource, "/parent/out")
        .await
        .unwrap();
    fixture
        .filesystem
        .operation(None)
        .write_chunk(
            &resource,
            &uuid::Uuid::new_v4().to_string().into(),
            b"output".to_vec(),
        )
        .await
        .unwrap();
    std::fs::remove_dir(root.join("parent")).unwrap();
    let effect: EffectId = uuid::Uuid::new_v4().to_string().into();
    let error = fixture
        .filesystem
        .operation(None)
        .close_writer(&resource, &effect)
        .await
        .unwrap_err();
    assert!(matches!(error, FilesystemError::Rejected(_)), "{error}");
    std::fs::create_dir(root.join("parent")).unwrap();
    let replay = fixture
        .filesystem
        .operation(None)
        .close_writer(&resource, &effect)
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), replay.to_string());
    assert!(!root.join("parent/out").exists());
    let resource: EffectId = uuid::Uuid::new_v4().to_string().into();
    fixture
        .filesystem
        .operation(None)
        .open_writer(&resource, "/")
        .await
        .unwrap();
    let error = fixture
        .filesystem
        .operation(None)
        .close_writer(&resource, &uuid::Uuid::new_v4().to_string().into())
        .await
        .unwrap_err();
    assert!(matches!(error, FilesystemError::Rejected(_)), "{error}");
}

#[tokio::test]
async fn writer_staging_is_free_and_close_checks_only_the_public_size() {
    let fixture = Fixture::with_filesystem_limit(Some(8)).await;
    let operation = fixture.filesystem.operation(None);
    let quota = fixture.filesystem.vfs().quota().unwrap();
    operation
        .write_bytes(&resource_effect_id(), "/out", b"original".to_vec())
        .await
        .unwrap();
    for bytes in [
        b"replaced".as_slice(),
        b"smaller",
        b"too large for the quota",
    ] {
        let resource = resource_effect_id();
        operation.open_writer(&resource, "/out").await.unwrap();
        let previous = std::fs::read(fixture.filesystem.vfs().root().join("out")).unwrap();
        operation
            .write_chunk(&resource, &resource_effect_id(), bytes.to_vec())
            .await
            .unwrap();
        assert_eq!(
            quota.used(),
            previous.len() as u64,
            "private staging changed quota"
        );
        let close = resource_effect_id();
        let result = operation.close_writer(&resource, &close).await;
        if bytes.len() <= 8 {
            result.unwrap();
            operation.close_writer(&resource, &close).await.unwrap();
            assert_eq!(quota.used(), bytes.len() as u64);
        } else {
            assert!(matches!(result, Err(FilesystemError::Rejected(_))));
            assert_eq!(quota.used(), previous.len() as u64);
            assert_eq!(
                std::fs::read(fixture.filesystem.vfs().root().join("out")).unwrap(),
                previous
            );
        }
    }
}

#[tokio::test]
async fn opening_existing_workspace_leaves_pending_mutation_for_explicit_retry() {
    let fixture = Fixture::new().await;
    let effect: EffectId = uuid::Uuid::new_v4().to_string().into();
    let request = TestCall::CreateDirectory {
        path: "/pending".into(),
    };
    fail_operation_updates(&fixture, "NEW.status='pending'").await;
    assert!(
        fixture
            .filesystem
            .call(&effect, request.clone())
            .await
            .is_err()
    );
    clear_operation_failure(&fixture).await;
    let opened = fixture
        .filesystem
        .reopen(
            fixture.filesystem.vfs().clone(),
            &fixture.run,
            fixture.units(),
        )
        .await
        .unwrap();
    assert!(!opened.vfs().root().join("pending").exists());
    drop(opened);
    let reopened = fixture
        .filesystem
        .reopen(
            fixture.filesystem.vfs().clone(),
            fixture.filesystem.run_id(),
            fixture.units(),
        )
        .await
        .unwrap();
    assert!(!reopened.vfs().root().join("pending").exists());
    let mut unit = (fixture.units())().await.unwrap();
    let pending = unit
        .get_journal_entry(&fixture.run, &effect)
        .await
        .unwrap()
        .unwrap();
    assert!(pending.state.is_none());
    drop(unit);
    reopened.call(&effect, request.clone()).await.unwrap();
    assert!(reopened.vfs().root().join("pending").is_dir());
    // A repeated completed call also leaves later directory contents alone.
    std::fs::write(reopened.vfs().root().join("pending/later"), b"later").unwrap();
    reopened.call(&effect, request).await.unwrap();
    assert_eq!(
        std::fs::read(reopened.vfs().root().join("pending/later")).unwrap(),
        b"later"
    );
}

#[tokio::test]
async fn direct_namespace_calls_reconcile_sql_gaps_and_preserve_later_paths() {
    for kind in ["move", "remove", "mkdir"] {
        let fixture = Fixture::new().await;
        let root = fixture.filesystem.vfs().root();
        std::fs::write(root.join("source"), b"original").unwrap();
        std::fs::write(root.join("destination"), b"old destination").unwrap();
        let effect: EffectId = uuid::Uuid::new_v4().to_string().into();
        fail_operation_updates(&fixture, "NEW.status='done'").await;
        assert!(
            namespace_call(&fixture.filesystem, kind, &effect)
                .await
                .is_err()
        );
        clear_operation_failure(&fixture).await;
        let journal_entry = {
            let mut unit = fixture.units()().await.unwrap();
            unit.get_journal_entry(&fixture.run, &effect)
                .await
                .unwrap()
                .unwrap()
        };
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&journal_entry.state.unwrap()).unwrap(),
            serde_json::json!({"Prepared": {}}),
            "{kind} stores no path or file identity"
        );
        let reopened = fixture
            .filesystem
            .reopen(
                fixture.filesystem.vfs().clone(),
                &fixture.run,
                fixture.units(),
            )
            .await
            .unwrap();
        let operation = reopened.operation(None);
        let changed_path = match kind {
            "move" => operation.move_path(&effect, "/source", "/other").await,
            "remove" => operation.remove_path(&effect, "/other").await,
            "mkdir" => operation.create_directory(&effect, "/other").await,
            _ => unreachable!(),
        };
        assert!(changed_path.is_err());
        assert!(!root.join("other").exists());
        namespace_call(&reopened, kind, &effect).await.unwrap();
        match kind {
            "move" => {
                assert!(!root.join("source").exists());
                assert_eq!(
                    std::fs::read(root.join("destination")).unwrap(),
                    b"original"
                );
            }
            "remove" => assert!(!root.join("source").exists()),
            "mkdir" => assert!(root.join("directory").is_dir()),
            _ => unreachable!(),
        }
        std::fs::write(root.join("source"), b"later source").unwrap();
        std::fs::write(root.join("destination"), b"later destination").unwrap();
        if kind == "mkdir" {
            std::fs::write(root.join("directory/child"), b"later child").unwrap();
        }
        namespace_call(&reopened, kind, &effect).await.unwrap();
        assert_eq!(std::fs::read(root.join("source")).unwrap(), b"later source");
        assert_eq!(
            std::fs::read(root.join("destination")).unwrap(),
            b"later destination"
        );
        assert!(
            reopened
                .operation(None)
                .append_bytes(&effect, "/source", b"collision".to_vec())
                .await
                .is_err()
        );
        fixture.assert_no_pending_work().await;
    }
}

async fn namespace_call(
    filesystem: &DurableFilesystem,
    kind: &str,
    effect: &EffectId,
) -> Result<Outcome, FilesystemError> {
    match kind {
        "move" => {
            filesystem
                .operation(None)
                .move_path(effect, "/source", "/destination")
                .await
        }
        "remove" => {
            filesystem
                .operation(None)
                .remove_path(effect, "/source")
                .await
        }
        "mkdir" => {
            filesystem
                .operation(None)
                .create_directory(effect, "/directory")
                .await
        }
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn empty_writer_close_records_read_only_refusal_without_staging() {
    use interpreter::runtime::filesystem::{Filesystem, FilesystemError as PortError};
    use interpreter::runtime::vfs::{Access, MountSpec};
    let fixture = Fixture::new().await;
    let read_only = fixture.root.path().join("read-only");
    std::fs::create_dir(&read_only).unwrap();
    let vfs = fixture
        .filesystem
        .vfs()
        .clone()
        .with_mount(MountSpec {
            guest_path: "/read-only".into(),
            host: read_only.clone(),
            volume: "read-only".into(),
            access: Access::ReadOnly,
            quota: None,
        })
        .unwrap();
    let filesystem = fixture
        .filesystem
        .reopen(vfs.clone(), &fixture.run, fixture.units())
        .await
        .unwrap();
    let opening = resource_effect_id();
    let closing = resource_effect_id();
    let mut writer = Filesystem::open_writer(&filesystem, &opening, None, "/read-only/file")
        .await
        .unwrap();
    let descriptor = recorded_resource(&writer);
    let first = Filesystem::close_writer(&filesystem, &closing, None, &mut writer)
        .await
        .unwrap_err();
    assert!(matches!(first, PortError::Rejected(_)), "{first}");
    assert!(writer.closed);
    drop(writer);
    drop(filesystem);
    let reopened = fixture
        .filesystem
        .reopen(vfs, &fixture.run, fixture.units())
        .await
        .unwrap();
    let mut writer = recorded_resource(&descriptor);
    let replay = Filesystem::close_writer(&reopened, &closing, None, &mut writer)
        .await
        .unwrap_err();
    assert_eq!(first.to_string(), replay.to_string());
    assert!(writer.closed);
    assert!(!read_only.join("file").exists());
    let staging = fixture
        .filesystem
        .vfs()
        .root()
        .join(".git-submilli-recovery")
        .join(fixture.run.as_str())
        .join(format!("writer-{}", descriptor.id));
    assert!(!staging.exists());
    let empty = resource_effect_id();
    let mut writer = Filesystem::open_writer(&reopened, &empty, None, "/empty")
        .await
        .unwrap();
    Filesystem::close_writer(&reopened, &resource_effect_id(), None, &mut writer)
        .await
        .unwrap();
    assert!(writer.closed);
    assert_eq!(
        std::fs::read(reopened.vfs().root().join("empty")).unwrap(),
        b""
    );
    fixture.assert_no_pending_work().await;
}

#[tokio::test]
async fn byte_writes_and_retries_do_not_use_the_filesystem_journal() {
    let fixture = Fixture::with_filesystem_limit(Some(8)).await;
    fixture.database.transaction(|connection| Box::pin(async move {
        sqlx::query("CREATE TRIGGER refuse_effect BEFORE INSERT ON durable_filesystem_operations BEGIN SELECT RAISE(FAIL, 'byte writes must not journal'); END").execute(connection).await?;
        Ok(())
    })).await.unwrap();
    let effect = resource_effect_id();
    for _ in 0..2 {
        fixture
            .filesystem
            .operation(None)
            .write_bytes(&effect, "/file", b"original".to_vec())
            .await
            .unwrap();
    }
    let reopened = fixture
        .filesystem
        .reopen(
            fixture.filesystem.vfs().clone(),
            &fixture.run,
            fixture.units(),
        )
        .await
        .unwrap();
    reopened
        .operation(None)
        .write_bytes(&effect, "/file", b"original".to_vec())
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(reopened.vfs().root().join("file")).unwrap(),
        b"original"
    );
    assert_eq!(reopened.vfs().quota().unwrap().used(), 8);
    let mut unit = fixture.units()().await.unwrap();
    assert!(
        unit.get_journal_entry(&fixture.run, &effect)
            .await
            .unwrap()
            .is_none()
    );
}

fn recorded_resource<T: serde::Serialize + serde::de::DeserializeOwned>(resource: &T) -> T {
    serde_json::from_slice(&serde_json::to_vec(resource).unwrap()).unwrap()
}
