//! A session's last run is written with its run's end, in the same unit.
use super::fault_tests::{CommitFault, FaultFactory};
use super::tests::{fixture_with_database, proceed};
use super::*;
use crate::adapters::session::last_run::{INLINE_BYTES, LastRuns, StoredLastRun};
use crate::application::sessions::close::CloseSession;
use crate::database::ServerDatabase;
use crate::domain::idempotent_request::RequestState;
use crate::domain::session::ClosedReason;
use crate::session::LastRun;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

struct Audit;
impl crate::application::sessions::ports::AuditLog for Audit {
    fn record(&self, _: &str, _: crate::application::sessions::ports::SessionEvent) {}
}

struct Fixture {
    _dir: tempfile::TempDir,
    database: Arc<ServerDatabase>,
    units: Arc<dyn UnitOfWorkFactory>,
    fault: Arc<CommitFault>,
    last_runs: Arc<LastRuns>,
    coordinator: Arc<Coordinator>,
}

impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let database = Arc::new(
            ServerDatabase::open(&dir.path().join("server.db"))
                .await
                .unwrap(),
        );
        let (database, units, _) = fixture_with_database(database).await;
        let fault = Arc::new(CommitFault::default());
        let units: Arc<dyn UnitOfWorkFactory> = Arc::new(FaultFactory {
            inner: units,
            fault: fault.clone(),
        });
        let last_runs = Arc::new(LastRuns::new(database.clone(), None, 1 << 20));
        let coordinator = Arc::new(
            Coordinator::new(units.clone(), database.generation().unwrap())
                .with_last_runs(last_runs.clone()),
        );
        Self {
            _dir: dir,
            database,
            units,
            fault,
            last_runs,
            coordinator,
        }
    }

    /// Run `id` unkeyed to its end, with `last_run` as what it produced.
    async fn finish(&self, id: &str, last_run: Option<&LastRun>) {
        self.finish_with(id, last_run, false).await;
    }

    /// [`Self::finish`], losing the acknowledgement of the end's commit.
    async fn finish_losing_ack(&self, id: &str, last_run: Option<&LastRun>) {
        self.finish_with(id, last_run, true).await;
        assert!(
            !self.fault.lose_ack.load(Ordering::SeqCst),
            "the end's commit lost its acknowledgement"
        );
    }

    async fn finish_with(&self, id: &str, last_run: Option<&LastRun>, lose_ack: bool) {
        let mut guard = proceed(
            self.coordinator
                .prepare("session", None, "code", &id.into(), None, "mcp")
                .await,
        );
        self.coordinator.claim(&id.into()).await.unwrap();
        if let Some(last_run) = last_run {
            guard.set_last_run(last_run);
        }
        self.fault.lose_ack.store(lose_ack, Ordering::SeqCst);
        guard.finish_unkeyed().await;
    }

    async fn last_result(&self) -> Option<String> {
        self.last_runs
            .get("session")
            .await
            .unwrap()
            .map(|stored| stored.value()["result"].as_str().unwrap().to_owned())
    }

    async fn state(&self, id: &str) -> RequestState {
        let mut unit = self.units.begin().await.unwrap();
        unit.get_run(id).await.unwrap().unwrap().state().clone()
    }

    /// The session's last-run files, once every run's end has settled.
    async fn files(&self) -> Vec<PathBuf> {
        self.coordinator.drain().await;
        let mut root = self.database.path().as_os_str().to_owned();
        root.push("-last-runs");
        let dir = Path::new(&root).join("session");
        match std::fs::read_dir(dir) {
            Ok(entries) => entries.map(|entry| entry.unwrap().path()).collect(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => panic!("{error}"),
        }
    }

    async fn sql(&self, statement: &'static str) {
        self.database
            .transaction(move |connection| {
                Box::pin(async move {
                    sqlx::query(statement).execute(connection).await?;
                    Ok(())
                })
            })
            .await
            .unwrap();
    }
}

fn run(result: &str) -> LastRun {
    LastRun {
        result: Some(result.into()),
        console: vec!["line".into()],
        error: None,
    }
}

fn large(result: &str) -> LastRun {
    run(&result.repeat(INLINE_BYTES))
}

#[tokio::test]
async fn a_run_that_ends_records_the_session_last_run() {
    let fixture = Fixture::new().await;
    assert_eq!(fixture.last_result().await, None);
    fixture.finish("unkeyed", Some(&run("first"))).await;
    assert_eq!(
        fixture.last_runs.get("session").await.unwrap(),
        Some(StoredLastRun::Json(run("first").encode().unwrap()))
    );
    let mut guard = proceed(fixture.coordinator.reserve("session", "key", "code").await);
    guard.set_last_run(&run("keyed"));
    guard.complete(200, "body".into()).await;
    assert_eq!(fixture.last_result().await.as_deref(), Some("keyed"));
}

#[tokio::test]
async fn a_run_that_never_dispatched_leaves_the_last_run_before() {
    let fixture = Fixture::new().await;
    fixture.finish("first", Some(&run("first"))).await;
    let mut guard = proceed(
        fixture
            .coordinator
            .prepare("session", None, "code", &"released".into(), None, "mcp")
            .await,
    );
    guard.set_last_run(&run("never ran"));
    guard.release().await;
    assert_eq!(fixture.last_result().await.as_deref(), Some("first"));
}

#[tokio::test]
async fn a_run_whose_end_is_unknown_leaves_no_last_run() {
    let fixture = Fixture::new().await;
    fixture.finish("first", Some(&run("first"))).await;
    let mut guard = proceed(fixture.coordinator.reserve("session", "key", "code").await);
    let run_id = guard.reservation_id.clone();
    fixture.coordinator.claim(&run_id).await.unwrap();
    guard.set_last_run(&run("unknown"));
    drop(guard);
    fixture.coordinator.drain().await;
    assert_eq!(fixture.last_result().await, None);
}

#[tokio::test]
async fn a_reservation_dropped_before_its_run_started_leaves_the_last_run_before() {
    let fixture = Fixture::new().await;
    fixture.finish("first", Some(&run("first"))).await;
    let guard = proceed(fixture.coordinator.reserve("session", "key", "code").await);
    drop(guard);
    fixture.coordinator.drain().await;
    assert_eq!(fixture.last_result().await.as_deref(), Some("first"));
}

#[tokio::test]
async fn a_large_last_run_replaces_the_file_before() {
    let fixture = Fixture::new().await;
    fixture.finish("first", Some(&large("a"))).await;
    fixture.finish("second", Some(&large("b"))).await;
    assert_eq!(fixture.files().await.len(), 1);
    assert_eq!(
        fixture.last_runs.get("session").await.unwrap(),
        Some(StoredLastRun::Json(large("b").encode().unwrap()))
    );
}

/// Closing a session cancels its runs, so a run that ends after it finds
/// itself already ended and writes nothing.
#[tokio::test]
async fn a_run_cancelled_by_its_session_closing_brings_nothing_back() {
    let fixture = Fixture::new().await;
    let mut guard = proceed(
        fixture
            .coordinator
            .prepare("session", None, "code", &"late".into(), None, "mcp")
            .await,
    );
    fixture.coordinator.claim(&"late".into()).await.unwrap();
    assert!(
        CloseSession::new(fixture.units.as_ref(), &Audit)
            .execute("session", ClosedReason::Deleted)
            .await
            .unwrap()
    );
    guard.set_last_run(&large("late"));
    guard.finish_unkeyed().await;
    assert_eq!(fixture.last_runs.get("session").await.unwrap(), None);
    assert!(
        fixture.files().await.is_empty(),
        "the staged file is no one's"
    );
}

#[tokio::test]
async fn a_last_run_that_cannot_be_written_leaves_the_run_unknown_and_no_file() {
    let fixture = Fixture::new().await;
    fixture.finish("first", Some(&run("first"))).await;
    fixture
        .sql(
            "CREATE TRIGGER fail_last_run BEFORE INSERT ON last_runs \
             BEGIN SELECT RAISE(FAIL, 'injected'); END",
        )
        .await;
    fixture.finish("failed", Some(&large("failed"))).await;
    assert!(fixture.files().await.is_empty());
    assert_eq!(fixture.state("failed").await, RequestState::Indeterminate);
    assert_eq!(fixture.last_result().await, None, "first is not the latest");
    fixture.sql("DROP TRIGGER fail_last_run").await;
    fixture.finish("recovered", Some(&run("recovered"))).await;
    assert_eq!(fixture.last_result().await.as_deref(), Some("recovered"));
}

#[tokio::test]
async fn a_lost_acknowledgement_still_releases_the_file_it_replaced() {
    let fixture = Fixture::new().await;
    fixture.finish("first", Some(&large("a"))).await;
    fixture.finish_losing_ack("second", Some(&large("b"))).await;
    assert_eq!(fixture.files().await.len(), 1);
    assert_eq!(
        fixture.last_runs.get("session").await.unwrap(),
        Some(StoredLastRun::Json(large("b").encode().unwrap()))
    );
}

#[tokio::test]
async fn a_run_cut_off_by_a_restart_leaves_no_last_run() {
    let fixture = Fixture::new().await;
    fixture.finish("first", Some(&run("first"))).await;
    let _guard = proceed(
        fixture
            .coordinator
            .prepare("session", None, "code", &"cut-off".into(), None, "mcp")
            .await,
    );
    fixture.coordinator.claim(&"cut-off".into()).await.unwrap();
    crate::application::sessions::runs::RecoverRuns::new(fixture.units.as_ref())
        .execute("a later generation")
        .await
        .unwrap();
    assert_eq!(fixture.state("cut-off").await, RequestState::Indeterminate);
    assert_eq!(fixture.last_result().await, None);
}

#[tokio::test]
async fn a_run_that_started_and_then_failed_to_settle_leaves_no_last_run() {
    let fixture = Fixture::new().await;
    fixture.finish("first", Some(&run("first"))).await;
    let mut guard = proceed(
        fixture
            .coordinator
            .prepare("session", None, "code", &"started".into(), None, "mcp")
            .await,
    );
    fixture.coordinator.claim(&"started".into()).await.unwrap();
    guard.set_last_run(&run("started"));
    guard.release().await;
    assert_eq!(fixture.state("started").await, RequestState::Indeterminate);
    assert_eq!(fixture.last_result().await, None);
}

/// Another run's end can come between this run's look at the session's last
/// run and its own end, so the end itself reports the file it replaced.
#[tokio::test]
async fn a_run_end_reports_the_file_it_replaced() {
    use crate::application::sessions::ports::LastRunStaging;
    let fixture = Fixture::new().await;
    fixture.finish("first", Some(&large("first"))).await;
    let replaced = fixture.last_runs.spill_file("session").await.unwrap();
    assert!(replaced.is_some());
    let _guard = proceed(
        fixture
            .coordinator
            .prepare("session", None, "code", &"second".into(), None, "mcp")
            .await,
    );
    fixture.coordinator.claim(&"second".into()).await.unwrap();
    let request = fixture
        .units
        .begin()
        .await
        .unwrap()
        .get_run("second")
        .await
        .unwrap()
        .unwrap();
    let staged = fixture
        .last_runs
        .stage("session", large("second").encode().unwrap())
        .await
        .unwrap();
    let reported = crate::application::sessions::runs::ResolveRun::new(fixture.units.clone())
        .execute(&request, &RunResolution::Finished, Some(&staged))
        .await
        .unwrap();
    assert_eq!(reported, replaced);
}
