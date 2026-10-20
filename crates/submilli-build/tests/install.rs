//! Integration tests for the network-free install core (`install_from_dir`):
//! the same path `submilli install` and `server install` run once the GitHub
//! tarball is on disk. Provenance, scope-vs-org enforcement, and the
//! re-install / `--upgrade` policy are exercised here without touching the
//! network.

use std::fs;
use std::path::Path;

use submilli_build::{
    GithubSource, InstallError, PackageSource, PackageStore, install_from_dir,
    read_package_artifact,
};
use tempfile::TempDir;

/// Lay down a single-package repo named `name` under a fresh temp dir.
fn write_repo(name: &str) -> TempDir {
    let repo = TempDir::new().expect("repo dir");
    let manifest = format!(
        "[[package]]\nname = \"{name}\"\nversion = \"0.1.0\"\ndescription = \"Test package.\"\n"
    );
    fs::write(repo.path().join("submilli.toml"), manifest).expect("write manifest");
    write(
        repo.path(),
        "src/lib.ts",
        "/** Say hello.\n * @returns One.\n */\nexport function hello(): number { return 1; }",
    );
    write(repo.path(), "docs/readme.md", "# Test\n");
    repo
}

fn write(root: &Path, relative: &str, text: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
    fs::write(path, text).expect("write file");
}

fn github(org: &str, repo: &str, sha: &str) -> PackageSource {
    PackageSource::Github(GithubSource {
        org: org.to_string(),
        repo: repo.to_string(),
        sha: sha.to_string(),
        source_hash: None,
    })
}

const SHA_A: &str = "0000000000000000000000000000000000000000";
const SHA_B: &str = "1111111111111111111111111111111111111111";

#[test]
fn installs_and_stamps_provenance() {
    let repo = write_repo("@pied-piper/widget");
    let store_root = TempDir::new().unwrap();
    let store = PackageStore::new(store_root.path());

    let report = install_from_dir(
        &store,
        repo.path(),
        None,
        &github("pied-piper", "widget", SHA_A),
        false,
    )
    .expect("install succeeds");

    assert_eq!(report.installed.len(), 1);
    assert_eq!(report.installed[0].name.as_str(), "@pied-piper/widget");
    assert!(report.up_to_date.is_empty());

    let dir = store.package_dir("@pied-piper/widget").unwrap();
    let artifact = read_package_artifact(&dir).expect("artifact readable");
    match artifact.metadata.source {
        Some(PackageSource::Github(gh)) => {
            assert_eq!(gh.org, "pied-piper");
            assert_eq!(gh.repo, "widget");
            assert_eq!(gh.sha, SHA_A);
        }
        other => panic!("expected GitHub provenance, got {other:?}"),
    }
}

#[test]
fn reinstall_at_same_sha_is_a_no_op() {
    let repo = write_repo("@pied-piper/widget");
    let store_root = TempDir::new().unwrap();
    let store = PackageStore::new(store_root.path());
    let source = github("pied-piper", "widget", SHA_A);

    install_from_dir(&store, repo.path(), None, &source, false).expect("first install");
    let report =
        install_from_dir(&store, repo.path(), None, &source, false).expect("second install");

    assert!(report.installed.is_empty());
    assert_eq!(report.up_to_date.len(), 1);
    assert_eq!(report.up_to_date[0].as_str(), "@pied-piper/widget");
}

#[test]
fn reinstall_at_new_sha_needs_upgrade() {
    let repo = write_repo("@pied-piper/widget");
    let store_root = TempDir::new().unwrap();
    let store = PackageStore::new(store_root.path());

    install_from_dir(
        &store,
        repo.path(),
        None,
        &github("pied-piper", "widget", SHA_A),
        false,
    )
    .expect("first install");

    let blocked = install_from_dir(
        &store,
        repo.path(),
        None,
        &github("pied-piper", "widget", SHA_B),
        false,
    );
    match blocked {
        Err(InstallError::Conflict { conflicts, .. }) => {
            assert_eq!(conflicts.len(), 1);
            assert_eq!(conflicts[0].name.as_str(), "@pied-piper/widget");
        }
        other => panic!("expected Conflict, got {other:?}"),
    }

    let upgraded = install_from_dir(
        &store,
        repo.path(),
        None,
        &github("pied-piper", "widget", SHA_B),
        true,
    )
    .expect("upgrade succeeds");
    assert_eq!(upgraded.installed.len(), 1);

    let dir = store.package_dir("@pied-piper/widget").unwrap();
    let artifact = read_package_artifact(&dir).unwrap();
    match artifact.metadata.source {
        Some(PackageSource::Github(gh)) => assert_eq!(gh.sha, SHA_B),
        other => panic!("expected GitHub provenance, got {other:?}"),
    }
}

#[test]
fn rejects_scope_that_is_not_the_source_org() {
    // Package claims `@other/widget` but the source org is `pied-piper`.
    let repo = write_repo("@other/widget");
    let store_root = TempDir::new().unwrap();
    let store = PackageStore::new(store_root.path());

    let result = install_from_dir(
        &store,
        repo.path(),
        None,
        &github("pied-piper", "widget", SHA_A),
        false,
    );
    match result {
        Err(InstallError::ScopeMismatch { org, packages, .. }) => {
            assert_eq!(org, "pied-piper");
            assert_eq!(packages.len(), 1);
            assert_eq!(packages[0].as_str(), "@other/widget");
        }
        other => panic!("expected ScopeMismatch, got {other:?}"),
    }

    let widget_wasm = store.package_dir("@other/widget").unwrap().join("pkg.wasm");
    assert!(
        !widget_wasm.exists(),
        "nothing should be written on scope mismatch"
    );
}

#[test]
fn missing_manifest_is_an_error() {
    let empty = TempDir::new().unwrap();
    let store_root = TempDir::new().unwrap();
    let store = PackageStore::new(store_root.path());
    let result = install_from_dir(
        &store,
        empty.path(),
        None,
        &github("pied-piper", "widget", SHA_A),
        false,
    );
    assert!(matches!(result, Err(InstallError::NoManifest { .. })));
}

#[test]
fn an_interrupted_install_is_replaced_rather_than_refused() {
    let repo = write_repo("@pied-piper/widget");
    let store_root = TempDir::new().unwrap();
    let store = PackageStore::new(store_root.path());
    // The directory exists, the artifact files do not: a write that died early.
    fs::create_dir_all(store.package_dir("@pied-piper/widget").unwrap()).unwrap();

    let report = install_from_dir(
        &store,
        repo.path(),
        None,
        &github("pied-piper", "widget", SHA_A),
        false,
    )
    .expect("install over the partial directory");

    assert_eq!(report.installed.len(), 1);
    assert!(read_package_artifact(store.package_dir("@pied-piper/widget").unwrap()).is_ok());
}

#[test]
fn a_fallback_copy_neither_conflicts_nor_satisfies() {
    let repo = write_repo("@pied-piper/widget");
    let fallback_root = TempDir::new().unwrap();
    let fallback = PackageStore::new(fallback_root.path());
    install_from_dir(
        &fallback,
        repo.path(),
        None,
        &github("pied-piper", "widget", SHA_A),
        false,
    )
    .expect("install into the fallback");

    // A layered store whose owned root is empty: the fallback copy is visible
    // to reads but is not this store's install.
    let owned_root = TempDir::new().unwrap();
    let store = PackageStore::new(owned_root.path()).with_fallback(fallback_root.path());
    assert!(
        store.load("@pied-piper/widget").is_ok(),
        "readable through the fallback"
    );

    let report = install_from_dir(
        &store,
        repo.path(),
        None,
        &github("pied-piper", "widget", SHA_B),
        false,
    )
    .expect("a different sha in the fallback is not a conflict");

    assert_eq!(report.installed.len(), 1, "installed into the owned root");
    assert!(report.up_to_date.is_empty());
    let owned = read_package_artifact(store.package_dir("@pied-piper/widget").unwrap()).unwrap();
    match owned.metadata.source {
        Some(PackageSource::Github(gh)) => assert_eq!(gh.sha, SHA_B),
        other => panic!("expected GitHub provenance, got {other:?}"),
    }
    let untouched =
        read_package_artifact(fallback.package_dir("@pied-piper/widget").unwrap()).unwrap();
    match untouched.metadata.source {
        Some(PackageSource::Github(gh)) => assert_eq!(gh.sha, SHA_A, "fallback copy is untouched"),
        other => panic!("expected GitHub provenance, got {other:?}"),
    }
}

#[test]
fn deny_warnings_preparation_preserves_store_and_reports_same_commit_warnings() {
    use submilli_build::InstallPreparation;
    let clean = write_repo("@pied-piper/dependency");
    let warned = write_repo("@pied-piper/warned");
    write(
        warned.path(),
        "src/lib.ts",
        "export function hello(): number { return 1; }\n",
    );
    write(
        warned.path(),
        "submilli.toml",
        "[dependencies]\n\"@pied-piper/dependency\" = \"0.1.0\"\n\n[[package]]\nname = \"@pied-piper/warned\"\nversion = \"0.1.0\"\ndescription = \"Warned package.\"\ndependencies = [\"@pied-piper/dependency\"]\n",
    );
    write(
        warned.path(),
        "src/lib.ts",
        "import { hello } from \"@pied-piper/dependency\";\nexport function answer(): number { return hello(); }\n",
    );
    let root = TempDir::new().unwrap();
    let store = PackageStore::new(root.path());
    let mut preparation = InstallPreparation::new(&store).unwrap();
    preparation
        .prepare_repo(
            clean.path(),
            None,
            &github("pied-piper", "dependency", SHA_A),
            false,
        )
        .unwrap();
    let report = preparation
        .prepare_repo(
            warned.path(),
            None,
            &github("pied-piper", "warned", SHA_A),
            false,
        )
        .unwrap();
    assert!(!report.warnings.is_empty());
    let error = preparation.publish(true).unwrap_err();
    assert!(matches!(error, InstallError::WarningsDenied { .. }));
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);

    install_from_dir(
        &store,
        clean.path(),
        None,
        &github("pied-piper", "dependency", SHA_A),
        false,
    )
    .unwrap();
    let permissive = install_from_dir(
        &store,
        warned.path(),
        None,
        &github("pied-piper", "warned", SHA_A),
        false,
    )
    .unwrap();
    assert!(!permissive.warnings.is_empty());
    let artifact = store.load_owned("@pied-piper/warned").unwrap();
    for sha in [SHA_A, SHA_B] {
        let mut preparation = InstallPreparation::new(&store).unwrap();
        let report = preparation
            .prepare_repo(
                warned.path(),
                None,
                &github("pied-piper", "warned", sha),
                true,
            )
            .unwrap();
        if sha == SHA_A {
            assert_eq!(report.up_to_date.len(), 1);
        }
        assert!(matches!(
            preparation.publish(true),
            Err(InstallError::WarningsDenied { .. })
        ));
        assert_eq!(
            store.load_owned("@pied-piper/warned").unwrap().wasm,
            artifact.wasm
        );
        assert_eq!(
            store
                .load_owned("@pied-piper/warned")
                .unwrap()
                .metadata
                .source,
            artifact.metadata.source
        );
    }
}

#[test]
fn deny_warnings_preparation_checks_overlapping_sibling_commits() {
    use submilli_build::InstallPreparation;
    let repo = write_repo("@pied-piper/shared");
    for (sha, upgrade, conflict) in [
        (SHA_A, false, false),
        (SHA_B, false, true),
        (SHA_B, true, false),
    ] {
        let root = TempDir::new().unwrap();
        let store = PackageStore::new(root.path());
        let mut preparation = InstallPreparation::new(&store).unwrap();
        preparation
            .prepare_repo(
                repo.path(),
                None,
                &github("pied-piper", "first", SHA_A),
                false,
            )
            .unwrap();
        let second = preparation.prepare_repo(
            repo.path(),
            None,
            &github("pied-piper", "second", sha),
            upgrade,
        );
        if conflict {
            assert!(matches!(second, Err(InstallError::Conflict { .. })));
            assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
        } else {
            let report = second.unwrap();
            assert_eq!(report.up_to_date.len(), usize::from(sha == SHA_A));
            preparation.publish(true).unwrap();
            let Some(PackageSource::Github(source)) = store
                .load_owned("@pied-piper/shared")
                .unwrap()
                .metadata
                .source
            else {
                panic!("GitHub provenance")
            };
            assert_eq!(source.sha, sha);
        }
    }
}

#[test]
fn configuration_is_compiled_persisted_and_reconfigured_at_the_same_commit() {
    use submilli_build::{ConfigInputs, InstallPreparation, PackageName};
    let repo = write_repo("@pied-piper/search");
    write(
        repo.path(),
        "src/config.ts",
        "export declare const baseUrl: string;\nexport const timeoutMs = 5000;\n",
    );
    write(
        repo.path(),
        "src/lib.ts",
        "import { baseUrl, timeoutMs } from './config';\n/** Read the endpoint.\n * @returns The configured endpoint.\n */\nexport function endpoint(): string { return baseUrl; }\n/** Read the timeout.\n * @returns Timeout.\n */\nexport function timeout(): number { return timeoutMs; }",
    );
    let root = TempDir::new().unwrap();
    let store = PackageStore::new(root.path());
    let source = github("pied-piper", "search", "aaaaaaaa");
    let only = PackageName::new("@pied-piper/search");
    let missing = install_from_dir(&store, repo.path(), Some(&only), &source, false).unwrap_err();
    assert!(matches!(
        missing,
        InstallError::ConfigurationRequired { .. }
    ));
    assert!(
        missing
            .configuration_command(false)
            .unwrap()
            .contains("--config 'baseUrl=<string>'")
    );
    assert!(
        missing
            .configuration_command(true)
            .unwrap()
            .starts_with("submilli server packages install")
    );
    assert!(store.load("@pied-piper/search").is_err());
    let inputs =
        ConfigInputs::parse(&["baseUrl=https://first.example/a?x=y".into()], &[], true).unwrap();
    let mut preparation = InstallPreparation::new(&store).unwrap();
    preparation
        .prepare_repo_configured(repo.path(), Some(&only), &source, false, &inputs)
        .unwrap();
    preparation.publish(false).unwrap();
    let first = store.load("@pied-piper/search").unwrap();
    assert_eq!(first.metadata.config.overrides, inputs.values);
    assert_eq!(
        first
            .sources
            .iter()
            .find(|module| module.path.as_str() == "config")
            .unwrap()
            .text,
        fs::read_to_string(repo.path().join("src/config.ts")).unwrap()
    );
    let unchanged = install_from_dir(&store, repo.path(), Some(&only), &source, false).unwrap();
    assert_eq!(unchanged.up_to_date, vec![only.clone()]);
    let inputs =
        ConfigInputs::parse(&["baseUrl=https://second.example".into()], &[], true).unwrap();
    let mut preparation = InstallPreparation::new(&store).unwrap();
    assert!(matches!(
        preparation.prepare_repo_configured(repo.path(), Some(&only), &source, false, &inputs),
        Err(InstallError::Conflict { .. })
    ));
    let mut preparation = InstallPreparation::new(&store).unwrap();
    preparation
        .prepare_repo_configured(repo.path(), Some(&only), &source, true, &inputs)
        .unwrap();
    preparation.publish(false).unwrap();
    let second = store.load("@pied-piper/search").unwrap();
    assert_ne!(first.wasm, second.wasm);
    assert_eq!(second.metadata.config.overrides, inputs.values);
    write(
        repo.path(),
        "src/config.ts",
        "export declare const baseUrl: string;\nexport const timeoutMs = 6000;\n",
    );
    install_from_dir(
        &store,
        repo.path(),
        Some(&only),
        &github("pied-piper", "search", "bbbbbbbb"),
        true,
    )
    .unwrap();
    let upgraded = store.load("@pied-piper/search").unwrap();
    assert_eq!(upgraded.metadata.config.overrides, inputs.values);
    assert_eq!(
        serde_json::to_value(&upgraded.metadata.config.values).unwrap()["timeoutMs"],
        6000.0
    );
}

#[test]
fn sibling_dependency_configuration_must_be_installed_separately() {
    use submilli_build::{ConfigInputs, InstallPreparation, PackageName};
    let repo = TempDir::new().unwrap();
    write(
        repo.path(),
        "submilli.toml",
        "[[package]]\nname = '@pied-piper/app'\nversion = '0.1.0'\ndescription = 'App.'\npath = 'app'\ndependencies = ['@pied-piper/service']\n[[package]]\nname = '@pied-piper/service'\nversion = '0.1.0'\ndescription = 'Service.'\npath = 'service'\n",
    );
    write(
        repo.path(),
        "app/src/lib.ts",
        "import { endpoint } from '@pied-piper/service';\n/** Endpoint.\n * @returns Endpoint.\n */\nexport function url(): string { return endpoint(); }",
    );
    write(
        repo.path(),
        "service/src/lib.ts",
        "import { baseUrl } from './config';\n/** Endpoint.\n * @returns Endpoint.\n */\nexport function endpoint(): string { return baseUrl; }",
    );
    write(
        repo.path(),
        "service/src/config.ts",
        "export declare const baseUrl: string;",
    );
    write(repo.path(), "app/docs/readme.md", "# App\n");
    write(repo.path(), "service/docs/readme.md", "# Service\n");
    let root = TempDir::new().unwrap();
    let store = PackageStore::new(root.path());
    let source = github("pied-piper", "both", SHA_A);
    let app = PackageName::new("@pied-piper/app");
    let dependency = PackageName::new("@pied-piper/service");
    let missing = install_from_dir(&store, repo.path(), Some(&app), &source, false).unwrap_err();
    assert!(
        matches!(&missing, InstallError::ConfigurationRequired { package, .. } if package == &dependency)
    );
    assert!(
        missing
            .configuration_command(false)
            .unwrap()
            .contains("@pied-piper/service --config")
    );
    assert!(
        store.locate_all().is_empty(),
        "failed preparation publishes nothing"
    );
    let inputs = ConfigInputs::parse(&["baseUrl=https://service.test".into()], &[], true).unwrap();
    let mut preparation = InstallPreparation::new(&store).unwrap();
    preparation
        .prepare_repo_configured(repo.path(), Some(&dependency), &source, false, &inputs)
        .unwrap();
    preparation.publish(false).unwrap();
    install_from_dir(&store, repo.path(), Some(&app), &source, false).unwrap();
    assert!(store.load("@pied-piper/app").is_ok());
    let manifest = submilli_build::load_manifest(&repo.path().join("submilli.toml")).unwrap();
    assert!(submilli_build::build_packages(&manifest, repo.path(), &store, Some(&app)).is_ok());
    assert!(
        matches!(
            submilli_build::build_packages(&manifest, repo.path(), &store, Some(&dependency)),
            Err(submilli_build::DriverError::MissingConfiguration { .. })
        ),
        "explicit local builds do not inherit the selected package's overrides"
    );
}

#[test]
fn configuration_resets_restore_defaults_and_reject_missing_required_values() {
    use submilli_build::{ConfigInputs, InstallPreparation, PackageName};
    let repo = write_repo("@pied-piper/search");
    write(
        repo.path(),
        "src/config.ts",
        "export const baseUrl = 'https://default.test'; export declare const tenant: string;",
    );
    let root = TempDir::new().unwrap();
    let store = PackageStore::new(root.path());
    let source = github("pied-piper", "search", SHA_A);
    let only = PackageName::new("@pied-piper/search");
    let inputs = ConfigInputs::parse(
        &[
            "baseUrl=https://override.test".into(),
            "tenant=pied-piper".into(),
        ],
        &[],
        true,
    )
    .unwrap();
    let mut preparation = InstallPreparation::new(&store).unwrap();
    preparation
        .prepare_repo_configured(repo.path(), Some(&only), &source, false, &inputs)
        .unwrap();
    preparation.publish(false).unwrap();
    let reset = ConfigInputs::parse(&[], &["baseUrl".into()], true).unwrap();
    let mut preparation = InstallPreparation::new(&store).unwrap();
    preparation
        .prepare_repo_configured(repo.path(), Some(&only), &source, true, &reset)
        .unwrap();
    preparation.publish(false).unwrap();
    let artifact = store.load("@pied-piper/search").unwrap();
    assert!(!artifact.metadata.config.overrides.contains_key("baseUrl"));
    assert_eq!(
        serde_json::to_value(&artifact.metadata.config.values).unwrap()["baseUrl"],
        "https://default.test"
    );
    let reset = ConfigInputs::parse(&[], &["tenant".into()], true).unwrap();
    let mut preparation = InstallPreparation::new(&store).unwrap();
    assert!(matches!(
        preparation.prepare_repo_configured(repo.path(), Some(&only), &source, true, &reset),
        Err(InstallError::ConfigurationRequired { .. })
    ));
    assert_eq!(
        store.load("@pied-piper/search").unwrap().metadata.config,
        artifact.metadata.config
    );
}

#[test]
fn selected_install_ignores_unrelated_broken_artifacts() {
    let repo = write_repo("@pied-piper/a");
    let mut manifest = fs::read_to_string(repo.path().join("submilli.toml")).unwrap();
    manifest.push_str("path = '.'\n");
    manifest.push_str("\n[[package]]\nname = '@pied-piper/b'\nversion = '0.1.0'\ndescription = 'Unrelated.'\npath = 'b'\n");
    write(repo.path(), "submilli.toml", &manifest);
    write(
        repo.path(),
        "b/src/lib.ts",
        "export function b(): number { return 1; }",
    );
    write(repo.path(), "b/docs/readme.md", "# B\n");
    let root = TempDir::new().unwrap();
    let store = PackageStore::new(root.path());
    write(
        root.path(),
        "@pied-piper/b/metadata.json",
        "broken metadata",
    );
    install_from_dir(
        &store,
        repo.path(),
        Some(&submilli_build::PackageName::new("@pied-piper/a")),
        &github("pied-piper", "both", SHA_A),
        false,
    )
    .unwrap();
    assert!(store.load("@pied-piper/a").is_ok());
}

#[test]
fn configuration_remediation_includes_upgrade_for_an_existing_installation() {
    let repo = write_repo("@pied-piper/service");
    let root = TempDir::new().unwrap();
    let store = PackageStore::new(root.path());
    install_from_dir(
        &store,
        repo.path(),
        None,
        &github("pied-piper", "service", SHA_A),
        false,
    )
    .unwrap();
    write(
        repo.path(),
        "src/config.ts",
        "export declare const endpoint: string;",
    );
    let error = install_from_dir(
        &store,
        repo.path(),
        Some(&submilli_build::PackageName::new("@pied-piper/service")),
        &github("pied-piper", "service", SHA_B),
        false,
    )
    .unwrap_err();
    assert!(
        error
            .configuration_command(false)
            .unwrap()
            .contains("--upgrade --config")
    );
    assert!(
        error
            .configuration_command(true)
            .unwrap()
            .contains("--upgrade --config")
    );
}

#[test]
fn existing_artifacts_without_configuration_metadata_remain_compatible() {
    let repo = write_repo("@pied-piper/service");
    let root = TempDir::new().unwrap();
    let store = PackageStore::new(root.path());
    let source = github("pied-piper", "service", SHA_A);
    install_from_dir(&store, repo.path(), None, &source, false).unwrap();
    let path = store
        .package_dir("@pied-piper/service")
        .unwrap()
        .join("metadata.json");
    let mut metadata: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    metadata.as_object_mut().unwrap().remove("config");
    fs::write(path, serde_json::to_vec(&metadata).unwrap()).unwrap();
    let artifact = store.load("@pied-piper/service").unwrap();
    assert_eq!(
        artifact.metadata.config,
        submilli_build::PackageConfig::default()
    );
    assert_eq!(artifact.metadata.schema_version, 11);
    let report = install_from_dir(&store, repo.path(), None, &source, false).unwrap();
    assert_eq!(
        report.up_to_date,
        [submilli_build::PackageName::new("@pied-piper/service")]
    );
}
