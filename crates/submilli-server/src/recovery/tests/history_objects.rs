use super::*;

async fn object_app(root: &std::path::Path, durable_execution: bool) -> AppState {
    AppState::new(ServerConfig {
        durable_execution,
        durable_objects: crate::config::DurableObjectConfig {
            inline_threshold: 0,
        },
        database_path: Some(root.join("server.db")),
        volume_root: Some(root.to_owned()),
        package_store_root: Some(root.join("packages")),
        session_cipher: Some(Arc::new(SecretCipher::from_key([19; 32]))),
        ..Default::default()
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn failed_object_retirement_retries_after_restart_with_durability_disabled() {
    let root = tempfile::tempdir().unwrap();
    let first = object_app(root.path(), true).await;
    let id = accept(&first, "custom.session", CODE, None).await;
    let run = recovery::claim(
        first.units().as_ref(),
        &id,
        first.idempotency().generation(),
    )
    .await
    .unwrap()
    .unwrap();
    let (_cancel, cancelled) = oneshot::channel();
    run_attempt(&first, &run, cancelled).await.unwrap();
    let directory = root
        .path()
        .join("durable-objects/custom.session")
        .join(id.as_str());
    assert!(directory.is_dir());
    let blocker = directory.join("unexpected-directory");
    std::fs::create_dir(&blocker).unwrap();
    assert!(
        retire(first.session_manager(), first.units().as_ref(), &id)
            .await
            .is_err()
    );
    assert_eq!(retention(&first, &id).await, (false, 1));
    first.database().unwrap().close().await.unwrap();
    drop(first);

    std::fs::remove_dir(blocker).unwrap();
    let second = object_app(root.path(), false).await;
    assert!(second.history_objects().is_some());
    second.boot().await.unwrap();
    until_retired(&second, &id).await;
    assert_eq!(retention(&second, &id).await, (true, 0));
    assert!(!directory.parent().unwrap().exists());
    stop(&second).await;
}

async fn retention(state: &AppState, id: &RunId) -> (bool, i64) {
    let id = id.to_string();
    state.database().unwrap().read(move |connection| Box::pin(async move {
        Ok(sqlx::query_as("SELECT resources_retired, (SELECT count(*) FROM durable_outcomes WHERE run_id=runs.run_id) FROM runs WHERE run_id=?")
            .bind(id).fetch_one(connection).await?)
    })).await.unwrap()
}
