use super::*;
use interpreter::runtime::effects::EffectHistory;
use interpreter::runtime::filesystem::{DownloadOptions, FilesystemError as PortError};
use interpreter::stdlib::http::transport::DownloadMeta;
use interpreter::stdlib::http::{HttpClient, HttpError, HttpRequest, HttpResponse};
use std::io::Write;
use std::path::Path;
use submilli_durable::history::{
    ExecutionAttempt, History, HistoryEntry, HistoryProgress, HistoryStore,
};
use submilli_durable::unit_of_work::StoreError;

fn options(overwrite: bool) -> DownloadOptions {
    DownloadOptions {
        overwrite,
        maximum_bytes: 1024,
        request: [1; 32],
    }
}

#[tokio::test]
async fn checked_download_retries_without_rechecking_existence() {
    for overwrite in [false, true] {
        let fixture = Fixture::new().await;
        let effect = uuid::Uuid::new_v4().to_string().into();
        let path = fixture.filesystem.vfs().root().join("file");
        for response in [b"first".as_slice(), b"different response"] {
            let mut download = fixture
                .filesystem
                .begin_download(&effect, "/file", options(overwrite), budget())
                .await
                .unwrap();
            download.write_all(response).unwrap();
            download.publish(budget()).await.unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), response);
        }
        let mut unit = (fixture.units())().await.unwrap();
        let admission = unit
            .get_journal_entry(&fixture.run, &effect)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            admission.state.as_deref(),
            Some(br#"{"Checked":{}}"#.as_slice())
        );
        assert_eq!(unit.journal_entry_count(&fixture.run).await.unwrap(), 1);
        drop(unit);
        let other = uuid::Uuid::new_v4().to_string().into();
        assert!(
            fixture
                .filesystem
                .begin_download(&other, "/file", options(false), budget())
                .await
                .is_err()
        );
        assert!(
            fixture
                .filesystem
                .begin_download(&effect, "/different", options(overwrite), budget())
                .await
                .is_err()
        );
        let mut changed = options(overwrite);
        changed.request = [2; 32];
        assert!(
            fixture
                .filesystem
                .begin_download(&effect, "/file", changed, budget())
                .await
                .is_err()
        );
        fixture.assert_no_pending_work().await;
    }
}

#[tokio::test]
async fn download_private_staging_is_disposable_and_publication_enforces_quota() {
    let fixture = Fixture::with_filesystem_limit(Some(3)).await;
    let path = fixture.filesystem.vfs().root().join("file");
    let effect = uuid::Uuid::new_v4().to_string().into();
    let mut download = fixture
        .filesystem
        .begin_download(&effect, "/file", options(false), budget())
        .await
        .unwrap();
    download.write_all(b"over quota").unwrap();
    assert!(!path.exists());
    assert!(matches!(
        download.publish(budget()).await,
        Err(PortError::Quota(_))
    ));
    assert!(!path.exists());
    let mut download = fixture
        .filesystem
        .begin_download(&effect, "/file", options(false), budget())
        .await
        .unwrap();
    download.write_all(b"ok").unwrap();
    download.publish(budget()).await.unwrap();
    let other = uuid::Uuid::new_v4().to_string().into();
    let mut download = fixture
        .filesystem
        .begin_download(&other, "/file", options(true), budget())
        .await
        .unwrap();
    download.write_all(b"partial").unwrap();
    drop(download);
    assert_eq!(std::fs::read(&path).unwrap(), b"ok");
    let private = path
        .parent()
        .unwrap()
        .join(".git-submilli-recovery")
        .join(fixture.run.as_str());
    assert_eq!(std::fs::read_dir(private).unwrap().count(), 0);
}

pub(super) async fn seed_run(database: &ServerDatabase, run: &RunId) -> ExecutionAttempt {
    let id = run.to_string();
    database.transaction(move |connection| Box::pin(async move {
        sqlx::query("INSERT OR IGNORE INTO runs(run_id,reservation_id,session_id,fingerprint,owner_generation,attempt,admission_order,created_at_unix_ms,state,recovery_mode) VALUES(?,?,'session','original','owner',1,1,0,'running','recorded')")
            .bind(&id).bind(&id).execute(connection).await?;
        Ok(())
    })).await.unwrap();
    ExecutionAttempt {
        run_id: run.clone(),
        owner_generation: "owner".into(),
        attempt: 1,
    }
}

#[derive(Clone, Copy)]
pub(super) enum HistoryFault {
    None,
    BeforeCommit,
    LostAcknowledgement,
}
struct DownloadHistory {
    units: submilli_durable::UnitFactory,
    fault: HistoryFault,
}
#[async_trait::async_trait]
impl HistoryStore for DownloadHistory {
    async fn progress(&self, attempt: &ExecutionAttempt) -> Result<HistoryProgress, StoreError> {
        (self.units)().await?.history_progress(attempt).await
    }
    async fn get(&self, run: &RunId, sequence: u64) -> Result<Option<HistoryEntry>, StoreError> {
        (self.units)().await?.get_history_entry(run, sequence).await
    }
    async fn append(
        &self,
        attempt: &ExecutionAttempt,
        entry: Arc<HistoryEntry>,
    ) -> Result<(), StoreError> {
        super::process::test_checkpoint("download-before-history");
        if matches!(self.fault, HistoryFault::BeforeCommit) {
            return Err(StoreError::Busy);
        }
        let mut unit = (self.units)().await?;
        unit.append_history(attempt, entry).await?;
        unit.commit().await?;
        super::process::test_checkpoint("download-after-history");
        if matches!(self.fault, HistoryFault::LostAcknowledgement) {
            return Err(StoreError::Busy);
        }
        Ok(())
    }
}

struct Responses(std::path::PathBuf);
#[async_trait::async_trait]
impl HttpClient for Responses {
    async fn send(&self, _: &HttpRequest) -> Result<HttpResponse, HttpError> {
        panic!("must stream")
    }
    async fn download(
        &self,
        req: &HttpRequest,
        writer: &mut (dyn Write + Send),
    ) -> Result<DownloadMeta, HttpError> {
        let counter = self.0.join("requests");
        let count: u64 = std::fs::read_to_string(&counter)
            .unwrap_or_default()
            .parse()
            .unwrap_or(0);
        std::fs::write(counter, (count + 1).to_string()).unwrap();
        let body = std::fs::read(self.0.join("response")).unwrap();
        writer
            .write_all(&body)
            .map_err(|error| HttpError::Other(error.to_string()))?;
        Ok(DownloadMeta {
            status: 200,
            final_url: req.url.clone(),
            status_text: "OK".into(),
            headers: vec![("content-type".into(), "text/plain".into())],
            bytes_written: body.len() as u64,
        })
    }
}

pub(super) async fn execute(
    filesystem: DurableFilesystem,
    units: submilli_durable::UnitFactory,
    attempt: ExecutionAttempt,
    root: &Path,
    overwrite: bool,
    fault: HistoryFault,
) -> wasmtime::Result<Option<String>> {
    use interpreter::runtime::{
        RuntimeConfig, StoreData, dispatch_main_async, install_runtime_async,
        instantiate_program_async,
    };
    let source = format!(
        r#"import {{download}} from "submilli:http";
        function main(): string {{ return JSON.stringify(download("https://example.test/file", "/file", {{overwrite: {overwrite}}}))!; }}"#
    );
    let compiled =
        interpreter::compile_script(&source, "download.ts", interpreter::FileId(0), &[], &[])
            .unwrap();
    let config = RuntimeConfig::default();
    let engine = config.engine().unwrap();
    let history = Arc::new(History::with_store(
        attempt,
        Arc::new(DownloadHistory { units, fault }),
    ));
    let mut data = StoreData::with_vfs(filesystem.run_id().clone(), filesystem.vfs().clone());
    data.filesystem = Arc::new(filesystem);
    data.effects = Some(history.clone());
    data.http_client = Arc::new(Responses(root.to_owned()));
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
    history.drain().await.unwrap();
    if result.is_ok() {
        history.ensure_history_consumed().await.unwrap();
    }
    result
}

#[tokio::test]
async fn download_history_retries_new_response_and_replays_without_touching_workspace() {
    for overwrite in [false, true] {
        let fixture = Fixture::new().await;
        let attempt = seed_run(&fixture.database, &fixture.run).await;
        std::fs::write(fixture.root.path().join("response"), b"first").unwrap();
        let run = |fault| {
            execute(
                fixture.filesystem.clone(),
                fixture.units(),
                attempt.clone(),
                fixture.root.path(),
                overwrite,
                fault,
            )
        };
        assert!(run(HistoryFault::BeforeCommit).await.is_err());
        std::fs::write(fixture.root.path().join("response"), b"second response").unwrap();
        let result = run(HistoryFault::LostAcknowledgement).await.unwrap();
        let meta: serde_json::Value = serde_json::from_str(result.as_ref().unwrap()).unwrap();
        assert_eq!(meta["bytesWritten"], 15);
        let path = fixture.filesystem.vfs().root().join("file");
        assert_eq!(std::fs::read(&path).unwrap(), b"second response");
        std::fs::write(&path, b"later").unwrap();
        assert_eq!(run(HistoryFault::None).await.unwrap(), result);
        assert_eq!(std::fs::read(&path).unwrap(), b"later");
        std::fs::remove_file(&path).unwrap();
        assert_eq!(run(HistoryFault::None).await.unwrap(), result);
        assert!(!path.exists());
        assert_eq!(
            std::fs::read_to_string(fixture.root.path().join("requests")).unwrap(),
            "2"
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn download_preserves_final_symlink_policy() {
    use std::os::unix::fs::symlink;
    for overwrite in [false, true] {
        let fixture = Fixture::new().await;
        let root = fixture.filesystem.vfs().root();
        std::fs::create_dir(root.join(".git")).unwrap();
        std::fs::write(root.join(".git/config"), b"protected").unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("file"), b"outside").unwrap();
        for target in [
            std::path::PathBuf::from(".git/config"),
            outside.path().join("file"),
        ] {
            symlink(target, root.join("alias")).unwrap();
            let effect = uuid::Uuid::new_v4().to_string().into();
            assert!(
                fixture
                    .filesystem
                    .begin_download(&effect, "/alias", options(overwrite), budget())
                    .await
                    .is_err()
            );
            assert!(
                std::fs::symlink_metadata(root.join("alias"))
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
            std::fs::remove_file(root.join("alias")).unwrap();
        }
        symlink("missing", root.join("alias")).unwrap();
        let effect = uuid::Uuid::new_v4().to_string().into();
        let mut download = fixture
            .filesystem
            .begin_download(&effect, "/alias", options(overwrite), budget())
            .await
            .unwrap();
        download.write_all(b"new").unwrap();
        download.publish(budget()).await.unwrap();
        assert_eq!(std::fs::read(root.join("alias")).unwrap(), b"new");
        assert!(!root.join("missing").exists());
    }
}

fn budget() -> Arc<interpreter::runtime::fuel::WorkBudget> {
    Arc::new(interpreter::runtime::fuel::WorkBudget::new(u64::MAX))
}

#[tokio::test]
async fn download_charges_retained_alias_validation_and_settles_publication() {
    let fixture = Fixture::new().await;
    let resource = uuid::Uuid::new_v4().to_string().into();
    let close = uuid::Uuid::new_v4().to_string().into();
    fixture
        .filesystem
        .operation(None)
        .open_writer(&resource, "/file")
        .await
        .unwrap();
    fixture
        .filesystem
        .operation(None)
        .write_chunk(&resource, &resource, b"old".to_vec())
        .await
        .unwrap();
    fixture
        .filesystem
        .operation(None)
        .close_writer(&resource, &close)
        .await
        .unwrap();
    let effect = uuid::Uuid::new_v4().to_string().into();
    let empty = Arc::new(interpreter::runtime::fuel::WorkBudget::new(0));
    assert!(
        fixture
            .filesystem
            .begin_download(&effect, "/file", options(true), empty.clone())
            .await
            .is_err()
    );
    let admission = budget();
    let mut download = fixture
        .filesystem
        .begin_download(&effect, "/file", options(true), admission.clone())
        .await
        .unwrap();
    assert!(admission.spent() > 0);
    download.write_all(b"replacement").unwrap();
    empty.before_effect();
    download.publish(empty.clone()).await.unwrap();
    assert!(
        empty.spent() > 0,
        "publication scans must settle even at zero fuel"
    );
    assert_eq!(
        std::fs::read(fixture.filesystem.vfs().root().join("file")).unwrap(),
        b"replacement"
    );
}

struct PausedPublication(std::sync::Mutex<Option<tokio::sync::oneshot::Receiver<()>>>);
#[async_trait::async_trait]
impl HttpClient for PausedPublication {
    async fn send(&self, _: &HttpRequest) -> Result<HttpResponse, HttpError> {
        panic!("must stream")
    }
    async fn download(
        &self,
        req: &HttpRequest,
        writer: &mut (dyn Write + Send),
    ) -> Result<DownloadMeta, HttpError> {
        writer.write_all(b"complete").unwrap();
        let wait = self.0.lock().unwrap().take().unwrap();
        let (ready, started) = tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || {
            ready.send(()).unwrap();
            wait.blocking_recv().unwrap();
        });
        started.await.unwrap();
        Ok(DownloadMeta {
            status: 200,
            status_text: "OK".into(),
            headers: vec![],
            final_url: req.url.clone(),
            bytes_written: 8,
        })
    }
}

#[test]
fn download_cancellation_drains_publication_before_releasing_workspace() {
    use interpreter::runtime::{
        RuntimeConfig, StoreData, dispatch_main_async, install_runtime_async,
        instantiate_program_async,
    };
    use std::time::Duration;
    tokio::runtime::Builder::new_current_thread().enable_all().max_blocking_threads(1)
        .build().unwrap().block_on(async {
        for durable in [false, true] {
            let fixture = Fixture::new().await;
            let source = r#"import {download} from "submilli:http"; function main(): void { download("https://example.test/file", "/file"); }"#;
            let compiled = interpreter::compile_script(source, "cancel.ts", interpreter::FileId(0), &[], &[]).unwrap();
            let config = RuntimeConfig::default();
            let engine = config.engine().unwrap();
            let mut data = StoreData::with_vfs(fixture.run.clone(), fixture.filesystem.vfs().clone());
            if durable { data.filesystem = Arc::new(fixture.filesystem.clone()); }
            let (release, wait) = tokio::sync::oneshot::channel();
            data.http_client = Arc::new(PausedPublication(std::sync::Mutex::new(Some(wait))));
            let mut store = config.store(&engine, data).unwrap();
            let module = wasmtime::Module::new(&engine, &compiled.wasm).unwrap();
            let mut linker = wasmtime::Linker::new(&engine);
            install_runtime_async(&mut linker, &mut store).await.unwrap();
            let instance = instantiate_program_async(&linker, &mut store, &module).await.unwrap();
            assert!(tokio::time::timeout(Duration::from_millis(200), dispatch_main_async(&mut store, &instance)).await.is_err());
            assert!(tokio::time::timeout(Duration::from_millis(50), store.data_mut().blocking_work.finish()).await.is_err(), "publication escaped the execution's drain list");
            release.send(()).unwrap();
            store.data_mut().blocking_work.finish().await.unwrap();
            assert_eq!(std::fs::read(fixture.filesystem.vfs().root().join("file")).unwrap(), b"complete");
        }
    });
}
