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
    let repo = write_repo("@acme/widget");
    let store_root = TempDir::new().unwrap();
    let store = PackageStore::new(store_root.path());

    let report = install_from_dir(
        &store,
        repo.path(),
        None,
        &github("acme", "widget", SHA_A),
        false,
    )
    .expect("install succeeds");

    assert_eq!(report.installed.len(), 1);
    assert_eq!(report.installed[0].name.as_str(), "@acme/widget");
    assert!(report.up_to_date.is_empty());

    let dir = store.package_dir("@acme/widget").unwrap();
    let artifact = read_package_artifact(&dir).expect("artifact readable");
    match artifact.metadata.source {
        Some(PackageSource::Github(gh)) => {
            assert_eq!(gh.org, "acme");
            assert_eq!(gh.repo, "widget");
            assert_eq!(gh.sha, SHA_A);
        }
        other => panic!("expected GitHub provenance, got {other:?}"),
    }
}

#[test]
fn reinstall_at_same_sha_is_a_no_op() {
    let repo = write_repo("@acme/widget");
    let store_root = TempDir::new().unwrap();
    let store = PackageStore::new(store_root.path());
    let source = github("acme", "widget", SHA_A);

    install_from_dir(&store, repo.path(), None, &source, false).expect("first install");
    let report =
        install_from_dir(&store, repo.path(), None, &source, false).expect("second install");

    assert!(report.installed.is_empty());
    assert_eq!(report.up_to_date.len(), 1);
    assert_eq!(report.up_to_date[0].as_str(), "@acme/widget");
}

#[test]
fn reinstall_at_new_sha_needs_upgrade() {
    let repo = write_repo("@acme/widget");
    let store_root = TempDir::new().unwrap();
    let store = PackageStore::new(store_root.path());

    install_from_dir(
        &store,
        repo.path(),
        None,
        &github("acme", "widget", SHA_A),
        false,
    )
    .expect("first install");

    let blocked = install_from_dir(
        &store,
        repo.path(),
        None,
        &github("acme", "widget", SHA_B),
        false,
    );
    match blocked {
        Err(InstallError::Conflict { conflicts, .. }) => {
            assert_eq!(conflicts.len(), 1);
            assert_eq!(conflicts[0].name.as_str(), "@acme/widget");
        }
        other => panic!("expected Conflict, got {other:?}"),
    }

    let upgraded = install_from_dir(
        &store,
        repo.path(),
        None,
        &github("acme", "widget", SHA_B),
        true,
    )
    .expect("upgrade succeeds");
    assert_eq!(upgraded.installed.len(), 1);

    let dir = store.package_dir("@acme/widget").unwrap();
    let artifact = read_package_artifact(&dir).unwrap();
    match artifact.metadata.source {
        Some(PackageSource::Github(gh)) => assert_eq!(gh.sha, SHA_B),
        other => panic!("expected GitHub provenance, got {other:?}"),
    }
}

#[test]
fn rejects_scope_that_is_not_the_source_org() {
    // Package claims `@other/widget` but the source org is `acme`.
    let repo = write_repo("@other/widget");
    let store_root = TempDir::new().unwrap();
    let store = PackageStore::new(store_root.path());

    let result = install_from_dir(
        &store,
        repo.path(),
        None,
        &github("acme", "widget", SHA_A),
        false,
    );
    match result {
        Err(InstallError::ScopeMismatch { org, packages, .. }) => {
            assert_eq!(org, "acme");
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
        &github("acme", "widget", SHA_A),
        false,
    );
    assert!(matches!(result, Err(InstallError::NoManifest { .. })));
}

#[test]
fn an_interrupted_install_is_replaced_rather_than_refused() {
    let repo = write_repo("@acme/widget");
    let store_root = TempDir::new().unwrap();
    let store = PackageStore::new(store_root.path());
    // The directory exists, the artifact files do not: a write that died early.
    fs::create_dir_all(store.package_dir("@acme/widget").unwrap()).unwrap();

    let report = install_from_dir(
        &store,
        repo.path(),
        None,
        &github("acme", "widget", SHA_A),
        false,
    )
    .expect("install over the partial directory");

    assert_eq!(report.installed.len(), 1);
    assert!(read_package_artifact(store.package_dir("@acme/widget").unwrap()).is_ok());
}

#[test]
fn a_fallback_copy_neither_conflicts_nor_satisfies() {
    let repo = write_repo("@acme/widget");
    let fallback_root = TempDir::new().unwrap();
    let fallback = PackageStore::new(fallback_root.path());
    install_from_dir(
        &fallback,
        repo.path(),
        None,
        &github("acme", "widget", SHA_A),
        false,
    )
    .expect("install into the fallback");

    // A layered store whose owned root is empty: the fallback copy is visible
    // to reads but is not this store's install.
    let owned_root = TempDir::new().unwrap();
    let store = PackageStore::new(owned_root.path()).with_fallback(fallback_root.path());
    assert!(
        store.load("@acme/widget").is_ok(),
        "readable through the fallback"
    );

    let report = install_from_dir(
        &store,
        repo.path(),
        None,
        &github("acme", "widget", SHA_B),
        false,
    )
    .expect("a different sha in the fallback is not a conflict");

    assert_eq!(report.installed.len(), 1, "installed into the owned root");
    assert!(report.up_to_date.is_empty());
    let owned = read_package_artifact(store.package_dir("@acme/widget").unwrap()).unwrap();
    match owned.metadata.source {
        Some(PackageSource::Github(gh)) => assert_eq!(gh.sha, SHA_B),
        other => panic!("expected GitHub provenance, got {other:?}"),
    }
    let untouched = read_package_artifact(fallback.package_dir("@acme/widget").unwrap()).unwrap();
    match untouched.metadata.source {
        Some(PackageSource::Github(gh)) => assert_eq!(gh.sha, SHA_A, "fallback copy is untouched"),
        other => panic!("expected GitHub provenance, got {other:?}"),
    }
}

#[test]
fn deny_warnings_preparation_preserves_store_and_reports_same_commit_warnings() {
    use submilli_build::InstallPreparation;
    let clean = write_repo("@acme/dependency");
    let warned = write_repo("@acme/warned");
    write(
        warned.path(),
        "src/lib.ts",
        "export function hello(): number { return 1; }\n",
    );
    write(
        warned.path(),
        "submilli.toml",
        "[dependencies]\n\"@acme/dependency\" = \"0.1.0\"\n\n[[package]]\nname = \"@acme/warned\"\nversion = \"0.1.0\"\ndescription = \"Warned package.\"\ndependencies = [\"@acme/dependency\"]\n",
    );
    write(
        warned.path(),
        "src/lib.ts",
        "import { hello } from \"@acme/dependency\";\nexport function answer(): number { return hello(); }\n",
    );
    let root = TempDir::new().unwrap();
    let store = PackageStore::new(root.path());
    let mut preparation = InstallPreparation::new(&store).unwrap();
    preparation
        .prepare_repo(
            clean.path(),
            None,
            &github("acme", "dependency", SHA_A),
            false,
        )
        .unwrap();
    let report = preparation
        .prepare_repo(warned.path(), None, &github("acme", "warned", SHA_A), false)
        .unwrap();
    assert!(!report.warnings.is_empty());
    let error = preparation.publish(true).unwrap_err();
    assert!(matches!(error, InstallError::WarningsDenied { .. }));
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);

    install_from_dir(
        &store,
        clean.path(),
        None,
        &github("acme", "dependency", SHA_A),
        false,
    )
    .unwrap();
    let permissive = install_from_dir(
        &store,
        warned.path(),
        None,
        &github("acme", "warned", SHA_A),
        false,
    )
    .unwrap();
    assert!(!permissive.warnings.is_empty());
    let artifact = store.load_owned("@acme/warned").unwrap();
    for sha in [SHA_A, SHA_B] {
        let mut preparation = InstallPreparation::new(&store).unwrap();
        let report = preparation
            .prepare_repo(warned.path(), None, &github("acme", "warned", sha), true)
            .unwrap();
        if sha == SHA_A {
            assert_eq!(report.up_to_date.len(), 1);
        }
        assert!(matches!(
            preparation.publish(true),
            Err(InstallError::WarningsDenied { .. })
        ));
        assert_eq!(
            store.load_owned("@acme/warned").unwrap().wasm,
            artifact.wasm
        );
        assert_eq!(
            store.load_owned("@acme/warned").unwrap().metadata.source,
            artifact.metadata.source
        );
    }
}

#[test]
fn deny_warnings_preparation_checks_overlapping_sibling_commits() {
    use submilli_build::InstallPreparation;
    let repo = write_repo("@acme/shared");
    for (sha, upgrade, conflict) in [
        (SHA_A, false, false),
        (SHA_B, false, true),
        (SHA_B, true, false),
    ] {
        let root = TempDir::new().unwrap();
        let store = PackageStore::new(root.path());
        let mut preparation = InstallPreparation::new(&store).unwrap();
        preparation
            .prepare_repo(repo.path(), None, &github("acme", "first", SHA_A), false)
            .unwrap();
        let second =
            preparation.prepare_repo(repo.path(), None, &github("acme", "second", sha), upgrade);
        if conflict {
            assert!(matches!(second, Err(InstallError::Conflict { .. })));
            assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
        } else {
            let report = second.unwrap();
            assert_eq!(report.up_to_date.len(), usize::from(sha == SHA_A));
            preparation.publish(true).unwrap();
            let Some(PackageSource::Github(source)) =
                store.load_owned("@acme/shared").unwrap().metadata.source
            else {
                panic!("GitHub provenance")
            };
            assert_eq!(source.sha, sha);
        }
    }
}
