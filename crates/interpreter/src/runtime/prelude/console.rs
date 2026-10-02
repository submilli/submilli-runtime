//! `console.log` — the Rust port of the prelude's `Console#log` wrapper.
//!
//! `Console` is `Dispatch::Static`, so the receiver is dropped: the host fn
//! takes the first argument plus the packed rest-`$Array`. Each value renders
//! through its vtable `toString` slot; the pieces are space-joined and written
//! to the store's console stream with a trailing newline.

use std::io::Write;

use wasmtime::{Caller, FuncType, HeapType, Linker, RefType, Val, ValType};

use crate::runtime::StoreData;
use crate::runtime::host::{intrinsic_array_type, register_host_fn_async};
use crate::runtime::intrinsic_types::build_intrinsic_types;
use crate::runtime::prelude::vtable::{dispatch_vtable_slot, read_string_units};
use crate::runtime::prelude::{MODULE_NAME, declare_method};
use crate::{MangledName, PackageDeclaration, Param, Type};

fn log_key() -> MangledName {
    crate::mangle::extend(&crate::mangle::prelude("Console"), "log")
}

pub fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let intr = build_intrinsic_types(&engine)?;
    let first_ty = ValType::Ref(RefType::new(
        true,
        HeapType::ConcreteStruct(intr.object.clone()),
    ));
    let rest_ty = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intrinsic_array_type(&engine)?),
    ));
    register_host_fn_async(
        linker,
        MODULE_NAME,
        log_key(),
        FuncType::new(&engine, [first_ty, rest_ty], []),
        false,
        |caller, params, _results| {
            Box::pin(async move {
                let mut units = to_string_units(caller, &params[0]).await?;
                for elem in super::array::read_array(caller, &params[1], "console.log")? {
                    units.push(u16::from(b' '));
                    units.extend(to_string_units(caller, &elem).await?);
                }
                let line = String::from_utf16_lossy(&units);
                writeln!(caller.data_mut().console, "{line}")?;
                Ok(())
            })
        },
    )
}

/// A logged value's display units: its vtable `toString` slot's result.
async fn to_string_units(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
) -> wasmtime::Result<Vec<u16>> {
    match val {
        Val::AnyRef(Some(_)) => {
            let s = dispatch_vtable_slot(caller, val, 0, &[]).await?;
            read_string_units(caller, &s, "console.log")
        }
        // `console.log` takes `unknown`, so a nullable value holding `null` reaches here.
        // It has no vtable to dispatch through; it prints as `null`, as in JavaScript.
        Val::AnyRef(None) => Ok("null".encode_utf16().collect()),
        other => Err(wasmtime::Error::msg(format!(
            "console.log: null/invalid value {other:?}"
        ))),
    }
}

pub fn declare(defs: &mut PackageDeclaration) {
    declare_method(
        defs,
        "log",
        log_key(),
        vec![
            Param::new("first", Type::Unknown),
            Param::rest("rest", Type::Array(Box::new(Type::Unknown))),
        ],
        Type::Void,
    );
}

/// The type/interface surface this module implements — its slice of the
/// prelude declaration (see `declaration::prelude_package_declaration`).
#[allow(clippy::too_many_lines)]
pub(crate) fn declare_types(defs: &mut crate::PackageDeclaration) {
    use crate::runtime::prelude::declaration::doc;
    use crate::{
        Dispatch, MethodSig, Param, Span, Type, TypeKind, TypeSymbol, ValueKind, ValueSymbol,
    };
    use std::collections::BTreeMap;
    defs.types.insert(
        "Console".to_string(),
        TypeSymbol {
            name: "Console".to_string(),
            mangled_name: crate::mangle::prelude("Console"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface { index: None,
                generics: Vec::new(),
                methods: BTreeMap::from([(
                    "log".to_string(),
                    MethodSig {
                        generics: Vec::new(),
                        params: vec![
                            Param::new("first", Type::Unknown),
                            Param::rest("rest", Type::Array(Box::new(Type::Unknown))),
                        ],
                        ret: Type::Void,
                        predicate: None,
                        doc: doc(
                            "/**\n * Writes one or more values to the console output, separated by spaces and followed by a newline.\n * @param args Values to write; each is coerced as in template interpolation.\n */",
                        ),
                    },
                )]),
                properties: BTreeMap::new(),
                dispatch: Dispatch::Static,
                doc: doc("/** Diagnostic console — `console.log` writes to stdout. */"),
            },
        },
    );

    defs.values.insert(
        "console".to_string(),
        ValueSymbol {
            name: "console".to_string(),
            mangled_name: crate::mangle::prelude("console"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: ValueKind::Const {
                ty: Type::prelude_interface("Console".to_string(), Vec::new()),
                doc: doc("/** The global console used for diagnostic output. */"),
            },
        },
    );
}
