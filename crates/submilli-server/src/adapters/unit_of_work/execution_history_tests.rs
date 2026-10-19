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
        sqlx::query("INSERT OR IGNORE INTO runs(run_id,reservation_id,session_id,fingerprint,owner_generation,attempt,admission_order,created_at_unix_ms,state,recovery_mode,execution_manifest) VALUES('00000000-0000-4000-8000-000000000001','00000000-0000-4000-8000-000000000001','session','original','owner',1,1,0,'running','recorded',X'01')")
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
    RecordedOutcome {
        result: Ok(interpreter::runtime::effects::OutcomeBytes(bytes.into())),
        host_fuel: 17,
        execution_time: std::time::Duration::ZERO,
        llm_spend: Default::default(),
        embedding_spend: Default::default(),
    }
}

#[tokio::test]
async fn execution_history_reopens_without_dispatching_saved_calls() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.db");
    let (database, units, attempt) = fixture(&path).await;
    let history = History::new(attempt, units.clone());
    let call = history
        .begin("test.native", [3; 32], memory())
        .await
        .unwrap();
    assert!(call.recorded.is_none());
    let id = call.id.clone();
    history
        .complete(&call, result(b"secret-native-output"), memory())
        .await
        .unwrap();
    history.finish().await.unwrap();
    drop(history);
    drop(units);
    database.close().await.unwrap();
    drop(database);
    let (_database, units, attempt) = fixture(&path).await;
    let history = History::new(attempt, units);
    assert!(history.finish().await.is_err());
    let call = history
        .begin("test.native", [3; 32], memory())
        .await
        .unwrap();
    assert_eq!(call.id, id);
    assert!(call.replay_complete);
    assert_eq!(call.recorded.unwrap(), result(b"secret-native-output"));
    history.finish().await.unwrap();
}

#[tokio::test]
async fn execution_history_missing_outcome_reuses_id_and_allows_another_mutation() {
    let directory = tempfile::tempdir().unwrap();
    let (_database, units, attempt) = fixture(&directory.path().join("db")).await;
    let history = History::new(attempt.clone(), units.clone());
    let unfinished = history.begin("increment", [0; 32], memory()).await.unwrap();
    let mut mutations = 1;
    assert!(history.begin("later", [0; 32], memory()).await.is_err());
    drop(history); // The action completed, but its outcome never committed.
    let history = History::new(attempt, units);
    let retry = history.begin("increment", [0; 32], memory()).await.unwrap();
    assert_eq!(retry.id, unfinished.id);
    assert!(retry.recorded.is_none());
    mutations += 1;
    history
        .complete(&retry, result(&[mutations]), memory())
        .await
        .unwrap();
    assert_eq!(mutations, 2); // Framework retry makes no idempotency promise.
}

#[tokio::test]
async fn execution_history_rejects_divergence_and_stale_completion() {
    let directory = tempfile::tempdir().unwrap();
    let (database, units, attempt) = fixture(&directory.path().join("db")).await;
    let history = History::new(attempt.clone(), units.clone());
    let call = history.begin("original", [1; 32], memory()).await.unwrap();
    history
        .complete(&call, result(b"one"), memory())
        .await
        .unwrap();
    assert!(
        History::new(attempt.clone(), units.clone())
            .begin("different", [1; 32], memory())
            .await
            .is_err()
    );
    assert!(
        History::new(attempt.clone(), units.clone())
            .begin("original", [2; 32], memory())
            .await
            .is_err()
    );
    let pending = history.begin("next", [0; 32], memory()).await.unwrap();
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
    assert!(
        history
            .complete(&pending, result(b"late"), memory())
            .await
            .is_err()
    );
    let mut unit = units().await.unwrap();
    assert!(
        unit.get_history_entry(&attempt.run_id, 1, memory())
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
    unit.append_history(&attempt, Arc::new(entry.clone()), memory())
        .await
        .unwrap();
    assert_eq!(unit.history_progress(&attempt).await.unwrap().position, 1);
    drop(unit);
    let mut unit = units().await.unwrap();
    assert_eq!(unit.history_progress(&attempt).await.unwrap().position, 0);
    assert!(
        unit.get_history_entry(&attempt.run_id, 0, memory())
            .await
            .unwrap()
            .is_none()
    );
    unit.append_history(&attempt, Arc::new(entry.clone()), memory())
        .await
        .unwrap();
    assert_eq!(unit.history_progress(&attempt).await.unwrap().host_fuel, 17);
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
        unit.append_history(&attempt, Arc::new(entry.clone()), memory())
            .await
            .is_err()
    );
}

async fn run_script(
    source: &str,
    history: Arc<History>,
    root: &std::path::Path,
) -> (Option<String>, u64) {
    run_script_with(source, history, root, |_| {}).await
}

async fn run_script_with(
    source: &str,
    history: Arc<History>,
    root: &std::path::Path,
    configure: impl FnOnce(&mut interpreter::runtime::StoreData),
) -> (Option<String>, u64) {
    use interpreter::runtime::{
        RuntimeConfig, StoreData, Vfs, dispatch_main_async, install_runtime_async,
        instantiate_program_async,
    };
    let compiled =
        interpreter::compile_script(source, "replay.ts", interpreter::FileId(0), &[], &[]).unwrap();
    let config = RuntimeConfig::default();
    let engine = config.engine().unwrap();
    let mut data = StoreData::with_vfs(Vfs::external(root.to_path_buf()).unwrap());
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
    let result = dispatch_main_async(&mut store, &instance).await.unwrap();
    history.finish().await.unwrap();
    store.data_mut().blocking_work.finish().await.unwrap();
    (result, store.data().host_fuel)
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
    assert_eq!(
        first, replay,
        "results and logical native fuel survive a fresh interpreter"
    );
    assert_eq!(
        std::fs::read(root.join("file")).unwrap(),
        b"unchanged during replay"
    );
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
        memory: interpreter::runtime::limits::HostMemory,
    ) -> Result<
        interpreter::runtime::effects::EffectCall,
        interpreter::runtime::effects::HistoryError,
    > {
        self.history.begin(operation, fingerprint, memory).await
    }
    async fn complete(
        &self,
        call: &interpreter::runtime::effects::EffectCall,
        outcome: RecordedOutcome,
        memory: interpreter::runtime::limits::HostMemory,
    ) -> Result<(), interpreter::runtime::effects::HistoryError> {
        if call.sequence == self.before_commit {
            return Err(interpreter::runtime::effects::HistoryError(
                "injected loss before outcome commit".into(),
            ));
        }
        self.history.complete(call, outcome, memory).await
    }
    async fn finish(&self) -> Result<(), interpreter::runtime::effects::HistoryError> {
        self.history.finish().await
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
    let mut data = StoreData::with_vfs(vfs);
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
    history.finish().await?;
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

fn memory() -> interpreter::runtime::limits::HostMemory {
    interpreter::runtime::limits::TenantLimits::new(256 * 1024 * 1024).host_memory()
}

#[tokio::test]
async fn execution_history_admits_small_rows_without_a_fixed_large_surcharge() {
    let directory = tempfile::tempdir().unwrap();
    let (_database, units, attempt) = fixture(&directory.path().join("db")).await;
    let limits = interpreter::runtime::limits::TenantLimits::new(16 * 1024);
    let original = History::new(attempt.clone(), units.clone());
    let call = original
        .begin("small", [0; 32], limits.host_memory())
        .await
        .unwrap();
    original
        .complete(&call, result(b"small"), limits.host_memory())
        .await
        .unwrap();
    assert_eq!(limits.host_attached_bytes(), 0);
    let replay = History::new(attempt, units);
    let call = replay
        .begin("small", [0; 32], limits.host_memory())
        .await
        .unwrap();
    assert!(call.recorded.is_some());
    assert!(limits.host_attached_bytes() > 0);
    drop(call);
    assert_eq!(limits.host_attached_bytes(), 0);
}

#[tokio::test]
async fn execution_history_refuses_a_blob_before_loading_it_without_memory() {
    let directory = tempfile::tempdir().unwrap();
    let (_database, units, attempt) = fixture(&directory.path().join("db")).await;
    let original = History::new(attempt.clone(), units.clone());
    let call = original.begin("bytes", [0; 32], memory()).await.unwrap();
    original
        .complete(&call, result(&vec![42; 65536]), memory())
        .await
        .unwrap();
    let limits = interpreter::runtime::limits::TenantLimits::new(1024);
    let replay = History::new(attempt, units);
    assert!(
        replay
            .begin("bytes", [0; 32], limits.host_memory())
            .await
            .is_err()
    );
    assert_eq!(limits.host_attached_bytes(), 0);
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
    let original = run_script_with(
        source,
        Arc::new(History::new(attempt.clone(), units.clone())),
        &root,
        |data| {
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
