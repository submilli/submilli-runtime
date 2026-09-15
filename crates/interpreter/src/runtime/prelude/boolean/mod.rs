//! The Rust port of the prelude's `Boolean` surface — `toString` / `toJson`,
//! both `"true"` / `"false"`. The receiver is the unboxed `i32` (0 = false).

use wasmtime::{FuncType, HeapType, Linker, RefType, Val, ValType};

use crate::runtime::StoreData;
use crate::runtime::host::{intrinsic_string_type, register_host_fn, write_submilli_string_struct};
use crate::runtime::prelude::{MODULE_NAME, declare_method};
use crate::{MangledName, PackageDeclaration, Param, Type};

fn method_key(method: &str) -> MangledName {
    crate::mangle::extend(&crate::mangle::prelude("Boolean"), method)
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
            true,
            |caller, params, results| {
                let truthy = match params[0] {
                    Val::I32(v) => v != 0,
                    ref other => {
                        return Err(wasmtime::Error::msg(format!(
                            "Boolean method expects i32, got {other:?}"
                        )));
                    }
                };
                let st =
                    write_submilli_string_struct(caller, if truthy { "true" } else { "false" })?;
                results[0] = Val::AnyRef(Some(st.to_anyref()));
                Ok(())
            },
        )?;
    }
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
}

/// The type/interface surface this module implements — its slice of the
/// prelude declaration (see `declaration::prelude_package_declaration`).
#[allow(clippy::too_many_lines)]
pub(crate) fn declare_types(defs: &mut crate::PackageDeclaration) {
    use crate::runtime::prelude::declaration::doc;
    use crate::{Dispatch, MethodSig, Span, Type, TypeKind, TypeSymbol};
    use std::collections::BTreeMap;
    defs.types.insert(
        "Boolean".to_string(),
        TypeSymbol {
            name: "Boolean".to_string(),
            mangled_name: crate::mangle::prelude("Boolean"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface {
                generics: Vec::new(),
                methods: BTreeMap::from([
                    (
                        "toString".to_string(),
                        MethodSig {
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
}
