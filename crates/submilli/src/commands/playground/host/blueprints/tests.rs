use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::*;

const V1: &str = "\
name: demo
permissions:
  main:
    - name: ok
      capability: test.com/ok
      action: allow
";

/// Declared volumes by name, with `managed-local` ones stored under a root.
struct Volumes {
    table: Mutex<BTreeMap<String, Access>>,
    managed_root: PathBuf,
}

impl Volumes {
    fn declared(&self) -> Vec<String> {
        self.table.lock().unwrap().keys().cloned().collect()
    }

    fn case_conflict(volume: &str, existing: &str) -> String {
        format!("volume '{volume}' differs only in letter case from '{existing}'")
    }
}

impl LocalVolumes for Volumes {
    fn is_declared(&self, volume: &str) -> bool {
        self.table.lock().unwrap().contains_key(volume)
    }

    fn check_reference(
        &self,
        volume: &str,
        requested: Option<Access>,
    ) -> Result<(), VolumeProblem> {
        match self.table.lock().unwrap().get(volume) {
            None => Err(VolumeProblem::Undeclared(format!(
                "volume '{volume}' is not declared"
            ))),
            Some(access) if requested.is_some_and(|requested| requested > *access) => Err(
                VolumeProblem::AccessExceeds(format!("volume '{volume}' is read_only")),
            ),
            Some(_) => Ok(()),
        }
    }

    fn check_managed(&self, volume: &str) -> Result<(), String> {
        let table = self.table.lock().unwrap();
        if table.contains_key(volume) {
            return Ok(());
        }
        let on_disk = std::fs::read_dir(&self.managed_root)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok());
        for existing in table.keys().cloned().chain(on_disk) {
            if existing != volume && existing.eq_ignore_ascii_case(volume) {
                return Err(Self::case_conflict(volume, &existing));
            }
        }
        Ok(())
    }

    fn declare_managed(&self, volume: &str) -> Result<bool, String> {
        let mut table = self.table.lock().unwrap();
        if table.contains_key(volume) {
            return Ok(false);
        }
        if let Some(existing) = table.keys().find(|name| name.eq_ignore_ascii_case(volume)) {
            return Err(Self::case_conflict(volume, existing));
        }
        table.insert(volume.to_owned(), Access::ReadWrite);
        Ok(true)
    }

    fn withdraw_managed(&self, volume: &str) {
        self.table.lock().unwrap().remove(volume);
    }
}

struct Host {
    blueprints: Blueprints,
    volumes: Arc<Volumes>,
    _dirs: tempfile::TempDir,
}

impl Host {
    fn new() -> Self {
        let dirs = tempfile::tempdir().unwrap();
        let volumes = Arc::new(Volumes {
            table: Mutex::default(),
            managed_root: dirs.path().join("managed-local"),
        });
        let blueprints = Blueprints::new(
            "demo".to_owned(),
            Arc::clone(&volumes) as _,
            None,
            PackageStore::new(dirs.path().join("packages")),
        );
        Self {
            blueprints,
            volumes,
            _dirs: dirs,
        }
    }

    fn managed_root(&self) -> &Path {
        &self.volumes.managed_root
    }

    async fn apply(&self, yaml: &str, tag: &str) -> LocalApplied {
        self.blueprints
            .apply(yaml, tag)
            .await
            .unwrap_or_else(|error| panic!("apply {tag}: {error:?}"))
    }

    async fn refused(&self, yaml: &str, tag: &str) -> LocalApplyError {
        self.blueprints.apply(yaml, tag).await.expect_err("refused")
    }

    async fn version_in_force(&self) -> Option<String> {
        self.blueprints
            .for_run()
            .await
            .map(|applied| applied.version_tag)
    }
}

fn with_volume(volume: &str) -> String {
    with_volumes(&[volume])
}

fn with_volumes(volumes: &[&str]) -> String {
    let mounts: String = volumes
        .iter()
        .enumerate()
        .map(|(i, volume)| format!("    /m{i}: {{mode: named, volume: {volume}}}\n"))
        .collect();
    format!(
        "{V1}    - capability: fs.write\n      action: allow\nvfs:\n  mode: per_session\n  mounts:\n{mounts}"
    )
}

#[tokio::test]
async fn a_run_reads_the_blueprint_with_the_version_tag_it_was_applied_with() {
    let host = Host::new();
    assert!(host.blueprints.for_run().await.is_none());
    let applied = host.apply(V1, "v1").await;
    assert_eq!(applied.name, "demo");
    assert!(applied.created);
    assert_eq!(host.version_in_force().await.as_deref(), Some("v1"));

    let v2 = V1.replace("name: ok", "name: still-ok");
    assert!(!host.apply(&v2, "v2").await.created);
    let found = host.blueprints.for_run().await.unwrap();
    assert_eq!(found.version_tag, "v2");
    assert_eq!(*found.blueprint, submilli_blueprint::parse(&v2).unwrap());
}

#[tokio::test]
async fn a_session_opened_before_an_apply_runs_next_under_the_new_blueprint() {
    let host = Host::new();
    let v1 = "\
name: demo
variables:
  customerId:
    required: true
permissions:
  main:
    - name: own-charges
      capability: test.com/charges
      filter: customerId == ${vars.customerId}
      action: allow
";
    host.apply(v1, "v1").await;
    // What the session holds from its first run.
    let opened_under = host.blueprints.for_run().await.unwrap();

    let v2 = v1.replace(
        "    required: true\n",
        "    required: true\n  region:\n    required: true\n",
    );
    host.apply(&v2, "v2").await;

    // Its next run looks the blueprint up again.
    let next = host.blueprints.for_run().await.unwrap();
    assert_eq!(opened_under.version_tag, "v1");
    assert_eq!(next.version_tag, "v2");
    assert!(next.blueprint.variables.contains_key("region"));
}

#[tokio::test]
async fn a_filter_on_a_field_the_capability_lacks_is_refused_and_the_last_good_version_stays() {
    let host = Host::new();
    host.apply(V1, "v1").await;
    let bad = format!(
        "{V1}    - capability: http.get\n      filter: nosuchfield == \"x\"\n      action: allow\n"
    );
    let error = host.refused(&bad, "v2").await;
    assert_eq!(error.code, "invalid_filter", "{error:?}");
    assert!(error.message.contains("nosuchfield"), "{error:?}");
    assert_eq!(host.version_in_force().await.as_deref(), Some("v1"));
}

#[tokio::test]
async fn invalid_yaml_is_refused_against_its_line_and_column() {
    let host = Host::new();
    host.apply(V1, "v1").await;
    let broken = "name: demo\npermissions:\n  main:\n    - capability: [unclosed\n";
    let error = host.refused(broken, "v2").await;
    assert_eq!(error.code, "parse_error");
    let first = error.diagnostics.first().expect("a diagnostic");
    assert!(matches!(first.line, Some(4 | 5)), "{error:?}");
    assert!(first.col.is_some(), "{error:?}");
    // Checking alone changes nothing either.
    assert!(host.blueprints.check(broken).await.is_err());
    assert_eq!(host.version_in_force().await.as_deref(), Some("v1"));
}

#[tokio::test]
async fn a_refused_apply_leaves_the_version_in_force() {
    let host = Host::new();
    host.apply("name: demo\n", "v1").await;
    host.refused("name: demo\nnot_a_key: 1\n", "v2").await;
    assert_eq!(host.version_in_force().await.as_deref(), Some("v1"));
}

#[tokio::test]
async fn a_blueprint_whose_name_changed_is_refused() {
    let host = Host::new();
    host.apply(V1, "v1").await;
    let renamed = V1.replace("name: demo", "name: other");
    let error = host.refused(&renamed, "v2").await;
    assert_eq!(error.code, "name_changed", "{error:?}");
    assert!(error.message.contains("`demo`"), "{error:?}");
    assert!(host.blueprints.check(&renamed).await.is_err());
    assert_eq!(host.version_in_force().await.as_deref(), Some("v1"));
}

#[tokio::test]
async fn a_newly_named_volume_is_declared_managed_local_without_a_restart() {
    let host = Host::new();
    host.apply(V1, "v1").await;
    let applied = host.apply(&with_volume("notes"), "v2").await;
    assert_eq!(applied.declared_volumes, ["notes"]);
    assert_eq!(host.volumes.declared(), ["notes"]);
    assert_eq!(host.version_in_force().await.as_deref(), Some("v2"));

    // Applying again declares nothing new.
    assert!(
        host.apply(&with_volume("notes"), "v3")
            .await
            .declared_volumes
            .is_empty()
    );
}

#[tokio::test]
async fn checking_a_newly_named_volume_declares_nothing() {
    let host = Host::new();
    host.blueprints.check(&with_volume("notes")).await.unwrap();
    assert!(host.volumes.declared().is_empty());
}

#[tokio::test]
async fn a_volume_name_that_cannot_be_a_directory_is_refused() {
    let host = Host::new();
    // The parser takes any non-empty name; a managed volume's must be a directory name.
    let yaml = "name: demo\nvfs:\n  mode: named\n  volume: '-notes'\n";
    assert!(submilli_blueprint::parse(yaml).is_ok());
    let error = host.refused(yaml, "v1").await;
    assert_eq!(error.code, "undeclared_volume", "{error:?}");
    assert!(host.volumes.declared().is_empty());
    assert!(!host.managed_root().join("-notes").exists());
}

#[tokio::test]
async fn a_later_declaration_that_fails_withdraws_the_earlier_ones() {
    let host = Host::new();
    host.apply(V1, "v1").await;
    // Each passes the check alone; declaring `notes` first makes `Notes` a case
    // conflict.
    let error = host.refused(&with_volumes(&["notes", "Notes"]), "v2").await;
    assert_eq!(error.code, "undeclared_volume", "{error:?}");
    assert!(host.volumes.declared().is_empty());
    assert_eq!(host.version_in_force().await.as_deref(), Some("v1"));
    let applied = host.apply(&with_volume("notes"), "v3").await;
    assert_eq!(applied.declared_volumes, ["notes"]);
}

#[tokio::test]
async fn a_volume_name_differing_only_in_case_from_a_stored_one_is_refused() {
    let host = Host::new();
    host.apply(&with_volume("notes"), "v1").await;
    for yaml in [with_volume("Notes"), with_volume("NOTES")] {
        let error = host.blueprints.check(&yaml).await.expect_err("refused");
        assert_eq!(error.code, "volume_name_conflict", "{error:?}");
        assert!(error.message.contains("'notes'"), "{error:?}");
        let error = host.refused(&yaml, "v2").await;
        assert_eq!(error.code, "volume_name_conflict", "{error:?}");
    }

    // A directory an earlier run left under the managed root counts too.
    let fresh = Host::new();
    std::fs::create_dir_all(fresh.managed_root().join("Ledger")).unwrap();
    let error = fresh.refused(&with_volume("ledger"), "v1").await;
    assert_eq!(error.code, "volume_name_conflict", "{error:?}");
    assert!(error.message.contains("'Ledger'"), "{error:?}");
    // The same name in the same case is that directory's volume.
    let applied = fresh.apply(&with_volume("Ledger"), "v2").await;
    assert_eq!(applied.declared_volumes, ["Ledger"]);
}

#[tokio::test]
async fn a_read_only_volume_asked_for_read_write_is_refused() {
    let host = Host::new();
    host.volumes
        .table
        .lock()
        .unwrap()
        .insert("shared".to_owned(), Access::ReadOnly);
    let yaml = "name: demo\nvfs:\n  mode: named\n  volume: shared\n  access: read_write\n";
    let error = host.refused(yaml, "v1").await;
    assert_eq!(error.code, "volume_access_exceeded", "{error:?}");
}

#[tokio::test]
async fn an_uninstalled_package_is_refused() {
    let host = Host::new();
    let error = host
        .refused(&format!("{V1}packages:\n  - '@acme/tools'\n"), "v1")
        .await;
    assert_eq!(error.code, "package_missing", "{error:?}");
}
