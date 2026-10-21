//! The Rust port of the prelude's `Boolean` surface — `toString` / `toJson`,
//! both `"true"` / `"false"`. The receiver is the unboxed `i32` (0 = false).
//! `Boolean(value)`, the `BooleanConstructor` call signature, is truthiness.

use crate::runtime::host::{abi_arg, abi_result};
use wasmtime::{FuncType, HeapType, Linker, RefType, Val, ValType};

use crate::runtime::StoreData;
use crate::runtime::host::{intrinsic_string_type, register_host_fn, write_submilli_string_struct};
use crate::runtime::prelude::{MODULE_NAME, declare_method};
use crate::{MangledName, PackageDeclaration, Param, Type};

fn method_key(method: &str) -> MangledName {
    crate::mangle::extend(&crate::mangle::prelude("Boolean"), method)
}

fn call_key() -> MangledName {
    crate::mangle::extend(&crate::mangle::prelude("BooleanConstructor"), "@call")
}

pub fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let string_struct = intrinsic_string_type(&engine)?;
    let string_ref = ValType::Ref(RefType::new(false, HeapType::ConcreteStruct(string_struct)));

    // `toString` and `toJson` are identical for a boolean — both spell the value.
    for method in ["toString", "toJson"] {
        let s = string_ref.clone();
        register_host_fn(
            linker,
            MODULE_NAME,
            method_key(method),
            FuncType::new(&engine, [ValType::I32], [s]),
            |caller, params, results| {
                let truthy = match *abi_arg(params, 0)? {
                    Val::I32(v) => v != 0,
                    ref other => {
                        return Err(wasmtime::Error::msg(format!(
                            "Boolean method expects i32, got {other:?}"
                        )));
                    }
                };
                let st =
                    write_submilli_string_struct(caller, if truthy { "true" } else { "false" })?;
                *abi_result(results, 0)? = Val::AnyRef(Some(st.to_anyref()));
                Ok(())
            },
        )?;
    }

    // `Boolean(value?: unknown)` (`Dispatch::Static`, only the value arrives).
    let object = ValType::Ref(RefType::new(
        true,
        HeapType::ConcreteStruct(
            crate::runtime::intrinsic_types::build_intrinsic_types(&engine)?.object,
        ),
    ));
    register_host_fn(
        linker,
        MODULE_NAME,
        call_key(),
        FuncType::new(&engine, [object], [ValType::I32]),
        |caller, params, results| {
            let truthy = super::value::truthy(caller, abi_arg(params, 0)?)?;
            *abi_result(results, 0)? = Val::I32(i32::from(truthy));
            Ok(())
        },
    )?;
    Ok(())
}

pub fn declare(defs: &mut PackageDeclaration) {
    let recv = || Param::new("value", Type::Boolean);
    declare_method(
        defs,
        "toString",
        method_key("toString"),
        vec![recv()],
        Type::String,
    );
    declare_method(
        defs,
        "toJson",
        method_key("toJson"),
        vec![recv()],
        Type::String,
    );
    declare_method(defs, "@call", call_key(), vec![call_value()], Type::Boolean);
}

fn call_value() -> Param {
    Param::with_default("value", Type::Unknown, crate::DefaultValue::Undefined)
}

/// The type/interface surface this module implements — its slice of the
/// prelude declaration (see `declaration::prelude_package_declaration`).
#[allow(clippy::too_many_lines)]
pub(crate) fn declare_types(defs: &mut crate::PackageDeclaration) {
    use crate::runtime::prelude::declaration::doc;
    use crate::{Dispatch, MethodSig, Span, Type, TypeKind, TypeSymbol, ValueKind, ValueSymbol};
    use std::collections::BTreeMap;
    defs.types.insert(
        "Boolean".to_string(),
        TypeSymbol {
            name: "Boolean".to_string(),
            mangled_name: crate::mangle::prelude("Boolean"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface { index: None,
                generics: Vec::new(),
                methods: BTreeMap::from([
                    (
                        "toString".to_string(),
                        MethodSig {
                            optional: false,
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::String,
                            predicate: None,
                            doc: doc("/** Returns `\"true\"` or `\"false\"`. */"),
                        },
                    ),
                    (
                        "toJson".to_string(),
                        MethodSig {
                            optional: false,
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::String,
                            predicate: None,
                            doc: doc(
                                "/** Returns `\"true\"` or `\"false\"` — same as `toString` (booleans are JSON-native). */",
                            ),
                        },
                    ),
                ]),
                properties: BTreeMap::new(),
                dispatch: Dispatch::Direct,
                doc: doc("/** The boolean type — `true` or `false`. */"),
            },
        },
    );
    defs.types.insert(
        "BooleanConstructor".to_string(),
        TypeSymbol {
            name: "BooleanConstructor".to_string(),
            mangled_name: crate::mangle::prelude("BooleanConstructor"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface {
                index: None,
                generics: Vec::new(),
                methods: BTreeMap::from([(
                    "@call".to_string(),
                    MethodSig {
                        optional: false,
                        generics: Vec::new(),
                        params: vec![call_value()],
                        ret: Type::Boolean,
                        predicate: None,
                        doc: doc(
                            "/**\n * Convert any value to a boolean by its truthiness: `false`, `0`, `NaN`, `\"\"`, `0n`, `null` and `undefined` are `false`; everything else is `true`.\n */",
                        ),
                    },
                )]),
                properties: BTreeMap::new(),
                dispatch: Dispatch::Static,
                doc: doc(
                    "/** Constructor object for `boolean`. Accessed via the global `Boolean` binding. */",
                ),
            },
        },
    );
    defs.values.insert(
        "Boolean".to_string(),
        ValueSymbol {
            name: "Boolean".to_string(),
            mangled_name: crate::mangle::prelude("Boolean"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: ValueKind::Const {
                ty: Type::prelude_interface("BooleanConstructor".to_string(), Vec::new()),
                doc: doc(
                    "/** The `Boolean` constructor — call `Boolean(x)` to convert any value to a boolean by its truthiness. */",
                ),
            },
        },
    );
}
