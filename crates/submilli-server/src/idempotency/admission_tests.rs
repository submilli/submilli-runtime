use super::tests::{fixture, proceed};
use super::*;
use crate::domain::idempotent_request::RunStatus;
use crate::domain::run_status::IndeterminateReason;

#[tokio::test]
async fn unkeyed_invocations_have_distinct_ordered_identity_and_no_response_body() {
    let (_, units, coordinator) = fixture().await;
    let first = "run-a".into();
    let second = "run-b".into();
    let (a, b) = tokio::join!(
        coordinator.prepare("session", None, "same", &first, Some("alice"), "mcp"),
        coordinator.prepare("session", None, "same", &second, Some("alice"), "http"),
    );
    let a = proceed(a);
    let b = proceed(b);
    coordinator.claim(&"run-a".into()).await.unwrap();
    assert!(
        coordinator.claim(&"run-a".into()).await.is_err(),
        "a claim is not a second launch permit"
    );
    a.finish_unkeyed().await;
    coordinator.claim(&"run-b".into()).await.unwrap();
    b.finish_unkeyed().await;
    let mut unit = units.begin().await.unwrap();
    let a = unit.get_run("run-a").await.unwrap().unwrap();
    let b = unit.get_run("run-b").await.unwrap().unwrap();
    assert_eq!(a.status(), RunStatus::Completed);
    assert_eq!(b.status(), RunStatus::Completed);
    assert_ne!(a.identity().admission_order, b.identity().admission_order);
    assert_eq!(a.identity().principal.as_deref(), Some("alice"));
    assert_eq!(a.identity().execution_id.as_deref(), Some("run-a"));
    assert_eq!(a.attempt(), 1);
    assert!(a.optional_key().is_none());
}

#[tokio::test]
async fn release_after_claim_keeps_the_key_and_cannot_authorize_a_retry() {
    let (_, units, coordinator) = fixture().await;
    let guard = proceed(
        coordinator
            .prepare("session", Some("key"), "code", &"run".into(), None, "http")
            .await,
    );
    coordinator.claim(&"run".into()).await.unwrap();
    guard.release().await;
    let run = units
        .begin()
        .await
        .unwrap()
        .get_run("run")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        run.status(),
        RunStatus::Indeterminate(IndeterminateReason::Interrupted)
    );
    assert_eq!(run.optional_key(), Some("key"));
    assert!(matches!(
        coordinator.reserve("session", "key", "code").await,
        Reservation::Refused(Refusal::Indeterminate)
    ));
}

#[tokio::test]
async fn closure_fences_prepared_writes_and_retains_completed_identity() {
    let (_, units, coordinator) = fixture().await;
    let guard = proceed(
        coordinator
            .prepare("session", Some("key"), "code", &"run".into(), None, "http")
            .await,
    );
    coordinator.claim(&"run".into()).await.unwrap();
    let mut unit = units.begin().await.unwrap();
    let mut stale = unit.get_run("run").await.unwrap().unwrap();
    drop(unit);
    let mut unit = units.begin().await.unwrap();
    let mut session = unit.get_session("session").await.unwrap().unwrap();
    session.close(crate::domain::session::ClosedReason::Deleted);
    crate::application::sessions::runs::cancel_session_runs(unit.as_mut(), "session")
        .await
        .unwrap();
    unit.save_session(session).await.unwrap();
    unit.commit().await.unwrap();
    stale.complete("run", SystemTime::now()).unwrap();
    let mut unit = units.begin().await.unwrap();
    assert!(unit.save_run(stale).await.is_err());
    drop(unit);
    guard.complete(200, "late".into()).await;
    let mut unit = units.begin().await.unwrap();
    assert_eq!(
        unit.get_run("run").await.unwrap().unwrap().status(),
        RunStatus::Cancelled
    );
    drop(unit);
    assert!(matches!(
        coordinator
            .prepare("session", None, "code", &"later".into(), None, "mcp")
            .await,
        Reservation::Refused(_)
    ));
}

/// A real process exit bypasses guards, runtime cleanup and database close.
#[test]
fn process_exit_preserves_unkeyed_run_without_inventing_replay() {
    const CHILD: &str = "SUB1836_CRASH_DATABASE";
    if let Some(path) = std::env::var_os(CHILD) {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let database = Arc::new(
                crate::database::ServerDatabase::open(std::path::Path::new(&path))
                    .await
                    .unwrap(),
            );
            let (_, _, coordinator) = super::tests::fixture_with_database(database).await;
            let _guard = proceed(
                coordinator
                    .prepare("session", None, "code", &"crashed".into(), None, "mcp")
                    .await,
            );
            coordinator.claim(&"crashed".into()).await.unwrap();
            std::process::exit(77);
        });
        unreachable!("child exits while the run is live");
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("db");
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "idempotency::admission_tests::process_exit_preserves_unkeyed_run_without_inventing_replay", "--nocapture"])
        .env(CHILD, &path).spawn().unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert_eq!(status.code(), Some(77));
            break;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("child did not reach crash boundary");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let database = Arc::new(crate::database::ServerDatabase::open(&path).await.unwrap());
        let units =
            crate::adapters::unit_of_work::for_sessions(database.clone(), Default::default(), None);
        let coordinator = Coordinator::new(units.clone(), database.generation().unwrap());
        let before = units
            .begin()
            .await
            .unwrap()
            .get_run("crashed")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(before.status(), RunStatus::Running);
        coordinator.recover().await.unwrap();
        let after = units
            .begin()
            .await
            .unwrap()
            .get_run("crashed")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            after.status(),
            RunStatus::Indeterminate(IndeterminateReason::Interrupted)
        );
        assert_eq!(before.identity(), after.identity());
        assert!(coordinator.claim(&"crashed".into()).await.is_err());
        database.close().await.unwrap();
    });
}

#[tokio::test]
async fn saving_a_session_cannot_reset_its_run_order() {
    let (_, units, coordinator) = fixture().await;
    let session = units
        .begin()
        .await
        .unwrap()
        .get_session("session")
        .await
        .unwrap()
        .unwrap();
    let first = proceed(
        coordinator
            .prepare("session", None, "code", &"first".into(), None, "mcp")
            .await,
    );
    first.release().await;
    let mut unit = units.begin().await.unwrap();
    unit.save_session(session).await.unwrap();
    unit.commit().await.unwrap();
    let second = proceed(
        coordinator
            .prepare("session", None, "code", &"second".into(), None, "mcp")
            .await,
    );
    second.release().await;
    let mut unit = units.begin().await.unwrap();
    assert_eq!(
        unit.get_run("first")
            .await
            .unwrap()
            .unwrap()
            .identity()
            .admission_order,
        1
    );
    assert_eq!(
        unit.get_run("second")
            .await
            .unwrap()
            .unwrap()
            .identity()
            .admission_order,
        2
    );
}

#[tokio::test]
async fn failed_run_insertion_rolls_back_the_session_counter() {
    let (database, units, coordinator) = fixture().await;
    database.transaction(|connection| Box::pin(async move {
        sqlx::query("CREATE TRIGGER reject_run BEFORE INSERT ON runs BEGIN SELECT RAISE(FAIL, 'injected'); END")
            .execute(connection).await?;
        Ok(())
    })).await.unwrap();
    assert!(matches!(
        coordinator
            .prepare("session", None, "code", &"failed".into(), None, "mcp")
            .await,
        Reservation::Refused(_)
    ));
    database
        .transaction(|connection| {
            Box::pin(async move {
                let order: i64 = sqlx::query_scalar(
                    "SELECT last_run_order FROM sessions WHERE session_id='session'",
                )
                .fetch_one(&mut *connection)
                .await?;
                assert_eq!(order, 0);
                sqlx::query("DROP TRIGGER reject_run")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .await
        .unwrap();
    let healthy = proceed(
        coordinator
            .prepare("session", None, "code", &"healthy".into(), None, "mcp")
            .await,
    );
    healthy.release().await;
    let mut unit = units.begin().await.unwrap();
    assert!(unit.get_run("failed").await.unwrap().is_none());
    assert_eq!(
        unit.get_run("healthy")
            .await
            .unwrap()
            .unwrap()
            .identity()
            .admission_order,
        1
    );
}

#[tokio::test]
async fn exhausted_run_order_refuses_admission_without_overflow() {
    let (database, units, coordinator) = fixture().await;
    database
        .transaction(|connection| {
            Box::pin(async move {
                sqlx::query("UPDATE sessions SET last_run_order=? WHERE session_id='session'")
                    .bind(i64::MAX - 1)
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .await
        .unwrap();
    let last = proceed(
        coordinator
            .prepare("session", None, "code", &"last".into(), None, "mcp")
            .await,
    );
    last.release().await;
    assert!(matches!(
        coordinator
            .prepare("session", None, "code", &"overflow".into(), None, "mcp")
            .await,
        Reservation::Refused(_)
    ));
    let mut unit = units.begin().await.unwrap();
    assert_eq!(
        unit.get_run("last")
            .await
            .unwrap()
            .unwrap()
            .identity()
            .admission_order,
        i64::MAX
    );
    assert!(unit.get_run("overflow").await.unwrap().is_none());
    drop(unit);
    database
        .read(|connection| {
            Box::pin(async move {
                let order: i64 = sqlx::query_scalar(
                    "SELECT last_run_order FROM sessions WHERE session_id='session'",
                )
                .fetch_one(connection)
                .await?;
                assert_eq!(order, i64::MAX);
                Ok(())
            })
        })
        .await
        .unwrap();
}
