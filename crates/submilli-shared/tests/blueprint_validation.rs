//! A blueprint's rule filters are checked against the fields the installed
//! packages' capabilities report.

use std::fs;
use std::path::Path;

use submilli_build::{PackageName, PackageStore, build_packages, install_packages, parse_manifest};
use submilli_shared::blueprint_validation::{PackageValidationError, validate_packages};
use tempfile::TempDir;

fn write(project: &Path, relative: &str, text: &str) {
    let path = project.join(relative);
    fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
    fs::write(path, text).expect("write file");
}

const MANIFEST: &str = r#"
[[package]]
name = "@pied-piper/sdk"
version = "0.1.0"
description = "SDK package."
path = "sdk"
"#;

const SOURCE: &str = r#"
import { check } from "submilli:security";
/**
 * Checks a purchase.
 * @param customer Customer identifier.
 * @param total Purchase total.
 * @capability pied-piper.example/v { customer, total }
 */
export function v(customer: string, total: number): void {
    check("pied-piper.example/v", { customer, total });
}
"#;

#[test]
fn rule_filters_may_test_only_reported_capability_fields() {
    let project = TempDir::new().unwrap();
    write(project.path(), "sdk/docs/readme.md", "# SDK\n");
    write(project.path(), "sdk/src/lib.ts", SOURCE);
    let store = PackageStore::new(project.path().join("store"));
    let manifest = parse_manifest(MANIFEST, project.path()).expect("manifest parses");
    let only = Some(PackageName::new("@pied-piper/sdk"));
    let built = build_packages(&manifest, project.path(), &store, only.as_ref()).unwrap();
    install_packages(&store, &built).unwrap();

    let yaml = "name: fields\npackages: ['@pied-piper/sdk']\npermissions:\n  main:\n    - capability: pied-piper.example/v\n      filter: total < 100\n      action: allow\n";
    let blueprint = submilli_blueprint::parse(yaml).unwrap();
    validate_packages(&blueprint, &store).unwrap();
    let invalid = submilli_blueprint::parse(&yaml.replace("total <", "typo <")).unwrap();
    let error = validate_packages(&invalid, &store).unwrap_err();
    let PackageValidationError::InvalidFilter(problem) = error else {
        panic!("expected invalid filter, got {error}");
    };
    assert_eq!(
        problem.path,
        Some(submilli_blueprint::yaml_path![
            "permissions",
            "main",
            0usize,
            "filter"
        ])
    );
    assert!(problem.message.contains("tests `typo`"));
}
