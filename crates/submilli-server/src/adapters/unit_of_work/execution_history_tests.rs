use super::durable::units;
use crate::database::ServerDatabase;
use interpreter::runtime::effects::{CODEC_VERSION, EffectHistory, RecordedOutcome};
use std::sync::Arc;
use submilli_durable::history::{ExecutionAttempt, History, HistoryEntry};
use submilli_shared::secret_store::SecretCipher;

async fn fixture(
    path: &std::path::Path,
) -> (
    Arc<ServerDatabase>,
    submilli_durable::UnitFactory,
    ExecutionAttempt,
) {
    let database = Arc::new(ServerDatabase::open(path).await.unwrap());
    database.transaction(|connection| Box::pin(async move {
        sqlx::query("INSERT OR IGNORE INTO runs(run_id,reservation_id,session_id,fingerprint,owner_generation,attempt,admission_order,created_at_unix_ms,state,recovery_mode) VALUES('00000000-0000-4000-8000-000000000001','00000000-0000-4000-8000-000000000001','session','original','owner',1,1,0,'running','recorded')")
            .execute(connection).await?;
        Ok(())
    })).await.unwrap();
    let factory = units(database.clone(), Arc::new(SecretCipher::from_key([9; 32])));
    let attempt = ExecutionAttempt {
        run_id: "00000000-0000-4000-8000-000000000001".into(),
        owner_generation: "owner".into(),
        attempt: 1,
    };
    (database, factory, attempt)
}
fn result(bytes: &[u8]) -> RecordedOutcome {
    Ok(interpreter::runtime::effects::OutcomeBytes(bytes.into()))
}

#[tokio::test]
async fn execution_history_records_secret_envelopes_before_decryption() {
    use interpreter::runtime::{EncryptedSecret, SecretProvider};
    use submilli_shared::BlueprintSecretProvider;

    // Deliberately fail after retrieval so the test can inspect exactly what the
    // effect committed, before any plaintext was returned to guest code.
    struct DecryptionUnavailable(BlueprintSecretProvider);
    impl SecretProvider for DecryptionUnavailable {
        fn get<'a>(
            &'a self,
            name: &'a str,
        ) -> futures::future::BoxFuture<'a, Result<Option<String>, String>> {
            self.0.get(name)
        }
        fn get_encrypted<'a>(
            &'a self,
            name: &'a str,
        ) -> futures::future::BoxFuture<'a, Result<Option<EncryptedSecret>, String>> {
            self.0.get_encrypted(name)
        }
        fn decrypt(&self, _: &str, _: &EncryptedSecret) -> Result<String, String> {
            Err("decryption temporarily unavailable".into())
        }
    }
    struct ReplayOnly(BlueprintSecretProvider);
    impl SecretProvider for ReplayOnly {
        fn get<'a>(
            &'a self,
            _: &'a str,
        ) -> futures::future::BoxFuture<'a, Result<Option<String>, String>> {
            Box::pin(async { panic!("replay must not retrieve secrets") })
        }
        fn decrypt(&self, name: &str, encrypted: &EncryptedSecret) -> Result<String, String> {
            self.0.decrypt(name, encrypted)
        }
    }
    fn provider(value: &str, key: u8) -> BlueprintSecretProvider {
        BlueprintSecretProvider::with_harness(
            Arc::new(
                submilli_blueprint::parse("name: test\nsecrets:\n  TOKEN: { harness: {} }\n")
                    .unwrap(),
            ),
            None,
            Arc::new(std::collections::BTreeMap::from([(
                "TOKEN".into(),
                value.into(),
            )])),
        )
        .with_cipher(Some(Arc::new(SecretCipher::from_key([key; 32]))))
    }
    async fn execute(
        history: Arc<dyn EffectHistory>,
        provider: Arc<dyn SecretProvider>,
    ) -> wasmtime::Result<Option<String>> {
        use interpreter::runtime::{
            RuntimeConfig, StoreData, Vfs, dispatch_main_async, install_runtime_async,
        };
        let source = r#"
            import { get } from "submilli:secrets";
            function main(): string {
                const secret = get("TOKEN");
                assert(get("ABSENT") === undefined);
                return secret ?? "missing";
            }
        "#;
        // Production main code cannot read secrets; compile under a package's
        // identity so this exercises the real package-only capability boundary.
        let compiled = interpreter::compile::compile_script_owned_by(
            "@test/secrets",
            source,
            "secrets.ts",
            interpreter::FileId(0),
            &[],
            &[],
        )
        .unwrap();
        let config = RuntimeConfig::default();
        let engine = config.engine().unwrap();
        let mut data = StoreData::with_vfs("test-run".into(), Vfs::tempdir().unwrap());
        data.effects = Some(history);
        data.secret_provider = provider;
        let mut store = config.store(&engine, data).unwrap();
        let module = wasmtime::Module::new(&engine, &compiled.wasm).unwrap();
        let mut linker = wasmtime::Linker::new(&engine);
        install_runtime_async(&mut linker, &mut store)
            .await
            .unwrap();
        let instance =
            interpreter::runtime::instantiate_program_async(&linker, &mut store, &module)
                .await
                .unwrap();
        dispatch_main_async(&mut store, &instance).await
    }

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("db");
    let (database, units, attempt) = fixture(&path).await;
    let original = "original-credential-canary";
    let history = Arc::new(History::new(attempt.clone(), units.clone()));
    let failure = execute(
        history.clone(),
        Arc::new(DecryptionUnavailable(provider(original, 3))),
    )
    .await
    .unwrap_err();
    assert!(failure.is::<interpreter::runtime::host::FatalHostError>());
    assert!(
        failure
            .to_string()
            .contains("decryption temporarily unavailable")
    );
    assert_eq!(history.position().await, 1);

    let mut unit = units().await.unwrap();
    let entry = unit
        .get_history_entry(&attempt.run_id, 0)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(entry.operation, "secrets.getEncrypted");
    let bytes = entry.outcome.as_ref().unwrap();
    // This is the decoded history entry, after removing SQL's outer encryption.
    // The effect itself must contain no plaintext, regardless of its storage.
    assert!(
        !bytes
            .0
            .windows(original.len())
            .any(|part| part == original.as_bytes())
    );
    let envelope: Option<EncryptedSecret> = rmp_serde::from_slice(&bytes.0).unwrap();
    let envelope = envelope.unwrap();
    let first_provider = provider(original, 3);
    assert_eq!(
        envelope.key_id,
        first_provider
            .get_encrypted("TOKEN")
            .await
            .unwrap()
            .unwrap()
            .key_id
    );
    assert_eq!(
        first_provider.decrypt("TOKEN", &envelope).unwrap(),
        original
    );
    drop(unit);
    drop(history);
    drop(units);
    database.close().await.unwrap();
    drop(database);

    let (_database, units, attempt) = fixture(&path).await;
    let history = Arc::new(History::new(attempt.clone(), units.clone()));
    let wrong_key = execute(history, Arc::new(provider("rotated", 4)))
        .await
        .unwrap_err();
    assert!(wrong_key.is::<interpreter::runtime::host::FatalHostError>());
    assert!(wrong_key.to_string().contains("different encryption key"));

    let history = Arc::new(History::new(attempt.clone(), units.clone()));
    let replay = execute(history.clone(), Arc::new(provider("rotated", 3)))
        .await
        .unwrap();
    assert_eq!(replay.as_deref(), Some(original));
    assert_eq!(history.position().await, 2);
    history.ensure_history_consumed().await.unwrap();
    let mut unit = units().await.unwrap();
    assert_eq!(
        unit.get_history_entry(&attempt.run_id, 0)
            .await
            .unwrap()
            .unwrap(),
        entry
    );
    drop(unit);

    // Once absence is recorded, replay needs no provider retrieval at all.
    let history = Arc::new(History::new(attempt, units));
    let replay = execute(history.clone(), Arc::new(ReplayOnly(provider("unused", 3))))
        .await
        .unwrap();
    assert_eq!(replay.as_deref(), Some(original));
    history.ensure_history_consumed().await.unwrap();
}

#[tokio::test]
async fn execution_history_reopens_without_dispatching_saved_calls() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.db");
    let (database, units, attempt) = fixture(&path).await;
    let history = History::new(attempt, units.clone());
    let call = history.begin("test.native", [3; 32]).await.unwrap();
    assert!(call.recorded.is_none());
    let id = call.id.clone();
    history
        .complete(&call, result(b"secret-native-output"))
        .await
        .unwrap();
    history.ensure_history_consumed().await.unwrap();
    drop(history);
    drop(units);
    database.close().await.unwrap();
    drop(database);
    let (_database, units, attempt) = fixture(&path).await;
    let history = History::new(attempt, units);
    assert!(history.ensure_history_consumed().await.is_err());
    let call = history.begin("test.native", [3; 32]).await.unwrap();
    assert_eq!(call.id, id);
    assert_eq!(call.recorded.unwrap(), result(b"secret-native-output"));
    history.ensure_history_consumed().await.unwrap();
}

#[tokio::test]
async fn execution_history_missing_outcome_reuses_id_and_allows_another_mutation() {
    let directory = tempfile::tempdir().unwrap();
    let (_database, units, attempt) = fixture(&directory.path().join("db")).await;
    let history = History::new(attempt.clone(), units.clone());
    let unfinished = history.begin("increment", [0; 32]).await.unwrap();
    let mut mutations = 1;
    assert!(history.begin("later", [0; 32]).await.is_err());
    drop(history); // The action completed, but its outcome never committed.
    let history = History::new(attempt, units);
    let retry = history.begin("increment", [0; 32]).await.unwrap();
    assert_eq!(retry.id, unfinished.id);
    assert!(retry.recorded.is_none());
    mutations += 1;
    history
        .complete(&retry, result(&[mutations]))
        .await
        .unwrap();
    assert_eq!(mutations, 2); // Framework retry makes no idempotency promise.
}

#[tokio::test]
async fn execution_history_rejects_divergence_and_stale_completion() {
    let directory = tempfile::tempdir().unwrap();
    let (database, units, attempt) = fixture(&directory.path().join("db")).await;
    let history = History::new(attempt.clone(), units.clone());
    let call = history.begin("original", [1; 32]).await.unwrap();
    history.complete(&call, result(b"one")).await.unwrap();
    assert!(
        History::new(attempt.clone(), units.clone())
            .begin("different", [1; 32])
            .await
            .is_err()
    );
    assert!(
        History::new(attempt.clone(), units.clone())
            .begin("original", [2; 32])
            .await
            .is_err()
    );
    let pending = history.begin("next", [0; 32]).await.unwrap();
    database
        .transaction(|connection| {
            Box::pin(async move {
                sqlx::query(
                    "UPDATE runs SET state='cancelled',attempt=attempt+1 WHERE run_id='00000000-0000-4000-8000-000000000001'",
                )
                .execute(connection)
                .await?;
                Ok(())
            })
        })
        .await
        .unwrap();
    assert!(history.complete(&pending, result(b"late")).await.is_err());
    let mut unit = units().await.unwrap();
    assert!(
        unit.get_history_entry(&attempt.run_id, 1)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn execution_history_append_and_accounting_roll_back_together_and_are_sealed() {
    let directory = tempfile::tempdir().unwrap();
    let (database, units, attempt) = fixture(&directory.path().join("db")).await;
    let entry = HistoryEntry {
        sequence: 0,
        operation: "native".into(),
        version: CODEC_VERSION,
        arguments_fingerprint: [7; 32],
        outcome: result(b"plaintext-canary"),
    };
    let mut unit = units().await.unwrap();
    unit.append_history(&attempt, Arc::new(entry.clone()))
        .await
        .unwrap();
    assert_eq!(unit.history_progress(&attempt).await.unwrap().position, 1);
    drop(unit);
    let mut unit = units().await.unwrap();
    assert_eq!(unit.history_progress(&attempt).await.unwrap().position, 0);
    assert!(
        unit.get_history_entry(&attempt.run_id, 0)
            .await
            .unwrap()
            .is_none()
    );
    unit.append_history(&attempt, Arc::new(entry.clone()))
        .await
        .unwrap();
    assert_eq!(unit.history_progress(&attempt).await.unwrap().position, 1);
    unit.commit().await.unwrap();
    let stored: Vec<u8> = database
        .transaction(|connection| {
            Box::pin(async move {
                Ok(sqlx::query_scalar(
                    "SELECT content FROM durable_outcomes WHERE run_id='00000000-0000-4000-8000-000000000001' AND sequence=0",
                )
                .fetch_one(connection)
                .await?)
            })
        })
        .await
        .unwrap();
    assert!(
        !stored
            .windows(b"plaintext-canary".len())
            .any(|w| w == b"plaintext-canary")
    );
    let mut unit = units().await.unwrap();
    assert!(
        unit.append_history(&attempt, Arc::new(entry.clone()))
            .await
            .is_err()
    );
}

async fn run_script(source: &str, history: Arc<History>, root: &std::path::Path) -> Option<String> {
    run_script_with(source, history, root, |_| {}).await
}

async fn run_script_with(
    source: &str,
    history: Arc<dyn EffectHistory>,
    root: &std::path::Path,
    configure: impl FnOnce(&mut interpreter::runtime::StoreData),
) -> Option<String> {
    try_run_script_with(source, history, root, configure)
        .await
        .unwrap()
}

async fn try_run_script_with(
    source: &str,
    history: Arc<dyn EffectHistory>,
    root: &std::path::Path,
    configure: impl FnOnce(&mut interpreter::runtime::StoreData),
) -> wasmtime::Result<Option<String>> {
    use interpreter::runtime::{
        RuntimeConfig, StoreData, Vfs, dispatch_main_async, install_runtime_async,
        instantiate_program_async,
    };
    let compiled =
        interpreter::compile_script(source, "replay.ts", interpreter::FileId(0), &[], &[]).unwrap();
    let config = RuntimeConfig::default();
    let engine = config.engine().unwrap();
    let mut data = StoreData::with_vfs(
        "test-run".into(),
        Vfs::external(root.to_path_buf()).unwrap(),
    );
    data.effects = Some(history.clone());
    configure(&mut data);
    let mut store = config.store(&engine, data).unwrap();
    let module = wasmtime::Module::new(&engine, &compiled.wasm).unwrap();
    let mut linker = wasmtime::Linker::new(&engine);
    install_runtime_async(&mut linker, &mut store)
        .await
        .unwrap();
    let instance = instantiate_program_async(&linker, &mut store, &module)
        .await
        .unwrap();
    let result = dispatch_main_async(&mut store, &instance).await;
    if result.is_ok() {
        history.ensure_history_consumed().await.unwrap();
    }
    store.data_mut().blocking_work.finish().await.unwrap();
    result
}

#[tokio::test]
async fn execution_history_replays_observations_and_skips_completed_filesystem_writes() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("files");
    std::fs::create_dir(&root).unwrap();
    let (_database, units, attempt) = fixture(&directory.path().join("db")).await;
    let source = r#"
        import { v4, v7 } from "submilli:uuid";
        import { randomBytes } from "submilli:crypto";
        import { writeText, appendText, size, exists } from "submilli:fs";
        const first = v4();
        function main(): string {
            writeText("/file", "hello");
            appendText("/file", "!");
            return first + "|" + v7() + "|" + Math.random().toString() + "|"
                + randomBytes(8).toString() + "|" + Temporal.Now.instant().toString()
                + "|" + Temporal.Now.timeZoneId() + "|" + Temporal.Now.zonedDateTimeISO("UTC").toString()
                + "|" + Temporal.Now.zonedDateTimeISO("America/New_York").toString()
                + "|" + Temporal.Now.zonedDateTimeISO("-05:30").toString()
                + "|" + Temporal.Now.zonedDateTimeISO().toString()
                + "|" + Temporal.Now.zonedDateTime("US/Eastern").toString()
                + "|" + Temporal.Now.plainDateISO("America/New_York").toString()
                + "|" + Temporal.Now.plainTimeISO("+02:00").toString()
                + "|" + Temporal.Now.plainDateTimeISO().toString()
                + "|" + size("/file").toString() + "|" + exists("/file").toString();
        }
    "#;
    let first = run_script(
        source,
        Arc::new(History::new(attempt.clone(), units.clone())),
        &root,
    )
    .await;
    assert_eq!(std::fs::read(root.join("file")).unwrap(), b"hello!");
    std::fs::write(root.join("file"), b"unchanged during replay").unwrap();
    let replay = run_script(source, Arc::new(History::new(attempt, units)), &root).await;
    assert_eq!(first, replay, "saved results survive a fresh interpreter");
    assert_eq!(
        std::fs::read(root.join("file")).unwrap(),
        b"unchanged during replay"
    );
}

#[tokio::test]
async fn execution_history_rejects_process_local_resources_with_run_ids() {
    use interpreter::runtime::{
        RuntimeConfig, StoreData, Vfs, dispatch_main_async, install_runtime_async,
        instantiate_program_async,
    };
    for (operation, resource) in [
        ("bytes(\"/input\", 3)", "reader"),
        ("list(\"/\", false)", "listing"),
        ("writer(\"/output\")", "writer"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("files");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("input"), b"abc").unwrap();
        let (_database, units, attempt) = fixture(&directory.path().join("db")).await;
        let history = Arc::new(History::new(attempt.clone(), units.clone()));
        let mut data = StoreData::with_vfs("test-run".into(), Vfs::external(root.clone()).unwrap());
        data.effects = Some(history);
        let source = format!(
            r#"
            import {{ bytes, list, writer }} from "submilli:fs";
            function main(): void {{ {operation}; }}
        "#
        );
        let compiled =
            interpreter::compile_script(&source, "resources.ts", interpreter::FileId(0), &[], &[])
                .unwrap();
        let config = RuntimeConfig::default();
        let engine = config.engine().unwrap();
        let mut store = config.store(&engine, data).unwrap();
        let module = wasmtime::Module::new(&engine, &compiled.wasm).unwrap();
        let mut linker = wasmtime::Linker::new(&engine);
        install_runtime_async(&mut linker, &mut store)
            .await
            .unwrap();
        let instance = instantiate_program_async(&linker, &mut store, &module)
            .await
            .unwrap();
        let error = dispatch_main_async(&mut store, &instance)
            .await
            .unwrap_err();
        store.data_mut().blocking_work.finish().await.unwrap();
        assert!(
            error.is::<interpreter::runtime::host::FatalHostError>(),
            "{error:#}"
        );
        assert!(
            format!("{error:#}").contains(&format!("requires a persistent {resource}")),
            "{error:#}"
        );
        let mut unit = units().await.unwrap();
        assert!(
            unit.get_history_entry(&attempt.run_id, 0)
                .await
                .unwrap()
                .is_none()
        );
    }
}

struct InterruptHistory {
    history: History,
    before_commit: u64,
}
#[async_trait::async_trait]
impl EffectHistory for InterruptHistory {
    async fn begin(
        &self,
        operation: &str,
        fingerprint: [u8; 32],
    ) -> Result<
        interpreter::runtime::effects::EffectCall,
        interpreter::runtime::effects::HistoryError,
    > {
        self.history.begin(operation, fingerprint).await
    }
    async fn complete(
        &self,
        call: &interpreter::runtime::effects::EffectCall,
        outcome: RecordedOutcome,
    ) -> Result<(), interpreter::runtime::effects::HistoryError> {
        if call.sequence == self.before_commit {
            return Err(interpreter::runtime::effects::HistoryError(
                "injected loss before outcome commit".into(),
            ));
        }
        self.history.complete(call, outcome).await
    }
    async fn ensure_history_consumed(
        &self,
    ) -> Result<(), interpreter::runtime::effects::HistoryError> {
        self.history.ensure_history_consumed().await
    }
}

async fn run_filesystem_script(
    source: &str,
    history: Arc<dyn EffectHistory>,
    root: &std::path::Path,
    units: submilli_durable::UnitFactory,
    attempt: &ExecutionAttempt,
) -> wasmtime::Result<Option<String>> {
    use interpreter::runtime::{
        RuntimeConfig, StoreData, Vfs, dispatch_main_async, install_runtime_async,
        instantiate_program_async,
    };
    let vfs = Vfs::external(root.to_path_buf())?;
    let filesystem =
        submilli_durable::DurableFilesystem::open(vfs.clone(), &attempt.run_id, units).await?;
    let mut data = StoreData::with_vfs(attempt.run_id.clone(), vfs);
    data.filesystem = Arc::new(filesystem);
    data.effects = Some(history.clone());
    let compiled =
        interpreter::compile_script(source, "resources.ts", interpreter::FileId(0), &[], &[])
            .unwrap();
    let config = RuntimeConfig::default();
    let engine = config.engine()?;
    let mut store = config.store(&engine, data)?;
    let module = wasmtime::Module::new(&engine, &compiled.wasm)?;
    let mut linker = wasmtime::Linker::new(&engine);
    install_runtime_async(&mut linker, &mut store).await?;
    let instance = instantiate_program_async(&linker, &mut store, &module).await?;
    let result = dispatch_main_async(&mut store, &instance).await;
    store.data_mut().blocking_work.finish().await?;
    let value = result?;
    history.ensure_history_consumed().await?;
    Ok(value)
}

#[tokio::test]
async fn execution_history_resumes_id_only_resources_after_each_uncommitted_result() {
    let source = r#"
        import { writeText, lines, bytes, writer, list, readText } from "submilli:fs";
        function main(): string {
            writeText("/input", "one\ntwo\nthree\n");
            let text = "";
            for (const line of lines("/input")) { text += line + "|"; }
            let count = 0;
            for (const chunk of bytes("/input", 4)) { count += chunk.length; }
            const output = writer("/output");
            output.writeLine(text);
            output.writeLine(count.toString());
            output.close();
            let entries = 0;
            for (const entry of list("/", false)) { entries++; }
            return (readText("/output") ?? "") + "|" + entries.toString();
        }
    "#;
    // Stop after each native operation, before its outer history commit. Each
    // fresh store first skips earlier saved calls, then retries the same effect ID.
    for before_commit in 0..24 {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("files");
        std::fs::create_dir(&root).unwrap();
        let (_database, units, attempt) = fixture(&directory.path().join("db")).await;
        let interrupted = Arc::new(InterruptHistory {
            history: History::new(attempt.clone(), units.clone()),
            before_commit,
        });
        let first =
            run_filesystem_script(source, interrupted, &root, units.clone(), &attempt).await;
        if let Err(error) = first.as_ref() {
            assert!(
                error.to_string().contains("injected loss"),
                "call {before_commit}: {error:#}"
            );
        }
        let replay = run_filesystem_script(
            source,
            Arc::new(History::new(attempt.clone(), units.clone())),
            &root,
            units,
            &attempt,
        )
        .await
        .unwrap_or_else(|error| panic!("call {before_commit}: {error:#}"));
        assert_eq!(
            replay.as_deref(),
            Some("one|two|three|\n14\n|2"),
            "call {before_commit}"
        );
    }
}

#[tokio::test]
async fn execution_history_replays_after_result_conversion_runs_out_of_fuel() {
    use interpreter::runtime::host::{read_string_arg, write_submilli_string_struct};
    use interpreter::runtime::{RuntimeConfig, StoreData, Vfs, effects, install_runtime_async};
    use std::sync::atomic::{AtomicUsize, Ordering};

    let directory = tempfile::tempdir().unwrap();
    let (_database, units, attempt) = fixture(&directory.path().join("db")).await;
    let calls = Arc::new(AtomicUsize::new(0));
    for replay in [false, true] {
        let history = Arc::new(History::new(attempt.clone(), units.clone()));
        let config = RuntimeConfig::default();
        let engine = config.engine().unwrap();
        let mut data = StoreData::with_vfs("test-run".into(), Vfs::none());
        data.effects = Some(history.clone());
        let mut store = config.store_async(&engine, data).unwrap();
        let mut linker = wasmtime::Linker::new(&engine);
        install_runtime_async(&mut linker, &mut store)
            .await
            .unwrap();
        let calls = calls.clone();
        let probe = wasmtime::Func::new_async(
            &mut store,
            wasmtime::FuncType::new(&engine, [], [wasmtime::ValType::I32]),
            move |mut caller, _, results| {
                let calls = calls.clone();
                Box::new(async move {
                    let value = effects::execute(
                        &mut caller,
                        "test.result_conversion",
                        (),
                        move |caller, _, ()| {
                            Box::pin(async move {
                                calls.fetch_add(1, Ordering::SeqCst);
                                // Model a completed operation that spent the remaining fuel.
                                interpreter::runtime::fuel::settle_host_fuel(
                                    &mut *caller,
                                    1_000_000,
                                )?;
                                caller.set_fuel(0)?;
                                Ok("saved result".to_owned())
                            })
                        },
                    )
                    .await?;
                    let string = write_submilli_string_struct(&mut caller, &value)?;
                    let value = wasmtime::Val::AnyRef(Some(string.to_anyref()));
                    assert_eq!(
                        read_string_arg(&mut caller, &value, "result")?,
                        "saved result"
                    );
                    results[0] = wasmtime::Val::I32(1);
                    Ok(())
                })
            },
        );
        store.set_fuel(1_000_000).unwrap();
        let mut output = [wasmtime::Val::I32(0)];
        let result = probe.call_async(&mut store, &[], &mut output).await;
        if replay {
            result.unwrap();
            assert_eq!(output[0].i32(), Some(1));
        } else {
            assert_eq!(
                result.unwrap_err().downcast_ref::<wasmtime::Trap>(),
                Some(&wasmtime::Trap::OutOfFuel)
            );
        }
        history.ensure_history_consumed().await.unwrap();
        let mut unit = units().await.unwrap();
        assert!(
            unit.get_history_entry(&attempt.run_id, 0)
                .await
                .unwrap()
                .is_some()
        );
    }
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "replay must not repeat the operation"
    );
}

#[tokio::test]
async fn execution_history_keeps_large_outcomes_without_a_guest_memory_reservation() {
    let directory = tempfile::tempdir().unwrap();
    let (_database, units, attempt) = fixture(&directory.path().join("db")).await;
    let original = History::new(attempt.clone(), units.clone());
    let call = original.begin("bytes", [0; 32]).await.unwrap();
    let outcome = result(&vec![42; 65536]);
    original.complete(&call, outcome.clone()).await.unwrap();
    let replay = History::new(attempt, units);
    let call = replay.begin("bytes", [0; 32]).await.unwrap();
    assert_eq!(call.recorded, Some(outcome));
}

struct CountedHttp(std::sync::atomic::AtomicUsize);
#[async_trait::async_trait]
impl interpreter::stdlib::http::HttpClient for CountedHttp {
    async fn send(
        &self,
        request: &interpreter::stdlib::http::HttpRequest,
    ) -> Result<interpreter::stdlib::http::HttpResponse, interpreter::stdlib::http::HttpError> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if request.url.ends_with("/fail") {
            return Err(interpreter::stdlib::http::HttpError::Other(
                "saved failure".into(),
            ));
        }
        Ok(interpreter::stdlib::http::HttpResponse {
            status: 200,
            status_text: "OK".into(),
            headers: vec![("x-original".into(), "yes".into())],
            body: "original response".as_bytes().to_vec(),
            final_url: request.url.clone(),
        })
    }
    async fn download(
        &self,
        _: &interpreter::stdlib::http::HttpRequest,
        _: &mut (dyn std::io::Write + Send),
    ) -> Result<
        interpreter::stdlib::http::transport::DownloadMeta,
        interpreter::stdlib::http::HttpError,
    > {
        panic!("download is not used by this fixture")
    }
}
#[tokio::test]
async fn execution_history_replays_each_code_filesystem_operation() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("files");
    std::fs::create_dir(&root).unwrap();
    for (name, content) in [
        ("a.txt", "before\n"),
        ("b.txt", "other\n"),
        ("c.txt", "old\n"),
    ] {
        std::fs::write(root.join(name), content).unwrap();
    }
    let path = directory.path().join("db");
    let (database, units, attempt) = fixture(&path).await;
    let source = r#"
        import { read, search, glob, tree, edit, insertAt, diffText, diffFiles, applyPatch } from "submilli:code";
        function main(): string {
            const window = read("/a.txt", 1, 1);
            const matches = search("before", { include: ["*.txt"] });
            const files = glob("*.txt");
            const listing = tree("/", 1);
            const comparison = diffFiles("/a.txt", "/b.txt");
            const edited = edit("/a.txt", "before", "after");
            const inserted = insertAt("/b.txt", 1, "head\n");
            const patched = applyPatch("/c.txt", "--- a\n+++ b\n@@ -1,1 +1,1 @@\n-old\n+new\n");
            const pure = diffText("\ud800a", "\ud800b");
            assert(window.lines[0].text === "before");
            assert(matches.matches.length === 1);
            assert(files.entries.length === 3 && listing.entries.length === 3);
            assert(comparison.includes("before") && comparison.includes("other"));
            assert(edited.changed && inserted.changed && patched.changed);
            assert(pure.includes("\ud800"));
            return JSON.stringify({ window, matches, files, listing, comparison, edited, inserted, patched, pure }) ?? "";
        }
    "#;
    let history = Arc::new(History::new(attempt.clone(), units.clone()));
    let original = run_filesystem_script(source, history.clone(), &root, units.clone(), &attempt)
        .await
        .unwrap();
    // The eight filesystem calls are recorded; pure diffText creates no effect.
    assert_eq!(history.position().await, 8);
    for (name, content) in [
        ("a.txt", "after\n"),
        ("b.txt", "head\nother\n"),
        ("c.txt", "new\n"),
    ] {
        assert_eq!(std::fs::read_to_string(root.join(name)).unwrap(), content);
        std::fs::remove_file(root.join(name)).unwrap();
    }
    std::fs::write(root.join("sentinel"), "untouched").unwrap();
    drop(history);
    drop(units);
    database.close().await.unwrap();
    drop(database);

    let (_database, units, attempt) = fixture(&path).await;
    let history = Arc::new(History::new(attempt.clone(), units.clone()));
    let replay = run_filesystem_script(source, history.clone(), &root, units, &attempt)
        .await
        .unwrap();
    assert_eq!(original, replay);
    assert_eq!(history.position().await, 8);
    assert_eq!(
        std::fs::read_dir(&root)
            .unwrap()
            .filter(|entry| entry.as_ref().unwrap().file_name() != ".git-submilli-recovery")
            .count(),
        1
    );
    assert_eq!(
        std::fs::read_to_string(root.join("sentinel")).unwrap(),
        "untouched"
    );
}

#[tokio::test]
async fn execution_history_replays_git_results_without_reopening_the_repository() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("files");
    std::fs::create_dir(&root).unwrap();
    let path = directory.path().join("db");
    let (database, units, attempt) = fixture(&path).await;
    let source = r#"
        import { Repository } from "submilli:git";
        import { writeText } from "submilli:fs";
        function main(): string {
            const repo = Repository.init("/repo");
            writeText("/repo/file.txt", "hello\n");
            repo.add(["file.txt"]);
            const commit = repo.commit("initial");
            const status = repo.status();
            const history = repo.log();
            const comparison = repo.diff();
            const contents = repo.show("HEAD", "file.txt");
            const branches = repo.branches();
            repo.createBranch("topic");
            repo.switchBranch("topic");
            repo.addRemote("origin", "https://example.com/repo.git");
            repo.setRemoteUrl("origin", "https://example.com/other.git");
            const remotes = repo.remotes();
            const reopened = Repository.open("/repo");
            const reopenedStatus = reopened.status();
            assert(contents[0] === 104);
            return JSON.stringify({ commit, status, history, comparison, branches, remotes, reopenedStatus }) ?? "";
        }
    "#;
    let configure = |data: &mut interpreter::runtime::StoreData| {
        data.git = Some(interpreter::stdlib::git::GitConfig {
            name: "Test".into(),
            email: "test@example.com".into(),
            username: None,
        });
    };
    let history = Arc::new(History::new(attempt, units.clone()));
    let original = run_script_with(source, history.clone(), &root, configure).await;
    assert_eq!(history.position().await, 16);
    assert_eq!(
        std::fs::read_to_string(root.join("repo/file.txt")).unwrap(),
        "hello\n"
    );
    std::fs::remove_dir_all(root.join("repo")).unwrap();
    drop(history);
    drop(units);
    database.close().await.unwrap();
    drop(database);

    let (_database, units, attempt) = fixture(&path).await;
    let history = Arc::new(History::new(attempt, units));
    let replay = run_script_with(source, history.clone(), &root, configure).await;
    assert_eq!(original, replay);
    assert_eq!(history.position().await, 16);
    assert!(!root.join("repo").exists());
}

#[tokio::test]
async fn execution_history_replays_native_http_errors_session_values_and_code_mutation() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("files");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("code"), "before\n").unwrap();
    let (_database, units, attempt) = fixture(&directory.path().join("db")).await;
    let http = Arc::new(CountedHttp(std::sync::atomic::AtomicUsize::new(0)));
    let source = r#"
        import { get } from "submilli:http";
        import * as session from "submilli:session";
        import { edit } from "submilli:code";
        function main(): string {
            const response = get("https://example.test/value");
            let failure = "";
            try { get("https://example.test/fail"); } catch (error) { failure = error.message; }
            session.set("surrogate", "\ud800");
            const saved = session.get<string>("surrogate");
            assert(saved !== undefined && saved.charCodeAt(0) === 55296);
            assert(edit("/code", "before", "after").changed);
            return response.body + "|" + failure;
        }
    "#;
    let filesystem = Arc::new(
        submilli_durable::DurableFilesystem::open(
            interpreter::runtime::Vfs::external(root.clone()).unwrap(),
            &attempt.run_id,
            units.clone(),
        )
        .await
        .unwrap(),
    );
    let original = run_script_with(
        source,
        Arc::new(History::new(attempt.clone(), units.clone())),
        &root,
        |data| {
            data.filesystem = filesystem.clone();
            data.http_client = http.clone();
            data.session_kv = Some(Arc::new(interpreter::runtime::InMemorySessionKv::default()));
        },
    )
    .await;
    assert_eq!(http.0.load(std::sync::atomic::Ordering::SeqCst), 2);
    std::fs::write(root.join("code"), "later contents").unwrap();
    let replay = run_script_with(
        source,
        Arc::new(History::new(attempt, units)),
        &root,
        |data| {
            data.filesystem = filesystem.clone();
            data.http_client = http.clone();
            data.session_kv = Some(Arc::new(interpreter::runtime::InMemorySessionKv::default()));
        },
    )
    .await;
    assert_eq!(original, replay);
    assert_eq!(http.0.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert_eq!(
        std::fs::read_to_string(root.join("code")).unwrap(),
        "later contents"
    );
}

/// Inspect budgets while the outcome is being saved, before execute returns.
struct InspectSpendHistory {
    history: History,
    inspect: Box<dyn Fn() + Send + Sync>,
    fail_commit: bool,
}
#[async_trait::async_trait]
impl EffectHistory for InspectSpendHistory {
    async fn begin(
        &self,
        operation: &str,
        fingerprint: [u8; 32],
    ) -> Result<
        interpreter::runtime::effects::EffectCall,
        interpreter::runtime::effects::HistoryError,
    > {
        self.history.begin(operation, fingerprint).await
    }
    async fn complete(
        &self,
        call: &interpreter::runtime::effects::EffectCall,
        outcome: RecordedOutcome,
    ) -> Result<(), interpreter::runtime::effects::HistoryError> {
        (self.inspect)();
        if self.fail_commit {
            return Err(interpreter::runtime::effects::HistoryError(
                "injected commit failure".into(),
            ));
        }
        self.history.complete(call, outcome).await
    }
    async fn ensure_history_consumed(
        &self,
    ) -> Result<(), interpreter::runtime::effects::HistoryError> {
        self.history.ensure_history_consumed().await
    }
}

struct SpendLlm {
    outcome: interpreter::runtime::llm::LlmOutcome,
    calls: std::sync::atomic::AtomicU64,
}
impl interpreter::runtime::llm::LlmProvider for SpendLlm {
    fn call<'a>(
        &'a self,
        _: &'a str,
        _: &'a [String],
        _: Option<&'a str>,
    ) -> std::pin::Pin<
        Box<
            dyn Future<
                    Output = Result<
                        Vec<interpreter::runtime::llm::LlmOutcome>,
                        interpreter::runtime::llm::LlmCallError,
                    >,
                > + Send
                + 'a,
        >,
    > {
        self.calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Box::pin(async { Ok(vec![self.outcome.clone()]) })
    }
    fn models<'a>(
        &'a self,
    ) -> std::pin::Pin<
        Box<
            dyn Future<
                    Output = Result<
                        Vec<interpreter::runtime::llm::LlmModel>,
                        interpreter::runtime::llm::LlmCallError,
                    >,
                > + Send
                + 'a,
        >,
    > {
        Box::pin(async { Ok(Vec::new()) })
    }
}

#[tokio::test]
async fn execution_history_llm_charges_after_commit_and_restores_run_spend() {
    use interpreter::runtime::llm::{
        ExecutionTokenBudget, LlmLimits, LlmOutcome, SharedTokenBudget,
    };
    for (input, output, used, held) in [
        (Some(3), Some(4), 7, 0),
        (None, None, 101, 101),
        (Some(5), None, 101, 96),
    ] {
        for fail_commit in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let (_database, units, attempt) = fixture(&directory.path().join("db")).await;
            let limits = LlmLimits {
                per_execution_tokens: 105,
                default_output_cap: 100,
                ..LlmLimits::default()
            };
            let aggregate = SharedTokenBudget::new(1_000);
            let budget = Arc::new(ExecutionTokenBudget::new(limits, aggregate.clone()));
            let provider = Arc::new(SpendLlm {
                outcome: LlmOutcome::success("done").with_usage(input, output),
                calls: Default::default(),
            });
            let inspected = budget.clone();
            let history = Arc::new(InspectSpendHistory {
                history: History::new(attempt.clone(), units.clone()),
                inspect: Box::new(move || {
                    assert_eq!(
                        inspected.used(),
                        101,
                        "live estimate remains until execute returns"
                    );
                    assert_eq!(
                        inspected.held(),
                        0,
                        "final usage is not charged inside execute"
                    );
                }),
                fail_commit,
            });
            let source = r#"import llm from "submilli:llm";
                function main(): string { return llm.call("m", "p").text ?? ""; }"#;
            let live = try_run_script_with(source, history, directory.path(), |data| {
                data.llm_provider = Some(provider.clone());
                data.llm_budget = Some(budget.clone());
            })
            .await;
            assert_eq!(live.is_err(), fail_commit);
            assert_eq!(budget.used(), used);
            assert_eq!(budget.held(), held);
            assert_eq!(
                aggregate.used(),
                used,
                "settled once, including on commit failure"
            );
            if fail_commit {
                continue;
            }
            assert_eq!(live.unwrap(), Some("done".into()));

            let replay_aggregate = SharedTokenBudget::new(0);
            let replay_budget =
                Arc::new(ExecutionTokenBudget::new(limits, replay_aggregate.clone()));
            let resumed = r#"import llm from "submilli:llm";
                function main(): string {
                    assert(llm.call("m", "p").text === "done");
                    try { llm.call("m", "q"); return "missed limit"; }
                    catch (e: QuotaExceededError) { return "quota"; }
                }"#;
            let replay = run_script_with(
                resumed,
                Arc::new(History::new(attempt, units)),
                directory.path(),
                |data| {
                    data.llm_provider = Some(provider.clone());
                    data.llm_budget = Some(replay_budget.clone());
                },
            )
            .await;
            assert_eq!(replay, Some("quota".into()));
            assert_eq!(replay_budget.used(), used);
            assert_eq!(replay_budget.held(), held);
            assert_eq!(
                replay_aggregate.used(),
                0,
                "replay makes no shared reservation"
            );
            assert_eq!(provider.calls.load(std::sync::atomic::Ordering::Relaxed), 1);
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum SpendEmbeddingOutcome {
    Reported,
    Unknown,
    PartialFailure,
    CountMismatch,
    Internal,
}
struct SpendEmbedding {
    outcome: SpendEmbeddingOutcome,
    calls: std::sync::atomic::AtomicU64,
}
impl interpreter::runtime::EmbeddingProvider for SpendEmbedding {
    fn embed<'a>(
        &'a self,
        alias: &'a str,
        _: &'a [String],
        _: interpreter::runtime::Purpose,
        budget: &'a interpreter::runtime::EmbeddingTokenBudget,
    ) -> std::pin::Pin<
        Box<
            dyn Future<
                    Output = Result<
                        interpreter::runtime::EmbeddingBatch,
                        interpreter::runtime::EmbeddingError,
                    >,
                > + Send
                + 'a,
        >,
    > {
        use interpreter::runtime::{
            EmbeddingBatch, EmbeddingError, EmbeddingFailureReason, SubBatchSettlement,
        };
        self.calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Box::pin(async move {
            budget.mark_sent(alias, 10)?;
            let settlement = SubBatchSettlement {
                estimate: 10,
                reported: if matches!(self.outcome, SpendEmbeddingOutcome::Unknown) {
                    0
                } else {
                    3
                },
                indeterminate: match self.outcome {
                    SpendEmbeddingOutcome::Unknown => 10,
                    SpendEmbeddingOutcome::PartialFailure | SpendEmbeddingOutcome::Internal => 7,
                    _ => 0,
                },
            };
            let settlements = vec![settlement];
            match self.outcome {
                SpendEmbeddingOutcome::PartialFailure => Err(EmbeddingError::Provider {
                    alias: alias.into(),
                    reason: EmbeddingFailureReason::Transport,
                    settlements,
                }),
                SpendEmbeddingOutcome::Internal => Err(EmbeddingError::Internal {
                    alias: alias.into(),
                    settlements,
                }),
                _ => {
                    let count = if matches!(self.outcome, SpendEmbeddingOutcome::CountMismatch) {
                        2
                    } else {
                        1
                    };
                    Ok(
                        EmbeddingBatch::new(vec![0.5; count], count, 1, "emb1:test", alias)
                            .unwrap()
                            .with_input_tokens(Some(settlement.reported))
                            .with_settlements(settlements),
                    )
                }
            }
        })
    }
    fn models<'a>(
        &'a self,
    ) -> std::pin::Pin<
        Box<
            dyn Future<
                    Output = Result<
                        Vec<interpreter::runtime::EmbeddingModel>,
                        interpreter::runtime::EmbeddingError,
                    >,
                > + Send
                + 'a,
        >,
    > {
        Box::pin(async { Ok(Vec::new()) })
    }
    fn max_input_bytes(&self, _: &str) -> Option<u64> {
        Some(100)
    }
    fn estimate_tokens(&self, _: &str, _: &[String]) -> u64 {
        10
    }
}

#[tokio::test]
async fn execution_history_embedding_charges_success_and_failure_after_commit() {
    use interpreter::runtime::llm::SharedTokenBudget;
    use interpreter::runtime::{EmbeddingLimits, EmbeddingTokenBudget};
    for (outcome, used, held, answer) in [
        (SpendEmbeddingOutcome::Reported, 3, 0, "ok"),
        (SpendEmbeddingOutcome::Unknown, 10, 10, "ok"),
        (SpendEmbeddingOutcome::PartialFailure, 10, 7, "error"),
        (SpendEmbeddingOutcome::CountMismatch, 3, 0, "error"),
        (SpendEmbeddingOutcome::Internal, 10, 7, ""),
    ] {
        for fail_commit in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let (_database, units, attempt) = fixture(&directory.path().join("db")).await;
            let limits = EmbeddingLimits {
                per_execution_tokens: 12,
                ..EmbeddingLimits::default()
            };
            let aggregate = SharedTokenBudget::new(100);
            let budget = Arc::new(EmbeddingTokenBudget::new(limits, aggregate.clone()));
            let provider = Arc::new(SpendEmbedding {
                outcome,
                calls: Default::default(),
            });
            let inspected = budget.clone();
            let history = Arc::new(InspectSpendHistory {
                history: History::new(attempt.clone(), units.clone()),
                inspect: Box::new(move || {
                    assert_eq!(
                        inspected.used(),
                        10,
                        "estimate is not settled before commit"
                    );
                    assert_eq!(
                        inspected.held(),
                        10,
                        "sent estimate remains held before commit"
                    );
                    assert_eq!(inspected.requests(), 1);
                }),
                fail_commit,
            });
            let source = r#"import embedding from "submilli:embedding";
                function main(): string {
                    try { embedding.embed("m", ["a"], "document"); return "ok"; }
                    catch (e: Error) { return "error"; }
                }"#;
            let live = try_run_script_with(source, history, directory.path(), |data| {
                data.embedding_provider = Some(provider.clone());
                data.embedding_budget = Some(budget.clone());
            })
            .await;
            let internal = matches!(outcome, SpendEmbeddingOutcome::Internal);
            assert_eq!(live.is_err(), fail_commit || internal, "{outcome:?}");
            assert_eq!(
                (budget.used(), budget.held(), budget.requests()),
                (used, held, 1)
            );
            assert_eq!(aggregate.used(), used);
            if fail_commit || internal {
                let mut unit = units().await.unwrap();
                assert!(
                    unit.get_history_entry(&attempt.run_id, 0)
                        .await
                        .unwrap()
                        .is_none()
                );
                continue;
            }
            assert_eq!(live.unwrap(), Some(answer.into()));
            let replay_aggregate = SharedTokenBudget::new(0);
            let replay_budget =
                Arc::new(EmbeddingTokenBudget::new(limits, replay_aggregate.clone()));
            let replay = run_script_with(
                source,
                Arc::new(History::new(attempt, units)),
                directory.path(),
                |data| {
                    data.embedding_provider = Some(provider.clone());
                    data.embedding_budget = Some(replay_budget.clone());
                },
            )
            .await;
            assert_eq!(replay, Some(answer.into()));
            assert_eq!(
                (
                    replay_budget.used(),
                    replay_budget.held(),
                    replay_budget.requests()
                ),
                (used, held, 1)
            );
            assert_eq!(replay_aggregate.used(), 0);
            assert_eq!(provider.calls.load(std::sync::atomic::Ordering::Relaxed), 1);
            assert!(
                replay_budget.reserve("m", 10).is_err(),
                "historical usage constrains new requests"
            );
        }
    }
}

#[tokio::test]
async fn execution_history_error_conversion_can_stop_for_fuel_without_losing_the_error() {
    use interpreter::runtime::{
        RuntimeConfig, StoreData, Vfs, effects, fuel, install_runtime_async,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Attempt {
        Direct,
        Record,
        ReplayEmpty,
        ReplayFunded,
    }

    let directory = tempfile::tempdir().unwrap();
    let (_database, units, attempt) = fixture(&directory.path().join("db")).await;
    let calls = Arc::new(AtomicUsize::new(0));
    for mode in [
        Attempt::Direct,
        Attempt::Record,
        Attempt::ReplayEmpty,
        Attempt::ReplayFunded,
    ] {
        let history = Arc::new(History::new(attempt.clone(), units.clone()));
        let config = RuntimeConfig::default();
        let engine = config.engine().unwrap();
        let mut data = StoreData::with_vfs("test-run".into(), Vfs::none());
        if mode != Attempt::Direct {
            data.effects = Some(history.clone());
        }
        let mut store = config.store_async(&engine, data).unwrap();
        let mut linker = wasmtime::Linker::new(&engine);
        install_runtime_async(&mut linker, &mut store)
            .await
            .unwrap();
        let action_calls = calls.clone();
        let probe = wasmtime::Func::new_async(
            &mut store,
            wasmtime::FuncType::new(&engine, [], []),
            move |mut caller, _, _| {
                let action_calls = action_calls.clone();
                Box::new(async move {
                    if mode == Attempt::ReplayEmpty {
                        // Enough for the one-byte () fingerprint, but not a guest error.
                        let pending = caller.data().host_fuel_pending;
                        caller.set_fuel(pending + fuel::HASH.cost(1))?;
                    }
                    effects::execute(
                        &mut caller,
                        "test.error_conversion",
                        (),
                        move |caller, _, ()| {
                            Box::pin(async move {
                                action_calls.fetch_add(1, Ordering::SeqCst);
                                caller.set_fuel(0)?;
                                Err::<(), _>(wasmtime::Error::msg("saved native failure"))
                            })
                        },
                    )
                    .await
                })
            },
        );
        store.set_fuel(1_000_000).unwrap();
        let error = probe
            .call_async(&mut store, &[], &mut [])
            .await
            .unwrap_err();
        if mode == Attempt::ReplayFunded {
            assert!(error.is::<wasmtime::ThrownException>(), "{error:#}");
        } else {
            assert_eq!(
                error.downcast_ref::<wasmtime::Trap>(),
                Some(&wasmtime::Trap::OutOfFuel),
                "{mode:?}: {error:#}"
            );
        }
        if mode != Attempt::Direct {
            // This also proves the low-fuel replay consumed its history entry.
            history.ensure_history_consumed().await.unwrap();
            let mut unit = units().await.unwrap();
            let entry = unit
                .get_history_entry(&attempt.run_id, 0)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                entry.outcome,
                Err(effects::NativeError::Error("saved native failure".into()))
            );
        }
    }
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "one direct call and one recorded call; neither replay dispatches"
    );
}

#[tokio::test]
async fn code_updates_resume_after_receipt_before_history_without_recomputing() {
    for (operation, expected) in [
        ("edit('/file', 'original', 'changed')", "changed\n"),
        ("insertAt('/file', 1, 'head\\n')", "head\noriginal\n"),
        (
            "applyPatch('/file', '--- a\\n+++ b\\n@@ -1,1 +1,1 @@\\n-original\\n+changed\\n')",
            "changed\n",
        ),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("files");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("file"), "original\n").unwrap();
        let (_database, units, attempt) = fixture(&directory.path().join("db")).await;
        let source = format!(
            r#"
            import {{ edit, insertAt, applyPatch }} from "submilli:code";
            function main(): string {{
                const result = {operation};
                assert(result.success && result.changed);
                assert(result.diff.includes("original"));
                return JSON.stringify(result) ?? "";
            }}
        "#
        );
        let interrupted = Arc::new(InterruptHistory {
            history: History::new(attempt.clone(), units.clone()),
            before_commit: 0,
        });
        let error = run_filesystem_script(&source, interrupted, &root, units.clone(), &attempt)
            .await
            .unwrap_err();
        assert!(
            error.is::<interpreter::runtime::host::FatalHostError>(),
            "{error:#}"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("file")).unwrap(),
            expected
        );
        let replay = Arc::new(History::new(attempt.clone(), units.clone()));
        let output = run_filesystem_script(&source, replay, &root, units.clone(), &attempt)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("file")).unwrap(),
            expected
        );
        std::fs::remove_file(root.join("file")).unwrap();
        let replay = Arc::new(History::new(attempt.clone(), units.clone()));
        assert_eq!(
            run_filesystem_script(&source, replay, &root, units, &attempt)
                .await
                .unwrap()
                .unwrap(),
            output
        );
        assert!(!root.join("file").exists());
    }
}

#[tokio::test]
async fn recorded_code_updates_require_a_persistent_filesystem() {
    for expression in [
        "edit('/file','original','after')",
        "insertAt('/file',1,'head\\n')",
        "applyPatch('/file','')",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("files");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("file"), "original\n").unwrap();
        let (_database, units, attempt) = fixture(&directory.path().join("db")).await;
        let history = Arc::new(History::new(attempt, units));
        let source = format!(
            "import {{ edit, insertAt, applyPatch }} from 'submilli:code'; function main(): void {{ {expression}; }}"
        );
        let error = try_run_script_with(&source, history, &root, |_| {})
            .await
            .unwrap_err();
        assert!(error.is::<interpreter::runtime::host::FatalHostError>());
        assert!(format!("{error:#}").contains("require a persistent filesystem"));
        assert_eq!(
            std::fs::read_to_string(root.join("file")).unwrap(),
            "original\n"
        );
    }
}
