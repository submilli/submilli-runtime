use interpreter::{ModulePath, PackageDeclaration};
use submilli_build::{
    ArtifactDependency, ArtifactMetadata, ArtifactSource, derive_capability_schema,
    write_package_artifact_with_docs_and_sources,
};

use super::*;

/// Boot rebuilds a package from a previous artifact schema, and a package that
/// cannot be rebuilt does not fail startup or take the others down.
#[tokio::test]
async fn boot_recompiles_previous_schema_packages_and_keeps_serving_the_rest() {
    let packages = tempfile::tempdir().unwrap();
    write_package(
        packages.path(),
        "@t/ok",
        "export function hello(): string {\n    return \"hello\";\n}\n",
    );
    write_package(packages.path(), "@t/bad", "export function broken( {\n");
    let state = AppState::new(ServerConfig {
        package_store_root: Some(packages.path().to_path_buf()),
        volume_root: Some(tempfile::tempdir().unwrap().keep()),
        ..crate::config::test_config()
    })
    .await
    .expect("app state");

    state
        .boot()
        .await
        .expect("a broken package does not fail boot");

    let ok = state
        .package_store()
        .load("@t/ok")
        .expect("rebuilt package loads");
    assert_eq!(
        ok.metadata.schema_version,
        submilli_build::ARTIFACT_SCHEMA_VERSION
    );
    assert_ne!(ok.wasm, b"STALE-WASM");
    let error = state
        .package_store()
        .load("@t/bad")
        .expect_err("broken package stays unavailable");
    let text = error.to_string();
    assert!(text.contains("@t/bad"), "{text}");
    assert!(
        text.contains(&format!(
            "artifact schema {}",
            submilli_build::ARTIFACT_SCHEMA_VERSION
        )),
        "{text}"
    );
}

fn write_package(root: &std::path::Path, name: &str, source: &str) {
    let dir = submilli_build::PackageStore::new(root)
        .package_dir(name)
        .unwrap();
    let mut declaration = PackageDeclaration::with_package(name);
    declaration.refresh_shapes();
    let mut metadata = ArtifactMetadata::new(name, "1.0.0", Vec::<ArtifactDependency>::new());
    metadata.schema_version = submilli_build::ARTIFACT_SCHEMA_VERSION - 1;
    write_package_artifact_with_docs_and_sources(
        dir,
        b"STALE-WASM",
        &interpreter::TypeInfoTable {
            package_name: name.to_string(),
            types: Vec::new(),
        },
        &derive_capability_schema(&declaration, &[], &[]),
        &declaration,
        &metadata,
        "",
        &[ArtifactSource {
            path: ModulePath::from("lib"),
            text: source.to_string(),
        }],
    )
    .unwrap();
}
