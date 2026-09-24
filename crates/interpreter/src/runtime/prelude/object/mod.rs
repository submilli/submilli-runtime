//! The Rust port of the prelude's `ObjectConstructor` statics —
//! `Object.keys`/`values`/`entries`/`hasOwn`/`is` — reading the `$ObjectShape`
//! field-name/field-value arrays directly. Every per-arity object subtype
//! subtypes `$ObjectShape`, so one `matches_ty` test covers them all; any other
//! value takes the "no fields" path (`[]`/`false`), and a null receiver throws
//! a catchable `Error`, matching the Wasm bodies in
//! `codegen/prelude/object_shape.rs`.

use wasmtime::{
    ArrayRef, ArrayRefPre, Caller, FuncType, HeapType, Linker, RefType, Rooted, StructRef,
    StructRefPre, StructType, Val, ValType,
};

use crate::MangledName;
use crate::runtime::StoreData;
use crate::runtime::host::{
    host_object_vtable, intrinsic_array_type, intrinsic_string_type, register_host_fn,
    register_host_fn_async, write_submilli_array_struct,
};
use crate::runtime::intrinsic_types::build_intrinsic_types;
use crate::runtime::prelude::collection::{is_a, string_units};
use crate::runtime::prelude::iterator::as_struct;
use crate::runtime::prelude::vtable::dispatch_vtable_slot;
use crate::runtime::prelude::{MODULE_NAME, declare_method};
use crate::{PackageDeclaration, Param, Type};

fn ctor_key(method: &str) -> MangledName {
    crate::mangle::extend(&crate::mangle::prelude("ObjectConstructor"), method)
}

#[derive(Clone, Copy)]
enum Enumerate {
    Keys,
    Values,
    Entries,
}

impl Enumerate {
    fn method(self) -> &'static str {
        match self {
            Enumerate::Keys => "keys",
            Enumerate::Values => "values",
            Enumerate::Entries => "entries",
        }
    }
}

/// The `$ObjectShape` field arrays `(field_names, object_fields)` of `obj`, or
/// `None` when `obj` is any other (non-null) value — the "no fields" path.
fn shape_arrays(
    caller: &mut Caller<'_, StoreData>,
    obj: &Val,
) -> wasmtime::Result<Option<(Rooted<ArrayRef>, Rooted<ArrayRef>)>> {
    let Val::AnyRef(Some(any)) = obj else {
        return Ok(None);
    };
    let Some(st) = any.as_struct(&mut *caller)? else {
        return Ok(None);
    };
    let shape = build_intrinsic_types(caller.engine())?.object_shape;
    if !st.matches_ty(&*caller, &shape)? {
        return Ok(None);
    }
    let names = field_array(caller, &st, 1)?;
    let values = field_array(caller, &st, 2)?;
    Ok(Some((names, values)))
}

fn field_array(
    caller: &mut Caller<'_, StoreData>,
    st: &Rooted<StructRef>,
    idx: usize,
) -> wasmtime::Result<Rooted<ArrayRef>> {
    match st.field(&mut *caller, idx)? {
        Val::AnyRef(Some(a)) => a.unwrap_array(&mut *caller),
        other => Err(wasmtime::Error::msg(format!(
            "$ObjectShape field {idx} is not an array: {other:?}"
        ))),
    }
}

/// A nullable optional slot has a separate presence flag on its field name.
pub(crate) fn field_is_present(
    caller: &mut Caller<'_, StoreData>,
    name: &Val,
    value: &Val,
) -> wasmtime::Result<bool> {
    if !matches!(value, Val::AnyRef(None)) {
        return Ok(true);
    }
    let name = as_struct(caller, name, "field name")?;
    let string = build_intrinsic_types(caller.engine())?.string;
    if StructType::eq(&name.ty(&*caller)?, &string) {
        return Ok(true);
    }
    Ok(matches!(name.field(&mut *caller, 2)?, Val::I32(value) if value != 0))
}

fn enumerate(
    caller: &mut Caller<'_, StoreData>,
    obj: &Val,
    kind: Enumerate,
) -> wasmtime::Result<Val> {
    if matches!(obj, Val::AnyRef(None)) {
        return Err(wasmtime::Error::msg(format!(
            "Object.{} called on null",
            kind.method()
        )));
    }
    let mut elems = Vec::new();
    if let Some((names, values)) = shape_arrays(caller, obj)? {
        let len = names.len(&mut *caller)?;
        elems.reserve(len as usize);
        for i in 0..len {
            let name = names.get(&mut *caller, i)?;
            let value = values.get(&mut *caller, i)?;
            if !field_is_present(caller, &name, &value)? {
                continue;
            }
            let elem = match kind {
                Enumerate::Keys => names.get(&mut *caller, i)?,
                Enumerate::Values => values.get(&mut *caller, i)?,
                Enumerate::Entries => {
                    let name = names.get(&mut *caller, i)?;
                    let value = values.get(&mut *caller, i)?;
                    let pair = write_submilli_array_struct(caller, &[name, value])?;
                    Val::AnyRef(Some(pair.to_anyref()))
                }
            };
            elems.push(elem);
        }
    }
    let arr = write_submilli_array_struct(caller, &elems)?;
    Ok(Val::AnyRef(Some(arr.to_anyref())))
}

/// Copy present own fields while preserving UTF-16 names and boxed values.
/// Each call snapshots its source before the next literal member is evaluated.
fn spread(
    caller: &mut Caller<'_, StoreData>,
    target: &Val,
    source: &Val,
    shape: &Val,
    mask: &Val,
) -> wasmtime::Result<Val> {
    let intr = build_intrinsic_types(caller.engine())?;
    let mut entries = std::collections::BTreeMap::new();
    let omitted = spread_omitted_fields(caller, mask)?;
    for (source_index, object) in [target, source].into_iter().enumerate() {
        let Some((names, values)) = shape_arrays(caller, object)? else {
            continue;
        };
        for index in 0..names.len(&mut *caller)? {
            let name = names.get(&mut *caller, index)?;
            let value = values.get(&mut *caller, index)?;
            if !field_is_present(caller, &name, &value)? {
                continue;
            }
            let units = string_units(caller, &name)?;
            if source_index == 1 && omitted.contains(&units) {
                continue;
            }
            entries.insert(units, (copy_field_name(caller, name, true)?, value));
        }
    }
    if let Some((names, _)) = shape_arrays(caller, shape)? {
        for index in 0..names.len(&mut *caller)? {
            let name = names.get(&mut *caller, index)?;
            let units = string_units(caller, &name)?;
            if let std::collections::btree_map::Entry::Vacant(entry) = entries.entry(units) {
                entry.insert((copy_field_name(caller, name, false)?, Val::AnyRef(None)));
            }
        }
    }
    let (names, values): (Vec<_>, Vec<_>) = entries.into_values().unzip();
    let names_pre = ArrayRefPre::new(&mut *caller, intr.field_names);
    let values_pre = ArrayRefPre::new(&mut *caller, intr.object_fields);
    let shape_pre = StructRefPre::new(&mut *caller, intr.object_shape);
    let names = ArrayRef::new_fixed(&mut *caller, &names_pre, &names)?;
    let values = ArrayRef::new_fixed(&mut *caller, &values_pre, &values)?;
    let vtable = host_object_vtable(caller)?;
    let object = StructRef::new(
        &mut *caller,
        &shape_pre,
        &[
            vtable,
            Val::AnyRef(Some(names.to_anyref())),
            Val::AnyRef(Some(values.to_anyref())),
        ],
    )?;
    Ok(Val::AnyRef(Some(object.to_anyref())))
}

fn copy_field_name(
    caller: &mut Caller<'_, StoreData>,
    name: Val,
    present: bool,
) -> wasmtime::Result<Val> {
    let object = as_struct(caller, &name, "field name")?;
    let string = build_intrinsic_types(caller.engine())?.string;
    if StructType::eq(&object.ty(&*caller)?, &string) {
        return Ok(name);
    }
    let ty = if present {
        build_intrinsic_types(caller.engine())?.string
    } else {
        object.ty(&*caller)?
    };
    let mut fields = vec![
        object.field(&mut *caller, 0)?,
        object.field(&mut *caller, 1)?,
    ];
    if !present {
        fields.push(Val::I32(0));
    }
    let pre = StructRefPre::new(&mut *caller, ty);
    Ok(Val::AnyRef(Some(
        StructRef::new(&mut *caller, &pre, &fields)?.to_anyref(),
    )))
}

/// The compiler marks rejected known fields with non-null mask slots.
fn spread_omitted_fields(
    caller: &mut Caller<'_, StoreData>,
    mask: &Val,
) -> wasmtime::Result<std::collections::BTreeSet<Vec<u16>>> {
    let mut omitted = std::collections::BTreeSet::new();
    if let Some((names, values)) = shape_arrays(caller, mask)? {
        for index in 0..names.len(&mut *caller)? {
            if !matches!(values.get(&mut *caller, index)?, Val::AnyRef(None)) {
                let name = names.get(&mut *caller, index)?;
                omitted.insert(string_units(caller, &name)?);
            }
        }
    }
    Ok(omitted)
}

fn has_own(caller: &mut Caller<'_, StoreData>, obj: &Val, key: &Val) -> wasmtime::Result<bool> {
    if matches!(obj, Val::AnyRef(None)) {
        return Err(wasmtime::Error::msg("Object.hasOwn called on null"));
    }
    let Some((names, values)) = shape_arrays(caller, obj)? else {
        return Ok(false);
    };
    let target = string_units(caller, key)?;
    for i in 0..names.len(&mut *caller)? {
        let name = names.get(&mut *caller, i)?;
        if string_units(caller, &name)? == target {
            let value = values.get(&mut *caller, i)?;
            return field_is_present(caller, &name, &value);
        }
    }
    Ok(false)
}

/// SameValue on two `f64` bit patterns: any NaN equals any NaN (Wasm arithmetic
/// varies sign/payload bits), otherwise exact bit equality — which separates
/// `+0` from `-0` and compares ordinary values exactly.
fn same_value_bits(a: u64, b: u64) -> bool {
    (f64::from_bits(a).is_nan() && f64::from_bits(b).is_nan()) || a == b
}

fn boxed_number_bits(caller: &mut Caller<'_, StoreData>, val: &Val) -> wasmtime::Result<u64> {
    let st = as_struct(caller, val, "Object.is operand")?;
    match st.field(&mut *caller, 1)? {
        Val::F64(bits) => Ok(bits),
        other => Err(wasmtime::Error::msg(format!(
            "Object.is: boxed number payload is {other:?}, not f64"
        ))),
    }
}

async fn same_value(
    caller: &mut Caller<'_, StoreData>,
    a: &Val,
    b: &Val,
) -> wasmtime::Result<bool> {
    match (a, b) {
        (Val::AnyRef(None), Val::AnyRef(None)) => return Ok(true),
        (Val::AnyRef(None), _) | (_, Val::AnyRef(None)) => return Ok(false),
        _ => {}
    }
    // The number arm must run before vtable dispatch: the boxed-number `equals`
    // slot is `===` (`0 === -0`, `NaN !== NaN`), while SameValue distinguishes
    // both.
    let boxed_number = build_intrinsic_types(caller.engine())?.boxed_number;
    if is_a(caller, a, &boxed_number)? && is_a(caller, b, &boxed_number)? {
        let (a_bits, b_bits) = (boxed_number_bits(caller, a)?, boxed_number_bits(caller, b)?);
        return Ok(same_value_bits(a_bits, b_bits));
    }
    match dispatch_vtable_slot(caller, a, 2, std::slice::from_ref(b)).await? {
        Val::I32(v) => Ok(v != 0),
        other => Err(wasmtime::Error::msg(format!(
            "Object.is: equals returned {other:?}, expected i32"
        ))),
    }
}

pub fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let intr = build_intrinsic_types(&engine)?;
    let obj = ValType::Ref(RefType::new(
        true,
        HeapType::ConcreteStruct(intr.object.clone()),
    ));
    let string = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intrinsic_string_type(&engine)?),
    ));
    let array = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intrinsic_array_type(&engine)?),
    ));
    let boolean = ValType::I32;
    let ft = |params: Vec<ValType>, results: Vec<ValType>| FuncType::new(&engine, params, results);

    for kind in [Enumerate::Keys, Enumerate::Values, Enumerate::Entries] {
        register_host_fn(
            linker,
            MODULE_NAME,
            ctor_key(kind.method()),
            ft(vec![obj.clone()], vec![array.clone()]),
            true,
            move |caller, params, results| {
                results[0] = enumerate(caller, &params[0], kind)?;
                Ok(())
            },
        )?;
    }

    register_host_fn(
        linker,
        MODULE_NAME,
        ctor_key("hasOwn"),
        ft(vec![obj.clone(), string], vec![boolean.clone()]),
        true,
        |caller, params, results| {
            results[0] = Val::I32(i32::from(has_own(caller, &params[0], &params[1])?));
            Ok(())
        },
    )?;

    let shape = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.object_shape.clone()),
    ));
    register_host_fn(
        linker,
        MODULE_NAME,
        ctor_key("#spread"),
        ft(
            vec![obj.clone(), obj.clone(), obj.clone(), obj.clone()],
            vec![shape],
        ),
        true,
        |caller, params, results| {
            results[0] = spread(caller, &params[0], &params[1], &params[2], &params[3])?;
            Ok(())
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        ctor_key("is"),
        ft(vec![obj.clone(), obj], vec![boolean]),
        true,
        |caller, params, results| {
            Box::pin(async move {
                results[0] = Val::I32(i32::from(same_value(caller, &params[0], &params[1]).await?));
                Ok(())
            })
        },
    )
}

pub fn declare(defs: &mut PackageDeclaration) {
    // Compiler-only helper: deliberately absent from ObjectConstructor's public surface.
    declare_method(
        defs,
        "#spread",
        ctor_key("#spread"),
        vec![
            Param::new("target", Type::Unknown),
            Param::new("source", Type::Unknown),
            Param::new("shape", Type::Unknown),
            Param::new("mask", Type::Unknown),
        ],
        Type::Object {
            fields: Default::default(),
        },
    );
    // `Dispatch::Static` drops the constructor receiver, so no receiver param.
    // Types mirror the prelude MethodSigs: `Type::Array`/`Type::Tuple` lower to
    // `(ref $Array)`, matching the registered FuncTypes above.
    let obj = || Param::new("obj", Type::Unknown);
    declare_method(
        defs,
        "keys",
        ctor_key("keys"),
        vec![obj()],
        Type::Array(Box::new(Type::String)),
    );
    declare_method(
        defs,
        "values",
        ctor_key("values"),
        vec![obj()],
        Type::Array(Box::new(Type::Unknown)),
    );
    declare_method(
        defs,
        "entries",
        ctor_key("entries"),
        vec![obj()],
        Type::Array(Box::new(Type::Tuple(vec![Type::String, Type::Unknown]))),
    );
    declare_method(
        defs,
        "hasOwn",
        ctor_key("hasOwn"),
        vec![obj(), Param::new("key", Type::String)],
        Type::Boolean,
    );
    declare_method(
        defs,
        "is",
        ctor_key("is"),
        vec![
            Param::new("a", Type::Unknown),
            Param::new("b", Type::Unknown),
        ],
        Type::Boolean,
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
        "Object".to_string(),
        TypeSymbol {
            name: "Object".to_string(),
            mangled_name: crate::mangle::prelude("Object"),
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
                            doc: doc("/** Returns `\"[object Object]\"`. */"),
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
                                "/** Returns the JSON representation of this object — `\"{\"` + `\"key\":value-json` pairs joined with `\",\"` + `\"}\"`. Field iteration order matches the type's struct layout. */",
                            ),
                        },
                    ),
                ]),
                properties: BTreeMap::new(),
                // Object opts OUT of direct dispatch — `obj.toString()`
                // and `obj.toJson()` route through the per-shape vtable
                // (slots 0 / 1). Each user-object shape generates its
                // own per-type body.
                dispatch: Dispatch::VTable,
                doc: doc("/** Universal base type for every object shape. */"),
            },
        },
    );
    defs.types.insert(
        "ObjectConstructor".to_string(),
        TypeSymbol {
            name: "ObjectConstructor".to_string(),
            mangled_name: crate::mangle::prelude("ObjectConstructor"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface {
                generics: Vec::new(),
                methods: BTreeMap::from([
                    (
                        "keys".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("obj", Type::Unknown)],
                            ret: Type::Array(Box::new(Type::String)),
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns the object's field names in canonical sorted order (the same order JSON output uses).\n * Optional fields are included even when they currently hold `null`. Non-object values yield `[]`; `null` throws a catchable `Error`.\n * @param obj The object to enumerate.\n */",
                            ),
                        },
                    ),
                    (
                        "values".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("obj", Type::Unknown)],
                            ret: Type::Array(Box::new(Type::Unknown)),
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns the object's field values, aligned with `Object.keys` order. Element types are erased to `unknown` — narrow with `typeof` / `as`.\n * Non-object values yield `[]`; `null` throws a catchable `Error`.\n * @param obj The object to enumerate.\n */",
                            ),
                        },
                    ),
                    (
                        "entries".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("obj", Type::Unknown)],
                            ret: Type::Array(Box::new(Type::Tuple(vec![
                                Type::String,
                                Type::Unknown,
                            ]))),
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns `[name, value]` pairs in canonical sorted key order. Values are erased to `unknown` — narrow with `typeof` / `as`.\n * Non-object values yield `[]`; `null` throws a catchable `Error`.\n * @param obj The object to enumerate.\n */",
                            ),
                        },
                    ),
                    (
                        "hasOwn".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("obj", Type::Unknown),
                                Param::new("key", Type::String),
                            ],
                            ret: Type::Boolean,
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns `true` when `obj` declares a field named `key`. Non-object values have no fields; `null` throws a catchable `Error`.\n * @param obj The object to test.\n * @param key The field name.\n */",
                            ),
                        },
                    ),
                    (
                        "is".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("a", Type::Unknown),
                                Param::new("b", Type::Unknown),
                            ],
                            ret: Type::Boolean,
                            predicate: None,
                            doc: doc(
                                "/**\n * SameValue comparison: like `===` but `Object.is(NaN, NaN)` is `true` and `Object.is(0, -0)` is `false`.\n * Object comparison follows the language's structural `===`, not JS reference identity.\n */",
                            ),
                        },
                    ),
                ]),
                properties: BTreeMap::new(),
                dispatch: Dispatch::Static,
                doc: doc(
                    "/** Constructor object for `Object` — the enumeration statics (`keys`/`values`/`entries`/`hasOwn`) and `is`. Accessed via the global `Object` binding. */",
                ),
            },
        },
    );

    defs.values.insert(
        "Object".to_string(),
        ValueSymbol {
            name: "Object".to_string(),
            mangled_name: crate::mangle::prelude("Object"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: ValueKind::Const {
                ty: Type::prelude_interface("ObjectConstructor".to_string(), Vec::new()),
                doc: doc(
                    "/** The `Object` namespace — `Object.keys(o)`, `Object.values(o)`, `Object.entries(o)`, `Object.hasOwn(o, k)`, `Object.is(a, b)`. */",
                ),
            },
        },
    );
}

#[cfg(test)]
mod tests {
    use super::same_value_bits;

    #[test]
    fn same_value_bit_rules() {
        assert!(same_value_bits(f64::NAN.to_bits(), (-f64::NAN).to_bits()));
        assert!(!same_value_bits(0f64.to_bits(), (-0f64).to_bits()));
        assert!(same_value_bits((-0f64).to_bits(), (-0f64).to_bits()));
        assert!(same_value_bits(1.5f64.to_bits(), 1.5f64.to_bits()));
        assert!(!same_value_bits(1f64.to_bits(), 2f64.to_bits()));
    }
}
