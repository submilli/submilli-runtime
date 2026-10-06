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
/// after its version was logged.
#[derive(Default)]
struct Flaky {
    inner: InMemoryBlueprintStore,
    fail: AtomicBool,
}

#[async_trait::async_trait]
impl BlueprintStore for Flaky {
    async fn add_yaml(&self, stored: StoredBlueprint) -> Result<(), StoreError> {
        self.inner.add_yaml(stored).await
    }
    async fn upsert_yaml(&self, stored: StoredBlueprint) -> Result<bool, StoreError> {
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
    applier: Applier,
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
            applier,
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
    let applier = Arc::new(fixture.applier);
    let _watch = watch(Arc::clone(&applier), Duration::from_millis(50)).unwrap();
    // Some watcher backends start delivering only after a short delay.
    tokio::time::sleep(Duration::from_millis(200)).await;
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
