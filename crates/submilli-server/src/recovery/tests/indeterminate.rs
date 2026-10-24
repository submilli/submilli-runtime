use super::*;

#[tokio::test]
async fn migrated_indeterminate_predecessor_allows_durable_execution() {
    let root = tempfile::tempdir().unwrap();
    legacy_database(root.path()).await;
    let state = app(root.path(), 1_000_000).await;
    let id = accept(&state, "s", CODE, None).await;
    state.boot().await.unwrap();
    let completed =
        tokio::time::timeout(DISCOVERY_INTERVAL * 2, until(&state, &id, "completed")).await;
    stop(&state).await;
    assert!(
        completed.is_ok(),
        "indeterminate predecessor blocked durable work"
    );
    let original = durable_runs::observe(&state, "legacy")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(original["status"], "indeterminate");
    assert_eq!(original["reason"], "legacy_without_history");
    assert_eq!(original["attempt"], 0);
}

#[tokio::test]
async fn startup_interrupted_predecessor_allows_durable_execution() {
    let root = tempfile::tempdir().unwrap();
    let state = app(root.path(), 1_000_000).await;
    let blueprint =
        submilli_blueprint::parse("name: durable\ndefault: allow\nvfs: none\n").unwrap();
    state.blueprints().upsert(blueprint.clone()).await.unwrap();
    state
        .session_manager()
        .ensure("s", &blueprint)
        .await
        .unwrap();
    let mut unit = state.units().begin().await.unwrap();
    let run = Run::reserve(
        "s".into(),
        "old-key".into(),
        "fingerprint".into(),
        "interrupted".into(),
        "old-startup".into(),
        SystemTime::now(),
    )
    .unwrap();
    unit.save_run(run).await.unwrap();
    let mut run = unit.get_run("interrupted").await.unwrap().unwrap();
    run.claim("old-startup").unwrap();
    unit.save_run(run).await.unwrap();
    unit.commit().await.unwrap();
    let id = accept(&state, "s", CODE, None).await;
    state.boot().await.unwrap();
    let completed =
        tokio::time::timeout(DISCOVERY_INTERVAL * 2, until(&state, &id, "completed")).await;
    stop(&state).await;
    assert!(completed.is_ok(), "startup recovery blocked the successor");
    let mut unit = state.units().begin().await.unwrap();
    let original = unit.run_for_key("s", "old-key").await.unwrap().unwrap();
    assert_eq!(original.reservation_id().as_str(), "interrupted");
    assert_eq!(
        original.status(),
        RunStatus::Indeterminate(IndeterminateReason::Interrupted)
    );
    assert!(unit.unfinished_runs().await.unwrap().is_empty());
}

#[tokio::test]
async fn indeterminate_history_is_excluded_from_recovery_and_session_cancellation() {
    use crate::application::sessions::recovery::RecoveryFilter;
    let root = tempfile::tempdir().unwrap();
    legacy_database(root.path()).await;
    let state = app(root.path(), 1_000_000).await;
    let id = accept(&state, "s", CODE, None).await;
    let mut unit = state.units().begin().await.unwrap();
    let original = unit.run_for_key("s", "legacy-key").await.unwrap().unwrap();
    for filter in [
        RecoveryFilter::Retained,
        RecoveryFilter::Execution,
        RecoveryFilter::Retirement,
    ] {
        let runs = unit
            .list_runs(RunQuery {
                recovery: Some(filter),
                limit: 100,
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(
            runs.iter()
                .all(|run| run.reservation_id() != original.reservation_id())
        );
    }
    let unfinished = unit
        .list_runs(RunQuery {
            session: Some("s".into()),
            unfinished_only: true,
            limit: 100,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(unfinished.len(), 1);
    assert_eq!(unfinished[0].reservation_id(), &id);
    unit.commit().await.unwrap();

    state.session_manager().wipe_now("s").await.unwrap();
    state
        .session_manager()
        .process_pending_cleanup()
        .await
        .unwrap();
    let mut unit = state.units().begin().await.unwrap();
    assert_eq!(
        unit.run_for_key("s", "legacy-key").await.unwrap(),
        Some(original)
    );
    assert_eq!(
        unit.get_run(&id).await.unwrap().unwrap().status(),
        RunStatus::Cancelled
    );
    assert!(unit.run_resources_retired(&id).await.unwrap());
    assert!(!unit.has_retained_run("s").await.unwrap());
    unit.commit().await.unwrap();
    assert!(!state.session_manager().contains("s").await.unwrap());
}

async fn legacy_database(root: &std::path::Path) {
    use sqlx::Connection;
    use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection};
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(root.join("server.db"))
            .create_if_missing(true),
    )
    .await
    .unwrap();
    let migrations = sqlx::migrate!("./migrations")
        .iter()
        .filter(|migration| migration.version <= 6)
        .cloned()
        .collect();
    sqlx::migrate::Migrator::with_migrations(migrations)
        .run_direct(None, &mut connection, false)
        .await
        .unwrap();
    sqlx::query("INSERT INTO sessions(session_id,record_version,status,blueprint_name,idle_timeout_ms,last_activity_unix_ms,root_vfs_type) VALUES ('s',1,'active','durable',600000,?,'none')")
        .bind(SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis() as i64)
        .execute(&mut connection).await.unwrap();
    sqlx::query("INSERT INTO idempotent_requests(session_id,request_key,fingerprint,reservation_id,owner_generation,created_at_unix_ms,state) VALUES ('s','legacy-key','fingerprint','legacy','old-startup',10,'indeterminate')")
        .execute(&mut connection).await.unwrap();
    connection.close().await.unwrap();
}
