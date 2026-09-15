//! Integration tests for project scaffolding (`build init` / `build new` backends).

use std::fs;
use std::path::Path;

use submilli_build::{
    PackageStore, ScaffoldError, add_package, build_packages, init_project, parse_manifest,
    refresh_editor_files,
};
use tempfile::TempDir;

fn manifest_text(dir: &Path) -> String {
    fs::read_to_string(dir.join("submilli.toml")).expect("read submilli.toml")
}

#[test]
fn init_scaffolds_a_buildable_root_package() {
    let tmp = TempDir::new().expect("tempdir");

    let scaffolded = init_project(tmp.path(), "@acme/app", Path::new(".")).expect("init succeeds");

    assert_eq!(scaffolded.manifest_path, tmp.path().join("submilli.toml"));
    assert_eq!(scaffolded.entrypoint, tmp.path().join("src").join("lib.ts"));
    assert_eq!(
        scaffolded.docs_readme,
        tmp.path().join("docs").join("readme.md")
    );
    assert!(scaffolded.docs_readme.is_file());
    assert!(
        tmp.path()
            .join(".submilli/tsconfig.submilli.json")
            .is_file()
    );
    assert!(
        tmp.path()
            .join(".submilli/types/lib.submilli.d.ts")
            .is_file()
    );
    assert!(tmp.path().join(".submilli/types/stdlib.d.ts").is_file());
    assert!(tmp.path().join("tsconfig.json").is_file());
    assert!(tmp.path().join(".vscode/tasks.json").is_file());
    assert!(
        fs::read_to_string(tmp.path().join(".gitignore"))
            .expect("read gitignore")
            .contains(".submilli/")
    );
    let text = manifest_text(tmp.path());
    assert!(text.contains("name = \"@acme/app\""), "got: {text}");
    assert!(
        text.contains("description = \"Package @acme/app.\""),
        "got: {text}"
    );
    assert!(text.contains("keywords = []"), "got: {text}");
    assert!(text.contains("path = \".\""), "got: {text}");

    // A runnable sample test is scaffolded alongside the entrypoint, using the
    // canonical `.ts` extension (matching `src/lib.ts`).
    assert_eq!(
        scaffolded.test_file,
        tmp.path().join("tests").join("lib.test.ts")
    );
    let test_src = fs::read_to_string(&scaffolded.test_file).expect("read sample test");
    assert!(
        test_src.contains("from \"submilli:test\"") && test_src.contains("from \"@acme/app\""),
        "sample test should import the harness and the package: {test_src}"
    );

    let manifest = parse_manifest(&text, tmp.path()).expect("scaffolded manifest parses");
    let store = PackageStore::new(tmp.path().join("store"));
    let built = build_packages(&manifest, tmp.path(), &store, None).expect("scaffold builds");
    assert_eq!(built.len(), 1);
    assert_eq!(built[0].name.as_str(), "@acme/app");
    // The sample test lives under tests/, not src/, so it doesn't enter the build.
    assert!(scaffolded.test_file.is_file());
}

#[test]
fn init_scaffolds_into_a_subdirectory() {
    let tmp = TempDir::new().expect("tempdir");

    let scaffolded =
        init_project(tmp.path(), "@acme/util", Path::new("packages/util")).expect("init succeeds");

    assert_eq!(
        scaffolded.entrypoint,
        tmp.path()
            .join("packages")
            .join("util")
            .join("src")
            .join("lib.ts")
    );
    assert_eq!(
        scaffolded.docs_readme,
        tmp.path()
            .join("packages")
            .join("util")
            .join("docs")
            .join("readme.md")
    );
    assert!(scaffolded.docs_readme.is_file());
    assert!(manifest_text(tmp.path()).contains("path = \"packages/util\""));
}

#[test]
fn init_refuses_to_overwrite_an_existing_manifest() {
    let tmp = TempDir::new().expect("tempdir");
    fs::write(tmp.path().join("submilli.toml"), "# existing\n").expect("write manifest");

    let err = init_project(tmp.path(), "@acme/app", Path::new(".")).expect_err("manifest exists");

    assert!(matches!(err, ScaffoldError::ManifestExists { .. }));
    assert!(err.to_string().contains("submilli build new"), "got: {err}");
    assert_eq!(manifest_text(tmp.path()), "# existing\n");
}

#[test]
fn init_rejects_unscoped_names() {
    let tmp = TempDir::new().expect("tempdir");

    let err = init_project(tmp.path(), "app", Path::new(".")).expect_err("unscoped name");

    assert!(matches!(err, ScaffoldError::InvalidPackageName { .. }));
    assert!(err.to_string().contains("@scope/name"), "got: {err}");
}

#[test]
fn add_makes_an_implicit_root_path_explicit() {
    let tmp = TempDir::new().expect("tempdir");
    fs::create_dir_all(tmp.path().join("src")).expect("create src");
    fs::write(tmp.path().join("src/lib.ts"), "export const x = 1;\n").expect("write lib");
    fs::write(
        tmp.path().join("submilli.toml"),
        "# my project\n[[package]]\nname = \"@acme/app\"\nversion = \"0.3.0\"\ndescription = \"App package.\"\n",
    )
    .expect("write manifest");

    add_package(
        &tmp.path().join("submilli.toml"),
        "@acme/util",
        Path::new("util"),
    )
    .expect("add succeeds");

    let text = manifest_text(tmp.path());
    assert!(text.contains("# my project"), "comment lost: {text}");
    assert!(
        text.contains("version = \"0.3.0\"\npath = \".\""),
        "got: {text}"
    );
    let manifest = parse_manifest(&text, tmp.path()).expect("grown manifest parses");
    assert_eq!(manifest.packages.len(), 2);
    assert_eq!(manifest.packages[0].path.as_path(), Path::new("."));
    assert_eq!(manifest.packages[1].name.as_str(), "@acme/util");
    assert!(tmp.path().join("util/src/lib.ts").is_file());
    let generated =
        fs::read_to_string(tmp.path().join(".submilli/tsconfig.submilli.json")).expect("tsconfig");
    assert!(generated.contains("\"@acme/app\""), "got: {generated}");
    assert!(generated.contains("\"@acme/util\""), "got: {generated}");
}

#[test]
fn add_rejects_duplicate_names_and_paths() {
    let tmp = TempDir::new().expect("tempdir");
    init_project(tmp.path(), "@acme/app", Path::new("app")).expect("init");
    let manifest_path = tmp.path().join("submilli.toml");

    let err =
        add_package(&manifest_path, "@acme/app", Path::new("other")).expect_err("duplicate name");
    assert!(matches!(err, ScaffoldError::DuplicatePackageName { .. }));

    let err =
        add_package(&manifest_path, "@acme/other", Path::new("app")).expect_err("duplicate path");
    assert!(matches!(err, ScaffoldError::DuplicatePackagePath { .. }));
    assert!(err.to_string().contains("@acme/app"), "got: {err}");
}

#[test]
fn add_requires_an_existing_manifest() {
    let tmp = TempDir::new().expect("tempdir");

    let err = add_package(
        &tmp.path().join("submilli.toml"),
        "@acme/util",
        Path::new("util"),
    )
    .expect_err("missing manifest");

    assert!(matches!(err, ScaffoldError::ManifestMissing { .. }));
    assert!(
        err.to_string().contains("submilli build init"),
        "got: {err}"
    );
}

#[test]
fn init_then_add_builds_as_a_monorepo() {
    let tmp = TempDir::new().expect("tempdir");
    init_project(tmp.path(), "@acme/app", Path::new("app")).expect("init");
    add_package(
        &tmp.path().join("submilli.toml"),
        "@acme/util",
        Path::new("util"),
    )
    .expect("add");

    let text = manifest_text(tmp.path());
    let manifest = parse_manifest(&text, tmp.path()).expect("manifest parses");
    let store = PackageStore::new(tmp.path().join("store"));
    let built = build_packages(&manifest, tmp.path(), &store, None).expect("monorepo builds");

    let mut names: Vec<&str> = built.iter().map(|p| p.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, vec!["@acme/app", "@acme/util"]);
}

#[test]
fn init_does_not_overwrite_user_editor_files() {
    let tmp = TempDir::new().expect("tempdir");
    fs::write(
        tmp.path().join("tsconfig.json"),
        "{\"compilerOptions\": {}}\n",
    )
    .expect("write tsconfig");
    fs::create_dir_all(tmp.path().join(".vscode")).expect("create vscode");
    fs::write(tmp.path().join(".vscode/tasks.json"), "{\"tasks\": []}\n").expect("write tasks");

    init_project(tmp.path(), "@acme/app", Path::new(".")).expect("init succeeds");

    assert_eq!(
        fs::read_to_string(tmp.path().join("tsconfig.json")).expect("read tsconfig"),
        "{\"compilerOptions\": {}}\n"
    );
    assert_eq!(
        fs::read_to_string(tmp.path().join(".vscode/tasks.json")).expect("read tasks"),
        "{\"tasks\": []}\n"
    );
    assert!(
        tmp.path()
            .join(".submilli/tsconfig.submilli.json")
            .is_file()
    );
    assert!(
        tmp.path()
            .join(".submilli/types/lib.submilli.d.ts")
            .is_file()
    );
    assert!(tmp.path().join(".submilli/types/stdlib.d.ts").is_file());
}

#[test]
fn generated_tsconfig_maps_package_names_to_source_entrypoints() {
    let tmp = TempDir::new().expect("tempdir");
    init_project(tmp.path(), "@acme/app", Path::new("app")).expect("init");
    add_package(
        &tmp.path().join("submilli.toml"),
        "@acme/util",
        Path::new("packages/util"),
    )
    .expect("add");

    let generated =
        fs::read_to_string(tmp.path().join(".submilli/tsconfig.submilli.json")).expect("tsconfig");

    assert!(generated.contains("\"@acme/app\""), "got: {generated}");
    assert!(generated.contains("../app/src/lib.ts"), "got: {generated}");
    assert!(generated.contains("\"@acme/util\""), "got: {generated}");
    assert!(
        generated.contains("../packages/util/src/lib.ts"),
        "got: {generated}"
    );
    assert!(generated.contains("\"noLib\": true"), "got: {generated}");
    assert!(!generated.contains("\"lib\""), "got: {generated}");
    assert!(
        generated.contains("\"module\": \"preserve\""),
        "got: {generated}"
    );
    assert!(
        generated.contains("\"target\": \"es2022\""),
        "got: {generated}"
    );
    assert!(
        generated.contains("\"strictNullChecks\": false"),
        "got: {generated}"
    );
    assert!(generated.contains("./types/**/*.d.ts"), "got: {generated}");
}

#[test]
fn generated_editor_declarations_cover_prelude_and_stdlib() {
    let tmp = TempDir::new().expect("tempdir");
    init_project(tmp.path(), "@acme/app", Path::new(".")).expect("init");

    let lib =
        fs::read_to_string(tmp.path().join(".submilli/types/lib.submilli.d.ts")).expect("lib d.ts");
    let stdlib =
        fs::read_to_string(tmp.path().join(".submilli/types/stdlib.d.ts")).expect("stdlib d.ts");

    assert!(lib.contains("interface Array<"), "got: {lib}");
    assert!(lib.contains("namespace JSON {"), "got: {lib}");
    assert!(stdlib.contains("declare module \"submilli:uuid\" {"));
    assert!(stdlib.contains("declare module \"submilli:http\" {"));
    assert!(!stdlib.contains("declare module \"submilli:security\""));
}

#[test]
fn refresh_editor_files_rewrites_generated_files() {
    let tmp = TempDir::new().expect("tempdir");
    init_project(tmp.path(), "@acme/app", Path::new(".")).expect("init");
    let types_dir = tmp.path().join(".submilli/types");
    fs::write(types_dir.join("stdlib.d.ts"), "stale\n").expect("write stale stdlib");
    fs::write(types_dir.join("lib.submilli.d.ts"), "stale\n").expect("write stale lib");
    let text = manifest_text(tmp.path());
    let manifest = parse_manifest(&text, tmp.path()).expect("manifest parses");

    refresh_editor_files(tmp.path(), &manifest).expect("refresh");

    let stdlib = fs::read_to_string(types_dir.join("stdlib.d.ts")).expect("read stdlib");
    let lib = fs::read_to_string(types_dir.join("lib.submilli.d.ts")).expect("read lib");
    assert!(stdlib.contains("declare module \"submilli:uuid\" {"));
    assert!(lib.contains("namespace JSON {"));
}
