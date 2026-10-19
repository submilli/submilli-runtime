use std::path::Path;

use interpreter::{ModulePath, PackageSourceModule, compile_package};
use submilli_build::{
    ArtifactDependency, ArtifactSource, write_package_artifact_with_docs_and_sources,
};

use super::*;

/// Install `source` as package `name` at version 1.0.0 in the store at `root`,
/// depending on `dependencies`, which must be installed already.
pub(crate) fn install(root: &Path, name: &str, source: &str, dependencies: &[&str]) {
    let store = PackageStore::new(root.to_path_buf());
    let declarations: Vec<PackageDeclaration> = dependencies
        .iter()
        .map(|dependency| store.load(dependency).unwrap().package_declaration)
        .collect();
    let declaration_refs: Vec<&PackageDeclaration> = declarations.iter().collect();
    let compiled = compile_package(
        name,
        ModulePath::from("lib"),
        &[PackageSourceModule {
            path: ModulePath::from("lib"),
            source,
        }],
        &declaration_refs,
    )
    .expect("compile the package");
    let (scope, short) = name.split_once('/').unwrap();
    write_package_artifact_with_docs_and_sources(
        root.join(scope).join(short),
        &compiled.wasm,
        &compiled.type_info,
        &submilli_build::derive_capability_schema(
            &compiled.declaration,
            &[],
            &compiled.required_capabilities,
        ),
        &compiled.declaration,
        &ArtifactMetadata::new(
            name,
            "1.0.0",
            dependencies
                .iter()
                .map(|dependency| ArtifactDependency::new(*dependency, "1.0.0"))
                .collect(),
        ),
        "",
        &[ArtifactSource {
            path: ModulePath::from("lib"),
            text: source.to_owned(),
        }],
    )
    .expect("write the package artifact");
}

fn blueprint(packages: &[&str]) -> Blueprint {
    Blueprint {
        name: "demo".into(),
        packages: packages.iter().map(ToString::to_string).collect(),
        ..Default::default()
    }
}

fn imports(code: &str) -> ScriptImports {
    super::super::compile::parse(code)
        .unwrap()
        .imports()
        .unwrap()
}

const USES_UTIL: &str =
    "import { answer } from \"@acme/util\";\nfunction main(): number { return answer(); }";

#[test]
fn a_prepared_set_survives_the_store_losing_its_files_until_the_cache_is_cleared() {
    let dir = tempfile::tempdir().unwrap();
    install(
        dir.path(),
        "@acme/util",
        "export function answer(): number { return 41; }",
        &[],
    );
    let cache = PackageCache::new(PackageStore::new(dir.path().to_path_buf()));
    let engine = interpreter::runtime::RuntimeConfig::default()
        .engine()
        .unwrap();
    let first = cache
        .prepare_for_imports(&engine, &blueprint(&["@acme/util"]), &imports(USES_UTIL))
        .unwrap();
    assert_eq!(first.modules.len(), 1);

    std::fs::remove_file(dir.path().join("@acme/util/pkg.wasm")).unwrap();
    let cached = cache
        .prepare_for_imports(&engine, &blueprint(&["@acme/util"]), &imports(USES_UTIL))
        .unwrap();
    assert!(Arc::ptr_eq(&first, &cached));

    cache.clear();
    let error = cache
        .prepare_for_imports(&engine, &blueprint(&["@acme/util"]), &imports(USES_UTIL))
        .unwrap_err();
    assert!(error.to_string().contains("@acme/util"), "{error}");
}

#[test]
fn only_blueprint_listed_imports_are_prepared() {
    let dir = tempfile::tempdir().unwrap();
    install(
        dir.path(),
        "@acme/util",
        "export function answer(): number { return 41; }",
        &[],
    );
    let cache = PackageCache::new(PackageStore::new(dir.path().to_path_buf()));
    let engine = interpreter::runtime::RuntimeConfig::default()
        .engine()
        .unwrap();
    let prepared = cache
        .prepare_for_imports(&engine, &blueprint(&[]), &imports(USES_UTIL))
        .unwrap();
    assert!(prepared.modules.is_empty());
    assert!(prepared.script_declarations.is_empty());
}

#[test]
fn a_finalizer_is_registered_under_its_package_and_dependency_versions() {
    let dir = tempfile::tempdir().unwrap();
    install(
        dir.path(),
        "@z/util",
        "export function forty(): number { return 40; }",
        &[],
    );
    install(
        dir.path(),
        "@a/app",
        "import { forty } from \"@z/util\";\nexport function finalizer(key: string, value: string): void { forty(); }",
        &["@z/util"],
    );
    let cache = PackageCache::new(PackageStore::new(dir.path().to_path_buf()));
    let engine = interpreter::runtime::RuntimeConfig::default()
        .engine()
        .unwrap();
    let prepared = cache
        .prepare_for_imports(
            &engine,
            &blueprint(&["@a/app"]),
            &imports("import { finalizer } from \"@a/app\";\nfunction main(): void {}"),
        )
        .unwrap();
    let versions = finalizer_package_versions(&prepared).unwrap();
    assert_eq!(
        versions,
        BTreeMap::from([(
            "@a/app".to_owned(),
            r#"{"@a/app":"1.0.0","@z/util":"1.0.0"}"#.to_owned()
        )])
    );
}
