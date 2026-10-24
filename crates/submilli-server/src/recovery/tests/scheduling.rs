use super::*;
use crate::domain::run_program::RunProgram;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queued_run_is_reached_past_stuck_runs_during_continuous_wakeups() {
    let root = tempfile::tempdir().unwrap();
    let state = app(root.path(), 1_000_000).await;
    let program = prepare_sessions(&state).await;
    for index in 0..101 {
        let id = admit_named(&state, "blocked", &format!("b-{index:03}"), &program).await;
        recovery::mark_stuck(state.units().as_ref(), &id, 0, StuckReason::Fuel)
            .await
            .unwrap();
    }
    let target = admit_named(&state, "ready", "z-target", &program).await;
    assert_claimed_during_wakeups(state, target).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queued_run_is_reached_past_waiting_runs_during_continuous_wakeups() {
    let root = tempfile::tempdir().unwrap();
    let state = app(root.path(), 1_000_000).await;
    let program = prepare_sessions(&state).await;
    // These remain queued behind the stuck predecessor, so filtering stuck rows
    // alone cannot make the target reachable if notifications reset the cursor.
    for index in 0..101 {
        admit_named(&state, "blocked", &format!("b-{index:03}"), &program).await;
    }
    let target = admit_named(&state, "ready", "z-target", &program).await;
    assert_claimed_during_wakeups(state, target).await;
}

#[tokio::test]
async fn failed_discovery_waits_before_retrying_the_same_page() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let root = tempfile::tempdir().unwrap();
    let state = app(root.path(), 1_000_000).await;
    let program = prepare_sessions(&state).await;
    for index in 0..100 {
        admit_named(&state, "blocked", &format!("b-{index:03}"), &program).await;
    }
    let target = admit_named(&state, "ready", "z-target", &program).await;
    // Inject invalid persisted metadata to make only the second page fail to
    // decode. This is an internal storage failure, not a guest-input failure.
    state
        .database()
        .unwrap()
        .transaction(|connection| {
            Box::pin(async move {
                sqlx::query("UPDATE runs SET session_id='' WHERE run_id='z-target'")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .await
        .unwrap();
    let scans = Arc::new(AtomicUsize::new(0));
    let count = scans.clone();
    let worker = state.clone();
    let draining = CancellationToken::new();
    let supervisor = tokio::spawn(supervise(
        move || {
            count.fetch_add(1, Ordering::SeqCst);
            Some(worker.clone())
        },
        Arc::new(Notify::new()),
        1,
        draining.clone(),
    ));
    tokio::time::timeout(Duration::from_secs(2), async {
        while scans.load(Ordering::SeqCst) < 2 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let attempts = scans.load(Ordering::SeqCst);
    state
        .database()
        .unwrap()
        .transaction(|connection| {
            Box::pin(async move {
                sqlx::query("UPDATE runs SET session_id='ready' WHERE run_id='z-target'")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .await
        .unwrap();
    let recovered =
        tokio::time::timeout(Duration::from_secs(2), until(&state, &target, "completed")).await;
    draining.cancel();
    supervisor.await.unwrap();
    assert!(
        attempts <= 3,
        "failed page retried {attempts} times without waiting"
    );
    assert!(
        recovered.is_ok(),
        "repaired page must remain eligible for retry"
    );
}

#[tokio::test]
async fn execution_and_retirement_queries_select_separate_work() {
    let root = tempfile::tempdir().unwrap();
    let state = app(root.path(), 1_000_000).await;
    let program = prepare_sessions(&state).await;
    let queued = admit_named(&state, "ready", "a-queued", &program).await;
    state
        .session_manager()
        .ensure("running", &program.blueprint)
        .await
        .unwrap();
    let running = admit_named(&state, "running", "b-running", &program).await;
    recovery::claim(state.units().as_ref(), &running, "previous-startup")
        .await
        .unwrap()
        .unwrap();
    let terminal = admit_named(&state, "ready", "c-terminal", &program).await;
    cancel_run(&state, &terminal).await;
    let retired = admit_named(&state, "ready", "d-retired", &program).await;
    cancel_run(&state, &retired).await;
    assert!(state.session_manager().retire_run(&retired).await);
    let legacy = Run::prepare(
        "ready".into(),
        None,
        "legacy".into(),
        "e-legacy".into(),
        "old-owner".into(),
        RunIdentity::default(),
        SystemTime::now(),
    )
    .unwrap();
    let mut unit = state.units().begin().await.unwrap();
    unit.save_run(legacy).await.unwrap();
    let mut legacy = unit.get_run("e-legacy").await.unwrap().unwrap();
    legacy.mark_indeterminate("e-legacy").unwrap();
    unit.save_run(legacy).await.unwrap();
    unit.commit().await.unwrap();

    let execution: Vec<_> = candidates(&state, None)
        .await
        .unwrap()
        .into_iter()
        .map(|run| run.reservation_id().clone())
        .collect();
    assert_eq!(execution, vec![queued, running]);
    let retirement: Vec<_> = retirement_candidates(&state, None)
        .await
        .unwrap()
        .into_iter()
        .map(|run| run.reservation_id().clone())
        .collect();
    assert_eq!(retirement, vec![terminal]);
    // Public listing still includes stuck, legacy and retired records.
    let mut unit = state.units().begin().await.unwrap();
    assert_eq!(
        unit.list_runs(RunQuery {
            limit: 100,
            ..Default::default()
        })
        .await
        .unwrap()
        .len(),
        6
    );
}

#[tokio::test]
async fn capacity_limit_resumes_after_the_last_visited_run() {
    let root = tempfile::tempdir().unwrap();
    let state = app(root.path(), 1_000_000).await;
    let program = prepare_sessions(&state).await;
    let first = admit_named(&state, "ready", "a-first", &program).await;
    let second = admit_named(&state, "ready", "b-second", &program).await;
    let mut tasks = JoinSet::new();
    let mut active = HashMap::new();
    let after = schedule(&state, &mut tasks, &mut active, None, 1)
        .await
        .unwrap();
    assert_eq!(after, Some(first));
    assert_eq!(active.len(), 1);
    tasks.join_next().await.unwrap().unwrap().unwrap();
    active.clear();
    schedule(&state, &mut tasks, &mut active, after, 1)
        .await
        .unwrap();
    assert_eq!(
        active.values().next().unwrap().run.reservation_id(),
        &second
    );
    tasks.join_next().await.unwrap().unwrap().unwrap();
}

#[tokio::test]
async fn retirement_progresses_while_execution_capacity_is_full() {
    let root = tempfile::tempdir().unwrap();
    let state = app(root.path(), u64::MAX).await;
    let mut program = prepare_sessions(&state).await;
    program.source = "function main(): void { while (true) {} }".into();
    let running = admit_named(&state, "ready", "a-running", &program).await;
    let terminal = admit_named(&state, "blocked", "z-terminal", &program).await;
    cancel_run(&state, &terminal).await;
    let draining = CancellationToken::new();
    let worker = state.clone();
    let supervisor = tokio::spawn(supervise(
        move || Some(worker.clone()),
        Arc::new(Notify::new()),
        1,
        draining.clone(),
    ));
    let retired = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let mut unit = state.units().begin().await.unwrap();
            if unit.run_resources_retired(&terminal).await.unwrap() {
                assert_eq!(
                    unit.get_run(&running).await.unwrap().unwrap().status(),
                    RunStatus::Running
                );
                break;
            }
            drop(unit);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    draining.cancel();
    supervisor.await.unwrap();
    assert!(
        retired.is_ok(),
        "full execution capacity must not stop retirement"
    );
}

async fn cancel_run(state: &AppState, id: &RunId) {
    let mut unit = state.units().begin().await.unwrap();
    let mut run = unit.get_run(id).await.unwrap().unwrap();
    run.cancel().unwrap();
    unit.save_run(run).await.unwrap();
    unit.commit().await.unwrap();
}

async fn prepare_sessions(state: &AppState) -> RunProgram {
    let template = accept(state, "blocked", CODE, None).await;
    let program = state
        .units()
        .begin()
        .await
        .unwrap()
        .get_run_program(&template)
        .await
        .unwrap()
        .unwrap();
    recovery::mark_stuck(state.units().as_ref(), &template, 0, StuckReason::Fuel)
        .await
        .unwrap();
    state
        .session_manager()
        .ensure("ready", &program.blueprint)
        .await
        .unwrap();
    program
}

async fn admit_named(state: &AppState, session: &str, id: &str, program: &RunProgram) -> RunId {
    let mut run = Run::prepare(
        session.into(),
        None,
        "fingerprint".into(),
        id.into(),
        state.idempotency().generation().into(),
        RunIdentity::default(),
        SystemTime::now(),
    )
    .unwrap();
    run.enable_durability().unwrap();
    recovery::admit(state.units().as_ref(), run, program.clone())
        .await
        .unwrap()
}

async fn assert_claimed_during_wakeups(state: AppState, target: RunId) {
    let wakeup = Arc::new(Notify::new());
    let draining = CancellationToken::new();
    let worker = state.clone();
    let signal = wakeup.clone();
    let stop = draining.clone();
    // Model the notifications from sustained admissions deterministically: a
    // notification is already pending whenever the owner reaches its select.
    let supervisor = tokio::spawn(supervise(
        move || {
            signal.notify_one();
            Some(worker.clone())
        },
        wakeup,
        1,
        stop,
    ));
    let reached = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let run = state
                .units()
                .begin()
                .await
                .unwrap()
                .get_run(&target)
                .await
                .unwrap()
                .unwrap();
            if run.attempt() != 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    draining.cancel();
    supervisor.await.unwrap();
    assert!(
        reached.is_ok(),
        "queued run was starved by continuous wakeups"
    );
}
