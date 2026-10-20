use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::*;
use crate::commands::playground::scaffold;

/// The starter project in a temporary directory, with a store of its own.
struct Fixture {
    _dir: tempfile::TempDir,
    package_dir: PathBuf,
    store: PathBuf,
}

impl Fixture {
    fn starter() -> Self {
        let dir = tempfile::tempdir().unwrap();
        scaffold::init(dir.path()).unwrap();
        Self {
            package_dir: dir.path().join("submilli"),
            store: dir.path().join("store"),
            _dir: dir,
        }
    }

    fn packages(&self) -> ProjectPackages {
        ProjectPackages::new(&self.package_dir, self.store.clone())
    }

    fn lib(&self) -> PathBuf {
        self.package_dir.join("packages/billing/src/lib.ts")
    }

    fn installed_source(&self) -> String {
        let installed =
            read_installed_sources(self.store.join("@pied-piper/billing")).expect("installed");
        installed.sources[0].text.clone()
    }

    fn blueprint(&self) -> Blueprint {
        let yaml =
            std::fs::read_to_string(self.package_dir.join("blueprints/billing.yaml")).unwrap();
        submilli_blueprint::parse(&yaml).unwrap()
    }
}

fn billing() -> BTreeSet<String> {
    BTreeSet::from(["@pied-piper/billing".to_owned()])
}

fn edit(path: &Path, from: &str, to: &str) {
    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.contains(from), "{from} is in {}", path.display());
    std::fs::write(path, text.replace(from, to)).unwrap();
}

#[test]
fn a_project_package_is_installed_once_and_reinstalled_only_after_an_edit() {
    let fixture = Fixture::starter();
    let packages = fixture.packages();
    let first = packages.sync(&billing()).unwrap();
    assert_eq!(first.reinstalled, ["@pied-piper/billing"]);
    assert!(first.evict);

    let again = packages.sync(&billing()).unwrap();
    assert_eq!(again, Synced::default(), "nothing changed");

    edit(
        &fixture.lib(),
        "found.push(charge);",
        "found.push(charge); // edited",
    );
    let edited = packages.sync(&billing()).unwrap();
    assert_eq!(edited.reinstalled, ["@pied-piper/billing"]);
    assert!(fixture.installed_source().contains("// edited"));
}

#[test]
fn a_run_that_reaches_no_project_package_checks_nothing() {
    let fixture = Fixture::starter();
    let packages = fixture.packages();
    let none = packages
        .sync(&BTreeSet::from(["@other/pkg".to_owned()]))
        .unwrap();
    assert_eq!(none, Synced::default());
    assert!(!fixture.store.join("@pied-piper/billing").exists());
}

#[test]
fn a_package_that_no_longer_builds_is_reported_and_its_installed_copy_is_kept() {
    let fixture = Fixture::starter();
    let packages = fixture.packages();
    packages.sync(&billing()).unwrap();
    let before = fixture.installed_source();

    edit(&fixture.lib(), "return found;", "return found +;");
    let failure = packages.sync(&billing()).unwrap_err();
    let ResolutionFailure::Build {
        package,
        diagnostic,
        ..
    } = &failure
    else {
        panic!("a build failure: {failure}");
    };
    assert_eq!(package, "@pied-piper/billing");
    assert!(diagnostic.contains("lib.ts"), "{diagnostic}");
    let message = failure.to_string();
    assert!(message.contains("`@pied-piper/billing`"), "{message}");
    assert!(message.contains("does not build"), "{message}");
    // Only a run refused before it started says so.
    assert!(!message.contains("the run"), "{message}");
    let for_run = failure.for_run();
    assert!(for_run.contains("no longer builds"), "{for_run}");
    assert!(for_run.contains("the run did not start"), "{for_run}");
    assert_eq!(
        fixture.installed_source(),
        before,
        "the old copy is untouched"
    );

    // The same broken source reports the same failure.
    let again = packages.sync(&billing()).unwrap_err();
    assert_eq!(again.to_string(), message);

    // Fixed: the next check builds and installs it.
    edit(&fixture.lib(), "return found +;", "return found; // fixed");
    assert_eq!(
        packages.sync(&billing()).unwrap().reinstalled,
        ["@pied-piper/billing"]
    );
}

#[test]
fn an_install_from_outside_is_noticed_by_the_next_check() {
    let fixture = Fixture::starter();
    let packages = fixture.packages();
    packages.sync(&billing()).unwrap();
    // Another install rewrites the artifact; its contents still match the source.
    let metadata = fixture.store.join("@pied-piper/billing/metadata.json");
    let text = std::fs::read(&metadata).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&metadata, text).unwrap();
    let synced = packages.sync(&billing()).unwrap();
    assert!(synced.reinstalled.is_empty());
    assert!(synced.evict, "the server's cached copy may be the old one");
}

#[test]
fn the_closure_marks_a_dependency_only_package_not_importable() {
    let fixture = Fixture::starter();
    let money = fixture.package_dir.join("packages/money");
    std::fs::create_dir_all(money.join("src")).unwrap();
    std::fs::create_dir_all(money.join("docs")).unwrap();
    std::fs::write(
        money.join("src/lib.ts"),
        "/** Cents as dollars. */\nexport function dollars(cents: number): number { return cents / 100; }\n",
    )
    .unwrap();
    std::fs::write(money.join("docs/readme.md"), "# @pied-piper/money\n").unwrap();
    let manifest = fixture.package_dir.join("submilli.toml");
    let mut text = std::fs::read_to_string(&manifest).unwrap();
    text.push_str("dependencies = [\"@pied-piper/money\"]\n\n[[package]]\nname = \"@pied-piper/money\"\nversion = \"0.1.0\"\ndescription = \"Money helpers.\"\npath = \"packages/money\"\n");
    std::fs::write(&manifest, text).unwrap();

    let (synced, closure) = fixture.packages().prepare(&fixture.blueprint()).unwrap();
    assert_eq!(synced.reinstalled.len(), 2, "{:?}", synced.reinstalled);
    assert_eq!(
        closure,
        [
            ClosureEntry {
                name: "@pied-piper/billing".into(),
                version: "0.1.0".into(),
                origin: Origin::Blueprint,
                importable: true,
                project: true,
                required_by: Vec::new(),
            },
            ClosureEntry {
                name: "@pied-piper/money".into(),
                version: "0.1.0".into(),
                origin: Origin::Dependency,
                importable: false,
                project: true,
                required_by: vec!["@pied-piper/billing".into()],
            },
        ]
    );
}

#[test]
fn a_missing_package_names_itself_and_the_install_command() {
    let fixture = Fixture::starter();
    let mut blueprint = fixture.blueprint();
    blueprint.packages.insert("@pied-piper/absent".to_owned());
    let failure = fixture.packages().prepare(&blueprint).unwrap_err();
    assert!(matches!(
        failure,
        ResolutionFailure::Store(PackageStoreError::MissingPackage { ref name, .. })
            if name == "@pied-piper/absent"
    ));
    let message = failure.to_string();
    assert!(message.contains("`@pied-piper/absent`"), "{message}");
    assert!(message.contains("submilli install"), "{message}");
    assert!(!message.contains("denied"), "{message}");
}

/// Builds `@pied-piper/money` in a project of its own and installs it into `store`, as
/// `submilli build publish-local` there would.
fn publish_money(store: &Path) {
    let dir = tempfile::tempdir().unwrap();
    let package = dir.path().join("packages/money");
    std::fs::create_dir_all(package.join("src")).unwrap();
    std::fs::create_dir_all(package.join("docs")).unwrap();
    std::fs::write(
        package.join("src/lib.ts"),
        "/** Cents as dollars. */\nexport function dollars(cents: number): number { return cents / 100; }\n",
    )
    .unwrap();
    std::fs::write(package.join("docs/readme.md"), "# @pied-piper/money\n").unwrap();
    let text = "[[package]]\nname = \"@pied-piper/money\"\nversion = \"0.1.0\"\ndescription = \"Money helpers.\"\npath = \"packages/money\"\n";
    std::fs::write(dir.path().join("submilli.toml"), text).unwrap();
    let manifest = parse_manifest(text, dir.path()).expect("the money manifest parses");
    let store = PackageStore::new(store);
    let built =
        submilli_build::build_packages(&manifest, dir.path(), &store, None).expect("money builds");
    install_packages(&store, &built).expect("money installs");
}

/// Makes billing's source import `@pied-piper/money` from the store.
fn import_money(fixture: &Fixture) {
    let mut text = std::fs::read_to_string(fixture.lib()).unwrap();
    text.insert_str(0, "import { dollars } from \"@pied-piper/money\";\n");
    text.push_str(
        "\n/** Cents as dollars, through the money package. */\nexport function inDollars(cents: number): number { return dollars(cents); }\n",
    );
    std::fs::write(fixture.lib(), text).unwrap();
}

const MONEY_DEPENDENCY: &str =
    "dependencies = [\"@pied-piper/money\"]\n\n[dependencies]\n\"@pied-piper/money\" = \"0.1.0\"\n";

fn declare_money(fixture: &Fixture) {
    let manifest = fixture.package_dir.join("submilli.toml");
    let mut text = std::fs::read_to_string(&manifest).unwrap();
    text.push_str(MONEY_DEPENDENCY);
    std::fs::write(&manifest, text).unwrap();
}

#[test]
fn a_build_fixed_in_the_manifest_alone_is_retried() {
    let fixture = Fixture::starter();
    publish_money(&fixture.store);
    import_money(&fixture);
    let packages = fixture.packages();
    let failure = packages.sync(&billing()).unwrap_err();
    assert!(
        matches!(failure, ResolutionFailure::Build { .. }),
        "{failure}"
    );
    // The same inputs report the remembered failure.
    assert!(packages.sync(&billing()).is_err());

    // Only `submilli.toml` changes: the source already imports the package.
    declare_money(&fixture);
    assert_eq!(
        packages.sync(&billing()).unwrap().reinstalled,
        ["@pied-piper/billing"]
    );
}

#[test]
fn installing_a_missing_dependency_retries_the_failed_build() {
    let fixture = Fixture::starter();
    import_money(&fixture);
    declare_money(&fixture);
    let packages = fixture.packages();
    let failure = packages.sync(&billing()).unwrap_err();
    let ResolutionFailure::Build { diagnostic, .. } = &failure else {
        panic!("a build failure: {failure}");
    };
    assert!(diagnostic.contains("@pied-piper/money"), "{diagnostic}");
    assert!(packages.sync(&billing()).is_err());

    publish_money(&fixture.store);
    assert_eq!(
        packages.sync(&billing()).unwrap().reinstalled,
        ["@pied-piper/billing"]
    );
}

#[test]
fn an_edited_manifest_entry_rebuilds_its_package() {
    let fixture = Fixture::starter();
    let packages = fixture.packages();
    packages.sync(&billing()).unwrap();

    // The keywords the installed copy carries no longer match the entry.
    let manifest = fixture.package_dir.join("submilli.toml");
    edit(&manifest, "\"charges\"]", "\"charges\", \"ledger\"]");
    assert_eq!(
        packages.sync(&billing()).unwrap().reinstalled,
        ["@pied-piper/billing"]
    );
    assert_eq!(packages.sync(&billing()).unwrap(), Synced::default());

    // The entrypoint moves from `lib.ts` to `lib.subm` with the same text: the entry
    // changed, though the embedded sources did not.
    let legacy = fixture.package_dir.join("packages/billing/src/lib.subm");
    std::fs::rename(fixture.lib(), &legacy).unwrap();
    assert_eq!(
        packages.sync(&billing()).unwrap().reinstalled,
        ["@pied-piper/billing"]
    );
    assert_eq!(packages.sync(&billing()).unwrap(), Synced::default());
}

#[tokio::test]
async fn a_check_whose_caller_stops_waiting_still_finishes_before_the_next_starts() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    let fixture = Fixture::starter();
    let freshness = Freshness::new(Arc::new(fixture.packages()));
    let building = Arc::new(AtomicBool::new(false));
    let (started, started_rx) = tokio::sync::oneshot::channel();
    let first = freshness.run_check({
        let building = Arc::clone(&building);
        move |_| {
            building.store(true, Ordering::SeqCst);
            let _ = started.send(());
            // A slow build.
            std::thread::sleep(Duration::from_millis(500));
            building.store(false, Ordering::SeqCst);
            Ok(Synced::default())
        }
    });
    // Wait until the first build runs, then stop waiting for it, as a run whose
    // client went away would.
    tokio::select! {
        _ = first => panic!("the first check finished before it was cancelled"),
        started = started_rx => started.expect("the first build started"),
    }

    let overlapped = tokio::time::timeout(
        Duration::from_secs(20),
        freshness.run_check({
            let building = Arc::clone(&building);
            move |_| {
                Ok(Synced {
                    reinstalled: Vec::new(),
                    evict: building.load(Ordering::SeqCst),
                })
            }
        }),
    )
    .await
    .expect("the second check ran")
    .unwrap()
    .evict;
    assert!(
        !overlapped,
        "the second check ran while the first build did"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reinstall_whose_caller_stops_waiting_still_evicts_the_cached_package() {
    use std::time::Duration;

    use crate::commands::playground::host::app::{App, ProgramRun};
    use crate::commands::playground::host::run::RunHostConfig;

    let fixture = Fixture::starter();
    let money = fixture.package_dir.join("packages/money");
    std::fs::create_dir_all(money.join("src")).unwrap();
    std::fs::create_dir_all(money.join("docs")).unwrap();
    std::fs::write(
        money.join("src/lib.ts"),
        "/** The release. */\nexport function release(): string { return \"first\"; }\n",
    )
    .unwrap();
    std::fs::write(money.join("docs/readme.md"), "# @pied-piper/money\n").unwrap();
    let manifest = fixture.package_dir.join("submilli.toml");
    let mut text = std::fs::read_to_string(&manifest).unwrap();
    text.push_str("\n[[package]]\nname = \"@pied-piper/money\"\nversion = \"0.1.0\"\ndescription = \"Money helpers.\"\npath = \"packages/money\"\n");
    std::fs::write(&manifest, text).unwrap();
    let wanted = BTreeSet::from(["@pied-piper/money".to_owned()]);
    let packages = Arc::new(fixture.packages());
    assert!(packages.sync(&wanted).unwrap().evict);

    let project = tempfile::tempdir().unwrap();
    let config = RunHostConfig::new(submilli_build::PackageStore::new(fixture.store.clone()));
    let state = App::for_tests(project.path(), "demo", config).await;
    state
        .blueprints
        .apply(
            "name: demo\ndefault: allow\nvfs:\n  mode: none\npackages:\n- '@pied-piper/money'\n",
            "1",
        )
        .await
        .unwrap();
    let release = || async {
        let response = state
            .run_program(ProgramRun {
                label: "test".into(),
                blueprint: "demo".into(),
                code: "import { release } from \"@pied-piper/money\";\nfunction main(): string { return release(); }\n".into(),
                variables: Default::default(),
                secrets: Default::default(),
            })
            .await;
        assert!(response.error.is_none(), "{:?}", response.error);
        response.result.expect("main returned")
    };
    assert_eq!(release().await, "first", "the run caches the package");

    edit(&money.join("src/lib.ts"), "\"first\"", "\"second\"");
    let freshness = Freshness::new(Arc::clone(&packages));
    let (started, started_rx) = tokio::sync::oneshot::channel();
    let run = Arc::clone(&state.run);
    let check = freshness.check_evicting(move || run.clear_prepared_packages(), {
        let wanted = wanted.clone();
        move |packages| {
            let _ = started.send(());
            // A slow build, so the caller is gone before it finishes.
            std::thread::sleep(Duration::from_millis(300));
            packages.sync(&wanted)
        }
    });
    tokio::select! {
        _ = check => panic!("the check finished before it was cancelled"),
        started = started_rx => started.expect("the check started"),
    }
    // The next check takes its turn only after the cancelled one finished.
    freshness
        .run_check(|_| Ok(Synced::default()))
        .await
        .unwrap();
    assert_eq!(
        release().await,
        "second",
        "the cancelled check evicted the cached copy it replaced"
    );
}

#[test]
fn automatic_rebuilds_retain_explicit_package_configuration() {
    let fixture = Fixture::starter();
    let config = fixture.lib().parent().unwrap().join("config.ts");
    std::fs::write(
        &config,
        "export declare const serviceUrl: string; export const timeoutMs = 10;",
    )
    .unwrap();
    let text = std::fs::read_to_string(fixture.package_dir.join("submilli.toml")).unwrap();
    let manifest = parse_manifest(&text, &fixture.package_dir).unwrap();
    let store = PackageStore::new(&fixture.store);
    let selected = PackageName::new("@pied-piper/billing");
    let inputs = submilli_build::ConfigInputs::parse(
        &["serviceUrl=https://internal.test".into()],
        &[],
        true,
    )
    .unwrap();
    let built = submilli_build::build_packages_with_inputs(
        &manifest,
        &fixture.package_dir,
        &store,
        Some(&selected),
        &inputs,
    )
    .unwrap();
    install_packages(&store, &built).unwrap();
    let packages = fixture.packages();
    packages.sync(&billing()).unwrap();
    std::fs::write(
        &config,
        "export declare const serviceUrl: string; export const timeoutMs = 20;",
    )
    .unwrap();
    let result = packages.sync(&billing()).unwrap();
    assert_eq!(result.reinstalled, ["@pied-piper/billing"]);
    let artifact = store.load("@pied-piper/billing").unwrap();
    assert_eq!(artifact.metadata.config.overrides, inputs.values);
    assert_eq!(
        serde_json::to_value(&artifact.metadata.config.values).unwrap()["timeoutMs"],
        20.0
    );
}

#[test]
fn automatic_rebuild_ignores_unrelated_broken_artifacts() {
    let fixture = Fixture::starter();
    let manifest_path = fixture.package_dir.join("submilli.toml");
    let mut manifest = std::fs::read_to_string(&manifest_path).unwrap();
    manifest.push_str("\n[[package]]\nname = '@pied-piper/unrelated'\nversion = '1.0.0'\ndescription = 'Unrelated.'\npath = 'unrelated'\n");
    std::fs::write(&manifest_path, manifest).unwrap();
    std::fs::create_dir_all(fixture.package_dir.join("unrelated/src")).unwrap();
    std::fs::write(
        fixture.package_dir.join("unrelated/src/lib.ts"),
        "export function value(): number { return 1; }",
    )
    .unwrap();
    std::fs::create_dir_all(fixture.store.join("@pied-piper/unrelated")).unwrap();
    std::fs::write(
        fixture.store.join("@pied-piper/unrelated/metadata.json"),
        "broken",
    )
    .unwrap();
    let result = fixture.packages().sync(&billing()).unwrap();
    assert_eq!(result.reinstalled, ["@pied-piper/billing"]);
}
