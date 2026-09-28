//! Supported closure ABI boundaries. Oversized source diagnostics belong to
//! SUB-633 item 02 stages 2–3; stage 1 checks the shared conversion separately.

use interpreter::compiler_limits::MAX_CLOSURE_ARITY;
use interpreter::runtime::{
    LinkedPackageModule, StoreData, Vfs, install_package_modules_async, install_runtime_async,
    install_tenant_limits,
};
use interpreter::{
    CompiledPackage, FileId, ModulePath, PackageSourceModule, RuntimeConfig, compile_package,
    compile_script, dispatch_main_async,
};
use wasmtime::{Linker, Module};

#[test]
fn declared_parameter_boundary_runs() {
    for arity in [MAX_CLOSURE_ARITY - 1, MAX_CLOSURE_ARITY] {
        let params = parameters(arity, "number");
        let args = arguments(arity);
        let result = format!("a0 + a{}", arity - 1);
        let cases = [
            (
                "arrow",
                format!(
                    "function main(): number {{ const f = ({params}): number => {result}; return f({args}); }}"
                ),
            ),
            (
                "named",
                format!(
                    "function f({params}): number {{ return {result}; }} function main(): number {{ return f({args}); }}"
                ),
            ),
            (
                "adapter",
                format!(
                    "function f({params}): number {{ return {result}; }} function main(): number {{ const g = f; return g({args}); }}"
                ),
            ),
            (
                "void",
                format!(
                    "function main(): number {{ let result = 0; const f = ({params}): void => {{ result = {result}; }}; f({args}); return result; }}"
                ),
            ),
            (
                "capture",
                format!(
                    "function main(): number {{ const offset = 1; const f = ({params}): number => offset + a{}; return f({args}); }}",
                    arity - 1
                ),
            ),
            (
                "interface",
                format!(
                    "interface Callable {{ f({params}): number; }} class C implements Callable {{ bias: number = 1; f({params}): number {{ return this.bias + a{}; }} }} function main(): number {{ const c: Callable = new C(); return c.f({args}); }}",
                    arity - 1
                ),
            ),
        ];
        for (name, source) in cases {
            check_script(&format!("{name}_{arity}"), &source, arity + 1);
        }
    }
}

#[test]
fn defaults_and_rest_count_declared_slots() {
    let fixed = MAX_CLOSURE_ARITY - 1;
    let params = parameters(fixed, "number");
    let args = arguments(fixed);
    let defaulted = format!(
        "function f({params}, last: number = 255): number {{ return a0 + last; }} function main(): number {{ return f({args}); }}"
    );
    check_script("default_omitted", &defaulted, 256);
    let supplied = defaulted.replace(&format!("f({args})"), &format!("f({args}, 300)"));
    check_script("default_supplied", &supplied, 301);
    let adapted = format!(
        "function f({params}, last: number = 255): number {{ return a0 + last; }} function main(): number {{ const g = (f as unknown) as ({params}) => number; return g({args}); }}"
    );
    check_script("default_adapter", &adapted, 256);

    // 257 source arguments become 254 fixed slots plus one packed rest slot.
    let rest = format!(
        "function main(): number {{ const f = ({params}, ...tail: number[]): number => a0 + tail.length + tail[tail.length - 1]; return f({args}, 255, 256, 257); }}"
    );
    check_script("rest_packed", &rest, 261);
}

#[test]
fn generic_descriptors_and_bound_receivers_are_separate() {
    let arity = MAX_CLOSURE_ARITY;
    let params = parameters(arity, "T");
    let args = arguments(arity);
    let generic = format!(
        "function make<T>(): ({params}) => T {{ return ({params}): T => a{}!; }} function main(): number {{ const f = make<number>(); return f({args}); }}",
        arity - 1,
    );
    check_script("generic_capture", &generic, arity);

    let params = parameters(arity, "number");
    let bound = format!(
        "class C {{ bias: number = 1; make(): ({params}) => number {{ return ({params}): number => this.bias + a{}; }} }} function main(): number {{ const f = new C().make(); return f({args}); }}",
        arity - 1,
    );
    check_script("lexical_receiver", &bound, arity + 1);
    let explicit = format!(
        "function main(): number {{ const f = function(this: {{ bias: number }}, {params}): number {{ return this.bias + a{}; }}; const c = {{ bias: 1, f }}; return c.f({args}); }}",
        arity - 1,
    );
    check_script("explicit_receiver", &explicit, arity + 1);
}

#[test]
fn boundary_calls_preserve_argument_evaluation_order() {
    let arity = MAX_CLOSURE_ARITY;
    let params = parameters(arity, "number");
    let args = vec!["next()"; arity].join(", ");
    let source = format!(
        "let counter = 0; function next(): number {{ counter += 1; return counter; }}
         function main(): number {{ const f = ({params}): number => a0 * 1000 + a{}; const value = f({args}); return value * 1000 + counter; }}",
        arity - 1,
    );
    check_script("evaluation_order", &source, (1000 + arity) * 1000 + arity);
}

#[test]
fn imported_signatures_and_inherited_methods_link_at_boundary() {
    let arity = MAX_CLOSURE_ARITY;
    let params = parameters(arity, "number");
    let args = arguments(arity);
    let result = format!("a0 + a{}", arity - 1);
    let library = format!(
        "export function tail({params}): number {{ return {result}; }}
         export function make(): ({params}) => number {{ return ({params}): number => {result}; }}
         export class Base {{ bias: number = 1; f({params}): number {{ return this.bias + a{}; }} }}",
        arity - 1,
    );
    let source = format!(
        "import {{ tail, make, Base }} from 'arity';
         class Derived extends Base {{}}
         function main(): number {{ const adapted = tail; const closure = make(); const c = new Derived(); return adapted({args}) + closure({args}) + c.f({args}); }}"
    );
    let package = compile_package(
        "arity",
        ModulePath::from("lib"),
        &[PackageSourceModule {
            path: ModulePath::from("lib"),
            source: &library,
        }],
        &[],
    )
    .expect("boundary package compiles");
    export_oracle(
        "imported",
        &source.replace("from 'arity'", "from './arity'"),
    );
    export_oracle("arity", &library);
    assert_eq!(run(&source, &[package]), (3 * (arity + 1)).to_string());
}

fn parameters(arity: usize, ty: &str) -> String {
    (0..arity)
        .map(|i| format!("a{i}: {ty}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn arguments(arity: usize) -> String {
    (1..=arity)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn check_script(name: &str, source: &str, expected: usize) {
    export_oracle(name, source);
    assert_eq!(run(source, &[]), expected.to_string(), "{name}");
}

// Opt-in export lets the TypeScript/Node oracle consume the exact generated
// programs. Only the package import specifier needs a filesystem adapter.
fn export_oracle(name: &str, source: &str) {
    if let Some(dir) = std::env::var_os("SUBMILLI_ARITY_ORACLE_DIR") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let suffix = if name == "arity" {
            ""
        } else {
            "\nconsole.log(main());\nexport {};\n"
        };
        std::fs::write(dir.join(format!("{name}.ts")), format!("{source}{suffix}")).unwrap();
    }
}

fn run(source: &str, packages: &[CompiledPackage]) -> String {
    let declarations: Vec<_> = packages.iter().map(|p| &p.declaration).collect();
    let compiled = compile_script(source, "arity.ts", FileId(0), &declarations, &[])
        .expect("boundary script compiles");
    let config = RuntimeConfig::default();
    let engine = config.engine_async().unwrap();
    let data = StoreData::with_vfs(Vfs::tempdir().unwrap());
    let mut store = config.store_async(&engine, data).unwrap();
    install_tenant_limits(&mut store);
    let mut linker = Linker::new(&engine);
    let modules: Vec<_> = packages
        .iter()
        .map(|p| Module::new(&engine, &p.wasm).unwrap())
        .collect();
    let linked: Vec<_> = packages
        .iter()
        .zip(&modules)
        .map(|(package, module)| LinkedPackageModule {
            module,
            declaration: &package.declaration,
            type_info: &package.type_info,
        })
        .collect();
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
        let _watchdog = config.arm_timeout(&engine);
        dispatch_main_async(&mut store, &instance)
            .await
            .unwrap()
            .expect("numeric main result")
    })
}
