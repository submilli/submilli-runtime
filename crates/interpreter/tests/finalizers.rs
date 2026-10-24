//! Embedder invocation of a package's reserved `finalizer`.

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use submilli_engine::runtime::security::AuditDecision;
use submilli_engine::runtime::session_finalizers::{FinalizerFuture, SessionFinalizers};
use submilli_engine::runtime::{
    CheckOutcome, FinalizerCall, FinalizerError, LinkedPackageModule, SecurityCheck, StoreData,
    Vfs, install_tenant_limits, invoke_finalizer_async,
};
use submilli_engine::{
    CompiledPackage, ModulePath, PackageSourceModule, RuntimeConfig,
    compile_package_with_transitive,
};
use wasmtime::Module;

const CLEANUP: &str = "test:cleanup";
const DEPENDENCY: &str = "test:dep";

fn package(name: &str, source: &str, dependencies: &[&CompiledPackage]) -> CompiledPackage {
    let modules = [PackageSourceModule {
        path: ModulePath::from("lib"),
        source,
    }];
    let declarations: Vec<_> = dependencies.iter().map(|p| &p.declaration).collect();
    compile_package_with_transitive(name, ModulePath::from("lib"), &modules, &declarations, &[])
        .unwrap_or_else(|d| panic!("package {name}: {d:#?}"))
}

fn cleanup(source: &str) -> Vec<CompiledPackage> {
    vec![package(CLEANUP, source, &[])]
}

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// Records every gated call and denies the listed `(caller, capability)` pairs.
#[derive(Default)]
struct Policy {
    deny: Vec<(&'static str, &'static str)>,
    calls: Mutex<Vec<(String, String)>>,
}

impl SecurityCheck for Policy {
    fn check(&self, caller: &str, capability: &str, _context: &Value) -> CheckOutcome {
        self.calls
            .lock()
            .unwrap()
            .push((caller.to_owned(), capability.to_owned()));
        if self.deny.contains(&(caller, capability)) {
            CheckOutcome::Deny {
                rule: Some(0),
                reason: format!("denied {capability} for {caller}"),
            }
        } else {
            CheckOutcome::Allow { rule: None }
        }
    }

    fn audit(&self, _decision: AuditDecision<'_>) {}
}

/// Accepts every registration, so a refusal can only come from finalizing mode.
struct Registry;

impl SessionFinalizers for Registry {
    fn register<'a>(&'a self, _: &'a str, _: &'a [u16], _: &'a [u16]) -> FinalizerFuture<'a> {
        Box::pin(async { Ok(()) })
    }
    fn unregister<'a>(&'a self, _: &'a str, _: &'a [u16]) -> FinalizerFuture<'a> {
        Box::pin(async { Ok(()) })
    }
}

#[derive(Clone, Default)]
struct Console(Arc<Mutex<Vec<u8>>>);

impl Write for Console {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct Run {
    config: RuntimeConfig,
    policy: Arc<Policy>,
    console: Console,
}

impl Default for Run {
    fn default() -> Self {
        Self {
            config: RuntimeConfig::default(),
            policy: Arc::new(Policy::default()),
            console: Console::default(),
        }
    }
}

struct Outcome {
    result: Result<(), FinalizerError>,
    console: String,
    calls: Vec<(String, String)>,
}

impl Run {
    async fn invoke(self, packages: &[CompiledPackage], key: &[u16], value: &[u16]) -> Outcome {
        let engine = self.config.engine_async().unwrap();
        let mut data = StoreData::with_vfs("finalizer-test".into(), Vfs::tempdir().unwrap());
        data.console = Box::new(self.console.clone());
        data.security_check = self.policy.clone();
        data.session_finalizers = Some(Arc::new(Registry));
        let mut store = self.config.store_async(&engine, data).unwrap();
        install_tenant_limits(&mut store);
        let _watchdog = self.config.arm_timeout(&engine).unwrap();
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
        let call = FinalizerCall {
            package: CLEANUP,
            key,
            value,
        };
        let result = invoke_finalizer_async(&mut store, &linked, call).await;
        store.data_mut().blocking_work.finish().await.unwrap();
        let console = String::from_utf8(self.console.0.lock().unwrap().clone()).unwrap();
        let calls = self.policy.calls.lock().unwrap().clone();
        Outcome {
            result,
            console,
            calls,
        }
    }
}

async fn invoke(packages: &[CompiledPackage], value: &str) -> Outcome {
    Run::default()
        .invoke(packages, &units("session-1"), &units(value))
        .await
}

const RECORDING: &str = r#"
export interface Resource { id: string; count: number; tags: string[] }
export function finalizer(key: string, value: Resource): void {
    console.log(`${key} ${value.id} ${value.count} ${value.tags.join(",")}`);
}
"#;

#[tokio::test]
async fn a_finalizer_receives_its_key_and_restored_value() {
    let outcome = invoke(
        &cleanup(RECORDING),
        r#"{"id":"r-1","count":2,"tags":["a","b"]}"#,
    )
    .await;
    outcome.result.unwrap();
    assert_eq!(outcome.console, "session-1 r-1 2 a,b\n");
}

#[tokio::test]
async fn primitive_parameters_are_restored_to_their_representation() {
    let packages = cleanup(
        r#"export function finalizer(key: string, value: number): void { console.log(`${value + 1}`); }"#,
    );
    let outcome = invoke(&packages, "41").await;
    outcome.result.unwrap();
    assert_eq!(outcome.console, "42\n");
}

#[tokio::test]
async fn lone_surrogates_survive_in_the_key_and_the_value() {
    let packages = cleanup(
        r#"
export function finalizer(key: string, value: { raw: string; escaped: string }): void {
    if (key.length !== 2 || key.charCodeAt(0) !== 0xD800 || key.charCodeAt(1) !== 0x61) {
        throw new Error("key changed");
    }
    if (value.raw.length !== 1 || value.raw.charCodeAt(0) !== 0xDC00) {
        throw new Error("raw value changed");
    }
    if (value.escaped.length !== 1 || value.escaped.charCodeAt(0) !== 0xD801) {
        throw new Error("escaped value changed");
    }
}
"#,
    );
    let mut value = units(r#"{"raw":""#);
    value.push(0xDC00);
    value.extend(units(r#"","escaped":"\ud801"}"#));
    let outcome = Run::default()
        .invoke(&packages, &[0xD800, 0x61], &value)
        .await;
    outcome.result.unwrap();
}

#[tokio::test]
async fn a_value_that_is_not_json_is_malformed() {
    let outcome = invoke(&cleanup(RECORDING), r#"{"id":"#).await;
    assert!(
        matches!(outcome.result, Err(FinalizerError::MalformedArgument(_))),
        "{:?}",
        outcome.result
    );
    assert_eq!(outcome.console, "");
}

#[tokio::test]
async fn a_value_of_another_shape_is_incompatible_and_names_the_mismatch() {
    let outcome = invoke(
        &cleanup(RECORDING),
        r#"{"id":"r-1","count":"two","tags":[]}"#,
    )
    .await;
    let Err(FinalizerError::IncompatibleArgument(message)) = outcome.result else {
        panic!("{:?}", outcome.result);
    };
    assert!(message.contains("type mismatch"), "{message}");
    assert!(message.contains("count"), "{message}");
    assert_eq!(outcome.console, "");
}

#[tokio::test]
async fn calls_are_attributed_to_the_package_whose_code_makes_them() {
    let dependency = package(
        DEPENDENCY,
        r#"import { writeText } from "submilli:fs";
export function release(id: string): void { writeText(`/${id}.dep`, id); }"#,
        &[],
    );
    let cleanup = package(
        CLEANUP,
        r#"import { writeText } from "submilli:fs";
import { release } from "test:dep";
export function finalizer(key: string, value: string): void {
    writeText(`/${value}.own`, key);
    release(value);
}"#,
        &[&dependency],
    );
    let outcome = invoke(&[dependency, cleanup], r#""r-1""#).await;
    outcome.result.unwrap();
    let writers: Vec<_> = outcome
        .calls
        .iter()
        .filter(|(_, capability)| capability == "fs.write")
        .map(|(caller, _)| caller.as_str())
        .collect();
    assert_eq!(writers, [CLEANUP, DEPENDENCY]);
}

#[tokio::test]
async fn a_denied_capability_is_reported_as_a_denial() {
    let packages = cleanup(
        r#"import { writeText } from "submilli:fs";
export function finalizer(key: string, value: string): void { writeText("/x", value); }"#,
    );
    let run = Run {
        policy: Arc::new(Policy {
            deny: vec![(CLEANUP, "fs.write")],
            ..Policy::default()
        }),
        ..Run::default()
    };
    let outcome = run.invoke(&packages, &units("k"), &units(r#""v""#)).await;
    let Err(FinalizerError::Denied(denial)) = outcome.result else {
        panic!("{:?}", outcome.result);
    };
    assert_eq!(denial.caller, CLEANUP);
    assert_eq!(denial.capability, "fs.write");
}

#[tokio::test]
async fn an_initializer_cannot_register_cleanup() {
    let packages = cleanup(
        r#"import * as session from "submilli:session";
session.registerFinalizer("again", "v");
export function finalizer(key: string, value: string): void { console.log("ran"); }"#,
    );
    let outcome = invoke(&packages, r#""v""#).await;
    let Err(FinalizerError::Failed(error)) = outcome.result else {
        panic!("{:?}", outcome.result);
    };
    assert!(
        format!("{error:#}").contains("finalizers cannot change finalizer registrations"),
        "{error:#}"
    );
    assert_eq!(outcome.console, "");
}

#[tokio::test]
async fn a_finalizer_cannot_register_or_release_cleanup() {
    for call in [
        r#"session.registerFinalizer("again", value)"#,
        r#"session.unregisterFinalizer(key)"#,
    ] {
        let packages = cleanup(&format!(
            r#"import * as session from "submilli:session";
export function finalizer(key: string, value: string): void {{ {call}; }}"#
        ));
        let outcome = invoke(&packages, r#""v""#).await;
        assert!(
            matches!(outcome.result, Err(FinalizerError::Failed(_))),
            "{call}: {:?}",
            outcome.result
        );
    }
}

const SPIN: &str = r#"
export function finalizer(key: string, value: string): void {
    let n = 0;
    while (true) { n = n + 1; }
}
"#;

#[tokio::test]
async fn running_out_of_fuel_exhausts_the_budget() {
    let run = Run {
        config: RuntimeConfig {
            fuel: 1_000_000,
            ..RuntimeConfig::default()
        },
        ..Run::default()
    };
    let outcome = run
        .invoke(&cleanup(SPIN), &units("k"), &units(r#""v""#))
        .await;
    assert!(
        matches!(outcome.result, Err(FinalizerError::BudgetExhausted(_))),
        "{:?}",
        outcome.result
    );
}

#[tokio::test]
async fn the_deadline_exhausts_the_budget() {
    let run = Run {
        config: RuntimeConfig {
            timeout: Some(Duration::from_millis(100)),
            ..RuntimeConfig::default()
        },
        ..Run::default()
    };
    let outcome = run
        .invoke(&cleanup(SPIN), &units("k"), &units(r#""v""#))
        .await;
    assert!(
        matches!(outcome.result, Err(FinalizerError::BudgetExhausted(_))),
        "{:?}",
        outcome.result
    );
}

#[tokio::test]
async fn blocking_work_drains_after_a_dropped_invocation() {
    let packages = cleanup(
        r#"import { writeText } from "submilli:fs";
export function finalizer(key: string, value: string): void {
    writeText("/started", value);
    let n = 0;
    while (true) { n = n + 1; }
}"#,
    );
    let config = RuntimeConfig {
        async_yield_fuel: Some(10_000),
        ..RuntimeConfig::default()
    };
    let engine = config.engine_async().unwrap();
    let mut data = StoreData::with_vfs("finalizer-test".into(), Vfs::tempdir().unwrap());
    data.session_finalizers = Some(Arc::new(Registry));
    let mut store = config.store_async(&engine, data).unwrap();
    install_tenant_limits(&mut store);
    let module = Module::new(&engine, &packages[0].wasm).unwrap();
    let linked = [LinkedPackageModule {
        module: &module,
        declaration: &packages[0].declaration,
        type_info: &packages[0].type_info,
    }];
    let (key, value) = (units("k"), units(r#""v""#));
    let call = FinalizerCall {
        package: CLEANUP,
        key: &key,
        value: &value,
    };
    let invocation = invoke_finalizer_async(&mut store, &linked, call);
    assert!(
        tokio::time::timeout(Duration::from_millis(200), invocation)
            .await
            .is_err(),
        "the finalizer spins until it is dropped"
    );
    store.data_mut().blocking_work.finish().await.unwrap();
}

#[tokio::test]
async fn a_package_without_a_finalizer_is_unavailable() {
    let packages = cleanup("export function open(): void {}");
    let outcome = invoke(&packages, r#""v""#).await;
    assert!(
        matches!(outcome.result, Err(FinalizerError::Unavailable(_))),
        "{:?}",
        outcome.result
    );
}

#[tokio::test]
async fn a_package_that_is_not_linked_is_unavailable() {
    let outcome = invoke(&[], r#""v""#).await;
    assert!(
        matches!(outcome.result, Err(FinalizerError::Unavailable(_))),
        "{:?}",
        outcome.result
    );
}

#[test]
fn only_a_package_with_a_finalizer_exports_its_argument_check() {
    let engine = RuntimeConfig::default().engine_async().unwrap();
    let exports = |source: &str| -> Vec<String> {
        let compiled = package(CLEANUP, source, &[]);
        Module::new(&engine, &compiled.wasm)
            .unwrap()
            .exports()
            .map(|export| export.name().to_owned())
            .collect()
    };
    let check = "test:cleanup#finalizer:argument".to_owned();
    assert!(exports(RECORDING).contains(&check));
    assert!(!exports("export function open(): void {}").contains(&check));
}

/// Each `(parameter type, stored value)` fails `value as T`, so it must fail
/// the argument check before the finalizer runs.
#[tokio::test]
async fn an_argument_is_checked_as_value_as_t_checks_it() {
    let cases = [
        ("J", r#"{"a":1}"#),
        ("J", "true"),
        ("Tree", r#"{"items":[{"items":[],"n":"x"}],"n":1}"#),
        ("A | B", r#"{"kind":"b","x":1}"#),
        ("Record<string, A>", r#"{"k":{"kind":"a","x":"1"}}"#),
        ("{}", "null"),
        ("object", "42"),
        ("Level", r#""loud""#),
    ];
    for (ty, value) in cases {
        let packages = cleanup(&format!(
            r#"
export type J = number | string | J[];
export type Tree = {{ items: Tree[]; n: number }};
export interface A {{ kind: "a"; x: number }}
export interface B {{ kind: "b"; y: string }}
export enum Level {{ Quiet = "quiet" }}
export function finalizer(key: string, value: {ty}): void {{ console.log("ran"); }}
"#
        ));
        let outcome = invoke(&packages, value).await;
        assert!(
            matches!(outcome.result, Err(FinalizerError::IncompatibleArgument(_))),
            "{ty} with {value}: {:?}",
            outcome.result
        );
        assert_eq!(outcome.console, "", "{ty} with {value}");
    }
}

#[tokio::test]
async fn a_parameter_type_from_a_dependency_is_checked() {
    let dependency = package(DEPENDENCY, "export interface Lease { id: string }", &[]);
    let cleanup = package(
        CLEANUP,
        r#"import type { Lease } from "test:dep";
export function finalizer(key: string, value: Lease): void { console.log(value.id); }"#,
        &[&dependency],
    );
    let packages = [dependency, cleanup];
    invoke(&packages, r#"{"id":"l-1"}"#).await.result.unwrap();
    let outcome = invoke(&packages, r#"{"id":1}"#).await;
    assert!(
        matches!(outcome.result, Err(FinalizerError::IncompatibleArgument(_))),
        "{:?}",
        outcome.result
    );
}

#[tokio::test]
async fn an_initializer_that_throws_fails_the_cleanup() {
    let packages = cleanup(
        r#"throw new Error("not configured");
export function finalizer(key: string, value: string): void { console.log("ran"); }"#,
    );
    let outcome = invoke(&packages, r#""v""#).await;
    let Err(FinalizerError::Failed(error)) = outcome.result else {
        panic!("{:?}", outcome.result);
    };
    assert!(format!("{error:#}").contains("not configured"), "{error:#}");
    assert_eq!(outcome.console, "");
}

#[test]
fn a_finalizer_value_type_the_runtime_cannot_check_does_not_compile() {
    for (ty, reason) in [
        ("Map<string, number>", "cannot be verified at runtime"),
        ("Lease", "cannot be verified at runtime"),
        ("Chain", "recursive interface types aren't supported"),
        ("Stat", "host-backed"),
    ] {
        let source = format!(
            r#"import type {{ Stat }} from "submilli:fs";
export class Lease {{ id: string = ""; }}
export interface Chain {{ next?: Chain }}
export type {{ Stat }};
export function finalizer(key: string, value: {ty}): void {{}}"#
        );
        let modules = [PackageSourceModule {
            path: ModulePath::from("lib"),
            source: &source,
        }];
        let diagnostics =
            compile_package_with_transitive(CLEANUP, ModulePath::from("lib"), &modules, &[], &[])
                .expect_err(ty);
        assert!(
            diagnostics.iter().any(|d| {
                d.message.contains("cannot be verified at runtime") && d.message.contains(reason)
            }),
            "{ty}: {diagnostics:#?}"
        );
    }
}

#[test]
fn an_unknown_finalizer_value_type_is_reported_once() {
    let modules = [PackageSourceModule {
        path: ModulePath::from("lib"),
        source: "export function finalizer(key: string, value: Missing): void {}",
    }];
    let diagnostics =
        compile_package_with_transitive(CLEANUP, ModulePath::from("lib"), &modules, &[], &[])
            .expect_err("unknown type");
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == submilli_engine::Severity::Error)
        .collect();
    assert_eq!(errors.len(), 1, "{diagnostics:#?}");
    assert!(errors[0].message.contains("Missing"), "{diagnostics:#?}");
}

#[tokio::test]
async fn a_finalizer_re_exported_from_a_module_checks_its_argument() {
    let modules = [
        PackageSourceModule {
            path: ModulePath::from("lib"),
            source: r#"export { finalizer, type Lease, type Owner } from "./cleanup";"#,
        },
        PackageSourceModule {
            path: ModulePath::from("cleanup"),
            source: r#"export interface Owner { name: string }
export interface Lease { id: string; owner: Owner }
export function finalizer(key: string, value: Lease): void { console.log(value.owner.name); }"#,
        },
    ];
    let packages =
        [
            compile_package_with_transitive(CLEANUP, ModulePath::from("lib"), &modules, &[], &[])
                .unwrap_or_else(|d| panic!("{d:#?}")),
        ];
    let outcome = invoke(&packages, r#"{"id":"l-1","owner":{"name":"ops"}}"#).await;
    outcome.result.unwrap();
    assert_eq!(outcome.console, "ops\n");
    let outcome = invoke(&packages, r#"{"id":"l-1","owner":{"name":1}}"#).await;
    let Err(FinalizerError::IncompatibleArgument(message)) = outcome.result else {
        panic!("{:?}", outcome.result);
    };
    assert!(message.contains(r#"$["owner"]["name"]"#), "{message}");
}
