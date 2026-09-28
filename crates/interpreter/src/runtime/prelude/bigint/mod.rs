//! The Rust port of the prelude's `BigInt` instance-method surface
//! (`toString(radix?)`, `toJson`). The arbitrary-precision compute lives in
//! [`crate::runtime::prelude::bigint::ops`] (num-bigint); this module is the wasmtime boundary
//! that reads the `$bigint` receiver and marshals the `$string` result.
//!
//! The arithmetic/conversion operators stay under `submilli:bigint` — the live
//! Wasm prelude still imports them — exactly as Number's low-level conversions
//! stay under `submilli:number`. They re-home when the prelude is fully ported.

pub(crate) mod ops;

use wasmtime::{Caller, FuncType, HeapType, Linker, RefType, Val, ValType};

use crate::runtime::StoreData;
use crate::runtime::host::{
    intrinsic_bigint_type, intrinsic_string_type, register_host_fn, write_submilli_string_struct,
};
use crate::runtime::prelude::bigint::ops::{limbs_to_bigint, read_bigint_struct};
use crate::runtime::prelude::{MODULE_NAME, declare_method};
use crate::{MangledName, PackageDeclaration, Param, Type};

/// Dispatch key for an instance method `BigInt#<m>` — the prelude's `BigInt`
/// interface mangled name extended by the method.
fn method_key(method: &str) -> MangledName {
    crate::mangle::extend(&crate::mangle::prelude("BigInt"), method)
}

pub fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let bigint_ref = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intrinsic_bigint_type(&engine)?),
    ));
    let string_ref = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intrinsic_string_type(&engine)?),
    ));

    // `BigInt#toString(radix)` — radix defaults to 10 (supplied by codegen from
    // the interface default); out of `[2, 36]` throws a `RangeError`.
    let s = string_ref.clone();
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("toString"),
        FuncType::new(&engine, [bigint_ref.clone(), ValType::F64], [s]),
        true,
        |caller, params, results| {
            let (sign, limbs) = read_bigint_struct(caller, &params[0], "BigInt#toString")?;
            let radix = match params[1] {
                Val::F64(bits) => f64::from_bits(bits),
                ref other => wasmtime::bail!("BigInt#toString expects f64 radix, got {other:?}"),
            };
            let truncated = radix.trunc();
            if radix.is_nan() || !(2.0..=36.0).contains(&truncated) {
                wasmtime::bail!("toString radix must be between 2 and 36");
            }
            let text = limbs_to_bigint(sign, &limbs).to_str_radix(truncated as u32);
            results[0] = string_val(caller, &text)?;
            Ok(())
        },
    )?;

    // `BigInt#toJson` — canonical decimal; JSON has no native bigint literal, so
    // it matches `toString()`.
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("toJson"),
        FuncType::new(&engine, [bigint_ref.clone()], [string_ref]),
        true,
        |caller, params, results| {
            let (sign, limbs) = read_bigint_struct(caller, &params[0], "BigInt#toJson")?;
            let text = limbs_to_bigint(sign, &limbs).to_str_radix(10);
            results[0] = string_val(caller, &text)?;
            Ok(())
        },
    )?;

    // `BigInt(value: string | number)` — the conversion call-signature
    // (`Dispatch::Static`, only the boxed value arrives).
    // Non-null: the declared `string | number` union has no null member, so
    // codegen's import lowers to `(ref $Object)`.
    let obj_param = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(
            crate::runtime::intrinsic_types::build_intrinsic_types(&engine)?.object,
        ),
    ));
    register_host_fn(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&crate::mangle::prelude("BigIntConstructor"), "@call"),
        FuncType::new(&engine, [obj_param], [bigint_ref]),
        true,
        |caller, params, results| {
            results[0] = bigint_ctor_call(caller, &params[0])?;
            Ok(())
        },
    )?;

    Ok(())
}

/// `BigInt(value)`: a `$string` parses as a decimal literal (trimmed; invalid
/// text throws); a `$boxed_number` must be a finite integer (matching JS
/// `BigInt(n)`), converted exactly for safe-range integers.
fn bigint_ctor_call(caller: &mut Caller<'_, StoreData>, value: &Val) -> wasmtime::Result<Val> {
    let Val::AnyRef(Some(any)) = value else {
        return Err(wasmtime::Error::msg("BigInt(value): value is null"));
    };
    let intr = crate::runtime::intrinsic_types::build_intrinsic_types(caller.engine())?;
    let Some(st) = any.as_struct(&mut *caller)? else {
        return Err(wasmtime::Error::msg("BigInt(value): not a struct value"));
    };
    if wasmtime::StructType::eq(&st.ty(&*caller)?, &intr.string) {
        let s = crate::runtime::host::read_string_arg(caller, value, "BigInt(string)")?;
        let trimmed = s.trim();
        let parsed: num_bigint::BigInt = trimmed.parse().map_err(|_| {
            crate::runtime::host::syntax_error(format!(
                "BigInt(string): invalid bigint literal: {trimmed:?}"
            ))
        })?;
        return crate::runtime::prelude::bigint::ops::make_bigint_struct(caller, parsed);
    }
    if wasmtime::StructType::eq(&st.ty(&*caller)?, &intr.boxed_number) {
        let n = match st.field(&mut *caller, 1)? {
            Val::F64(bits) => f64::from_bits(bits),
            other => wasmtime::bail!("BigInt(number): field 1 is {other:?}, not f64"),
        };
        if !n.is_finite() {
            return Err(crate::runtime::host::range_error(format!(
                "BigInt(number): cannot convert non-finite number to bigint ({n})"
            )));
        }
        if n.fract() != 0.0 {
            return Err(crate::runtime::host::range_error(format!(
                "BigInt(number): cannot convert non-integer number to bigint ({n})"
            )));
        }
        // For integer doubles up to 2^53 the cast is exact; beyond, the f64
        // itself already lost precision (matches JS `BigInt(N)`).
        return crate::runtime::prelude::bigint::ops::make_bigint_struct(
            caller,
            num_bigint::BigInt::from(n as i128),
        );
    }
    Err(wasmtime::Error::msg(
        "BigInt(value): expected a string or number",
    ))
}

pub fn declare(defs: &mut PackageDeclaration) {
    // First param is the (implicit) `$bigint` receiver.
    declare_method(
        defs,
        "toString",
        method_key("toString"),
        vec![
            Param::new("value", Type::BigInt),
            Param::new("radix", Type::Number),
        ],
        Type::String,
    );
    declare_method(
        defs,
        "toJson",
        method_key("toJson"),
        vec![Param::new("value", Type::BigInt)],
        Type::String,
    );
    declare_method(
        defs,
        "@call",
        crate::mangle::extend(&crate::mangle::prelude("BigIntConstructor"), "@call"),
        vec![Param::new(
            "value",
            Type::Union(vec![Type::String, Type::Number]),
        )],
        Type::BigInt,
    );
}

fn string_val(caller: &mut Caller<'_, StoreData>, s: &str) -> wasmtime::Result<Val> {
    let st = write_submilli_string_struct(caller, s)?;
    Ok(Val::AnyRef(Some(st.to_anyref())))
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
        "BigInt".to_string(),
        TypeSymbol {
            name: "BigInt".to_string(),
            mangled_name: crate::mangle::prelude("BigInt"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface { index: None,
                generics: Vec::new(),
                methods: BTreeMap::from([
                    (
                        "toString".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::with_default(
                                "radix",
                                Type::Number,
                                crate::DefaultValue::Number(10.0),
                            )],
                            ret: Type::String,
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns this bigint formatted in the given base.\n * @param radix Base 2–36 (out of range throws a `RangeError`). Defaults to `10`.\n */",
                            ),
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
                                "/** Returns the canonical decimal representation — JSON has no native bigint literal, so this matches `toString`. A future strict mode may quote the result. */",
                            ),
                        },
                    ),
                ]),
                properties: BTreeMap::new(),
                dispatch: Dispatch::Direct,
                doc: doc(
                    "/** Arbitrary-precision integer. Literals use the `n` suffix: `42n`. */",
                ),
            },
        },
    );
    defs.types.insert(
        "BigIntConstructor".to_string(),
        TypeSymbol {
            name: "BigIntConstructor".to_string(),
            mangled_name: crate::mangle::prelude("BigIntConstructor"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface { index: None,
                generics: Vec::new(),
                methods: BTreeMap::from([(
                    "@call".to_string(),
                    MethodSig {
                        generics: Vec::new(),
                        params: vec![Param::new(
                            "value",
                            Type::Union(vec![Type::String, Type::Number]),
                        )],
                        ret: Type::BigInt,
                        predicate: None,
                        doc: doc(
                            "/**\n * Convert a `string` or `number` to a `bigint`.\n * - String input is parsed as a decimal of arbitrary length. Throws `Error` on a malformed string.\n * - Number input must be a finite integer. Throws on NaN, ±Inf, or non-integer values.\n */",
                        ),
                    },
                )]),
                properties: BTreeMap::new(),
                dispatch: Dispatch::Static,
                doc: doc(
                    "/** Constructor object for `bigint`. Accessed via the global `BigInt` binding. */",
                ),
            },
        },
    );
    defs.values.insert(
        "BigInt".to_string(),
        ValueSymbol {
            name: "BigInt".to_string(),
            mangled_name: crate::mangle::prelude("BigInt"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: ValueKind::Const {
                ty: Type::prelude_interface("BigIntConstructor".to_string(), Vec::new()),
                doc: doc(
                    "/** The `BigInt` constructor — call `BigInt(n)` to convert an integer-valued number to a bigint. Use `BigInt.fromString(s)` for string parsing. */",
                ),
            },
        },
    );
}
