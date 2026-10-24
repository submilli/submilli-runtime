use super::*;
use crate::config::ServerConfig;
use crate::domain::run_status::IndeterminateReason;
use crate::handlers::durable_runs;
use submilli_shared::secret_store::SecretCipher;

const CODE: &str =
    r#"import { v4 } from "submilli:uuid"; function main(): string { return v4(); }"#;

async fn app(root: &std::path::Path, fuel: u64) -> AppState {
    configured_app(
        root,
        interpreter::runtime::RuntimeConfig {
            fuel,
            ..Default::default()
        },
    )
    .await
}

async fn configured_app(
    root: &std::path::Path,
    runtime: interpreter::runtime::RuntimeConfig,
) -> AppState {
    AppState::new(ServerConfig {
        durable_execution: true,
        database_path: Some(root.join("server.db")),
        volume_root: Some(root.to_owned()),
        package_store_root: Some(root.join("packages")),
        session_cipher: Some(Arc::new(SecretCipher::from_key([19; 32]))),
        runtime,
        ..Default::default()
    })
    .await
    .unwrap()
}

async fn accept(state: &AppState, session: &str, code: &str, key: Option<&str>) -> RunId {
    let blueprint =
        Arc::new(submilli_blueprint::parse("name: durable\ndefault: allow\nvfs: none\n").unwrap());
    accept_with_blueprint(state, session, code, key, blueprint).await
}

async fn accept_with_blueprint(
    state: &AppState,
    session: &str,
    code: &str,
    key: Option<&str>,
    blueprint: Arc<submilli_blueprint::Blueprint>,
) -> RunId {
    state
        .blueprints()
        .upsert((*blueprint).clone())
        .await
        .unwrap();
    state
        .session_manager()
        .ensure(session, &blueprint)
        .await
        .unwrap();
    admit(
        state,
        ExecuteInputs {
            session_id: session,
            code,
            blueprint_name: "durable",
            blueprint,
            variables: Arc::default(),
            harness_secrets: Arc::default(),
            audit: None,
            vfs_source: execute::VfsSource::Rest,
            run_entry: "session",
            idempotency_key: key,
            tools: None,
        },
    )
    .await
    .unwrap()
}

async fn until(state: &AppState, id: &RunId, status: &str) -> serde_json::Value {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let current = durable_runs::observe(state, id).await.unwrap().unwrap();
            if current["status"] == status {
                return current;
            }
            if current["status"] == "stuck" && status != "stuck" {
                panic!("unexpected stuck run: {current}");
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap()
}

async fn stop(state: &AppState) {
    state.graceful_shutdown().close();
    state.graceful_shutdown().wait().await;
}

#[tokio::test]
async fn admission_survives_restart_and_unkeyed_result_is_retained() {
    let root = tempfile::tempdir().unwrap();
    let first = app(root.path(), 10_000_000).await;
    // Admission without boot models a crash after its commit and before launch.
    let id = accept(&first, "session", CODE, None).await;
    assert_eq!(
        durable_runs::observe(&first, &id).await.unwrap().unwrap()["status"],
        "queued"
    );
    first.database().unwrap().close().await.unwrap();
    drop(first);
    let second = app(root.path(), 10_000_000).await;
    second.boot().await.unwrap();
    let result = until(&second, &id, "completed").await;
    assert!(result["response"]["error"].is_null(), "{result}");
    let value = result["response"]["result"].as_str().unwrap();
    assert!(uuid::Uuid::parse_str(value).is_ok(), "{value}");
    assert_eq!(result["attempt"], 1);
    stop(&second).await;
}

#[tokio::test]
async fn stuck_requires_explicit_resume_and_does_not_block_another_session() {
    let root = tempfile::tempdir().unwrap();
    let first = app(root.path(), 100_000).await;
    let id = accept(
        &first,
        "stuck",
        "function main(): void { while (true) {} }",
        Some("key"),
    )
    .await;
    let behind = accept(&first, "stuck", CODE, None).await;
    let other = accept(&first, "other", CODE, None).await;
    first.boot().await.unwrap();
    let stuck = until(&first, &id, "stuck").await;
    assert_eq!(stuck["reason"], "fuel_exhausted");
    until(&first, &other, "completed").await;
    assert_eq!(
        durable_runs::observe(&first, &behind)
            .await
            .unwrap()
            .unwrap()["status"],
        "queued"
    );
    let duplicate = accept(
        &first,
        "stuck",
        "function main(): void { while (true) {} }",
        Some("key"),
    )
    .await;
    assert_eq!(duplicate, id);
    stop(&first).await;
    first.database().unwrap().close().await.unwrap();
    drop(first);
    let second = app(root.path(), 9_000_000).await;
    second.boot().await.unwrap();
    tokio::time::sleep(Duration::from_millis(600)).await;
    let unchanged = durable_runs::observe(&second, &id).await.unwrap().unwrap();
    assert_eq!(unchanged["attempt"], 1);
    assert_eq!(unchanged["revision"], stuck["revision"]);
    let revision = unchanged["revision"].as_i64().unwrap();
    assert!(
        recovery::resume(second.units().as_ref(), &id, revision - 1)
            .await
            .is_err()
    );
    recovery::resume(second.units().as_ref(), &id, revision)
        .await
        .unwrap();
    second.wake_recovery();
    let resumed = until(&second, &id, "stuck").await;
    assert_eq!(resumed["attempt"], 2);
    assert_eq!(resumed["reason"], "fuel_exhausted");
    stop(&second).await;
}

#[tokio::test]
async fn claim_is_one_launch_per_startup_and_corrupt_manifest_stays_observable() {
    let root = tempfile::tempdir().unwrap();
    let state = app(root.path(), 1_000_000).await;
    let id = accept(&state, "s", CODE, None).await;
    let run = recovery::claim(state.units().as_ref(), &id, "first")
        .await
        .unwrap()
        .unwrap();
    assert!(
        recovery::claim(state.units().as_ref(), &id, "first")
            .await
            .unwrap()
            .is_none()
    );
    let next = recovery::claim(state.units().as_ref(), &id, "second")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(next.attempt(), run.attempt() + 1);
    let database = state.database().unwrap();
    let corrupt_id = id.to_string();
    database
        .transaction(move |connection| {
            Box::pin(async move {
                sqlx::query("UPDATE run_programs SET content=x'00' WHERE run_id=?")
                    .bind(corrupt_id)
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .await
        .unwrap();
    state.boot().await.unwrap();
    let stuck = until(&state, &id, "stuck").await;
    assert_eq!(stuck["reason"], "invalid_history");
    // Closure must fence a damaged run without decoding its program.
    state.session_manager().close_owned("s").await.unwrap();
    assert_eq!(
        durable_runs::observe(&state, &id).await.unwrap().unwrap()["status"],
        "cancelled"
    );
    stop(&state).await;
}

mod lifecycle;
pub(super) mod process;

#[tokio::test]
async fn recorded_runs_cannot_become_indeterminate() {
    let root = tempfile::tempdir().unwrap();
    let state = app(root.path(), 1_000_000).await;
    let id = accept(&state, "s", CODE, None).await;
    let mut run = recovery::claim(state.units().as_ref(), &id, "first")
        .await
        .unwrap()
        .unwrap();
    let before = run.clone();
    assert!(matches!(
        run.mark_indeterminate(&id),
        Err(crate::domain::run::RequestError::InvalidTransition)
    ));
    assert_eq!(run, before);
    assert!(
        crate::domain::run::Run::restore(
            run.session_id().into(),
            run.optional_key().map(str::to_owned),
            run.fingerprint().into(),
            id,
            run.owner_generation().into(),
            run.created_at(),
            crate::domain::idempotent_request::RunStatus::Indeterminate(
                IndeterminateReason::Interrupted
            ),
            None,
            run.identity().clone(),
            run.revision(),
            run.attempt(),
            run.history_position(),
            run.is_durable(),
        )
        .is_err()
    );
}

#[tokio::test]
async fn retained_work_protects_expired_sessions_and_resume_survives_lost_ack() {
    use crate::idempotency::fault_tests::{CommitFault, FaultFactory};
    use std::sync::atomic::Ordering;
    let root = tempfile::tempdir().unwrap();
    let state = app(root.path(), 100_000).await;
    let id = accept(&state, "s", CODE, None).await;
    state.database().unwrap().transaction(|connection| Box::pin(async move {
        sqlx::query("UPDATE sessions SET last_activity_unix_ms=0,idle_timeout_ms=1 WHERE session_id='s'").execute(connection).await?;
        Ok(())
    })).await.unwrap();
    assert!(state.session_manager().get("s").await.is_ok());
    let blueprint =
        submilli_blueprint::parse("name: durable\ndefault: allow\nvfs: none\n").unwrap();
    state
        .session_manager()
        .bind("s", &blueprint, Arc::default(), Arc::default())
        .await
        .unwrap();
    let fault = Arc::new(CommitFault::default());
    let units = FaultFactory {
        inner: state.units(),
        fault: fault.clone(),
    };
    fault.lose_ack.store(true, Ordering::SeqCst);
    let run = recovery::claim(&units, &id, "startup")
        .await
        .unwrap()
        .unwrap();
    assert!(
        recovery::claim(&units, &id, "startup")
            .await
            .unwrap()
            .is_none()
    );
    recovery::stop_attempt(&units, &run, StuckReason::Fuel)
        .await
        .unwrap();
    let mut unit = state.units().begin().await.unwrap();
    assert!(
        unit.retire_run_resources(&id).await.is_err(),
        "stuck resources must remain"
    );
    drop(unit);
    let revision = durable_runs::observe(&state, &id).await.unwrap().unwrap()["revision"]
        .as_i64()
        .unwrap();
    fault.lose_ack.store(true, Ordering::SeqCst);
    recovery::resume(&units, &id, revision).await.unwrap();
    assert!(
        recovery::resume(&units, &id, revision).await.is_err(),
        "a duplicate resume cannot grant a second attempt"
    );
    state.boot().await.unwrap();
    until(&state, &id, "completed").await;
    stop(&state).await;
}

#[tokio::test]
async fn http_session_execution_and_paginated_run_observation() {
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let root = tempfile::tempdir().unwrap();
    let state = app(root.path(), 1_000_000).await;
    let initial = accept(&state, "s", CODE, None).await;
    let router = crate::app::app(state.clone());
    let request = || {
        Request::post("/v1/sessions/s/execute")
            .header("Content-Type", "application/json")
            .header("Idempotency-Key", "http-key")
            .body(Body::from(serde_json::json!({"code": CODE}).to_string()))
            .unwrap()
    };
    let response = router.clone().oneshot(request()).await.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["mcp-session-id"], "s");
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let first: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(first["status"], "finished", "{first}");
    assert!(first["error"].is_null(), "{first}");
    state.blueprints().remove("durable").await.unwrap();
    let second = router
        .clone()
        .oneshot(request())
        .await
        .unwrap()
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    assert_eq!(body, second);
    let invalid = Request::post("/v1/sessions/s/execute")
        .header("Content-Type", "application/json")
        .header("Idempotency-Key", "")
        .body(Body::from(serde_json::json!({"code": CODE}).to_string()))
        .unwrap();
    assert_eq!(router.clone().oneshot(invalid).await.unwrap().status(), 400);
    let mut unit = state.units().begin().await.unwrap();
    unit.expire_responses("s", vec![first["execution_id"].as_str().unwrap().into()])
        .await
        .unwrap();
    unit.commit().await.unwrap();
    assert_eq!(
        router.clone().oneshot(request()).await.unwrap().status(),
        409
    );

    let page = router
        .oneshot(
            Request::get("/v1/runs?session=s&limit=1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let page: serde_json::Value =
        serde_json::from_slice(&page.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(page["runs"].as_array().unwrap().len(), 1);
    assert!(page["next"].is_string());
    until(&state, &initial, "completed").await;
    stop(&state).await;
}

#[tokio::test]
async fn restart_keeps_timeout_when_current_default_has_no_deadline() {
    let root = tempfile::tempdir().unwrap();
    let runtime = interpreter::runtime::RuntimeConfig {
        fuel: u64::MAX,
        timeout: Some(Duration::from_millis(20)),
        ..Default::default()
    };
    let first = configured_app(root.path(), runtime).await;
    let id = accept(
        &first,
        "s",
        "function main(): void { while (true) {} }",
        None,
    )
    .await;
    first.database().unwrap().close().await.unwrap();
    drop(first);
    let second = configured_app(
        root.path(),
        interpreter::runtime::RuntimeConfig {
            fuel: u64::MAX,
            timeout: None,
            ..Default::default()
        },
    )
    .await;
    second.boot().await.unwrap();
    assert_eq!(
        until(&second, &id, "stuck").await["reason"],
        "execution_timeout"
    );
    stop(&second).await;
}

#[tokio::test]
async fn changed_engine_limits_refuse_replay() {
    let root = tempfile::tempdir().unwrap();
    let first = app(root.path(), 1_000_000).await;
    let id = accept(&first, "s", CODE, None).await;
    let mut runtime = first.runtime().clone();
    runtime.max_wasm_stack *= 2;
    first.database().unwrap().close().await.unwrap();
    drop(first);
    let second = configured_app(root.path(), runtime).await;
    second.boot().await.unwrap();
    assert_eq!(
        until(&second, &id, "stuck").await["reason"],
        "incompatible_environment"
    );
    stop(&second).await;
}

async fn until_retired(state: &AppState, id: &RunId) {
    let id = id.to_string();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let run_id = id.clone();
            let retired: bool = state
                .database()
                .unwrap()
                .read(move |connection| {
                    Box::pin(async move {
                        Ok(
                            sqlx::query_scalar("SELECT resources_retired FROM runs WHERE run_id=?")
                                .bind(run_id)
                                .fetch_one(connection)
                                .await?,
                        )
                    })
                })
                .await
                .unwrap();
            if retired {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn retirement_does_not_recreate_a_deleted_working_directory() {
    let root = tempfile::tempdir().unwrap();
    let state = app(root.path(), 1_000_000).await;
    let blueprint = Arc::new(
        submilli_blueprint::parse(
            "name: durable\ndefault: allow\nvfs:\n  mode: per_session\n  cwd: /work\n",
        )
        .unwrap(),
    );
    let source = r#"import { remove } from "submilli:fs"; function main(): void { remove("/work", false); }"#;
    let id = accept_with_blueprint(&state, "s", source, None, blueprint).await;
    state.boot().await.unwrap();
    let result = until(&state, &id, "completed").await;
    assert!(result["response"]["error"].is_null(), "{result}");
    until_retired(&state, &id).await;
    assert!(!root.path().join("sessions/s/work").exists());
    stop(&state).await;
}

#[tokio::test]
async fn program_and_admission_commit_together() {
    let root = tempfile::tempdir().unwrap();
    let state = app(root.path(), 1_000_000).await;
    let template = accept(&state, "s", CODE, None).await;
    let program = state
        .units()
        .begin()
        .await
        .unwrap()
        .get_run_program(&template)
        .await
        .unwrap()
        .unwrap();
    let mut run = crate::domain::run::Run::prepare(
        "s".into(),
        Some("atomic".into()),
        "fingerprint".into(),
        "new-run".into(),
        "owner".into(),
        crate::domain::run::RunIdentity::default(),
        SystemTime::now(),
    )
    .unwrap();
    run.enable_durability().unwrap();
    let database = state.database().unwrap();
    database.transaction(|connection| Box::pin(async move {
        sqlx::query("CREATE TRIGGER reject_program BEFORE INSERT ON run_programs BEGIN SELECT RAISE(FAIL,'injected'); END").execute(connection).await?;
        Ok(())
    })).await.unwrap();
    assert!(
        recovery::admit(state.units().as_ref(), run.clone(), program.clone())
            .await
            .is_err()
    );
    let mut unit = state.units().begin().await.unwrap();
    assert!(unit.get_run("new-run").await.unwrap().is_none());
    assert!(unit.run_for_key("s", "atomic").await.unwrap().is_none());
    drop(unit);
    database
        .transaction(|connection| {
            Box::pin(async move {
                sqlx::query("DROP TRIGGER reject_program")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .await
        .unwrap();
    recovery::admit(state.units().as_ref(), run, program.clone())
        .await
        .unwrap();
    let mut unit = state.units().begin().await.unwrap();
    assert!(unit.get_run("new-run").await.unwrap().unwrap().is_durable());
    assert_eq!(
        unit.get_run_program("new-run").await.unwrap(),
        Some(program)
    );
}

#[tokio::test]
async fn completion_failure_rolls_back_result_and_expiry_keeps_run_identity() {
    use crate::application::sessions::runs::ResolveRun;
    use crate::domain::idempotent_request::RecordedOutcome;
    let root = tempfile::tempdir().unwrap();
    let state = app(root.path(), 1_000_000).await;
    let id = accept(&state, "s", CODE, Some("atomic")).await;
    let run = recovery::claim(state.units().as_ref(), &id, "owner")
        .await
        .unwrap()
        .unwrap();
    let database = state.database().unwrap();
    database.transaction(|connection| Box::pin(async move {
        sqlx::query("CREATE TRIGGER reject_completion BEFORE UPDATE ON runs WHEN NEW.state='completed' BEGIN SELECT RAISE(FAIL,'injected'); END").execute(connection).await?;
        Ok(())
    })).await.unwrap();
    let resolution = RunResolution::Completed(RecordedOutcome {
        status: 200,
        body: "exact result\n".into(),
    });
    let resolve = ResolveRun::new(state.units());
    assert!(
        resolve
            .execute_recorded(&run, &resolution, None, 0)
            .await
            .is_err()
    );
    let mut unit = state.units().begin().await.unwrap();
    assert_eq!(
        unit.get_run(&id).await.unwrap().unwrap().status(),
        RunStatus::Running
    );
    assert!(unit.run_result(&id).await.unwrap().is_none());
    drop(unit);
    database
        .transaction(|connection| {
            Box::pin(async move {
                sqlx::query("DROP TRIGGER reject_completion")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .await
        .unwrap();
    resolve
        .execute_recorded(&run, &resolution, None, 0)
        .await
        .unwrap();
    let mut unit = state.units().begin().await.unwrap();
    assert_eq!(
        unit.run_result(&id).await.unwrap().unwrap().body,
        "exact result\n"
    );
    assert_eq!(
        unit.expire_responses("s", vec![id.to_string()])
            .await
            .unwrap(),
        1
    );
    assert!(unit.run_result(&id).await.unwrap().is_none());
    let retained = unit.run_for_key("s", "atomic").await.unwrap().unwrap();
    assert_eq!(retained.reservation_id(), &id);
    assert_eq!(retained.status(), RunStatus::Expired);
    assert!(unit.get_run_program(&id).await.unwrap().is_some());
    unit.commit().await.unwrap();
    assert!(
        recovery::claim(state.units().as_ref(), &id, "another-owner")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn stuck_filter_includes_every_reason_and_resume_clears_it() {
    use crate::application::sessions::recovery::{RunQuery, RunStatusFilter};
    let root = tempfile::tempdir().unwrap();
    let state = app(root.path(), 1_000_000).await;
    let fuel = accept(&state, "s", CODE, None).await;
    let credentials = accept(&state, "s", CODE, None).await;
    recovery::mark_stuck(state.units().as_ref(), &fuel, 0, StuckReason::Fuel)
        .await
        .unwrap();
    recovery::mark_stuck(
        state.units().as_ref(),
        &credentials,
        0,
        StuckReason::Credentials,
    )
    .await
    .unwrap();
    let mut unit = state.units().begin().await.unwrap();
    let runs = unit
        .list_runs(RunQuery {
            status: Some(RunStatusFilter::Stuck),
            limit: 10,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(runs.len(), 2);
    assert!(
        runs.iter()
            .any(|run| run.status() == RunStatus::Stuck(StuckReason::Fuel))
    );
    assert!(
        runs.iter()
            .any(|run| run.status() == RunStatus::Stuck(StuckReason::Credentials))
    );
    drop(unit);
    recovery::resume(state.units().as_ref(), &fuel, 1)
        .await
        .unwrap();
    let observed = durable_runs::observe(&state, &fuel).await.unwrap().unwrap();
    assert_eq!(observed["status"], "queued");
    assert!(observed["reason"].is_null());
}
