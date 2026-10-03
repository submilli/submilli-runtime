//! `submilli:test` — the test-authoring package test files import.
//!
//! Unlike every other `submilli:*` package this one is **not** in the global
//! stdlib registry (see [`crate::stdlib::stdlib_package_declarations`]). It is
//! only made importable by the `submilli build test` runner, which passes
//! [`package_declaration`] into the compile and calls [`install`] itself. An
//! ordinary `submilli run` never offers it, so importing it there fails with a
//! `package ... not found` diagnostic.
//!
//! `label(description)` records a segment boundary in
//! [`StoreData::test_labels`](crate::runtime::StoreData). `expectException`
//! re-enters the guest to run the supplied closure and catches its thrown
//! `Error` host-side (via the store's pending-exception slot): no throw or a
//! `name` mismatch fails the test with an `Err` the runtime turns into a
//! catchable guest `Error` — the same machinery a failed `assert` uses.
//!
//! None of these are gated (no `check_security`): they only manipulate the
//! in-store test ledger.

use crate::runtime::host::invariant_trap;
use crate::runtime::host::{abi_arg, abi_result};
use wasmtime::{AsContextMut, Caller, FuncType, HeapType, Linker, RefType, Val, ValType};

use crate::runtime::StoreData;
use crate::runtime::host::{read_string_arg, register_host_fn, register_host_fn_async};
use crate::runtime::intrinsic_types::build_intrinsic_types;
use crate::runtime::prelude::closure;
use crate::runtime::prelude::iterator::{as_struct, void_closure_type};
use crate::{PackageDeclaration, Param, Span, Type, ValueKind, ValueSymbol};

pub const MODULE_NAME: &str = "submilli:test";

/// `name` sits at object-fields payload slot 1 of the class-shaped `$Error`
/// (`message` at slot 0) — see `runtime/prelude/error.rs`.
const ERROR_FIELDS: usize = 2;
const NAME_SLOT: u32 = 1;

pub fn package_declaration() -> PackageDeclaration {
    let mut defs = PackageDeclaration::with_package(MODULE_NAME);
    defs.values.insert(
        "label".to_string(),
        ValueSymbol {
            name: "label".to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, "label"),
            declaration_span: Span::at(crate::FileId::TEST),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params: vec![Param::new("description", Type::String)],
                ret: Type::Void,
                type_predicate: None,
                doc: crate::doc(
                    crate::FileId::TEST,
                    "/**\n * Start a new named test segment. Everything between this call and the next `label` (or the end of `main`) is reported under `description`; the segment passes if execution reaches the next boundary without throwing. A test file with no `label` calls is reported as a single anonymous test.\n * @param description Human-readable name for the segment.\n */",
                ),
            },
        },
    );
    defs.values.insert(
        "expectException".to_string(),
        ValueSymbol {
            name: "expectException".to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, "expectException"),
            declaration_span: Span::at(crate::FileId::TEST),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params: vec![
                    Param::new(
                        "fn",
                        Type::Function {
                            params: Vec::new(),
                            ret: Box::new(Type::Void),
                            predicate: None,
                            has_rest: false,
                        },
                    ),
                    Param {
                        name: "errorType".to_string(),
                        ty: Type::String,
                        default: Some(crate::DefaultValue::String(String::new())),
                        rest: false,
                    },
                ],
                ret: Type::prelude_error_class(),
                type_predicate: None,
                doc: crate::doc(
                    crate::FileId::TEST,
                    "/**\n * Run `fn` and assert that it throws an `Error`, returning the caught error for inspection. Fails the test if `fn` returns without throwing. When `errorType` is given, also asserts the caught error's `name` equals it.\n * @param fn The closure expected to throw.\n * @param errorType Optional expected value of the caught error's `name`.\n */",
                ),
            },
        },
    );
    defs
}

/// Register the host fns. Called by the `submilli build test` runner after the
/// runtime install; `submilli run` deliberately never calls this.
pub fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let intr = build_intrinsic_types(&engine)?;
    let string = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.string.clone()),
    ));

    register_host_fn(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "label"),
        FuncType::new(&engine, [string.clone()], []),
        /* deterministic = */ true,
        |caller, params, _results| {
            let description = read_string_arg(
                &mut *caller,
                abi_arg(params, 0)?,
                "test.label (description)",
            )?;
            caller
                .data()
                .test_labels
                .try_borrow_mut()
                .map_err(|_| invariant_trap("test: labels already borrowed"))?
                .push(description);
            Ok(())
        },
    )?;

    let (_, closure_struct) = void_closure_type(&engine, &intr)?;
    let closure_param = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(closure_struct),
    ));
    let error_result = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.error.clone()),
    ));
    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "expectException"),
        FuncType::new(&engine, [closure_param, string], [error_result]),
        /* deterministic = */ true,
        |caller, params, results| {
            Box::pin(async move {
                let expected = read_string_arg(
                    &mut *caller,
                    abi_arg(params, 1)?,
                    "expectException (errorType)",
                )?;
                let closure = closure::read(caller, abi_arg(params, 0)?, "expectException")?;
                match closure.call_void_args(caller, &[]).await {
                    Ok(()) => {
                        let detail = if expected.is_empty() {
                            String::new()
                        } else {
                            format!(" a {expected}")
                        };
                        Err(wasmtime::Error::msg(format!(
                            "expectException: expected the closure to throw{detail}, but it returned normally"
                        )))
                    }
                    Err(err) if err.is::<wasmtime::ThrownException>() => {
                        let error = take_thrown_error(caller)?;
                        let actual = error_name(caller, &error)?;
                        if expected.is_empty() || actual == expected {
                            *abi_result(results, 0)? = error;
                            Ok(())
                        } else {
                            Err(wasmtime::Error::msg(format!(
                                "expectException: expected a {expected} error, but caught {actual}"
                            )))
                        }
                    }
                    // A genuine trap (not a thrown `Error`) propagates uncaught.
                    Err(err) => Err(err),
                }
            })
        },
    )?;

    Ok(())
}

/// Pull the `$Error` payload out of the store's pending exception — the throw
/// the closure just raised.
fn take_thrown_error(caller: &mut Caller<'_, StoreData>) -> wasmtime::Result<Val> {
    let exn = caller
        .as_context_mut()
        .take_pending_exception()
        .ok_or_else(|| {
            invariant_trap("expectException: throw completed without a pending exception")
        })?;
    exn.field(&mut *caller, 0)
}

/// Read the caught error's `name` from the object-fields payload.
fn error_name(caller: &mut Caller<'_, StoreData>, error: &Val) -> wasmtime::Result<String> {
    let st = as_struct(caller, error, "expectException (caught error)")?;
    let fields = match st.field(&mut *caller, ERROR_FIELDS)? {
        Val::AnyRef(Some(any)) => any.unwrap_array(&mut *caller)?,
        other => {
            return Err(wasmtime::Error::msg(format!(
                "expectException: malformed error payload {other:?}"
            )));
        }
    };
    let name = fields.get(&mut *caller, NAME_SLOT)?;
    read_string_arg(caller, &name, "expectException (error name)")
}

#[cfg(test)]
mod tests {
    use crate::runtime::{
        RuntimeConfig, StoreData, Vfs, dispatch_main_async, install_runtime_async,
    };
    use crate::{Severity, compile_script};

    /// Compile `source` with `submilli:test` importable, run `main`, and return
    /// the dispatch result alongside the recorded segment labels.
    async fn run(source: &str) -> (wasmtime::Result<Option<String>>, Vec<String>) {
        let compiled = compile_script(
            source,
            "test.subm",
            crate::FileId(0),
            &[&super::package_declaration()],
            &[],
        )
        .expect("compile clean");
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let mut store = cfg
            .store_async(
                &engine,
                StoreData::with_vfs(Vfs::tempdir().expect("tempdir")),
            )
            .expect("store");
        let module = wasmtime::Module::new(&engine, &compiled.wasm).expect("module");
        let mut linker = wasmtime::Linker::<StoreData>::new(&engine);
        install_runtime_async(&mut linker, &mut store)
            .await
            .expect("install runtime");
        super::install(&mut linker).expect("install test host fns");
        let inst = linker
            .instantiate_async(&mut store, &module)
            .await
            .expect("instantiate");
        let result = dispatch_main_async(&mut store, &inst).await;
        let labels = store.data().test_labels.borrow().clone();
        (result, labels)
    }

    #[tokio::test]
    async fn label_records_segments_in_order() {
        let (result, labels) = run(r#"
            import { label } from "submilli:test";
            function main(): void {
                label("first segment");
                label("second segment");
            }
        "#)
        .await;
        result.expect("main runs clean");
        assert_eq!(labels, vec!["first segment", "second segment"]);
    }

    #[tokio::test]
    async fn expect_exception_returns_caught_error() {
        let (result, _labels) = run(r#"
            import { expectException } from "submilli:test";
            function main(): void {
                const e = expectException(() => { throw new Error("boom"); });
                assert(e.message === "boom", "returns the caught error");
                assert(e.name === "Error", "name is readable");
            }
        "#)
        .await;
        result.expect("main runs clean");
    }

    #[tokio::test]
    async fn expect_exception_catches_failed_assert() {
        // A failed `assert` throws a catchable Error, so `expectException`
        // reports it as the thrown error.
        let (result, _labels) = run(r#"
            import { expectException } from "submilli:test";
            function main(): void {
                const e = expectException(() => { assert(false, "nope"); });
                assert(e.message === "nope", "assert message round-trips");
            }
        "#)
        .await;
        result.expect("main runs clean");
    }

    #[tokio::test]
    async fn expect_exception_matching_name_passes() {
        let (result, _labels) = run(r#"
            import { expectException } from "submilli:test";
            function main(): void {
                expectException(() => { throw new Error("x"); }, "Error");
            }
        "#)
        .await;
        result.expect("main runs clean");
    }

    #[tokio::test]
    async fn expect_exception_without_throw_fails() {
        let (result, _labels) = run(r#"
            import { expectException } from "submilli:test";
            function main(): void {
                expectException(() => { const _ = 1 + 1; });
            }
        "#)
        .await;
        let err = result.expect_err("a closure that doesn't throw should fail the test");
        assert!(
            format!("{err}").contains("returned normally"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn expect_exception_name_mismatch_fails() {
        let (result, _labels) = run(r#"
            import { expectException } from "submilli:test";
            function main(): void {
                expectException(() => { throw new Error("x"); }, "TypeError");
            }
        "#)
        .await;
        let err = result.expect_err("a name mismatch should fail the test");
        let msg = format!("{err}");
        assert!(
            msg.contains("expected a TypeError error, but caught Error"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn submilli_test_not_importable_without_declaration() {
        let source = r#"
            import { label } from "submilli:test";
            function main(): void {}
        "#;
        let diags = compile_script(source, "t.subm", crate::FileId(0), &[], &[])
            .expect_err("submilli:test must not be importable on a plain run");
        let not_found = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .find(|d| d.message.contains("submilli:test") && d.message.contains("not found"))
            .unwrap_or_else(|| {
                panic!(
                    "expected not-found diagnostic, got: {:?}",
                    diags.iter().map(|d| &d.message).collect::<Vec<_>>()
                )
            });
        assert!(
            not_found
                .help
                .iter()
                .any(|h| h.contains("submilli build test")),
            "expected help pointing at `submilli build test`, got: {:?}",
            not_found.help
        );
    }
}
