//! Compiling through a `&dyn FrontEnd`, as the server and the CLI do
//! (SUB-2285), including a front end that isn't the engine's.

use submilli_engine::compile::{
    CompiledPackage, compile_package_request, compile_parsed_script, package_modules,
    package_sources,
};
use submilli_engine::runtime::{
    LinkedPackageModule, StoreData, Vfs, install_package_modules_async, install_runtime_async,
    install_tenant_limits,
};
use submilli_engine::{
    CheckedPackage, CheckedScript, CompileError, CompiledScript, Diagnostic, EngineFrontEnd,
    FileId, FrontEnd, ModulePath, PackageDeclaration, PackageRequest, PackageSourceModule,
    PackageSurface, ParsedScript, RuntimeConfig, ScriptRequest, Severity, Stdlib, classify_imports,
    dispatch_main_async, runtime_declarations, script_request,
};
use wasmtime::{Linker, Module};

const PACKAGE: &str = "@test/greeting";

const LIB: &str = r#"
/** The greeting for `name`. @returns the greeting */
export function greet(name: string): string { return "hello, " + name; }
"#;

const MAIN: &str = r#"
import { greet } from "@test/greeting";
function main(): string { return greet("world"); }
"#;

/// A front end of its own that hands back the tree the engine's front end
/// built, as another implementation (ts-rust) would hand back its own. It
/// drops the engine front end's warnings, so a Submilli diagnostic that still
/// appears comes from the engine's back end, not from a front end.
struct Relay;

struct RelayedScript(Box<dyn ParsedScript>);

impl FrontEnd for Relay {
    fn parse_script(&self, source: String, file: FileId) -> Box<dyn ParsedScript> {
        Box::new(RelayedScript(EngineFrontEnd.parse_script(source, file)))
    }

    fn check_package(&self, request: &PackageRequest<'_>) -> Result<CheckedPackage, CompileError> {
        let mut checked = EngineFrontEnd.check_package(request)?;
        checked.diagnostics.retain(is_error);
        Ok(checked)
    }
}

impl ParsedScript for RelayedScript {
    fn source(&self) -> &str {
        self.0.source()
    }

    fn file(&self) -> FileId {
        self.0.file()
    }

    fn diagnostics(&self) -> &[Diagnostic] {
        self.0.diagnostics()
    }

    fn imports(&self) -> Result<Vec<String>, CompileError> {
        self.0.imports()
    }

    fn check(self: Box<Self>, request: &ScriptRequest<'_>) -> Result<CheckedScript, CompileError> {
        let mut checked = self.0.check(request)?;
        checked.diagnostics.retain(is_error);
        Ok(checked)
    }
}

fn is_error(diagnostic: &Diagnostic) -> bool {
    diagnostic.severity == Severity::Error
}

/// A thread with the stack of a runtime worker, far less than the compiler
/// needs. Every entry point runs on its own stack, so this is enough.
fn on_small_stack<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(work)
        .unwrap()
        .join()
        .unwrap()
}

fn compile_package(front_end: &dyn FrontEnd, source: &str) -> CompiledPackage {
    let stdlib = Stdlib::core().package_declarations();
    let stdlib: Vec<&PackageDeclaration> = stdlib.iter().collect();
    let sources = package_sources(&[PackageSourceModule {
        path: ModulePath::from("lib"),
        source,
    }])
    .unwrap();
    let modules = package_modules(&sources);
    let runtime = runtime_declarations();
    compile_package_request(
        front_end,
        &PackageRequest {
            name: PACKAGE,
            root_module: &ModulePath::from("lib"),
            modules: &modules,
            sources: &sources,
            prelude: runtime.prelude,
            host: runtime.host,
            stdlib: &stdlib,
            dependencies: &[],
            transitive: &[],
            config: None,
            surface: PackageSurface::Public,
        },
    )
    .expect("the package compiles")
}

fn compile_script(
    front_end: &dyn FrontEnd,
    source: &str,
    packages: &[&PackageDeclaration],
) -> Result<CompiledScript, CompileError> {
    let stdlib = Stdlib::core().package_declarations();
    let stdlib: Vec<&PackageDeclaration> = stdlib.iter().collect();
    let parsed = front_end.parse_script(source.to_owned(), FileId(0));
    let imports = classify_imports(&parsed.imports().unwrap(), &stdlib);
    for package in packages {
        assert!(
            imports.registry_packages.contains(&package.package_name),
            "{imports:?}"
        );
    }
    compile_parsed_script(
        parsed,
        "main.ts",
        &ScriptRequest {
            dependencies: packages,
            ..script_request(&stdlib)
        },
    )
}

fn run(package: &CompiledPackage, script: &CompiledScript) -> String {
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
    let module = Module::new(&engine, &script.wasm).unwrap();
    pollster::block_on(async {
        install_runtime_async(&mut linker, &mut store)
            .await
            .unwrap();
        install_package_modules_async(&mut linker, &mut store, &linked)
            .await
            .unwrap();
        store.data_mut().install_type_info(script.type_info.clone());
        let instance = linker.instantiate_async(&mut store, &module).await.unwrap();
        let _watchdog = config.arm_timeout(&engine).unwrap();
        dispatch_main_async(&mut store, &instance)
            .await
            .unwrap()
            .expect("main result")
    })
}

fn error_messages(error: CompileError) -> Vec<String> {
    error
        .into_diagnostics(FileId(0))
        .into_iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Error)
        .map(|diagnostic| diagnostic.message)
        .collect()
}

fn front_ends() -> [&'static dyn FrontEnd; 2] {
    [&EngineFrontEnd, &Relay]
}

#[test]
fn a_script_and_a_package_compile_and_run_through_either_front_end() {
    for front_end in front_ends() {
        let (package, script) = on_small_stack(move || {
            let package = compile_package(front_end, LIB);
            let script = compile_script(front_end, MAIN, &[&package.declaration]).unwrap();
            (package, script)
        });
        assert_eq!(run(&package, &script), "hello, world");
    }
}

#[test]
fn submilli_rules_run_whichever_front_end_built_the_tree() {
    // A script without `main` is fine TypeScript; only Submilli rejects it.
    let source = "function helper(): number { return 1; }\n";
    for front_end in front_ends() {
        let errors = error_messages(compile_script(front_end, source, &[]).unwrap_err());
        assert_eq!(errors, ["missing `main` function"]);
    }
    // An exported function without a doc comment warns, from Submilli's
    // package rules.
    let undocumented = "export function greet(): string { return \"hi\"; }\n";
    for front_end in front_ends() {
        let package = compile_package(front_end, undocumented);
        assert!(
            package
                .warnings
                .iter()
                .any(|warning| warning.message == "exported symbol `greet` has no doc comment"),
            "{:?}",
            package.warnings
        );
    }
}

#[test]
fn type_errors_and_submilli_errors_report_together() {
    // The front end keeps its errors with the tree, so Submilli's rules still
    // run, and report after them.
    let source = "function helper(): number { return \"one\"; }\n";
    for front_end in front_ends() {
        let errors = error_messages(compile_script(front_end, source, &[]).unwrap_err());
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert_eq!(errors[1], "missing `main` function", "{errors:?}");
    }
}

/// A package that stops before inference builds its tree: its errors, with
/// no internal failure from the rules that have no tree to run on.
fn package_errors(root_module: &str, source: &str) -> Vec<Vec<String>> {
    let stdlib = Stdlib::core().package_declarations();
    let stdlib: Vec<&PackageDeclaration> = stdlib.iter().collect();
    let sources = package_sources(&[PackageSourceModule {
        path: ModulePath::from("lib"),
        source,
    }])
    .unwrap();
    let modules = package_modules(&sources);
    let runtime = runtime_declarations();
    let mut reports = Vec::new();
    for front_end in front_ends() {
        let error = compile_package_request(
            front_end,
            &PackageRequest {
                name: PACKAGE,
                root_module: &ModulePath::from(root_module),
                modules: &modules,
                sources: &sources,
                prelude: runtime.prelude,
                host: runtime.host,
                stdlib: &stdlib,
                dependencies: &[],
                transitive: &[],
                config: None,
                surface: PackageSurface::Public,
            },
        )
        .unwrap_err();
        assert!(error.fatal.is_none(), "{error:?}");
        reports.push(error_messages(error));
    }
    reports
}

#[test]
fn a_package_whose_imports_dont_resolve_reports_only_that() {
    let source = "import { gone } from \"./missing\";\nexport function f(): number { return 1; }\n";
    for errors in package_errors("lib", source) {
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("./missing"), "{errors:?}");
    }
}

#[test]
fn a_package_without_its_root_module_reports_only_that() {
    for errors in package_errors("nope", LIB) {
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("nope"), "{errors:?}");
    }
}

#[test]
fn a_package_whose_interfaces_stop_inference_reports_only_that() {
    // Inference stops at this inheritance error with the root module already
    // set, so no rule may run on the partial tree.
    let mut source = String::from("export interface AChild extends Alias0 {}\n");
    for index in 0..64 {
        source.push_str(&format!("type Alias{index} = Alias{};\n", index + 1));
    }
    source.push_str("type Alias64 = ZBase;\ninterface ZBase { [key: string]: number; }\n");
    for errors in package_errors("lib", &source) {
        assert_eq!(
            errors,
            ["interface inheritance alias limit exceeded"],
            "{errors:?}"
        );
    }
}

#[test]
fn syntax_errors_are_reported_before_any_package_loads() {
    let front_end: &dyn FrontEnd = &EngineFrontEnd;
    let parsed = front_end.parse_script("function main(): number { return ; ".into(), FileId(0));
    assert!(parsed.has_errors(), "{:?}", parsed.diagnostics());
    let stdlib = Stdlib::core().package_declarations();
    let stdlib: Vec<&PackageDeclaration> = stdlib.iter().collect();
    assert!(compile_parsed_script(parsed, "main.ts", &script_request(&stdlib)).is_err());
}

#[test]
fn imports_are_specifiers_in_source_order_once() {
    let parsed = EngineFrontEnd.parse_script(
        r#"
            import { b } from "@x/b";
            import { local } from "./local";
            import { a } from "@x/a";
            import { c } from "@x/b";
            import linear from "@mcp/linear";
            function main(): void {}
        "#
        .into(),
        FileId(0),
    );
    assert_eq!(parsed.imports().unwrap(), ["@x/b", "@x/a", "@mcp/linear"]);
}
