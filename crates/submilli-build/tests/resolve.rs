//! Recursive GitHub-dependency resolution over a fake fetcher (no network).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use submilli_build::{
    FetchError, FetchedRepo, Lockfile, PackageStore, RepoFetcher, ResolveError, install_plan,
    load_manifest, resolve_github_closure,
};
use tempfile::TempDir;

const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const SHA_C: &str = "cccccccccccccccccccccccccccccccccccccccc";
const SHA_C2: &str = "dddddddddddddddddddddddddddddddddddddddd";

/// Serves prepared fixture repos by URL and counts fetches.
struct FakeFetcher {
    repos: BTreeMap<String, (String, String, PathBuf)>,
    fetches: AtomicUsize,
}

impl RepoFetcher for FakeFetcher {
    fn fetch(&self, url: &str, sha: &str) -> Result<FetchedRepo, FetchError> {
        self.fetches.fetch_add(1, Ordering::SeqCst);
        let (org, repo, dir) = self
            .repos
            .get(url)
            .ok_or_else(|| FetchError::new(format!("no fixture repo for {url}")))?;
        Ok(FetchedRepo {
            org: org.clone(),
            repo: repo.clone(),
            root: dir.clone(),
            source_hash: format!("sha256:{sha}"),
            keep_alive: None,
        })
    }
}

struct World {
    _root: TempDir,
    root_dir: PathBuf,
    fetcher: FakeFetcher,
    store: PackageStore,
    _store_dir: TempDir,
}

fn world(repos: &[(&str, &str, &str, &str)], root_manifest: &str) -> World {
    // repos: (url, org, repo, manifest)
    let root = tempfile::tempdir().expect("tempdir");
    let mut map = BTreeMap::new();
    for (url, org, repo, manifest) in repos {
        let dir = root.path().join(repo);
        write_repo(&dir, manifest);
        map.insert(
            (*url).to_string(),
            ((*org).to_string(), (*repo).to_string(), dir),
        );
    }
    let root_dir = root.path().join("__project__");
    write_repo(&root_dir, root_manifest);

    let store_dir = tempfile::tempdir().expect("store tempdir");
    World {
        root_dir,
        fetcher: FakeFetcher {
            repos: map,
            fetches: AtomicUsize::new(0),
        },
        store: PackageStore::new(store_dir.path()),
        _store_dir: store_dir,
        _root: root,
    }
}

fn write_repo(dir: &Path, manifest: &str) {
    write_repo_with_lib(
        dir,
        manifest,
        "export function value(): number { return 1; }\n",
    );
}

fn write_repo_with_lib(dir: &Path, manifest: &str, lib: &str) {
    let src = dir.join("src");
    std::fs::create_dir_all(&src).expect("create src");
    std::fs::create_dir_all(dir.join("docs")).expect("create docs");
    std::fs::write(dir.join("submilli.toml"), manifest).expect("write manifest");
    std::fs::write(dir.join("docs/readme.md"), "# pkg\n").expect("write docs");
    std::fs::write(src.join("lib.ts"), lib).expect("write lib");
}

fn dep(name: &str, url: &str, sha: &str) -> String {
    format!("\"{name}\" = {{ github = \"{url}\", rev = \"{sha}\" }}\n")
}

fn package(name: &str, version: &str, deps: &[&str]) -> String {
    let list = deps
        .iter()
        .map(|d| format!("\"{d}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "[[package]]\nname = \"{name}\"\nversion = \"{version}\"\ndescription = \"pkg\"\ndependencies = [{list}]\n"
    )
}

fn names(closure: &[submilli_build::LockedPackage]) -> Vec<&str> {
    closure.iter().map(|p| p.name.as_str()).collect()
}

fn resolve(
    world: &World,
    lock: Option<&Lockfile>,
) -> Result<Vec<submilli_build::LockedPackage>, ResolveError> {
    let manifest = load_manifest(&world.root_dir.join("submilli.toml")).expect("root manifest");
    let closure = resolve_github_closure(&world.store, &manifest, &world.fetcher, lock)?;
    install_plan(&world.store, &closure.plan, true).expect("install plan");
    Ok(closure.locked)
}

#[test]
fn installs_chain_deps_before_dependents() {
    // root -> @acme/a -> @acme/c
    let repo_a = format!(
        "[dependencies]\n{}\n{}",
        dep("@acme/c", "github.com/acme/c", SHA_C),
        package("@acme/a", "1.0.0", &["@acme/c"])
    );
    let repo_c = package("@acme/c", "2.0.0", &[]);
    let root = format!(
        "[dependencies]\n{}\n{}",
        dep("@acme/a", "github.com/acme/a", SHA_A),
        package("@me/app", "0.1.0", &["@acme/a"])
    );
    let world = world(
        &[
            ("github.com/acme/a", "acme", "a", &repo_a),
            ("github.com/acme/c", "acme", "c", &repo_c),
        ],
        &root,
    );

    let closure = resolve(&world, None).expect("resolves");
    assert_eq!(names(&closure), vec!["@acme/c", "@acme/a"]);
    assert!(world.store.load("@acme/a").is_ok());
    assert!(world.store.load("@acme/c").is_ok());
    // @acme/a records @acme/c's fetched version, not a declared one.
    let a = closure.iter().find(|p| p.name == "@acme/a").unwrap();
    assert_eq!(a.version, "1.0.0");
    let c = closure.iter().find(|p| p.name == "@acme/c").unwrap();
    assert_eq!(c.version, "2.0.0");
}

#[test]
fn diamond_installs_shared_dep_once() {
    // root -> a, b ; a -> c, b -> c (same sha)
    let repo_a = format!(
        "[dependencies]\n{}\n{}",
        dep("@acme/c", "github.com/acme/c", SHA_C),
        package("@acme/a", "1.0.0", &["@acme/c"])
    );
    let repo_b = format!(
        "[dependencies]\n{}\n{}",
        dep("@acme/c", "github.com/acme/c", SHA_C),
        package("@acme/b", "1.0.0", &["@acme/c"])
    );
    let repo_c = package("@acme/c", "2.0.0", &[]);
    let root = format!(
        "[dependencies]\n{}{}\n{}",
        dep("@acme/a", "github.com/acme/a", SHA_A),
        dep("@acme/b", "github.com/acme/b", SHA_B),
        package("@me/app", "0.1.0", &["@acme/a", "@acme/b"])
    );
    let world = world(
        &[
            ("github.com/acme/a", "acme", "a", &repo_a),
            ("github.com/acme/b", "acme", "b", &repo_b),
            ("github.com/acme/c", "acme", "c", &repo_c),
        ],
        &root,
    );

    let closure = resolve(&world, None).expect("resolves");
    let c_count = closure.iter().filter(|p| p.name == "@acme/c").count();
    assert_eq!(c_count, 1, "shared dep installed once");
    assert_eq!(closure.len(), 3);
    // c fetched exactly once despite two requirers.
    assert_eq!(world.fetcher.fetches.load(Ordering::SeqCst), 3);
}

#[test]
fn divergent_sha_is_a_conflict() {
    // a -> c@SHA_C, b -> c@SHA_C2
    let repo_a = format!(
        "[dependencies]\n{}\n{}",
        dep("@acme/c", "github.com/acme/c", SHA_C),
        package("@acme/a", "1.0.0", &["@acme/c"])
    );
    let repo_b = format!(
        "[dependencies]\n{}\n{}",
        dep("@acme/c", "github.com/acme/c", SHA_C2),
        package("@acme/b", "1.0.0", &["@acme/c"])
    );
    let repo_c = package("@acme/c", "2.0.0", &[]);
    let root = format!(
        "[dependencies]\n{}{}\n{}",
        dep("@acme/a", "github.com/acme/a", SHA_A),
        dep("@acme/b", "github.com/acme/b", SHA_B),
        package("@me/app", "0.1.0", &["@acme/a", "@acme/b"])
    );
    let world = world(
        &[
            ("github.com/acme/a", "acme", "a", &repo_a),
            ("github.com/acme/b", "acme", "b", &repo_b),
            ("github.com/acme/c", "acme", "c", &repo_c),
        ],
        &root,
    );

    let err = resolve(&world, None).expect_err("conflict");
    let ResolveError::ShaConflict { name, .. } = &err else {
        panic!("expected ShaConflict, got {err:?}");
    };
    assert_eq!(name.as_str(), "@acme/c");
    let message = err.to_string();
    assert!(
        message.contains("@acme/a"),
        "names first requirer: {message}"
    );
    assert!(
        message.contains("@acme/b"),
        "names second requirer: {message}"
    );
}

#[test]
fn cycle_is_rejected() {
    // a -> b -> a
    let repo_a = format!(
        "[dependencies]\n{}\n{}",
        dep("@acme/b", "github.com/acme/b", SHA_B),
        package("@acme/a", "1.0.0", &["@acme/b"])
    );
    let repo_b = format!(
        "[dependencies]\n{}\n{}",
        dep("@acme/a", "github.com/acme/a", SHA_A),
        package("@acme/b", "1.0.0", &["@acme/a"])
    );
    let root = format!(
        "[dependencies]\n{}\n{}",
        dep("@acme/a", "github.com/acme/a", SHA_A),
        package("@me/app", "0.1.0", &["@acme/a"])
    );
    let world = world(
        &[
            ("github.com/acme/a", "acme", "a", &repo_a),
            ("github.com/acme/b", "acme", "b", &repo_b),
        ],
        &root,
    );

    let err = resolve(&world, None).expect_err("cycle");
    assert!(matches!(err, ResolveError::Cycle { .. }), "got {err:?}");
}

#[test]
fn dependent_compiles_against_imported_transitive_package() {
    // @acme/a imports @acme/c across repos; it only compiles if C's declaration
    // is installed in the store first.
    let root_tmp = tempfile::tempdir().expect("tempdir");
    let a_dir = root_tmp.path().join("a");
    let c_dir = root_tmp.path().join("c");
    let project_dir = root_tmp.path().join("project");

    write_repo_with_lib(
        &c_dir,
        &package("@acme/c", "2.0.0", &[]),
        "export function base(): number { return 41; }\n",
    );
    write_repo_with_lib(
        &a_dir,
        &format!(
            "[dependencies]\n{}\n{}",
            dep("@acme/c", "github.com/acme/c", SHA_C),
            package("@acme/a", "1.0.0", &["@acme/c"])
        ),
        "import { base } from \"@acme/c\";\nexport function value(): number { return base() + 1; }\n",
    );
    write_repo(
        &project_dir,
        &format!(
            "[dependencies]\n{}\n{}",
            dep("@acme/a", "github.com/acme/a", SHA_A),
            package("@me/app", "0.1.0", &["@acme/a"])
        ),
    );

    let mut repos = BTreeMap::new();
    repos.insert(
        "github.com/acme/a".to_string(),
        ("acme".to_string(), "a".to_string(), a_dir),
    );
    repos.insert(
        "github.com/acme/c".to_string(),
        ("acme".to_string(), "c".to_string(), c_dir),
    );
    let fetcher = FakeFetcher {
        repos,
        fetches: AtomicUsize::new(0),
    };
    let store_dir = tempfile::tempdir().expect("store");
    let store = PackageStore::new(store_dir.path());

    let manifest = load_manifest(&project_dir.join("submilli.toml")).expect("manifest");
    let closure = resolve_github_closure(&store, &manifest, &fetcher, None).expect("resolves");
    install_plan(&store, &closure.plan, true).expect("install plan");

    assert_eq!(names(&closure.locked), vec!["@acme/c", "@acme/a"]);
    assert!(store.load("@acme/a").is_ok(), "dependent installed");
}

#[test]
fn satisfied_lock_skips_fetching() {
    let repo_a = package("@acme/a", "1.0.0", &[]);
    let root = format!(
        "[dependencies]\n{}\n{}",
        dep("@acme/a", "github.com/acme/a", SHA_A),
        package("@me/app", "0.1.0", &["@acme/a"])
    );
    let world = world(&[("github.com/acme/a", "acme", "a", &repo_a)], &root);

    let closure = resolve(&world, None).expect("first resolve");
    assert_eq!(world.fetcher.fetches.load(Ordering::SeqCst), 1);

    // Re-resolve with the produced lock: the store is satisfied, so nothing is
    // fetched again.
    let lock = Lockfile::new(closure);
    let again = resolve(&world, Some(&lock)).expect("second resolve");
    assert_eq!(names(&again), vec!["@acme/a"]);
    assert_eq!(
        world.fetcher.fetches.load(Ordering::SeqCst),
        1,
        "no additional fetch when the lock is satisfied"
    );
}

#[test]
fn a_lock_is_not_satisfied_by_a_fallback_copy() {
    let repo_a = package("@acme/a", "1.0.0", &[]);
    let root = format!(
        "[dependencies]\n{}\n{}",
        dep("@acme/a", "github.com/acme/a", SHA_A),
        package("@me/app", "0.1.0", &["@acme/a"])
    );
    let world = world(&[("github.com/acme/a", "acme", "a", &repo_a)], &root);
    let closure = resolve(&world, None).expect("first resolve");
    let lock = Lockfile::new(closure);

    // Re-resolve through a store that only *reads* the populated one: the
    // locked package is visible but not owned, so it must be fetched again.
    let owned_dir = tempfile::tempdir().expect("owned tempdir");
    let layered = PackageStore::new(owned_dir.path()).with_fallback(world.store.root());
    let manifest = load_manifest(&world.root_dir.join("submilli.toml")).expect("root manifest");
    let again = resolve_github_closure(&layered, &manifest, &world.fetcher, Some(&lock))
        .expect("second resolve");

    assert_eq!(names(&again.locked), vec!["@acme/a"]);
    assert_eq!(
        world.fetcher.fetches.load(Ordering::SeqCst),
        2,
        "the fallback copy does not satisfy the lock"
    );
}
