use super::tests::{fixture, proceed};
use super::*;
use crate::domain::idempotent_request::RequestState;

#[tokio::test]
async fn unkeyed_invocations_have_distinct_ordered_identity_and_no_response_body() {
    let (_, units, coordinator) = fixture().await;
    let (a, b) = tokio::join!(
        coordinator.prepare("session", None, "same", "run-a", Some("alice"), "mcp"),
        coordinator.prepare("session", None, "same", "run-b", Some("alice"), "http"),
    );
    let a = proceed(a);
    let b = proceed(b);
    coordinator.claim("run-a").await.unwrap();
    assert!(
        coordinator.claim("run-a").await.is_err(),
        "a claim is not a second launch permit"
    );
    a.finish_unkeyed().await;
    coordinator.claim("run-b").await.unwrap();
    b.finish_unkeyed().await;
    let mut unit = units.begin().await.unwrap();
    let a = unit.get_run("run-a").await.unwrap().unwrap();
    let b = unit.get_run("run-b").await.unwrap().unwrap();
    assert_eq!(a.state(), &RequestState::Finished);
    assert_eq!(b.state(), &RequestState::Finished);
    assert_ne!(a.identity().admission_order, b.identity().admission_order);
    assert_eq!(a.identity().origin_node, b.identity().origin_node);
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
            .prepare("session", Some("key"), "code", "run", None, "http")
            .await,
    );
    coordinator.claim("run").await.unwrap();
    guard.release().await;
    let run = units
        .begin()
        .await
        .unwrap()
        .get_run("run")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.state(), &RequestState::Indeterminate);
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
            .prepare("session", Some("key"), "code", "run", None, "http")
            .await,
    );
    coordinator.claim("run").await.unwrap();
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
    stale
        .complete(
            "run",
            RecordedOutcome {
                status: 200,
                body: "late".into(),
            },
            SystemTime::now(),
        )
        .unwrap();
    let mut unit = units.begin().await.unwrap();
    assert!(unit.save_run(stale).await.is_err());
    drop(unit);
    guard.complete(200, "late".into()).await;
    let mut unit = units.begin().await.unwrap();
    assert_eq!(
        unit.get_run("run").await.unwrap().unwrap().state(),
        &RequestState::Cancelled
    );
    drop(unit);
    assert!(matches!(
        coordinator
            .prepare("session", None, "code", "later", None, "mcp")
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
                    .prepare("session", None, "code", "crashed", None, "mcp")
                    .await,
            );
            coordinator.claim("crashed").await.unwrap();
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
        assert_eq!(before.state(), &RequestState::Running);
        coordinator.recover().await.unwrap();
        let after = units
            .begin()
            .await
            .unwrap()
            .get_run("crashed")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(after.state(), &RequestState::Indeterminate);
        assert_eq!(before.identity(), after.identity());
        assert!(coordinator.claim("crashed").await.is_err());
        database.close().await.unwrap();
    });
}
