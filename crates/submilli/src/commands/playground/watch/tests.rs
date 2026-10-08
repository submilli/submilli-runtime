use std::sync::atomic::{AtomicBool, Ordering};

use submilli_server::ServerConfig;
use submilli_server::blueprint::{
    BlueprintStore, InMemoryBlueprintStore, StoreError, StoredBlueprint,
};

use super::*;
use crate::commands::playground::store::changes::WindowStart;

const PINNED: &str = "\
name: demo
variables:
  customerId:
    required: true
permissions:
  main:
    - name: charges-for-signed-in-customer
      capability: test.com/charges
      filter: customerId == ${vars.customerId} and amount < 500
      action: allow
";

/// An in-memory store whose writes can be made to fail, for an apply that fails
/// after its version was logged, or held, for an apply still in flight.
struct Flaky {
    inner: InMemoryBlueprintStore,
    fail: AtomicBool,
    /// The next write announces itself on `entered` and waits for a `release` permit.
    hold_next: AtomicBool,
    entered: tokio::sync::Notify,
    release: tokio::sync::Semaphore,
}

impl Default for Flaky {
    fn default() -> Self {
        Self {
            inner: InMemoryBlueprintStore::default(),
            fail: AtomicBool::new(false),
            hold_next: AtomicBool::new(false),
            entered: tokio::sync::Notify::new(),
            release: tokio::sync::Semaphore::new(0),
        }
    }
}

#[async_trait::async_trait]
impl BlueprintStore for Flaky {
    async fn add_yaml(&self, stored: StoredBlueprint) -> Result<(), StoreError> {
        self.inner.add_yaml(stored).await
    }
    async fn upsert_yaml(&self, stored: StoredBlueprint) -> Result<bool, StoreError> {
        if self.hold_next.swap(false, Ordering::SeqCst) {
            self.entered.notify_one();
            if let Ok(permit) = self.release.acquire().await {
                permit.forget();
            }
        }
        if self.fail.load(Ordering::SeqCst) {
            return Err(StoreError::Io("disk full".into()));
        }
        self.inner.upsert_yaml(stored).await
    }
    async fn list(&self) -> Result<Vec<String>, StoreError> {
        self.inner.list().await
    }
    async fn list_blueprints(&self) -> Result<Vec<Blueprint>, StoreError> {
        self.inner.list_blueprints().await
    }
    async fn get(&self, name: &str) -> Result<Option<Blueprint>, StoreError> {
        self.inner.get(name).await
    }
    async fn get_yaml(&self, name: &str) -> Result<Option<String>, StoreError> {
        self.inner.get_yaml(name).await
    }
    async fn remove(&self, name: &str) -> Result<bool, StoreError> {
        self.inner.remove(name).await
    }
}

struct Fixture {
    dir: tempfile::TempDir,
    blueprints: Arc<Flaky>,
    store: Arc<Store>,
    applier: Arc<Applier>,
}

impl Fixture {
    async fn new(yaml: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("demo.yaml"), yaml).unwrap();
        let store = Arc::new(Store::open(&dir.path().join("store")).unwrap());
        let (blueprints, applier) = Self::serve(dir.path(), &store);
        applier.start().await.expect("the first version applies");
        Self {
            dir,
            blueprints,
            store,
            applier: Arc::new(applier),
        }
    }

    /// A fresh server over the same project, as after a restart.
    fn serve(dir: &Path, store: &Arc<Store>) -> (Arc<Flaky>, Applier) {
        let blueprints = Arc::new(Flaky::default());
        let state = AppState::new(ServerConfig {
            blueprints: Some(blueprints.clone()),
            session_storage_root: Some(dir.join("sessions")),
            managed_volume_root: Some(dir.join("volumes")),
            package_store_root: Some(dir.join("packages")),
            ..ServerConfig::default()
        })
        .unwrap();
        let applier = Applier::new(
            state,
            Arc::clone(store),
            dir.join("demo.yaml"),
            "demo".into(),
        );
        (blueprints, applier)
    }

    async fn save(&self, yaml: &str) -> Outcome {
        std::fs::write(self.dir.path().join("demo.yaml"), yaml).unwrap();
        self.applier.apply_file().await
    }

    fn versions(&self) -> Vec<u64> {
        self.store
            .changes()
            .unwrap()
            .versions
            .iter()
            .map(|v| v.version)
            .collect()
    }

    fn current(&self) -> crate::commands::playground::store::changes::Version {
        self.store.changes().unwrap().current().cloned().unwrap()
    }

    async fn registered(&self) -> Blueprint {
        self.blueprints.get("demo").await.unwrap().unwrap()
    }

    fn status(&self) -> BlueprintStatus {
        self.applier.status().lock().unwrap().clone()
    }
}

#[tokio::test]
async fn the_first_version_is_logged_and_applied_at_start() {
    let fixture = Fixture::new(PINNED).await;
    assert_eq!(fixture.versions(), [1]);
    let current = fixture.current();
    assert_eq!(current.bytes, PINNED);
    assert_eq!(current.classification["classification"], "initial");
    assert_eq!(fixture.status().version, Some(1));
    assert_eq!(fixture.registered().await.name, "demo");
}

// Covers AE5, R36, R37: applied with no approval, classified, and the pin flagged.
#[tokio::test]
async fn removing_the_pin_is_applied_as_a_widening_with_the_pin_flagged() {
    let fixture = Fixture::new(PINNED).await;
    let edited = PINNED.replace("customerId == ${vars.customerId} and ", "");
    let Outcome::Applied {
        version,
        diff: Some(diff),
    } = fixture.save(&edited).await
    else {
        panic!("applied");
    };
    assert_eq!(version, 2);
    assert_eq!(diff.pin_removals().count(), 1);
    let current = fixture.current();
    assert_eq!(current.version, 2);
    assert_eq!(current.classification["classification"], "widening");
    assert_eq!(
        current.classification["changes"][0]["pin"]["kind"],
        "removed"
    );
    assert!(
        current.summary.contains("no longer pins `customerId`"),
        "{}",
        current.summary
    );
    let registered = fixture.registered().await;
    let rule = &registered.permissions["main"][0];
    assert_eq!(
        rule.filter.as_ref().map(ToString::to_string).as_deref(),
        Some("amount < 500")
    );
}

#[tokio::test]
async fn a_comment_only_edit_updates_the_current_versions_bytes() {
    let fixture = Fixture::new(PINNED).await;
    let window = fixture.store.changes().unwrap().audit_window();
    let commented = format!("# who may see charges\n{PINNED}  # end\n");
    assert!(matches!(
        fixture.save(&commented).await,
        Outcome::BytesUpdated { version: 1 }
    ));
    assert_eq!(fixture.versions(), [1]);
    assert_eq!(fixture.current().bytes, commented);
    // The audit window does not move.
    assert_eq!(fixture.store.changes().unwrap().audit_window(), window);
    // Saving the same text again is no change at all.
    assert!(matches!(fixture.save(&commented).await, Outcome::Unchanged));
}

#[tokio::test]
async fn returning_to_an_earlier_text_is_a_new_version() {
    let fixture = Fixture::new(PINNED).await;
    let edited = PINNED.replace("amount < 500", "amount < 100");
    assert!(matches!(
        fixture.save(&edited).await,
        Outcome::Applied { version: 2, .. }
    ));
    assert!(matches!(
        fixture.save(PINNED).await,
        Outcome::Applied { version: 3, .. }
    ));
    let changes = fixture.store.changes().unwrap();
    assert_eq!(changes.versions[0].hash, changes.versions[2].hash);
    assert_eq!(changes.audit_window().1, WindowStart::Version(3));
    // Widening back from 100 to 500 is not one of the classified shapes.
    assert_eq!(
        changes.versions[2].classification["classification"],
        "unknown"
    );
}

// Covers AE8.
#[tokio::test]
async fn invalid_yaml_is_reported_against_its_line_and_the_last_good_version_stays() {
    let fixture = Fixture::new(PINNED).await;
    let broken = format!("{PINNED}    - capability: [unclosed\n");
    let Outcome::Refused(refusal) = fixture.save(&broken).await else {
        panic!("refused");
    };
    assert_eq!(refusal.code, "parse_error");
    assert!(refusal.line.is_some_and(|line| line >= 11), "{refusal:?}");
    assert_eq!(fixture.versions(), [1]);
    assert_eq!(fixture.status().version, Some(1));
    assert_eq!(fixture.status().refused, Some(refusal));
    assert_eq!(
        fixture.registered().await,
        submilli_blueprint::parse(PINNED).unwrap()
    );

    // Fixing it clears the report.
    fixture.save(PINNED).await;
    assert_eq!(fixture.status().refused, None);
}

#[tokio::test]
async fn changing_the_name_is_refused_and_the_last_good_version_stays() {
    let fixture = Fixture::new(PINNED).await;
    let renamed = PINNED.replace("name: demo", "name: other");
    let Outcome::Refused(refusal) = fixture.save(&renamed).await else {
        panic!("refused");
    };
    assert_eq!(refusal.code, "name_changed");
    assert!(refusal.message.contains("`demo` to `other`"), "{refusal:?}");
    assert_eq!(fixture.versions(), [1]);
    assert!(fixture.blueprints.get("other").await.unwrap().is_none());
}

#[tokio::test]
async fn a_filter_on_a_field_the_capability_lacks_is_refused_and_nothing_is_logged() {
    let fixture = Fixture::new(PINNED).await;
    let bad = format!(
        "{PINNED}    - capability: http.get\n      filter: nosuchfield == \"x\"\n      action: allow\n"
    );
    let Outcome::Refused(refusal) = fixture.save(&bad).await else {
        panic!("refused");
    };
    assert_eq!(refusal.code, "invalid_filter");
    assert_eq!(fixture.versions(), [1]);
    assert_eq!(
        fixture.registered().await,
        submilli_blueprint::parse(PINNED).unwrap()
    );
}

#[tokio::test]
async fn an_apply_that_fails_after_logging_voids_its_version() {
    let fixture = Fixture::new(PINNED).await;
    fixture.blueprints.fail.store(true, Ordering::SeqCst);
    let edited = PINNED.replace("amount < 500", "amount < 100");
    let Outcome::ApplyFailed { version, reason } = fixture.save(&edited).await else {
        panic!("apply failed");
    };
    assert_eq!(version, 2);
    assert!(reason.contains("disk full"), "{reason}");
    let changes = fixture.store.changes().unwrap();
    assert_eq!(changes.voided, [2]);
    // Readers ignore the voided version: version 1 is still current and starts the window.
    assert_eq!(changes.current().map(|v| v.version), Some(1));
    assert_eq!(changes.audit_window().1, WindowStart::Version(1));
    assert_eq!(fixture.status().version, Some(1));

    // The next good save is version 3: the voided number is not reused.
    fixture.blueprints.fail.store(false, Ordering::SeqCst);
    assert!(matches!(
        fixture.save(&edited).await,
        Outcome::Applied { version: 3, .. }
    ));
    assert_eq!(fixture.versions(), [1, 3]);
}

#[tokio::test]
async fn a_new_required_variable_is_applied_and_flagged_in_its_version() {
    let fixture = Fixture::new(PINNED).await;
    let edited = PINNED.replace(
        "    required: true\n",
        "    required: true\n  region:\n    required: true\n",
    );
    assert!(matches!(
        fixture.save(&edited).await,
        Outcome::Applied { version: 2, .. }
    ));
    let current = fixture.current();
    assert_eq!(
        current.classification["new_required_variables"],
        json!(["region"])
    );
    assert!(
        current
            .summary
            .contains("sessions must now bind variable `region`"),
        "{}",
        current.summary
    );
}

#[tokio::test]
async fn a_restart_with_the_file_unchanged_logs_no_new_version() {
    let fixture = Fixture::new(PINNED).await;
    let edited = PINNED.replace("amount < 500", "amount < 100");
    fixture.save(&edited).await;
    let (blueprints, applier) = Fixture::serve(fixture.dir.path(), &fixture.store);
    applier.start().await.unwrap();
    assert_eq!(fixture.versions(), [1, 2]);
    assert_eq!(applier.status().lock().unwrap().version, Some(2));
    assert_eq!(
        blueprints.get("demo").await.unwrap(),
        Some(submilli_blueprint::parse(&edited).unwrap())
    );
}

#[tokio::test]
async fn a_restart_whose_registration_fails_fails_the_start_and_keeps_the_version() {
    let fixture = Fixture::new(PINNED).await;
    let (blueprints, applier) = Fixture::serve(fixture.dir.path(), &fixture.store);
    blueprints.fail.store(true, Ordering::SeqCst);
    let Outcome::RegisterFailed { version, reason } = applier.apply_file_at(Moment::Start).await
    else {
        panic!("registering again failed");
    };
    assert_eq!(version, 1);
    assert!(reason.contains("disk full"), "{reason}");
    // The version applied before and runs may record it: it is not voided.
    let changes = fixture.store.changes().unwrap();
    assert!(changes.voided.is_empty(), "{:?}", changes.voided);
    assert_eq!(changes.current().map(|v| v.version), Some(1));
    assert_eq!(applier.status().lock().unwrap().version, None);

    let error = applier.start().await.unwrap_err().to_string();
    assert!(error.contains("disk full"), "{error}");
    assert!(fixture.store.changes().unwrap().voided.is_empty());
}

#[tokio::test]
async fn a_start_on_a_refused_file_fails_with_its_line() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("demo.yaml"), "name: demo\nbad: [\n").unwrap();
    let store = Arc::new(Store::open(&dir.path().join("store")).unwrap());
    let (_, applier) = Fixture::serve(dir.path(), &store);
    let error = applier.start().await.unwrap_err().to_string();
    assert!(error.contains("line "), "{error}");
    assert!(store.changes().unwrap().versions.is_empty());
}

#[tokio::test]
async fn a_save_by_rename_is_picked_up_by_the_watcher() {
    let fixture = Fixture::new(PINNED).await;
    let applier = Arc::clone(&fixture.applier);
    let _watch = watching(&fixture, Duration::from_millis(50)).await;
    let edited = PINNED.replace("amount < 500", "amount < 100");
    let temp = fixture.dir.path().join(".demo.yaml.tmp");
    std::fs::write(&temp, &edited).unwrap();
    std::fs::rename(&temp, fixture.dir.path().join("demo.yaml")).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while applier.status().lock().unwrap().version != Some(2) {
        assert!(
            std::time::Instant::now() < deadline,
            "the rename-save was not picked up"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(
        fixture.store.changes().unwrap().current().unwrap().bytes,
        edited
    );
}

/// The watcher over `fixture`'s file, once it delivers events.
async fn watching(fixture: &Fixture, debounce: Duration) -> Watch {
    let watch = watch(Arc::clone(&fixture.applier), debounce).unwrap();
    // Some watcher backends start delivering only after a short delay.
    tokio::time::sleep(Duration::from_millis(200)).await;
    watch
}

/// Waits up to 20 seconds for `done` to hold of the applier's status.
async fn wait_until(applier: &Applier, what: &str, done: impl Fn(&BlueprintStatus) -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        let status = applier.status().lock().unwrap().clone();
        if done(&status) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}: {status:?}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[tokio::test]
async fn a_blueprint_deleted_then_recreated_is_refused_then_applied() {
    let fixture = Fixture::new(PINNED).await;
    let applier = Arc::clone(&fixture.applier);
    let _watch = watching(&fixture, Duration::from_millis(50)).await;
    let file = fixture.dir.path().join("demo.yaml");
    std::fs::remove_file(&file).unwrap();
    wait_until(&applier, "the deletion's refusal", |status| {
        status
            .refused
            .as_ref()
            .is_some_and(|refusal| refusal.code == "unreadable")
    })
    .await;
    assert_eq!(fixture.versions(), [1]);
    assert_eq!(applier.status().lock().unwrap().version, Some(1));

    let edited = PINNED.replace("amount < 500", "amount < 100");
    std::fs::write(&file, &edited).unwrap();
    wait_until(&applier, "version 2", |status| {
        status.version == Some(2) && status.refused.is_none()
    })
    .await;
    assert_eq!(fixture.current().bytes, edited);
}

#[tokio::test]
async fn two_saves_within_one_debounce_are_one_version_with_the_last_text() {
    let fixture = Fixture::new(PINNED).await;
    let applier = Arc::clone(&fixture.applier);
    let _watch = watching(&fixture, Duration::from_millis(400)).await;
    let file = fixture.dir.path().join("demo.yaml");
    std::fs::write(&file, PINNED.replace("amount < 500", "amount < 100")).unwrap();
    let last = PINNED.replace("amount < 500", "amount < 200");
    std::fs::write(&file, &last).unwrap();
    wait_until(&applier, "version 2", |status| status.version == Some(2)).await;
    // Long enough for a second look, had the saves not been coalesced.
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(fixture.versions(), [1, 2]);
    assert_eq!(fixture.current().bytes, last);
}

#[tokio::test]
async fn a_save_during_an_apply_is_applied_after_it() {
    let fixture = Fixture::new(PINNED).await;
    let applier = Arc::clone(&fixture.applier);
    let _watch = watching(&fixture, Duration::from_millis(50)).await;
    let file = fixture.dir.path().join("demo.yaml");
    fixture.blueprints.hold_next.store(true, Ordering::SeqCst);
    std::fs::write(&file, PINNED.replace("amount < 500", "amount < 100")).unwrap();
    tokio::time::timeout(
        Duration::from_secs(20),
        fixture.blueprints.entered.notified(),
    )
    .await
    .expect("the first save's apply started");

    let last = PINNED.replace("amount < 500", "amount < 200");
    std::fs::write(&file, &last).unwrap();
    // Let the second save settle while the first apply is still held.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(fixture.versions(), [1, 2]);
    fixture.blueprints.release.add_permits(1);

    wait_until(&applier, "version 3", |status| status.version == Some(3)).await;
    assert_eq!(fixture.current().bytes, last);
    assert_eq!(
        fixture.registered().await,
        submilli_blueprint::parse(&last).unwrap()
    );
}

// An explicit `default: deny` reads as a different blueprint from an absent default,
// so it is a new version; the classifier finds nothing that changes what programs may
// do, so the version is `unknown` and says so.
#[tokio::test]
async fn an_explicit_default_deny_is_a_version_with_no_change_to_access() {
    let fixture = Fixture::new(PINNED).await;
    let explicit = PINNED.replace("name: demo\n", "name: demo\ndefault: deny\n");
    assert!(matches!(
        fixture.save(&explicit).await,
        Outcome::Applied { version: 2, .. }
    ));
    let current = fixture.current();
    assert_eq!(current.classification["classification"], "unknown");
    assert_eq!(current.classification["changes"], json!([]));
    assert_eq!(current.summary, "No change to what programs may do.");
}

const WITH_PACKAGE: &str = "\
name: demo
packages:
- '@acme/billing'
permissions:
  main:
    - capability: acme.com/charges.list
      filter: customerId == \"cus_a\"
      action: allow
  '@acme/billing':
    - capability: fs.read
      filter: path == \"/billing/charges.json\"
      action: allow
";

#[tokio::test]
async fn a_save_during_a_slow_package_check_is_applied_after_it() {
    let dir = tempfile::tempdir().unwrap();
    crate::commands::playground::scaffold::init(dir.path()).unwrap();
    std::fs::write(dir.path().join("demo.yaml"), WITH_PACKAGE).unwrap();
    let store = Arc::new(Store::open(&dir.path().join("store")).unwrap());
    let (blueprints, applier) = Fixture::serve(dir.path(), &store);
    let packages = Arc::new(super::super::packages::ProjectPackages::new(
        &dir.path().join("submilli"),
        dir.path().join("packages"),
    ));
    let freshness = Arc::new(Freshness::new(packages));
    let applier = Arc::new(applier.with_packages(Arc::clone(&freshness)));
    applier.start().await.expect("the first version applies");
    let fixture = Fixture {
        dir,
        blueprints,
        store,
        applier: Arc::clone(&applier),
    };
    let _watch = watching(&fixture, Duration::from_millis(50)).await;

    // A package check that does not finish until released, as a slow build would.
    let (started, started_rx) = tokio::sync::oneshot::channel();
    let (release, release_rx) = std::sync::mpsc::channel::<()>();
    let slow = tokio::spawn({
        let freshness = Arc::clone(&freshness);
        async move {
            freshness
                .run_check(move |_| {
                    let _ = started.send(());
                    let _ = release_rx.recv();
                    Ok(Default::default())
                })
                .await
        }
    });
    started_rx.await.expect("the slow check started");

    let file = fixture.dir.path().join("demo.yaml");
    std::fs::write(&file, WITH_PACKAGE.replace("cus_a", "cus_b")).unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let last = WITH_PACKAGE.replace("cus_a", "cus_c");
    std::fs::write(&file, &last).unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        fixture.versions(),
        [1],
        "nothing applies while the check runs"
    );
    release.send(()).unwrap();
    slow.await.unwrap().unwrap();

    wait_until(&applier, "the last save", |status| {
        status.refused.is_none() && status.version.is_some_and(|version| version > 1)
    })
    .await;
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while fixture.current().bytes != last {
        assert!(
            std::time::Instant::now() < deadline,
            "the last save was not applied: {:?}",
            fixture.versions()
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(fixture.versions().len() <= 3, "{:?}", fixture.versions());
    assert_eq!(
        fixture.registered().await,
        submilli_blueprint::parse(&last).unwrap()
    );
}
