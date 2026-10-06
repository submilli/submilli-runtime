use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use submilli_build::{
    DependencyKind, DriverError, FetchError, FetchedRepo, PackageName, PackageStore, RepoFetcher,
    ResolveError, ResolvedDependency, build_packages, parse_manifest, resolve_github_closure,
};

#[test]
fn dependency_limits_on_request_stack() {
    if std::env::var_os("SUBMILLI_DEPENDENCY_LIMIT_CHILD").is_some() {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(check_dependency_limits)
            .unwrap()
            .join()
            .unwrap();
        return;
    }
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "dependency_limits_on_request_stack",
            "--nocapture",
        ])
        .env("SUBMILLI_DEPENDENCY_LIMIT_CHILD", "1")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "dependency child failed: {status}");
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("dependency child timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn check_dependency_limits() {
    let root = tempfile::tempdir().unwrap();
    let store = PackageStore::new(root.path().join("store"));
    std::fs::create_dir(root.path().join("src")).unwrap();
    std::fs::write(root.path().join("src/lib.ts"), "").unwrap();
    let seed = parse_manifest(&package(0, None), root.path()).unwrap();
    std::fs::remove_dir_all(root.path().join("src")).unwrap();
    for count in [128, 129, 3_000, 1] {
        let mut manifest = seed.clone();
        manifest.packages = (0..count)
            .map(|index| {
                let mut item = seed.packages[0].clone();
                item.name = PackageName::new(format!("@test/p{index}"));
                if index + 1 < count {
                    item.dependencies.push(ResolvedDependency {
                        name: PackageName::new(format!("@test/p{}", index + 1)),
                        version: None,
                        kind: DependencyKind::Sibling,
                    });
                }
                item
            })
            .collect();
        let error = build_packages(&manifest, root.path(), &store, None).unwrap_err();
        if count > 128 {
            assert!(
                matches!(error, DriverError::DependencyDepth { .. }),
                "{error}"
            );
        } else {
            // Reaching documentation loading proves the graph preflight accepted it.
            assert!(
                matches!(error, DriverError::DocumentationRead { .. }),
                "{error}"
            );
        }
    }
    std::fs::create_dir(root.path().join("src")).unwrap();
    std::fs::write(root.path().join("src/lib.ts"), "").unwrap();
    for count in [128, 129, 3_000, 1] {
        let fetcher = ChainFetcher {
            root: tempfile::tempdir().unwrap(),
            count,
            fetches: AtomicUsize::new(0),
        };
        let manifest = parse_manifest(&package(count, Some(0)), root.path()).unwrap();
        let result = resolve_github_closure(&store, &manifest, &fetcher, None);
        if count > 128 {
            assert!(matches!(result, Err(ResolveError::DependencyDepth { .. })));
            assert_eq!(fetcher.fetches.load(Ordering::SeqCst), 128);
        } else {
            let closure = result.unwrap();
            assert_eq!(closure.plan.len(), count);
            assert_eq!(
                closure.plan[0].name.as_str(),
                format!("@test/p{}", count - 1)
            );
        }
    }
}

struct ChainFetcher {
    root: tempfile::TempDir,
    count: usize,
    fetches: AtomicUsize,
}

impl RepoFetcher for ChainFetcher {
    fn fetch(&self, url: &str, _sha: &str) -> Result<FetchedRepo, FetchError> {
        let index = self.fetches.fetch_add(1, Ordering::SeqCst);
        assert_eq!(url, format!("github.com/test/p{index}"));
        let root = self.root.path().join(format!("p{index}"));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/lib.ts"), "").unwrap();
        let next = (index + 1 < self.count).then_some(index + 1);
        std::fs::write(root.join("submilli.toml"), package(index, next)).unwrap();
        Ok(FetchedRepo {
            org: "test".into(),
            repo: format!("p{index}"),
            root,
            source_hash: "test-hash".into(),
            keep_alive: None,
        })
    }
}

fn package(index: usize, next: Option<usize>) -> String {
    let (dependencies, declared) = next.map_or_else(
        || (String::new(), String::new()),
        |next| (
            format!("[dependencies]\n\"@test/p{next}\" = {{ github = \"github.com/test/p{next}\", rev = \"{}\" }}\n", "a".repeat(40)),
            format!("\"@test/p{next}\""),
        ),
    );
    format!(
        "{dependencies}\n[[package]]\nname = \"@test/p{index}\"\nversion = \"1.0.0\"\ndescription = \"test\"\ndependencies = [{declared}]\n"
    )
}
