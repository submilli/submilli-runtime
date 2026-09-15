//! ABI wiring for the Rust `TextEncoder` / `TextDecoder` (+ their constructors):
//! registers each method under its dispatch key and declares the value symbols
//! codegen routes through. Instances are stateless — `new` builds an empty
//! `$ObjectShape` carrying the host `object` vtable; the methods ignore the
//! receiver (`params[0]`).

use wasmtime::{
    ArrayRef, ArrayRefPre, Caller, FuncType, HeapType, Linker, RefType, StructRef, StructRefPre,
    Val, ValType,
};

use crate::runtime::StoreData;
use crate::runtime::host::{
    host_object_vtable, intrinsic_string_type, intrinsic_uint8_array_type, read_string_arg,
    read_uint8_array_arg, register_host_fn, write_submilli_string_struct,
    write_submilli_uint8array_struct,
};
use crate::runtime::intrinsic_types::build_intrinsic_types;
use crate::runtime::prelude::{MODULE_NAME, declare_method};
use crate::{MangledName, PackageDeclaration, Param, Type};

fn enc_key(method: &str) -> MangledName {
    crate::mangle::extend(&crate::mangle::prelude("TextEncoder"), method)
}

fn dec_key(method: &str) -> MangledName {
    crate::mangle::extend(&crate::mangle::prelude("TextDecoder"), method)
}

fn enc_ctor_key(method: &str) -> MangledName {
    crate::mangle::extend(&crate::mangle::prelude("TextEncoderConstructor"), method)
}

fn dec_ctor_key(method: &str) -> MangledName {
    crate::mangle::extend(&crate::mangle::prelude("TextDecoderConstructor"), method)
}

fn ref_to(struct_ty: wasmtime::StructType) -> ValType {
    ValType::Ref(RefType::new(false, HeapType::ConcreteStruct(struct_ty)))
}

/// Build a fresh stateless instance: an empty `$ObjectShape { object_vtable, [], [] }`.
fn new_instance(caller: &mut Caller<'_, StoreData>) -> wasmtime::Result<Val> {
    let intr = build_intrinsic_types(caller.engine())?;
    let vtable = host_object_vtable(caller)?;
    let names = {
        let pre = ArrayRefPre::new(&mut *caller, intr.field_names.clone());
        let arr = ArrayRef::new_fixed(&mut *caller, &pre, &[])?;
        Val::AnyRef(Some(arr.to_anyref()))
    };
    let fields = {
        let pre = ArrayRefPre::new(&mut *caller, intr.object_fields.clone());
        let arr = ArrayRef::new_fixed(&mut *caller, &pre, &[])?;
        Val::AnyRef(Some(arr.to_anyref()))
    };
    let pre = StructRefPre::new(&mut *caller, intr.object_shape.clone());
    let st = StructRef::new(&mut *caller, &pre, &[vtable, names, fields])?;
    Ok(Val::AnyRef(Some(st.to_anyref())))
}

pub fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let intr = build_intrinsic_types(&engine)?;
    let obj_null = ValType::Ref(RefType::new(
        true,
        HeapType::ConcreteStruct(intr.object.clone()),
    ));
    let string = ref_to(intrinsic_string_type(&engine)?);
    let uint8 = ref_to(intrinsic_uint8_array_type(&engine)?);

    let ft = |params: Vec<ValType>, results: Vec<ValType>| FuncType::new(&engine, params, results);

    register_host_fn(
        linker,
        MODULE_NAME,
        enc_key("encode"),
        ft(vec![obj_null.clone(), string.clone()], vec![uint8.clone()]),
        true,
        |caller, params, results| {
            let s = read_string_arg(caller, &params[1], "TextEncoder#encode")?;
            let st = write_submilli_uint8array_struct(caller, s.as_bytes())?;
            results[0] = Val::AnyRef(Some(st.to_anyref()));
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        dec_key("decode"),
        ft(vec![obj_null.clone(), uint8.clone()], vec![string.clone()]),
        true,
        |caller, params, results| {
            let bytes = read_uint8_array_arg(caller, &params[1], "TextDecoder#decode")?;
            let s = std::str::from_utf8(&bytes).map_err(|e| {
                crate::runtime::host::type_error(format!(
                    "TextDecoder.decode: invalid UTF-8 at byte {}: {e}",
                    e.valid_up_to(),
                ))
            })?;
            let st = write_submilli_string_struct(caller, s)?;
            results[0] = Val::AnyRef(Some(st.to_anyref()));
            Ok(())
        },
    )?;
    for key in [enc_ctor_key("new"), dec_ctor_key("new")] {
        register_host_fn(
            linker,
            MODULE_NAME,
            key,
            ft(Vec::new(), vec![obj_null.clone()]),
            true,
            |caller, _params, results| {
                results[0] = new_instance(caller)?;
                Ok(())
            },
        )?;
    }
    Ok(())
}

pub fn declare(defs: &mut PackageDeclaration) {
    let encoder = || Type::prelude_interface("TextEncoder".to_string(), Vec::new());
    let decoder = || Type::prelude_interface("TextDecoder".to_string(), Vec::new());
    declare_method(
        defs,
        "encode",
        enc_key("encode"),
        vec![Param::new("self", encoder()), Param::new("s", Type::String)],
        Type::Uint8Array,
    );
    declare_method(
        defs,
        "decode",
        dec_key("decode"),
        vec![
            Param::new("self", decoder()),
            Param::new("bytes", Type::Uint8Array),
        ],
        Type::String,
    );
    declare_method(defs, "new", enc_ctor_key("new"), Vec::new(), encoder());
    declare_method(defs, "new", dec_ctor_key("new"), Vec::new(), decoder());
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
        "TextEncoder".to_string(),
        TypeSymbol {
            name: "TextEncoder".to_string(),
            mangled_name: crate::mangle::prelude("TextEncoder"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface {
                generics: Vec::new(),
                methods: BTreeMap::from([(
                    "encode".to_string(),
                    MethodSig {
                        generics: Vec::new(),
                        params: vec![Param::new("s", Type::String)],
                        ret: Type::Uint8Array,
                        predicate: None,
                        doc: doc(
                            "/**\n * UTF-8 encode `s` into a `Uint8Array`. Strings are UTF-16 internally; each Unicode code point becomes one to four UTF-8 bytes.\n * @param s The string to encode.\n */",
                        ),
                    },
                )]),
                properties: BTreeMap::new(),
                dispatch: Dispatch::Direct,
                doc: doc(
                    "/** Encodes UTF-16 strings to UTF-8 byte arrays. Mirrors the WHATWG Encoding Standard `TextEncoder`. */",
                ),
            },
        },
    );

    defs.types.insert(
        "TextDecoder".to_string(),
        TypeSymbol {
            name: "TextDecoder".to_string(),
            mangled_name: crate::mangle::prelude("TextDecoder"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface {
                generics: Vec::new(),
                methods: BTreeMap::from([(
                    "decode".to_string(),
                    MethodSig {
                        generics: Vec::new(),
                        params: vec![Param::new("bytes", Type::Uint8Array)],
                        ret: Type::String,
                        predicate: None,
                        doc: doc(
                            "/**\n * Decode `bytes` as UTF-8 into a UTF-16 string. Throws a catchable `TypeError` on invalid UTF-8.\n * @param bytes The bytes to decode.\n */",
                        ),
                    },
                )]),
                properties: BTreeMap::new(),
                dispatch: Dispatch::Direct,
                doc: doc(
                    "/** Decodes UTF-8 byte arrays into UTF-16 strings. Mirrors the WHATWG Encoding Standard `TextDecoder`. UTF-8 only in v1 (no `encoding` / `fatal` / `stream` options). */",
                ),
            },
        },
    );

    defs.types.insert(
        "TextEncoderConstructor".to_string(),
        TypeSymbol {
            name: "TextEncoderConstructor".to_string(),
            mangled_name: crate::mangle::prelude("TextEncoderConstructor"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface {
                generics: Vec::new(),
                methods: BTreeMap::from([(
                    "new".to_string(),
                    MethodSig {
                        generics: Vec::new(),
                        params: Vec::new(),
                        ret: Type::prelude_interface("TextEncoder".to_string(), Vec::new()),
                        predicate: None,
                        doc: doc(
                            "/** Construct a new `TextEncoder` instance. v1 takes no options (UTF-8 only). */",
                        ),
                    },
                )]),
                properties: BTreeMap::new(),
                dispatch: Dispatch::Static,
                doc: doc(
                    "/** Constructor object for `TextEncoder`. Accessed via the global `TextEncoder` binding — call `new TextEncoder()`. */",
                ),
            },
        },
    );

    defs.types.insert(
        "TextDecoderConstructor".to_string(),
        TypeSymbol {
            name: "TextDecoderConstructor".to_string(),
            mangled_name: crate::mangle::prelude("TextDecoderConstructor"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface {
                generics: Vec::new(),
                methods: BTreeMap::from([(
                    "new".to_string(),
                    MethodSig {
                        generics: Vec::new(),
                        params: Vec::new(),
                        ret: Type::prelude_interface("TextDecoder".to_string(), Vec::new()),
                        predicate: None,
                        doc: doc(
                            "/** Construct a new `TextDecoder` instance. v1 takes no options (UTF-8 only; throws on invalid bytes). */",
                        ),
                    },
                )]),
                properties: BTreeMap::new(),
                dispatch: Dispatch::Static,
                doc: doc(
                    "/** Constructor object for `TextDecoder`. Accessed via the global `TextDecoder` binding — call `new TextDecoder()`. */",
                ),
            },
        },
    );
    defs.values.insert(
        "TextEncoder".to_string(),
        ValueSymbol {
            name: "TextEncoder".to_string(),
            mangled_name: crate::mangle::prelude("TextEncoder"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: ValueKind::Const {
                ty: Type::prelude_interface("TextEncoderConstructor".to_string(), Vec::new()),
                doc: doc("/** The `TextEncoder` constructor. */"),
            },
        },
    );
    defs.values.insert(
        "TextDecoder".to_string(),
        ValueSymbol {
            name: "TextDecoder".to_string(),
            mangled_name: crate::mangle::prelude("TextDecoder"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: ValueKind::Const {
                ty: Type::prelude_interface("TextDecoderConstructor".to_string(), Vec::new()),
                doc: doc("/** The `TextDecoder` constructor. */"),
            },
        },
    );
}
