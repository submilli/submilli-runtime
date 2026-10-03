//! Integration tests for the cross-package compilation driver.

use std::fs;
use std::path::Path;

use interpreter::{FileId, ModulePath, PackageSourceModule, compile_package, compile_script};
use submilli_build::{
    ArtifactDependency, ArtifactMetadata, DriverError, PackageName, PackageStore, build_packages,
    derive_capability_schema, install_packages, parse_manifest, write_package_artifact,
};
use tempfile::TempDir;

fn write_module(project: &Path, relative: &str, text: &str) {
    let path = project.join(relative);
    fs::create_dir_all(path.parent().expect("module parent")).expect("create module dir");
    fs::write(path, text).expect("write module");
}

fn write_docs(project: &Path, package_path: &str, text: &str) {
    let path = project.join(package_path).join("docs/readme.md");
    fs::create_dir_all(path.parent().expect("docs parent")).expect("create docs dir");
    fs::write(path, text).expect("write docs");
}

fn write_external_dep(store_root: &Path) {
    let package = compile_package(
        "@ext/dep",
        ModulePath::from("lib"),
        &[PackageSourceModule {
            path: ModulePath::from("lib"),
            source: "export function base(): number { return 40; }",
        }],
        &[],
    )
    .expect("compile external package");
    write_package_artifact(
        store_root.join("@ext").join("dep"),
        &package.wasm,
        &package.type_info,
        &derive_capability_schema(&package.declaration, &[], &package.required_capabilities),
        &package.declaration,
        &ArtifactMetadata::new("@ext/dep", "1.0.0", Vec::new()),
    )
    .expect("write external artifact");
}

fn build(
    project: &Path,
    externals: &PackageStore,
    manifest_toml: &str,
    only: Option<&str>,
) -> Result<Vec<submilli_build::BuiltPackage>, DriverError> {
    let manifest = parse_manifest(manifest_toml, project).expect("manifest parses");
    let only = only.map(PackageName::new);
    build_packages(&manifest, project, externals, only.as_ref())
}

const GRAPH_MANIFEST: &str = r#"
[dependencies]
"@ext/dep" = "1.0.0"

[[package]]
name = "@acme/app"
version = "0.1.0"
description = "App package."
path = "app"
dependencies = ["@acme/util"]

[[package]]
name = "@acme/util"
version = "0.2.0"
description = "Utility package."
path = "util"
dependencies = ["@ext/dep"]
"#;

fn write_graph_project(project: &Path) {
    write_docs(project, "app", "# App\n");
    write_docs(project, "util", "# Util\n");
    write_module(
        project,
        "util/src/lib.subm",
        r#"
            import { base } from "@ext/dep";
            export function answer(): number { return base() + 2; }
        "#,
    );
    write_module(
        project,
        "app/src/wrap.subm",
        r#"
            import { answer } from "@acme/util";
            export function fortyTwo(): number { return answer(); }
        "#,
    );
    write_module(
        project,
        "app/src/lib.subm",
        "export { fortyTwo } from \"./wrap\";\n",
    );
}

#[test]
fn graph_compiles_in_dependency_order_and_artifacts_reload() {
    let externals_dir = TempDir::new().expect("externals tempdir");
    write_external_dep(externals_dir.path());
    let externals = PackageStore::new(externals_dir.path());
    let project = TempDir::new().expect("project tempdir");
    write_graph_project(project.path());

    // The manifest lists app before util; the driver must reorder.
    let built = build(project.path(), &externals, GRAPH_MANIFEST, None).expect("build succeeds");
    let names: Vec<&str> = built.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, vec!["@acme/util", "@acme/app"]);

    let target_dir = TempDir::new().expect("target tempdir");
    let target = PackageStore::new(target_dir.path());
    let dirs = install_packages(&target, &built).expect("install succeeds");
    assert_eq!(dirs.len(), 2);

    let util = target.load("@acme/util").expect("util reloads");
    assert_eq!(util.metadata.package_version, "0.2.0");
    assert_eq!(
        util.metadata.dependencies,
        vec![ArtifactDependency::new("@ext/dep", "1.0.0")]
    );
    let app = target.load("@acme/app").expect("app reloads");
    assert_eq!(
        app.metadata.dependencies,
        vec![ArtifactDependency::new("@acme/util", "0.2.0")]
    );

    let script = r#"
        import { fortyTwo } from "@acme/app";
        function main(): number { return fortyTwo(); }
    "#;
    compile_script(
        script,
        "main.subm",
        FileId(1),
        &[&app.package_declaration],
        &[],
    )
    .expect("consumer compiles against reloaded declaration");
}

#[test]
fn artifact_includes_capabilities_schema_with_literal_requires() {
    let project = TempDir::new().expect("project tempdir");
    write_module(
        project.path(),
        "sdk/src/lib.subm",
        r#"
            import { check } from "submilli:security";
            /**
             * Charge a customer.
             * @param customer Stripe customer id.
             * @capability acme.com/charge { customer }
             */
            export function charge(customer: string): void {
                check("acme.com/charge", { customer });
            }
        "#,
    );
    write_module(
        project.path(),
        "app/src/lib.subm",
        r#"
            import { charge } from "@acme/sdk";
            export function run(): void { charge("cus_123"); }
        "#,
    );
    write_docs(project.path(), "app", "# App\n");
    write_docs(project.path(), "sdk", "# SDK\n");
    let manifest = r#"
[[package]]
name = "@acme/app"
version = "0.1.0"
description = "App package."
path = "app"
dependencies = ["@acme/sdk"]

[[package]]
name = "@acme/sdk"
version = "0.1.0"
description = "SDK package."
path = "sdk"
"#;
    let externals = PackageStore::new(project.path().join("store"));

    let built = build(project.path(), &externals, manifest, None).expect("build succeeds");
    let sdk = built
        .iter()
        .find(|package| package.name.as_str() == "@acme/sdk")
        .expect("sdk package built");
    assert_eq!(sdk.capabilities.namespace, "acme");
    assert_eq!(sdk.capabilities.provides.len(), 1);
    assert_eq!(sdk.capabilities.provides[0].name, "acme.com/charge");
    assert_eq!(
        sdk.capabilities.provides[0].fields["customer"]
            .description
            .as_deref(),
        Some("Stripe customer id.")
    );

    let app = built
        .iter()
        .find(|package| package.name.as_str() == "@acme/app")
        .expect("app package built");
    assert_eq!(app.capabilities.requires.len(), 1);
    assert_eq!(app.capabilities.requires[0].capability, "acme.com/charge");
    assert_eq!(
        app.capabilities.requires[0].filter.as_deref(),
        Some("customer == \"cus_123\"")
    );

    let target_dir = TempDir::new().expect("target tempdir");
    let target = PackageStore::new(target_dir.path());
    install_packages(&target, &built).expect("install succeeds");
    let reloaded = target.load("@acme/app").expect("app reloads");
    assert_eq!(reloaded.capabilities, app.capabilities);
}

#[test]
fn class_members_provide_and_require_capabilities() {
    let project = TempDir::new().expect("project tempdir");
    write_module(
        project.path(),
        "types/src/lib.ts",
        r#"
            import { check } from "submilli:security";
            /** What a request acts for. */
            export interface Owner {
                /** Owning team. */
                teamId: string;
            }
            /** Shared behavior. */
            export class Base {
                /**
                 * Read an item.
                 * @param id Item id.
                 * @capability acme.com/read { id }
                 */
                read(id: string): void { check("acme.com/read", { id }); }
            }
        "#,
    );
    write_module(
        project.path(),
        "sdk/src/lib.ts",
        r#"
            import { check } from "submilli:security";
            import { Base, Owner } from "@acme/types";
            /** A client. */
            export class Client extends Base {
                /**
                 * Open a client.
                 * @param host Target host.
                 * @capability acme.com/open { host }
                 */
                static open(host: string): Client {
                    check("acme.com/open", { host });
                    return new Client();
                }
                /**
                 * Open a client holding a value.
                 * @capability acme.com/wrap { id }
                 */
                static wrap<T>(id: string, value: T): Box<T> {
                    check("acme.com/wrap", { id });
                    return new Box<T>(value);
                }
                /**
                 * Delete an item.
                 * @capability acme.com/delete { owner: $owner.teamId }
                 */
                remove(owner: Owner): void { check("acme.com/delete", { owner: owner.teamId }); }
                /**
                 * Copy from another client.
                 * @capability acme.com/copy { id: $other.id }
                 */
                copy(other: Client): void { check("acme.com/copy", { id: "" }); }
                /**
                 * Never reaches the schema.
                 * @capability acme.com/hidden { id }
                 */
                private hidden(id: string): void { check("acme.com/hidden", { id }); }
                /**
                 * Never reaches the schema.
                 * @capability acme.com/secret { id }
                 */
                private static secret(id: string): void { check("acme.com/secret", { id }); }
            }
            /** Holds a value. */
            export class Box<T> {
                private value: T;
                constructor(value: T) { this.value = value; }
                /**
                 * Replace the value.
                 * @capability acme.com/put { id }
                 */
                put(id: string, value: T): void { check("acme.com/put", { id }); this.value = value; }
            }
        "#,
    );
    write_module(
        project.path(),
        "app/src/lib.ts",
        r#"
            import { check } from "submilli:security";
            import { Client } from "@acme/sdk";
            import { Repository } from "submilli:git";
            // A consumer could not use `Audited` (SUB-1169), so only the
            // package without consumers declares a hidden ancestor.
            class Hidden {
                /**
                 * Audit an item.
                 * @param id Item id.
                 * @capability acme.com/audit { id }
                 */
                audit(id: string): void { check("acme.com/audit", { id }); }
            }
            /** Audits through its ancestor. */
            export class Audited extends Hidden {}
            /** Overrides `remove` without a check of its own. */
            export class Wrapped extends Client {
                remove(owner: { teamId: string }): void { super.remove({ teamId: "via-super" }); }
            }
            /** Calls every kind of class member. */
            export function run(maybe: Client | null, wrapped: Wrapped): void {
                const client = Client.open("example.com");
                client.read("direct");
                wrapped.read("inherited");
                wrapped.remove({ teamId: "overridden" });
                maybe?.read("chained");
                Wrapped.open("inherited.example.com");
                Client.wrap<number>("generic-static", 1).put("generic-method", 2);
            }
            /** Calls the standard library's class members. */
            export function git(): void {
                const repository = Repository.init("/repo");
                repository.fetch("origin", "main");
                Repository.clone("https://GitHub.com", "clone");
            }
        "#,
    );
    for package in ["app", "sdk", "types"] {
        write_docs(project.path(), package, "# Package\n");
    }
    let manifest = r#"
[[package]]
name = "@acme/app"
version = "0.1.0"
description = "App package."
path = "app"
dependencies = ["@acme/sdk"]

[[package]]
name = "@acme/sdk"
version = "0.1.0"
description = "SDK package."
path = "sdk"
dependencies = ["@acme/types"]

[[package]]
name = "@acme/types"
version = "0.1.0"
description = "Types package."
path = "types"
"#;
    let externals = PackageStore::new(project.path().join("store"));

    let built = build(project.path(), &externals, manifest, None).expect("build succeeds");
    let package = |name: &str| {
        built
            .iter()
            .find(|package| package.name.as_str() == name)
            .expect("package built")
    };
    let provides = |name: &str| -> Vec<(String, Vec<(String, String)>)> {
        package(name)
            .capabilities
            .provides
            .iter()
            .map(|cap| {
                let fields = cap
                    .fields
                    .iter()
                    .map(|(name, field)| (name.clone(), field.ty.clone()))
                    .collect();
                (cap.name.clone(), fields)
            })
            .collect()
    };
    let owned = |entries: &[(&str, &[(&str, &str)])]| -> Vec<(String, Vec<(String, String)>)> {
        entries
            .iter()
            .map(|(name, fields)| {
                let fields = fields
                    .iter()
                    .map(|(field, ty)| (field.to_string(), ty.to_string()))
                    .collect();
                (name.to_string(), fields)
            })
            .collect()
    };
    // A method a same-package ancestor declares is provided; one from another
    // package's class is that package's to provide.
    assert_eq!(
        provides("@acme/app"),
        owned(&[("acme.com/audit", &[("id", "string")])])
    );
    assert_eq!(
        provides("@acme/types"),
        owned(&[("acme.com/read", &[("id", "string")])])
    );
    // A path through a class does not resolve, so its type is unknown.
    assert_eq!(
        provides("@acme/sdk"),
        owned(&[
            ("acme.com/copy", &[("id", "unknown")]),
            ("acme.com/delete", &[("owner", "string")]),
            ("acme.com/open", &[("host", "string")]),
            ("acme.com/put", &[("id", "string")]),
            ("acme.com/wrap", &[("id", "string")]),
        ])
    );

    let requires: Vec<(&str, Option<&str>)> = package("@acme/app")
        .capabilities
        .requires
        .iter()
        .map(|required| (required.capability.as_str(), required.filter.as_deref()))
        .collect();
    assert_eq!(
        requires,
        [
            ("acme.com/delete", Some("owner == \"via-super\"")),
            ("acme.com/open", Some("host == \"example.com\"")),
            ("acme.com/open", Some("host == \"inherited.example.com\"")),
            ("acme.com/put", Some("id == \"generic-method\"")),
            ("acme.com/read", Some("id == \"chained\"")),
            ("acme.com/read", Some("id == \"direct\"")),
            ("acme.com/read", Some("id == \"inherited\"")),
            ("acme.com/wrap", Some("id == \"generic-static\"")),
            (
                "git.clone",
                Some(
                    "remote == \"https://github.com/\" \
                     and remoteName == \"origin\""
                )
            ),
            ("git.fetch", Some("remoteName == \"origin\"")),
            ("git.init", Some("path == \"/repo\"")),
        ]
    );
}

#[test]
fn non_literal_requires_warning_has_no_filter() {
    let project = TempDir::new().expect("project tempdir");
    write_module(
        project.path(),
        "sdk/src/lib.subm",
        r#"
            import { check } from "submilli:security";
            /** @capability acme.com/charge { customer } */
            export function charge(customer: string): void {
                check("acme.com/charge", { customer });
            }
        "#,
    );
    write_module(
        project.path(),
        "app/src/lib.subm",
        r#"
            import { charge } from "@acme/sdk";
            export function run(customer: string): void { charge(customer); }
        "#,
    );
    write_docs(project.path(), "app", "# App\n");
    write_docs(project.path(), "sdk", "# SDK\n");
    let manifest = r#"
[[package]]
name = "@acme/app"
version = "0.1.0"
description = "App package."
path = "app"
dependencies = ["@acme/sdk"]

[[package]]
name = "@acme/sdk"
version = "0.1.0"
description = "SDK package."
path = "sdk"
"#;
    let externals = PackageStore::new(project.path().join("store"));

    let built = build(project.path(), &externals, manifest, None).expect("build succeeds");
    let app = built
        .iter()
        .find(|package| package.name.as_str() == "@acme/app")
        .expect("app package built");

    assert_eq!(app.capabilities.requires.len(), 1);
    assert_eq!(app.capabilities.requires[0].filter, None);
    assert!(
        app.warnings
            .iter()
            .any(|warning| warning.contains("non-literal argument")),
        "expected non-literal warning, got: {:?}",
        app.warnings
    );
}

#[test]
fn unresolved_http_host_warning_is_exposed_by_package_build() {
    let project = TempDir::new().expect("project tempdir");
    write_module(
        project.path(),
        "app/src/lib.ts",
        r#"
            import { get } from "submilli:http";

            function endpoint(): string { return "https://api.example.com/items"; }

            export function run(): string { return get(endpoint()).body; }
        "#,
    );
    write_docs(project.path(), "app", "# App\n");
    let manifest = r#"
[[package]]
name = "@acme/app"
version = "0.1.0"
description = "App package."
path = "app"
"#;
    let externals = PackageStore::new(project.path().join("store"));

    let built = build(project.path(), &externals, manifest, None).expect("build succeeds");
    let app = built.first().expect("app package built");

    assert_eq!(app.capabilities.requires.len(), 1);
    assert_eq!(app.capabilities.requires[0].filter.as_deref(), None);
    assert!(
        app.warnings.iter().any(|warning| warning
            .contains("cannot statically resolve the host in the URL passed to `http.get`")),
        "expected unresolved HTTP host warning, got: {:?}",
        app.warnings
    );
}

#[test]
fn sibling_dependency_cycle_is_fatal() {
    let project = TempDir::new().expect("project tempdir");
    write_module(project.path(), "a/src/lib.subm", "export const x = 1;\n");
    write_module(project.path(), "b/src/lib.subm", "export const y = 1;\n");
    let manifest = r#"
[[package]]
name = "@acme/a"
version = "0.1.0"
description = "Package A."
path = "a"
dependencies = ["@acme/b"]

[[package]]
name = "@acme/b"
version = "0.1.0"
description = "Package B."
path = "b"
dependencies = ["@acme/a"]
"#;
    let externals = PackageStore::new(project.path().join("store"));

    let err = build(project.path(), &externals, manifest, None).expect_err("cycle is fatal");

    assert!(matches!(err, DriverError::DependencyCycle { .. }));
    let text = err.to_string();
    assert!(text.contains("circular package dependency"), "got: {text}");
    assert!(
        text.contains("@acme/a -> @acme/b -> @acme/a")
            || text.contains("@acme/b -> @acme/a -> @acme/b"),
        "got: {text}"
    );
}

#[test]
fn missing_external_dependency_lists_available_packages() {
    let externals_dir = TempDir::new().expect("externals tempdir");
    write_external_dep(externals_dir.path());
    let externals = PackageStore::new(externals_dir.path());
    let project = TempDir::new().expect("project tempdir");
    write_module(project.path(), "src/lib.subm", "export const x = 1;\n");
    let manifest = r#"
[dependencies]
"@ext/missing" = "1.0.0"

[[package]]
name = "@acme/app"
version = "0.1.0"
description = "App package."
dependencies = ["@ext/missing"]
"#;

    let err = build(project.path(), &externals, manifest, None).expect_err("missing external");

    assert!(matches!(err, DriverError::MissingExternal { .. }));
    let text = err.to_string();
    assert!(text.contains("@ext/missing"), "got: {text}");
    assert!(text.contains("@ext/dep"), "available list missing: {text}");
}

#[test]
fn external_version_mismatch_is_fatal() {
    let externals_dir = TempDir::new().expect("externals tempdir");
    write_external_dep(externals_dir.path());
    let externals = PackageStore::new(externals_dir.path());
    let project = TempDir::new().expect("project tempdir");
    write_module(project.path(), "src/lib.subm", "export const x = 1;\n");
    let manifest = r#"
[dependencies]
"@ext/dep" = "2.0.0"

[[package]]
name = "@acme/app"
version = "0.1.0"
description = "App package."
dependencies = ["@ext/dep"]
"#;

    let err = build(project.path(), &externals, manifest, None).expect_err("version mismatch");

    assert!(matches!(err, DriverError::ExternalVersionMismatch { .. }));
    let text = err.to_string();
    assert!(
        text.contains("2.0.0") && text.contains("1.0.0"),
        "got: {text}"
    );
}

#[test]
fn only_builds_the_sibling_dependency_closure() {
    let project = TempDir::new().expect("project tempdir");
    write_module(project.path(), "util/src/lib.subm", "export const u = 1;\n");
    write_module(
        project.path(),
        "app/src/lib.subm",
        "import { u } from \"@acme/util\";\nexport const a = u;\n",
    );
    write_module(
        project.path(),
        "other/src/lib.subm",
        "export const o = 1;\n",
    );
    write_docs(project.path(), "app", "# App\n");
    write_docs(project.path(), "util", "# Util\n");
    let manifest = r#"
[[package]]
name = "@acme/app"
version = "0.1.0"
description = "App package."
path = "app"
dependencies = ["@acme/util"]

[[package]]
name = "@acme/util"
version = "0.1.0"
description = "Utility package."
path = "util"

[[package]]
name = "@acme/other"
version = "0.1.0"
description = "Other package."
path = "other"
"#;
    let externals = PackageStore::new(project.path().join("store"));

    let built = build(project.path(), &externals, manifest, Some("@acme/app"))
        .expect("scoped build succeeds");

    let names: Vec<&str> = built.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, vec!["@acme/util", "@acme/app"]);
}

#[test]
fn unknown_only_package_lists_declared_packages() {
    let project = TempDir::new().expect("project tempdir");
    write_module(project.path(), "src/lib.subm", "export const x = 1;\n");
    let manifest = r#"
[[package]]
name = "@acme/app"
version = "0.1.0"
description = "App package."
"#;
    let externals = PackageStore::new(project.path().join("store"));

    let err = build(project.path(), &externals, manifest, Some("@acme/nope"))
        .expect_err("unknown package");

    assert!(matches!(err, DriverError::UnknownPackage { .. }));
    let text = err.to_string();
    assert!(
        text.contains("@acme/nope") && text.contains("@acme/app"),
        "got: {text}"
    );
}

#[test]
fn compile_errors_render_with_source_path_and_caret() {
    let project = TempDir::new().expect("project tempdir");
    write_module(project.path(), "src/lib.subm", "export const x = 1;\n");
    write_module(
        project.path(),
        "src/extra.subm",
        "export function bad(): number { return \"x\"; }\n",
    );
    write_docs(project.path(), ".", "# App\n");
    let manifest = r#"
[[package]]
name = "@acme/app"
version = "0.1.0"
description = "App package."
"#;
    let externals = PackageStore::new(project.path().join("store"));

    let err = build(project.path(), &externals, manifest, None).expect_err("compile error");

    let DriverError::Compile { package, rendered } = err else {
        panic!("expected Compile error, got: {err}");
    };
    assert_eq!(package.as_str(), "@acme/app");
    assert!(rendered.contains("error:"), "got: {rendered}");
    assert!(
        rendered.contains("--> src/extra.subm:"),
        "missing source path: {rendered}"
    );
    assert!(rendered.contains('^'), "missing caret: {rendered}");
}

#[test]
fn capability_payload_fields_are_checked_before_building_artifacts() {
    let project = TempDir::new().unwrap();
    write_docs(project.path(), "sdk", "# SDK\n");
    let manifest = r#"
[[package]]
name = "@acme/sdk"
version = "0.1.0"
description = "SDK package."
path = "sdk"
"#;
    let store = PackageStore::new(project.path().join("store"));
    for payload in ["{ customer, total }", "context"] {
        let source = format!(
            r#"
import {{ check }} from "submilli:security";
interface Context {{ customer: string; total?: number }}
/**
 * Checks a purchase.
 * @param customer Customer identifier.
 * @param total Purchase total.
 * @capability acme.com/v {{ customer }}
 */
export function v(customer: string, total: number): void {{
    const context: Context = {{ customer, total }};
    check("acme.com/v", {payload});
}}
"#
        );
        write_module(project.path(), "sdk/src/lib.ts", &source);
        let error = build(project.path(), &store, manifest, None).unwrap_err();
        assert!(
            error.to_string().contains("payload key `total` missing"),
            "{error}"
        );
        let fixed = source.replace("{ customer }", "{ customer, total }");
        write_module(project.path(), "sdk/src/lib.ts", &fixed);
        let built = build(project.path(), &store, manifest, None).unwrap();
        let capability = &built[0].capabilities.provides[0];
        assert_eq!(
            capability
                .fields
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["customer", "total"]
        );
        install_packages(&store, &built).unwrap();
        let yaml = "name: fields\npackages: ['@acme/sdk']\npermissions:\n  main:\n    - capability: acme.com/v\n      filter: total < 100\n      action: allow\n";
        let blueprint = submilli_blueprint::parse(yaml).unwrap();
        submilli_build::blueprint_validation::validate_packages(&blueprint, &store).unwrap();
        let invalid = submilli_blueprint::parse(&yaml.replace("total <", "typo <")).unwrap();
        let error =
            submilli_build::blueprint_validation::validate_packages(&invalid, &store).unwrap_err();
        let submilli_build::blueprint_validation::PackageValidationError::InvalidFilter(problem) =
            error
        else {
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
}
