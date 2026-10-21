use super::*;

async fn mounted_app(root: &std::path::Path, durable_execution: bool) -> AppState {
    AppState::new(ServerConfig {
        durable_execution,
        database_path: Some(root.join("server.db")),
        volume_root: Some(root.to_owned()),
        package_store_root: Some(root.join("packages")),
        session_cipher: Some(Arc::new(SecretCipher::from_key([19; 32]))),
        volumes: crate::config::VolumeTable::from([(
            "data".into(),
            crate::config::VolumeSpec::managed(crate::config::SizeLimit::Bytes(1_000_000)),
        )]),
        runtime: interpreter::runtime::RuntimeConfig {
            fuel: 100_000,
            ..Default::default()
        },
        ..Default::default()
    })
    .await
    .unwrap()
}

fn mounted_blueprint() -> Arc<submilli_blueprint::Blueprint> {
    Arc::new(submilli_blueprint::parse("name: durable\ndefault: allow\nvfs:\n  mode: per_session\n  mounts:\n    /data: { mode: named, volume: data }\n").unwrap())
}

#[tokio::test]
async fn close_retires_named_mount_artifacts_after_stuck_work_drains() {
    let root = tempfile::tempdir().unwrap();
    let state = mounted_app(root.path(), true).await;
    let code = r#"import { appendText } from "submilli:fs";
        function main(): void { appendText("/data/result", "once"); while (true) {} }"#;
    let id = accept_with_blueprint(&state, "s", code, None, mounted_blueprint()).await;
    state.boot().await.unwrap();
    until(&state, &id, "stuck").await;
    let private = root
        .path()
        .join("managed-local/data/.git-submilli-recovery")
        .join(id.as_str());
    assert!(private.exists(), "{}", private.display());
    state.session_manager().wipe_now("s").await.unwrap();
    until_retired(&state, &id).await;
    assert!(!private.exists());
    assert_eq!(
        std::fs::read_to_string(root.path().join("managed-local/data/result")).unwrap(),
        "once"
    );
    assert!(!root.path().join("sessions/s").exists());
    assert_eq!(
        durable_runs::observe(&state, &id).await.unwrap().unwrap()["status"],
        "cancelled"
    );
    stop(&state).await;
}

#[tokio::test]
async fn restart_does_not_recreate_a_missing_managed_root() {
    let root = tempfile::tempdir().unwrap();
    let state = mounted_app(root.path(), true).await;
    let id = accept_with_blueprint(&state, "s", CODE, None, mounted_blueprint()).await;
    state.database().unwrap().close().await.unwrap();
    drop(state);
    let named = root.path().join("managed-local/data");
    std::fs::remove_dir_all(&named).unwrap();
    let state = mounted_app(root.path(), true).await;
    state.boot().await.unwrap();
    assert_eq!(
        until(&state, &id, "stuck").await["reason"],
        "storage_unavailable"
    );
    assert!(!named.exists());
    stop(&state).await;
}

#[tokio::test]
async fn disabled_durability_retains_queued_work_without_executing_it() {
    let root = tempfile::tempdir().unwrap();
    let state = mounted_app(root.path(), true).await;
    let id = accept(&state, "s", CODE, None).await;
    state.database().unwrap().close().await.unwrap();
    drop(state);
    let state = mounted_app(root.path(), false).await;
    state.boot().await.unwrap();
    let stuck = until(&state, &id, "stuck").await;
    assert_eq!(stuck["reason"], "durability_disabled");
    assert_eq!(stuck["attempt"], 0);
    stop(&state).await;
}

#[tokio::test]
async fn failed_retirement_keeps_roots_until_cleanup_can_retry() {
    let root = tempfile::tempdir().unwrap();
    let state = mounted_app(root.path(), true).await;
    let id = accept_with_blueprint(
        &state,
        "s",
        "function main(): void { while (true) {} }",
        None,
        mounted_blueprint(),
    )
    .await;
    state.boot().await.unwrap();
    until(&state, &id, "stuck").await;
    let named = root.path().join("managed-local/data");
    let unavailable = root.path().join("offline");
    std::fs::rename(&named, &unavailable).unwrap();
    state.session_manager().wipe_now("s").await.unwrap();
    assert_eq!(
        durable_runs::observe(&state, &id).await.unwrap().unwrap()["status"],
        "cancelled"
    );
    assert!(
        root.path().join("sessions/s").exists(),
        "cleanup must retain the owned root for retry"
    );
    std::fs::rename(&unavailable, &named).unwrap();
    state
        .session_manager()
        .process_pending_cleanup()
        .await
        .unwrap();
    until_retired(&state, &id).await;
    assert!(!root.path().join("sessions/s").exists());
    stop(&state).await;
}

#[tokio::test]
async fn queued_admission_and_rebind_leave_stuck_runs_files_untouched() {
    let root = tempfile::tempdir().unwrap();
    let state = app(root.path(), 100_000).await;
    let blueprint = Arc::new(
        submilli_blueprint::parse(
            "name: durable\ndefault: allow\nvfs:\n  mode: per_session\n  cwd: /work\n",
        )
        .unwrap(),
    );
    let source = r#"import { remove } from "submilli:fs"; function main(): void { remove("/work", false); while (true) {} }"#;
    let id = accept_with_blueprint(&state, "s", source, None, blueprint.clone()).await;
    state.boot().await.unwrap();
    until(&state, &id, "stuck").await;
    let cwd = root.path().join("sessions/s/work");
    assert!(!cwd.exists());
    let queued = accept_with_blueprint(&state, "s", CODE, None, blueprint.clone()).await;
    assert!(
        !cwd.exists(),
        "admission must not prepare the preceding run's cwd"
    );
    state
        .session_manager()
        .bind("s", &blueprint, Arc::default(), Arc::default())
        .await
        .unwrap();
    assert!(
        !cwd.exists(),
        "rebind must not prepare a retained run's cwd"
    );
    assert_eq!(
        durable_runs::observe(&state, &queued)
            .await
            .unwrap()
            .unwrap()["status"],
        "queued"
    );
    stop(&state).await;
}

#[tokio::test]
async fn retained_rebind_does_not_recreate_a_missing_named_root() {
    let root = tempfile::tempdir().unwrap();
    let state = mounted_app(root.path(), true).await;
    let blueprint = Arc::new(
        submilli_blueprint::parse(
            "name: durable\ndefault: allow\nvfs: { mode: named, volume: data }\n",
        )
        .unwrap(),
    );
    let id = accept_with_blueprint(&state, "s", CODE, None, blueprint.clone()).await;
    let named = root.path().join("managed-local/data");
    std::fs::remove_dir_all(&named).unwrap();
    state
        .session_manager()
        .bind("s", &blueprint, Arc::default(), Arc::default())
        .await
        .unwrap();
    assert!(!named.exists());
    state.boot().await.unwrap();
    assert_eq!(
        until(&state, &id, "stuck").await["reason"],
        "storage_unavailable"
    );
    assert!(!named.exists());
    stop(&state).await;
}

#[tokio::test]
async fn entirely_readonly_filesystem_is_refused_before_admission() {
    let root = tempfile::tempdir().unwrap();
    let state = mounted_app(root.path(), true).await;
    let blueprint = Arc::new(submilli_blueprint::parse("name: durable\ndefault: allow\nvfs: { mode: named, volume: data, access: read_only }\n").unwrap());
    state
        .blueprints()
        .upsert((*blueprint).clone())
        .await
        .unwrap();
    state
        .session_manager()
        .ensure("s", &blueprint)
        .await
        .unwrap();
    let result = admit(
        &state,
        ExecuteInputs {
            session_id: "s",
            code: CODE,
            blueprint_name: "durable",
            blueprint,
            variables: Arc::default(),
            harness_secrets: Arc::default(),
            audit: None,
            vfs_source: execute::VfsSource::Rest,
            run_entry: "session",
            idempotency_key: None,
            tools: None,
        },
    )
    .await;
    assert!(result.unwrap_err().contains("writable root or mount"));
    let mut unit = state.units().begin().await.unwrap();
    assert!(!unit.has_retained_run("s").await.unwrap());
}

#[tokio::test]
async fn enabling_durability_preserves_existing_http_response_bytes() {
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let root = tempfile::tempdir().unwrap();
    let first = mounted_app(root.path(), false).await;
    let blueprint =
        submilli_blueprint::parse("name: durable\ndefault: allow\nvfs: none\n").unwrap();
    first.blueprints().upsert(blueprint.clone()).await.unwrap();
    first
        .session_manager()
        .ensure("s", &blueprint)
        .await
        .unwrap();
    let request = || {
        Request::post("/v1/sessions/s/execute")
            .header("Content-Type", "application/json")
            .header("Idempotency-Key", "key")
            .body(Body::from(serde_json::json!({"code": CODE}).to_string()))
            .unwrap()
    };
    let original = crate::app::app(first.clone())
        .oneshot(request())
        .await
        .unwrap();
    assert_eq!(original.status(), 200);
    let body = original.into_body().collect().await.unwrap().to_bytes();
    stop(&first).await;
    first.database().unwrap().close().await.unwrap();
    drop(first);
    let second = mounted_app(root.path(), true).await;
    let replay = crate::app::app(second.clone())
        .oneshot(request())
        .await
        .unwrap();
    assert_eq!(replay.status(), 200);
    assert_eq!(replay.headers()["mcp-session-id"], "s");
    assert_eq!(replay.into_body().collect().await.unwrap().to_bytes(), body);
    stop(&second).await;
}
