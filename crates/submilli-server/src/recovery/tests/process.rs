use super::*;
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub(crate) fn checkpoint(point: &str) {
    if std::env::var("SUBMILLI_RECOVERY_CRASH_POINT")
        .ok()
        .as_deref()
        != Some(point)
    {
        return;
    }
    let root = PathBuf::from(std::env::var_os("SUBMILLI_RECOVERY_CRASH_ROOT").unwrap());
    std::fs::write(root.join("checkpoint"), point).unwrap();
    loop {
        std::thread::park();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn recovery_child() {
    let Some(root) = std::env::var_os("SUBMILLI_RECOVERY_CRASH_ROOT") else {
        return;
    };
    submilli_durable::set_test_checkpoint(checkpoint);
    let root = PathBuf::from(root);
    let state = app(&root, 10_000_000).await;
    let blueprint = Arc::new(
        submilli_blueprint::parse("name: durable\ndefault: allow\nvfs: per_session\n").unwrap(),
    );
    state
        .blueprints()
        .upsert((*blueprint).clone())
        .await
        .unwrap();
    state
        .session_manager()
        .ensure("session", &blueprint)
        .await
        .unwrap();
    let source = r#"
        import { v4 } from "submilli:uuid";
        import { appendText, readText } from "submilli:fs";
        function main(): string {
            const value = v4();
            appendText("/result", value);
            return readText("/result") ?? "missing";
        }
    "#;
    let id = admit(
        &state,
        ExecuteInputs {
            session_id: "session",
            code: source,
            blueprint_name: "durable",
            blueprint,
            variables: Arc::default(),
            harness_secrets: Arc::default(),
            audit: None,
            vfs_source: execute::VfsSource::Rest,
            run_entry: "session",
            idempotency_key: Some("key"),
            tools: None,
        },
    )
    .await
    .unwrap();
    std::fs::write(root.join("run-id"), id.as_str()).unwrap();
    state.boot().await.unwrap();
    // The parent kills this process at an explicit durable boundary.
    tokio::time::sleep(Duration::from_secs(60)).await;
    panic!("crash checkpoint was never reached");
}

#[tokio::test]
async fn killed_process_recovers_claim_append_and_terminal_boundaries() {
    for point in [
        "claimed",
        "append-written",
        "effect-receipt",
        "before-terminal",
    ] {
        let root = tempfile::tempdir().unwrap();
        let log = std::fs::File::create(root.path().join("child.log")).unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "recovery::tests::process::recovery_child",
                "--nocapture",
            ])
            .env("SUBMILLI_RECOVERY_CRASH_ROOT", root.path())
            .env("SUBMILLI_RECOVERY_CRASH_POINT", point)
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while !root.path().join("checkpoint").exists() {
            let exited = child.try_wait().unwrap();
            if exited.is_some() || std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!(
                    "{point}: child did not reach checkpoint: {}",
                    std::fs::read_to_string(root.path().join("child.log")).unwrap()
                );
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        child.kill().unwrap();
        child.wait().unwrap();
        let id = RunId::from(std::fs::read_to_string(root.path().join("run-id")).unwrap());
        let state = app(root.path(), 10_000_000).await;
        state.boot().await.unwrap();
        let result = until(&state, &id, "completed").await;
        assert!(result["response"]["error"].is_null(), "{point}: {result}");
        let content = std::fs::read_to_string(root.path().join("sessions/session/result")).unwrap();
        assert!(
            uuid::Uuid::parse_str(&content).is_ok(),
            "{point}: append repeated: {content}"
        );
        assert_eq!(result["response"]["result"], content, "{point}");
        assert_eq!(result["attempt"], 2, "{point}");
        stop(&state).await;
        state.database().unwrap().close().await.unwrap();
    }
}

const APPEND_A: &str = r#"import { appendText } from "submilli:fs"; function main(): void { appendText("/log", "A"); }"#;
const APPEND_B: &str = r#"import { appendText } from "submilli:fs"; function main(): void { appendText("/log", "B"); }"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shared_volume_child() {
    let Some(root) = std::env::var_os("SUBMILLI_RECOVERY_CRASH_ROOT") else {
        return;
    };
    submilli_durable::set_test_checkpoint(checkpoint);
    let root = PathBuf::from(root);
    let state = volumes::volume_app(&root).await;
    volumes::accept_named_writer(&state, "first", volumes::LATE_ID, APPEND_A).await;
    state.boot().await.unwrap();
    tokio::time::sleep(Duration::from_secs(60)).await;
    panic!("append intent checkpoint was never reached");
}

#[tokio::test]
async fn killed_shared_writer_recovers_before_a_queued_append() {
    let root = tempfile::tempdir().unwrap();
    let log = std::fs::File::create(root.path().join("child.log")).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "recovery::tests::process::shared_volume_child",
            "--nocapture",
        ])
        .env("SUBMILLI_RECOVERY_CRASH_ROOT", root.path())
        .env("SUBMILLI_RECOVERY_CRASH_POINT", "effect-intent")
        .stdout(Stdio::from(log.try_clone().unwrap()))
        .stderr(Stdio::from(log))
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !root.path().join("checkpoint").exists() {
        if child.try_wait().unwrap().is_some() || std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "child did not prepare append: {}",
                std::fs::read_to_string(root.path().join("child.log")).unwrap()
            );
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    child.kill().unwrap();
    child.wait().unwrap();
    let log_path = root.path().join("managed-local/data/log");
    assert!(!log_path.exists(), "kill must precede the append write");
    let state = volumes::volume_app(root.path()).await;
    let recovering = RunId::from(volumes::LATE_ID);
    let queued = volumes::accept_named_writer(&state, "second", volumes::EARLY_ID, APPEND_B).await;
    // B sorts ahead of A. With one slot, the old scheduler completes B first,
    // then A retries its saved offset zero and overwrites B's committed bytes.
    let mut tasks = JoinSet::new();
    let mut active = HashMap::new();
    for _ in 0..2 {
        schedule(&state, &mut tasks, &mut active, None, 1)
            .await
            .unwrap();
        assert_eq!(active.len(), 1);
        tasks.join_next().await.unwrap().unwrap().unwrap();
        active.clear();
    }
    assert_eq!(until(&state, &recovering, "completed").await["attempt"], 2);
    assert_eq!(until(&state, &queued, "completed").await["attempt"], 1);
    assert_eq!(std::fs::read_to_string(log_path).unwrap(), "AB");
    state.database().unwrap().close().await.unwrap();
}
