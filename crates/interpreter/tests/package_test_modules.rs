//! A package's tests import its non-entry modules by relative path; only a
//! test build of the package exports them (SUB-1236).

use std::collections::{BTreeMap, BTreeSet};

use submilli_engine::compile::compile_package_configured;
use submilli_engine::runtime::{
    LinkedPackageModule, StoreData, Vfs, install_package_modules_async, install_runtime_async,
    install_tenant_limits,
};
use submilli_engine::{
    CompiledPackage, FileId, ModulePath, PackageSourceModule, PackageSurface, PackageTestSurface,
    RuntimeConfig, Severity, Stdlib, compile_package_test, compile_script, dispatch_main_async,
};
use wasmtime::{Linker, Module};

const PACKAGE: &str = "@test/paths";

const LIB: &str = r#"
import { customerPath } from "./paths";
/** The length of a customer path. @returns its length */
export function answer(): number { return customerPath("c").length; }
"#;

const PATHS: &str = r#"
export const BASE: string = "/v1";
export let counter: number = 0;
export interface Route { path: string; depth: number }
export function customerPath(id: string): string {
  counter = counter + 1;
  return BASE + "/customers/" + id;
}
export function route(id: string): Route { return { path: customerPath(id), depth: 2 }; }
export function first<T>(items: T[]): T { return items[0]; }
export class Builder {
  private parts: string[] = [];
  static separator: string = "/";
  static create(): Builder { return new Builder(); }
  add(part: string): Builder { this.parts.push(part); return this; }
  build(): string { return this.parts.join(Builder.separator); }
}
export function makeBuilder(): Builder { return new Builder(); }
class Base { greet(): string { return "hi"; } }
export class Derived extends Base { name(): string { return "d"; } }
function notExported(): number { return 1; }
"#;

fn modules<'a>(entries: &[(&str, &'a str)]) -> Vec<PackageSourceModule<'a>> {
    entries
        .iter()
        .map(|(path, source)| PackageSourceModule {
            path: ModulePath::from(*path),
            source,
        })
        .collect()
}

fn compile(entries: &[(&str, &str)], surface: PackageSurface) -> CompiledPackage {
    compile_package_configured(
        Stdlib::core(),
        PACKAGE,
        ModulePath::from("lib"),
        &modules(entries),
        &[],
        &[],
        &BTreeMap::new(),
        surface,
    )
    .expect("package compiles")
}

fn paths_package(surface: PackageSurface) -> CompiledPackage {
    compile(&[("lib", LIB), ("paths", PATHS)], surface)
}

fn export_names(wasm: &[u8]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        if let wasmparser::Payload::ExportSection(exports) = payload.expect("valid module") {
            for export in exports {
                names.insert(export.expect("valid export").name.to_string());
            }
        }
    }
    names
}

fn compile_test(
    package: &CompiledPackage,
    importer: &str,
    source: &str,
) -> Result<submilli_engine::CompiledScript, Vec<String>> {
    let no_modules = PackageTestSurface::default();
    compile_package_test(
        &package.declaration,
        package.test_surface.as_ref().unwrap_or(&no_modules),
        &ModulePath::from(importer),
        source,
        "paths.test.ts",
        FileId(0),
        &[&package.declaration],
    )
    .map_err(|diagnostics| {
        diagnostics
            .into_iter()
            .filter(|diagnostic| diagnostic.severity == Severity::Error)
            .map(|diagnostic| format!("{} | {}", diagnostic.message, diagnostic.help.join(" ")))
            .collect()
    })
}

fn run_test(package: &CompiledPackage, source: &str) -> String {
    let compiled = compile_test(package, "tests/paths.test", source).expect("test compiles");
    let config = RuntimeConfig::default();
    let engine = config.engine_async().unwrap();
    let data = StoreData::with_vfs("test-run".into(), Vfs::tempdir().unwrap());
    let mut store = config.store_async(&engine, data).unwrap();
    install_tenant_limits(&mut store);
    let mut linker = Linker::new(&engine);
    let package_module = Module::new(&engine, &package.wasm).unwrap();
    let linked = [LinkedPackageModule {
        module: &package_module,
        declaration: &package.declaration,
        type_info: &package.type_info,
    }];
    let module = Module::new(&engine, &compiled.wasm).unwrap();
    pollster::block_on(async {
        install_runtime_async(&mut linker, &mut store)
            .await
            .unwrap();
        install_package_modules_async(&mut linker, &mut store, &linked)
            .await
            .unwrap();
        store
            .data_mut()
            .install_type_info(compiled.type_info.clone());
        let instance = linker.instantiate_async(&mut store, &module).await.unwrap();
        let _watchdog = config.arm_timeout(&engine).unwrap();
        dispatch_main_async(&mut store, &instance)
            .await
            .unwrap()
            .expect("main result")
    })
}

#[test]
fn only_a_test_build_exports_non_entry_modules() {
    let public = paths_package(PackageSurface::Public);
    let test = paths_package(PackageSurface::WithTestModules);

    let internal = format!("mod:{PACKAGE}#paths#customerPath");
    let internal_static = format!("mod:{PACKAGE}#paths#Builder#static#create");
    let public_exports = export_names(&public.wasm);
    let test_exports = export_names(&test.wasm);
    assert!(!public_exports.contains(&internal), "{public_exports:?}");
    assert!(!public_exports.contains(&internal_static));
    assert!(test_exports.contains(&internal), "{test_exports:?}");
    assert!(test_exports.contains(&internal_static), "{test_exports:?}");
    assert!(public_exports.is_subset(&test_exports));

    assert_eq!(public.test_surface, None);
    let surface = test.test_surface.as_ref().expect("test surface");
    assert_eq!(
        surface
            .modules
            .keys()
            .map(ModulePath::as_str)
            .collect::<Vec<_>>(),
        ["paths"]
    );
    // The source surface is the same. Internal functions widen their Wasm
    // signatures in a test build, since a test can pass them any value.
    assert_eq!(public.declaration.values, test.declaration.values);
    assert_eq!(public.declaration.types, test.declaration.types);
    assert_eq!(
        public.declaration.runtime_types,
        test.declaration.runtime_types
    );
    let answer = submilli_engine::mangle::package_symbol(PACKAGE, "answer");
    assert_eq!(
        public.declaration.runtime_functions.get(&answer),
        test.declaration.runtime_functions.get(&answer)
    );
    assert_eq!(public.required_capabilities, test.required_capabilities);
    assert_eq!(public.authority_map, test.authority_map);
    assert_eq!(public.warnings, test.warnings);
}

#[test]
fn a_declaration_two_modules_export_is_exported_once() {
    let test = compile(
        &[
            (
                "lib",
                "import { customerPath } from \"./routes\";\n\
                 /** A path. @returns it */\n\
                 export function answer(): string { return customerPath(\"c\"); }\n\
                 export { customerPath } from \"./paths\";\n",
            ),
            ("paths", PATHS),
            (
                "routes",
                "export { customerPath, Builder } from \"./paths\";\n",
            ),
        ],
        PackageSurface::WithTestModules,
    );
    wasmparser::validate(&test.wasm).expect("no duplicate exports");
    let names = export_names(&test.wasm);
    assert!(names.contains(&format!("mod:{PACKAGE}#paths#customerPath")));
}

#[test]
fn a_test_reaches_internal_declarations_through_the_linked_instance() {
    let package = paths_package(PackageSurface::WithTestModules);
    let result = run_test(
        &package,
        r#"
import { customerPath, BASE, counter, route, Route, first, Builder, makeBuilder, Derived } from "../src/paths";
import * as paths from "../src/paths";
import { answer } from "@test/paths";
function main(): string {
  const length = answer();
  const r: Route = route("x");
  const derived = new Derived();
  return [
    customerPath("42"),
    BASE,
    String(length),
    String(counter),
    r.path,
    String(r.depth),
    String(first([3, 4])),
    first(["a"]),
    makeBuilder().add("a").add("b").build(),
    Builder.create().add("z").build() + Builder.separator,
    derived.greet() + derived.name(),
    paths.customerPath("n"),
  ].join(" ");
}
"#,
    );
    // `counter` reads 3: the public `answer`, `customerPath("42")` and `route`
    // all ran in the one package instance the test links against.
    assert_eq!(
        result,
        "/v1/customers/42 /v1 15 3 /v1/customers/x 2 3 a a/b z/ hid /v1/customers/n"
    );
}

#[test]
fn a_test_may_import_only_its_own_packages_src_modules() {
    let package = paths_package(PackageSurface::WithTestModules);
    let cases = [
        (
            "import { x } from \"../../../other/src/x\";",
            "import escapes package directory: `../../../other/src/x` climbs above the package root | \
             a test may import only its own package's `src/` modules by relative path; import \
             another package by name: `import { … } from \"@org/pkg\"`",
        ),
        (
            "import { y } from \"./helpers\";",
            "`./helpers` resolves to `tests/sub/helpers`, outside the package's `src/` | move \
             shared code into `src/` (e.g. `../../src/helpers`) or import the other package by name",
        ),
        (
            "import { answer } from \"../../src/lib\";",
            "`../../src/lib` is the package's entry module | import it by name: `from \"@test/paths\"`",
        ),
        (
            "import { z } from \"../../src/pahts\";",
            "no such module `../../src/pahts` | modules in src/: `../../src/paths`",
        ),
        (
            "import { notExported } from \"../../src/paths\";",
            "`notExported` is not exported from `../../src/paths` | add `export` to `notExported` \
             in `src/paths`; non-entry exports reach only this package's tests, never programs or \
             `submilli docs`",
        ),
    ];
    for (import, expected) in cases {
        let source = format!("{import}\nfunction main(): void {{}}\n");
        let errors = compile_test(&package, "tests/sub/refusals.test", &source)
            .expect_err("import is refused");
        assert_eq!(errors, [expected], "{import}");
    }

    let errors = compile_test(
        &package,
        "tests/paths.test",
        "import { nothing } from \"../src/paths\";\nfunction main(): void {}\n",
    )
    .expect_err("missing export");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].starts_with("module `../src/paths` does not export `nothing` | exports: `BASE`"),
        "{errors:?}"
    );
}

#[test]
fn a_public_build_gives_tests_no_modules() {
    let package = paths_package(PackageSurface::Public);
    let errors = compile_test(
        &package,
        "tests/paths.test",
        "import { customerPath } from \"../src/paths\";\nfunction main(): void {}\n",
    )
    .expect_err("no test surface");
    assert_eq!(
        errors,
        ["no such module `../src/paths` | `src/` has no modules besides the entry module"]
    );
}

#[test]
fn an_ordinary_script_keeps_the_single_file_error() {
    let errors: Vec<String> = compile_script(
        "import { customerPath } from \"./paths\";\nfunction main(): void {}\n",
        "main.ts",
        FileId(0),
        &[],
        &[],
    )
    .expect_err("relative import is refused")
    .into_iter()
    .map(|diagnostic| diagnostic.message)
    .collect();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].contains("this is a single-file program"),
        "{errors:?}"
    );
}

#[test]
fn check_discipline_sees_a_non_entry_function_as_internal_in_both_builds() {
    let ops = "import { check } from \"submilli:security\";\n\
               export interface Input { channelId: string }\n\
               function post(channelId: string): void {}\n\
               export function guard(input: Input): void {\n\
                 check(\"x/guard\", { channelId: input.channelId });\n\
                 post(input.channelId);\n\
               }\n";
    let lib = "import { guard } from \"./ops\";\n\
               /** Send. @param channelId where to send */\n\
               export function send(channelId: string): void { guard({ channelId }); }\n";
    let entries = [("lib", lib), ("ops", ops)];
    let public = compile(&entries, PackageSurface::Public);
    let test = compile(&entries, PackageSurface::WithTestModules);
    let messages = |package: &CompiledPackage| -> Vec<String> {
        package
            .warnings
            .iter()
            .map(|warning| warning.message.clone())
            .filter(|message| message.contains("check()") && !message.contains("@capability"))
            .collect()
    };
    assert_eq!(
        messages(&public),
        [
            "`check()` is called in `guard`, which is not part of the package's public API",
            "`input.channelId` is read more than once in `guard`, which calls `check()`",
        ]
    );
    assert_eq!(public.warnings, test.warnings);
    assert!(export_names(&test.wasm).contains(&format!("mod:{PACKAGE}#ops#guard")));
}
