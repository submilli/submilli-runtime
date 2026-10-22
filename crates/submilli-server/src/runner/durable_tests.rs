use super::durable::{COMPATIBILITY, capture_program, history, publish};
use super::*;
use crate::application::sessions::ports::{LastRunStorage, StagedLastRun};
use crate::application::sessions::runs::{
    PrepareRun, RecoverRuns, RunDisposition, RunResolution, claim_run, reopen_run,
};
use crate::blueprint::{BlueprintStore, SqliteBlueprintStore, StoredBlueprint};
use crate::domain::idempotent_request::{RecordedOutcome, RequestState};
use crate::domain::run::{Run, RunIdentity};
use crate::session_store::{DurableSessionStore, RootVfsType, SessionRecord, SqliteSessionStore};
use std::time::{Duration, SystemTime};
use submilli_durable::history::History;
use submilli_shared::secret_store::SecretCipher;

struct UnusedMcp;
impl interpreter::runtime::McpTransport for UnusedMcp {
    fn call<'a>(
        &'a self,
        _: &'a str,
        _: &'a str,
        _: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = interpreter::runtime::mcp::McpOutcome> + Send + 'a>,
    > {
        Box::pin(async { panic!("unused MCP transport") })
    }
}
fn services() -> HostServices {
    let defaults = StoreData::with_tempdir().unwrap();
    HostServices {
        effects: None,
        filesystem: None,
        session_manager: None,
        audit: None,
        git: Ok(None),
        auth_proxy: defaults.auth_proxy,
        variables: defaults.variables,
        secret_provider: defaults.secret_provider,
        security_check: defaults.security_check,
        http_client: defaults.http_client,
        mcp_transport: Arc::new(UnusedMcp),
        tool_provider: None,
        session_kv: Arc::new(interpreter::runtime::InMemorySessionKv::default()),
        session_finalizers: None,
        resource_use: None,
        cursor_keys: None,
        llm_provider: None,
        llm_budget: None,
        embedding_provider: None,
        embedding_budget: None,
        cancel_requested: None,
    }
}
async fn execute(
    run: &Run,
    history: Arc<History>,
    units: submilli_durable::UnitFactory,
    root: &std::path::Path,
) -> Result<RunOutcome, String> {
    let config = RuntimeConfig::default();
    let engine = config.engine().unwrap();
    let mut linker = Linker::new(&engine);
    interpreter::runtime::install_runtime_host_functions(&mut linker).unwrap();
    let packages = Arc::new(PreparedBlueprintPackages {
        stdlib_declarations: interpreter::runtime::stdlib_package_declarations(),
        ..Default::default()
    });
    let mcps = Arc::new(McpCatalog::empty());
    super::durable::run(
        run,
        history,
        units,
        RunnerRuntime {
            run_id: run.reservation_id(),
            blueprint: "durable",
            session: "session",
            engine: &engine,
            base_linker: &linker,
            config: &config,
        },
        Vfs::external(root.to_owned()).unwrap(),
        StoreData::with_tempdir().unwrap().vfs_info,
        services(),
        RunnerImports {
            packages: &packages,
            mcps: &mcps,
            tools: None,
        },
    )
    .await
}

#[tokio::test]
async fn recorded_run_reopens_original_program_and_publishes_terminal_response() {
    let root = tempfile::tempdir().unwrap();
    let session_root = root.path().join("sessions");
    let files = session_root.join("session");
    std::fs::create_dir_all(&files).unwrap();
    let db_path = root.path().join("server.db");
    let cipher = Arc::new(SecretCipher::from_key([11; 32]));
    let database = Arc::new(
        crate::database::ServerDatabase::open(&db_path)
            .await
            .unwrap(),
    );
    let units = crate::adapters::unit_of_work::for_sessions(
        database.clone(),
        session_root.clone(),
        Some(cipher.clone()),
    );
    let fs_units = crate::adapters::unit_of_work::durable::units(database.clone(), cipher.clone());
    let yaml = "name: durable\ndefault: allow\nvfs: per_session\n";
    SqliteBlueprintStore::new(database.clone(), None)
        .add_yaml(StoredBlueprint::new(
            submilli_blueprint::parse(yaml).unwrap(),
            yaml.into(),
        ))
        .await
        .unwrap();
    let sessions = SqliteSessionStore::new(database.clone(), None, session_root.clone());
    sessions
        .put(SessionRecord {
            session_id: "session".into(),
            blueprint_name: "durable".into(),
            root_vfs_type: RootVfsType::PerSession,
            root_vfs_path: Some(files.clone()),
            idle_timeout: Duration::from_secs(3600),
            last_activity: SystemTime::now(),
            ..Default::default()
        })
        .await
        .unwrap();
    drop(sessions);
    let source = r#"
        import { v4 } from "submilli:uuid";
        import { writeText, readText } from "submilli:fs";
        const value = v4();
        function main(): string {
            writeText("/result", value);
            console.log(value);
            return readText("/result") ?? "missing";
        }
    "#;
    let blueprint =
        submilli_blueprint::parse("name: durable\ndefault: allow\nvfs: per_session\n").unwrap();
    let program = capture_program(
        source.into(),
        blueprint,
        &services(),
        &RuntimeConfig::default(),
    );
    let mut prepared = Run::prepare(
        "session".into(),
        Some("key".into()),
        "source-fingerprint".into(),
        "00000000-0000-4000-8000-000000000010".into(),
        "first".into(),
        RunIdentity {
            execution_id: Some("execution".into()),
            principal: Some("user".into()),
            entry: "http".into(),
            admission_order: 0,
        },
        SystemTime::now(),
    )
    .unwrap();
    prepared.record_program(program.clone()).unwrap();
    assert!(matches!(
        PrepareRun::new(units.as_ref())
            .execute(prepared.clone())
            .await
            .unwrap(),
        RunDisposition::Proceed
    ));
    // The first process died after admission, before claiming or dispatching.
    let first = reopen_run(
        units.as_ref(),
        prepared.reservation_id(),
        "replacement",
        COMPATIBILITY,
    )
    .await
    .unwrap();
    assert_eq!(first.attempt(), 1);
    assert_eq!(first.history_position(), 0);
    assert_eq!(first.optional_key(), Some("key"));
    assert!(
        claim_run(units.as_ref(), prepared.reservation_id(), "first")
            .await
            .is_err()
    );
    let first_history = history(&first, fs_units.clone());
    let original = execute(&first, first_history.clone(), fs_units.clone(), &files)
        .await
        .unwrap();
    assert!(original.error.is_none(), "{:?}", original.error);
    assert_eq!(
        original.console_raw,
        format!("{}\n", original.value.as_deref().unwrap())
    );
    // Lose the process after all native outcomes, but before terminal publication.
    drop(first_history);
    drop(fs_units);
    drop(units);
    database.close().await.unwrap();
    drop(database);
    std::fs::write(files.join("result"), "later filesystem contents").unwrap();

    let database = Arc::new(
        crate::database::ServerDatabase::open(&db_path)
            .await
            .unwrap(),
    );
    let units = crate::adapters::unit_of_work::for_sessions(
        database.clone(),
        session_root,
        Some(cipher.clone()),
    );
    let fs_units = crate::adapters::unit_of_work::durable::units(database.clone(), cipher);
    RecoverRuns::new(units.as_ref())
        .execute("second")
        .await
        .unwrap();
    let resumed = reopen_run(
        units.as_ref(),
        first.reservation_id(),
        "second",
        COMPATIBILITY,
    )
    .await
    .unwrap();
    assert_eq!(resumed.program(), Some(&program));
    // UUID, write and read are recorded; console writes again without a history row.
    assert_eq!(resumed.history_position(), 3);
    assert_eq!(resumed.attempt(), first.attempt() + 1);
    let replay = history(&resumed, fs_units.clone());
    let outcome = execute(&resumed, replay.clone(), fs_units, &files)
        .await
        .unwrap();
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert_eq!(outcome.value, original.value);
    assert_eq!(outcome.console_raw, original.console_raw);
    assert_eq!(
        std::fs::read_to_string(files.join("result")).unwrap(),
        "later filesystem contents"
    );
    let response = RecordedOutcome {
        status: 200,
        body: outcome.value.unwrap(),
    };
    let last = StagedLastRun {
        encoded_bytes: 2,
        storage: LastRunStorage::Inline {
            payload: b"{}".to_vec(),
            key_id: None,
        },
    };
    assert!(
        publish(
            units.clone(),
            &first,
            &replay,
            &RunResolution::Completed(response.clone()),
            Some(&last)
        )
        .await
        .is_err()
    );
    publish(
        units.clone(),
        &resumed,
        &replay,
        &RunResolution::Completed(response.clone()),
        Some(&last),
    )
    .await
    .unwrap();
    let mut unit = units.begin().await.unwrap();
    let completed = unit.get_run(first.reservation_id()).await.unwrap().unwrap();
    assert_eq!(
        completed.state(),
        &RequestState::Completed(response.clone())
    );
    drop(unit);
    let last_id: String = database
        .transaction(|connection| {
            Box::pin(async move {
                Ok(
                    sqlx::query_scalar("SELECT run_id FROM last_runs WHERE session_id='session'")
                        .fetch_one(connection)
                        .await?,
                )
            })
        })
        .await
        .unwrap();
    assert_eq!(last_id, first.reservation_id().as_str());

    assert!(
        matches!(PrepareRun::new(units.as_ref()).execute(prepared).await.unwrap(), RunDisposition::Completed(saved) if saved == response)
    );
    assert!(
        reopen_run(
            units.as_ref(),
            first.reservation_id(),
            "third",
            COMPATIBILITY
        )
        .await
        .is_err()
    );
}
