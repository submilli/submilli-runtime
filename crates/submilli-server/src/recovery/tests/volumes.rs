use super::*;

pub(super) const EARLY_ID: &str = "00000000-0000-4000-8000-000000000001";
pub(super) const LATE_ID: &str = "ffffffff-ffff-4fff-8fff-ffffffffffff";

pub(super) async fn volume_app(root: &std::path::Path) -> AppState {
    AppState::new(ServerConfig {
        durable_execution: true,
        database_path: Some(root.join("server.db")),
        volume_root: Some(root.to_owned()),
        package_store_root: Some(root.join("packages")),
        session_cipher: Some(Arc::new(SecretCipher::from_key([19; 32]))),
        volumes: ["data", "other"]
            .into_iter()
            .map(|name| {
                (
                    name.into(),
                    crate::config::VolumeSpec::managed(crate::config::SizeLimit::Bytes(1_000_000)),
                )
            })
            .collect(),
        runtime: interpreter::runtime::RuntimeConfig {
            fuel: 10_000_000,
            ..Default::default()
        },
        ..Default::default()
    })
    .await
    .unwrap()
}

pub(super) fn volume_blueprint(volume: &str) -> Arc<submilli_blueprint::Blueprint> {
    Arc::new(
        submilli_blueprint::parse(&format!(
            "name: durable\ndefault: allow\nvfs:\n  mode: named\n  volume: {volume}\n"
        ))
        .unwrap(),
    )
}

pub(super) async fn accept_named_writer(
    state: &AppState,
    session: &str,
    id: &str,
    source: &str,
) -> RunId {
    let blueprint = volume_blueprint("data");
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
    let audit = crate::audit::ExecutionAudit::for_run(
        state.audit().clone(),
        id.into(),
        "test",
        "durable",
        Some(session),
    );
    admit(
        state,
        ExecuteInputs {
            session_id: session,
            code: source,
            blueprint_name: "durable",
            blueprint,
            variables: Arc::default(),
            harness_secrets: Arc::default(),
            audit: Some(audit),
            vfs_source: execute::VfsSource::Rest,
            run_entry: "session",
            idempotency_key: None,
            suspension: None,
        },
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn shared_writable_volume_has_one_live_run() {
    let root = tempfile::tempdir().unwrap();
    let state = volume_app(root.path()).await;
    accept_with_blueprint(&state, "a", CODE, None, volume_blueprint("data")).await;
    accept_with_blueprint(&state, "b", CODE, None, volume_blueprint("data")).await;
    let mut tasks = JoinSet::new();
    let mut active = HashMap::new();
    schedule(&state, &mut tasks, &mut active, None, 4)
        .await
        .unwrap();
    let launched = active.len();
    while tasks.join_next().await.is_some() {}
    assert_eq!(launched, 1, "shared writers must not launch together");
    active.clear();
    schedule(&state, &mut tasks, &mut active, None, 4)
        .await
        .unwrap();
    assert_eq!(
        active.len(),
        1,
        "the waiting writer must launch after draining"
    );
    tasks.join_next().await.unwrap().unwrap().unwrap();
}

#[tokio::test]
async fn different_writable_volumes_can_run_together() {
    let root = tempfile::tempdir().unwrap();
    let state = volume_app(root.path()).await;
    accept_with_blueprint(&state, "a", CODE, None, volume_blueprint("data")).await;
    accept_with_blueprint(&state, "b", CODE, None, volume_blueprint("other")).await;
    let mut tasks = JoinSet::new();
    let mut active = HashMap::new();
    schedule(&state, &mut tasks, &mut active, None, 4)
        .await
        .unwrap();
    assert_eq!(active.len(), 2);
    while let Some(result) = tasks.join_next().await {
        result.unwrap().unwrap();
    }
}

#[tokio::test]
async fn stuck_writer_releases_volume_for_another_session() {
    let root = tempfile::tempdir().unwrap();
    let state = volume_app(root.path()).await;
    let stuck = accept_named_writer(
        &state,
        "stuck",
        EARLY_ID,
        "function main(): void { while (true) {} }",
    )
    .await;
    let waiting = accept_named_writer(&state, "waiting", LATE_ID, CODE).await;
    state.boot().await.unwrap();
    assert_eq!(
        until(&state, &stuck, "stuck").await["reason"],
        "fuel_exhausted"
    );
    until(&state, &waiting, "completed").await;
    stop(&state).await;
}

#[tokio::test]
async fn terminal_writer_keeps_volume_until_its_task_is_joined() {
    let root = tempfile::tempdir().unwrap();
    let state = volume_app(root.path()).await;
    let first = accept_named_writer(&state, "first", EARLY_ID, CODE).await;
    let second = accept_named_writer(&state, "second", LATE_ID, CODE).await;
    let mut tasks = JoinSet::new();
    let mut active = HashMap::new();
    schedule(&state, &mut tasks, &mut active, None, 4)
        .await
        .unwrap();
    until(&state, &first, "completed").await;
    // Completion may publish before native cleanup drains. The owner releases
    // the reservation only when join_next_with_id removes it from `active`.
    schedule(&state, &mut tasks, &mut active, None, 4)
        .await
        .unwrap();
    assert_eq!(until(&state, &second, "queued").await["attempt"], 0);
    tasks.join_next().await.unwrap().unwrap().unwrap();
    active.clear();
    schedule(&state, &mut tasks, &mut active, None, 4)
        .await
        .unwrap();
    tasks.join_next().await.unwrap().unwrap().unwrap();
    until(&state, &second, "completed").await;
}

#[tokio::test]
async fn writable_mounts_conflict_with_named_roots_but_read_only_mounts_do_not() {
    for access in ["read_write", "read_only"] {
        let root = tempfile::tempdir().unwrap();
        let state = volume_app(root.path()).await;
        std::fs::create_dir_all(root.path().join("managed-local/data/nested")).unwrap();
        let mounted = Arc::new(submilli_blueprint::parse(&format!(
            "name: durable\ndefault: allow\nvfs:\n  mode: per_session\n  mounts:\n    /shared: {{mode: named, volume: data, access: {access}, subPath: nested}}\n"
        )).unwrap());
        accept_with_blueprint(&state, "mounted", CODE, None, mounted).await;
        accept_with_blueprint(&state, "root", CODE, None, volume_blueprint("data")).await;
        let mut tasks = JoinSet::new();
        let mut active = HashMap::new();
        schedule(&state, &mut tasks, &mut active, None, 4)
            .await
            .unwrap();
        let expected = if access == "read_write" { 1 } else { 2 };
        assert_eq!(active.len(), expected, "{access}");
        while let Some(result) = tasks.join_next().await {
            result.unwrap().unwrap();
        }
    }
}

#[tokio::test]
async fn corrupt_recovering_writer_does_not_stop_discovery() {
    let root = tempfile::tempdir().unwrap();
    let state = volume_app(root.path()).await;
    let queued = accept_named_writer(&state, "queued", EARLY_ID, CODE).await;
    let corrupt = accept_named_writer(&state, "corrupt", LATE_ID, CODE).await;
    recovery::claim(state.units().as_ref(), &corrupt, "previous-startup")
        .await
        .unwrap()
        .unwrap();
    state
        .database()
        .unwrap()
        .transaction(|connection| {
            Box::pin(async move {
                sqlx::query("UPDATE run_programs SET content=x'00' WHERE run_id=?")
                    .bind(LATE_ID)
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .await
        .unwrap();
    state.boot().await.unwrap();
    assert_eq!(
        until(&state, &corrupt, "stuck").await["reason"],
        "invalid_history"
    );
    until(&state, &queued, "completed").await;
    stop(&state).await;
}
