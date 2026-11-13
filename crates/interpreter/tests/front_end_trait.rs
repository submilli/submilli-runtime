//! Compiling through a `&dyn FrontEnd`, as the server and the CLI do (SUB-2285).

use submilli_engine::compile::{
    CompiledPackage, compile_package_request, compile_parsed_script, package_modules,
    package_sources,
};
use submilli_engine::runtime::{
    LinkedPackageModule, StoreData, Vfs, install_package_modules_async, install_runtime_async,
    install_tenant_limits,
};
use submilli_engine::{
    CompiledScript, EngineFrontEnd, FileId, FrontEnd, ModulePath, PackageDeclaration,
    PackageRequest, PackageSourceModule, PackageSurface, RuntimeConfig, ScriptRequest, Severity,
    Stdlib, dispatch_main_async, runtime_declarations, script_request,
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

fn compile_package(front_end: &dyn FrontEnd) -> CompiledPackage {
    let stdlib = Stdlib::core().package_declarations();
    let stdlib: Vec<&PackageDeclaration> = stdlib.iter().collect();
    let sources = package_sources(&[PackageSourceModule {
        path: ModulePath::from("lib"),
        source: LIB,
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

fn compile_main(front_end: &dyn FrontEnd, package: &PackageDeclaration) -> CompiledScript {
    let stdlib = Stdlib::core().package_declarations();
    let stdlib: Vec<&PackageDeclaration> = stdlib.iter().collect();
    let parsed = front_end.parse_script(MAIN.to_owned(), FileId(0));
    let imports = parsed.imports(&stdlib).unwrap();
    assert!(imports.registry_packages.contains(PACKAGE), "{imports:?}");
    compile_parsed_script(
        parsed,
        "main.ts",
        &ScriptRequest {
            dependencies: &[package],
            ..script_request(&stdlib)
        },
    )
    .expect("the script compiles")
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

#[test]
fn a_script_and_a_package_compile_through_a_dyn_front_end() {
    let (package, script) = on_small_stack(|| {
        let front_end: &dyn FrontEnd = &EngineFrontEnd;
        let package = compile_package(front_end);
        let script = compile_main(front_end, &package.declaration);
        (package, script)
    });
    assert_eq!(run(&package, &script), "hello, world");
}

#[test]
fn syntax_errors_are_reported_before_any_package_loads() {
    let front_end: &dyn FrontEnd = &EngineFrontEnd;
    let parsed = front_end.parse_script("function main(): number { return ; ".into(), FileId(0));
    assert!(
        parsed
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error),
        "{:?}",
        parsed.diagnostics()
    );
    let stdlib = Stdlib::core().package_declarations();
    let stdlib: Vec<&PackageDeclaration> = stdlib.iter().collect();
    assert!(parsed.check(&script_request(&stdlib)).is_err());
}

#[test]
fn the_engine_front_end_names_itself_and_its_version() {
    assert_eq!(
        EngineFrontEnd.id(),
        concat!("submilli-engine/", env!("CARGO_PKG_VERSION"))
    );
}
