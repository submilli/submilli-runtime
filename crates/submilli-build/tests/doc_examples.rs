//! CI gate for package doc examples: every package in the repo's manifest must
//! ship at least one fenced `ts` example in `docs/readme.md`, and every example
//! must compile against the package's own surface. `submilli build test` runs
//! the same checks with the full dependency closure; this test is the
//! `cargo test --workspace` hook that keeps CI honest.

use std::fs;
use std::path::Path;

use submilli_build::{
    PackageStore, build_packages, compile_check_doc_example, extract_doc_examples, parse_manifest,
};
use tempfile::TempDir;

#[test]
fn every_repo_package_ships_a_compiling_example() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let manifest_text = fs::read_to_string(root.join("submilli.toml")).expect("read submilli.toml");
    let manifest = parse_manifest(&manifest_text, root).expect("parse submilli.toml");

    let store_root = TempDir::new().expect("tempdir");
    let store = PackageStore::new(store_root.path());
    let built = build_packages(&manifest, root, &store, None).expect("packages build");
    assert!(!built.is_empty(), "the repo manifest declares packages");

    let mut failures = Vec::new();
    for pkg in &built {
        let examples = extract_doc_examples(&pkg.documentation);
        if examples.is_empty() {
            failures.push(format!(
                "{}: docs/readme.md has no ```ts example",
                pkg.name.as_str()
            ));
            continue;
        }
        for (index, example) in examples.iter().enumerate() {
            let display_path = format!("{}/docs/readme.md", pkg.name.as_str());
            if let Err(rendered) =
                compile_check_doc_example(example, &display_path, &[&pkg.declaration])
            {
                failures.push(format!(
                    "{} example {}:\n{rendered}",
                    pkg.name.as_str(),
                    index + 1
                ));
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
