use super::*;

async fn capped_app(root: &std::path::Path, suspended: usize) -> AppState {
    AppState::new(ServerConfig {
        durable_execution: true,
        max_durable_runs: Some(1),
        max_suspended_runs: Some(suspended),
        database_path: Some(root.join("server.db")),
        volume_root: Some(root.to_owned()),
        package_store_root: Some(root.join("packages")),
        session_cipher: Some(Arc::new(SecretCipher::from_key([19; 32]))),
        volumes: [(
            "data".into(),
            crate::config::VolumeSpec::managed(crate::config::SizeLimit::Bytes(1_000_000)),
        )]
        .into(),
        runtime: interpreter::runtime::RuntimeConfig {
            fuel: 1_000_000,
            ..Default::default()
        },
        ..Default::default()
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn deleting_a_session_cancels_its_wait_without_recording_an_answer() {
    let root = tempfile::tempdir().unwrap();
    let state = capped_app(root.path(), 1).await;
    let (router, session) = session(&state, "30s").await;
    let pending = execute(&router, &session, ONE).await;
    let id = pending["execution_id"].as_str().unwrap();
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/sessions/{session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let (status, body) = resume(&router, id, &call(&pending), 42).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "cancelled");
    assert_eq!(
        durable_runs::observe(&state, id).await.unwrap().unwrap()["status"],
        "cancelled"
    );
    assert_eq!(outcomes(&state, id).await, 0);
    assert!(state.session_manager().runs().get(id).is_none());
    stop(&state).await;
}

#[tokio::test]
async fn suspended_capacity_refusal_interrupts_without_a_durable_outcome() {
    let root = tempfile::tempdir().unwrap();
    // This constructs the production RunRegistry::new(0), with a durable entry.
    let state = capped_app(root.path(), 0).await;
    let (router, session) = session(&state, "30s").await;
    let stopped = execute(&router, &session, ONE).await;
    assert_eq!(stopped["status"], "stuck", "{stopped}");
    assert_eq!(stopped["reason"], "execution_interrupted");
    let id = stopped["execution_id"].as_str().unwrap();
    assert_eq!(outcomes(&state, id).await, 0);
    assert!(state.session_manager().runs().get(id).is_none());
    stop(&state).await;
}

#[tokio::test]
async fn suspended_runs_release_execution_capacity_but_keep_session_and_volume_order() {
    let root = tempfile::tempdir().unwrap();
    let state = capped_app(root.path(), 2).await;
    let (router, session) =
        session_with_vfs(&state, "30s", "\n  mode: named\n  volume: data").await;
    let pending = execute(&router, &session, ONE).await;
    let id = pending["execution_id"].as_str().unwrap();
    let blueprint = state.blueprints().get("callbacks").await.unwrap().unwrap();
    // Neither a later run of this session nor another writer of the volume may
    // pass the suspended owner, even though unrelated work gets its execution slot.
    let same_session = accept_with_blueprint(
        &state,
        &session,
        "function main(): number { return 1; }",
        Some("behind"),
        Arc::new(blueprint.clone()),
    )
    .await;
    let other_writer = accept_with_blueprint(
        &state,
        "writer",
        "function main(): number { return 2; }",
        None,
        Arc::new(blueprint),
    )
    .await;
    let free = accept(
        &state,
        "free",
        "function main(): number { return 3; }",
        None,
    )
    .await;
    let result = until(&state, &free, "completed").await;
    assert_eq!(result["response"]["result"], "3");
    for waiting in [&same_session, &other_writer] {
        assert_eq!(
            durable_runs::observe(&state, waiting)
                .await
                .unwrap()
                .unwrap()["status"],
            "queued"
        );
    }
    assert_eq!(
        resume(&router, id, &call(&pending), 42).await.0,
        StatusCode::OK
    );
    until(&state, &same_session, "completed").await;
    until(&state, &other_writer, "completed").await;
    stop(&state).await;
}

#[tokio::test]
async fn a_tool_program_queued_behind_a_stuck_run_returns_within_the_execute_bound() {
    let root = tempfile::tempdir().unwrap();
    let state = capped_app(root.path(), 1).await;
    let (router, session) = session(&state, "30s").await;
    let stuck = execute(
        &router,
        &session,
        "function main(): void { while (true) {} }",
    )
    .await;
    assert_eq!(stuck["status"], "stuck", "{stuck}");
    let request = Request::builder()
        .method("POST")
        .uri(format!("/v1/sessions/{session}/execute"))
        .header("content-type", "application/json")
        .header("idempotency-key", "queued")
        .body(Body::from(json!({"code": ONE}).to_string()))
        .unwrap();
    let started = tokio::time::Instant::now();
    let response = tokio::time::timeout(Duration::from_secs(30), router.oneshot(request))
        .await
        .expect("queued execute must honor its 25-second bound")
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["status"], "queued", "{body}");
    assert!(started.elapsed() >= Duration::from_secs(25));
    assert_eq!(
        outcomes(&state, body["execution_id"].as_str().unwrap()).await,
        0
    );
    stop(&state).await;
}

#[tokio::test]
async fn an_answer_committed_before_fuel_exhaustion_returns_the_stuck_view() {
    let root = tempfile::tempdir().unwrap();
    let state = capped_app(root.path(), 1).await;
    let (router, session) = session(&state, "30s").await;
    let source = r#"import tools from "@tools"; function main(): void { tools.echo({value: 1}); while (true) {} }"#;
    let pending = execute(&router, &session, source).await;
    let id = pending["execution_id"].as_str().unwrap();
    let (status, body) = resume(&router, id, &call(&pending), 42).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "stuck");
    assert_eq!(body["reason"], "fuel_exhausted");
    assert_eq!(outcomes(&state, id).await, 1);
    assert_eq!(
        resume(&router, id, &call(&pending), 99).await,
        (status, body)
    );
    stop(&state).await;
}
