//! The Rust port of the prelude's `Number` / `NumberConstructor` surface.
//!
//! The compute lives in [`crate::runtime::number`] (Ryū-backed formatting,
//! Eisel-Lemire parsing); this module is the wasmtime boundary that marshals
//! `f64` ↔ `$string` across it and registers each method under its dispatch key.
//! It also hosts `toNumber` — the string→number coercion `Number(x)` calls — the
//! former `submilli:number` module, folded into the prelude-host surface.

use wasmtime::{
    Caller, FuncType, Global, GlobalType, HeapType, Linker, Mutability, RefType, Store, Val,
    ValType,
};

use crate::runtime::StoreData;
use crate::runtime::host::{
    intrinsic_string_type, read_string_arg, register_host_fn, write_submilli_string_struct,
};
use crate::runtime::number::{
    format_number_js, parse_float_js, parse_int_js, to_exponential_js, to_fixed_js,
    to_precision_js, to_string_radix_js,
};
use crate::runtime::prelude::{MODULE_NAME, declare_method};
use crate::{MangledName, PackageDeclaration, Param, Span, Type, ValueKind, ValueSymbol};

/// `2^53 - 1`, the largest integer an `f64` can represent without losing
/// neighbours — the `isSafeInteger` bound.
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// The `NumberConstructor` static constants — `Number.EPSILON`, … — as
/// `(field-name suffix, value)`. `MIN_VALUE` is the smallest positive
/// *subnormal* (`5e-324`), not `f64::MIN_POSITIVE`.
const NUMBER_CONSTANTS: [(&str, f64); 8] = [
    ("EPSILON", f64::EPSILON),
    ("MAX_VALUE", f64::MAX),
    ("MIN_VALUE", 5e-324),
    ("MAX_SAFE_INTEGER", MAX_SAFE_INTEGER),
    ("MIN_SAFE_INTEGER", -MAX_SAFE_INTEGER),
    ("POSITIVE_INFINITY", f64::INFINITY),
    ("NEGATIVE_INFINITY", f64::NEG_INFINITY),
    ("NaN", f64::NAN),
];

/// The `globalThis` f64 constants — `NaN` / `Infinity` — keyed by their bare
/// prelude name (not the `NumberConstructor#` prefix the statics use).
const GLOBAL_CONSTANTS: [(&str, f64); 2] = [("NaN", f64::NAN), ("Infinity", f64::INFINITY)];

/// The value of a `globalThis` numeric constant, or `None` for any other name.
/// The typechecker folds these into a literal parameter default, which stays
/// correct only while it reads the same table [`install_constants`] installs from.
pub(crate) fn global_constant_value(name: &str) -> Option<f64> {
    GLOBAL_CONSTANTS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|&(_, v)| v)
}

/// The mangled name of a `NumberConstructor` static constant, matching the
/// member-access key codegen resolves (`Number.EPSILON` → this).
/// `Number(value)`: a `$string` parses with JS `Number(...)` semantics; a
/// `$bigint` converts to f64 (∞ on overflow). Anything else mirrors the Wasm
/// wrapper's failed cast as a catchable error.
fn number_ctor_call(caller: &mut Caller<'_, StoreData>, value: &Val) -> wasmtime::Result<f64> {
    let Val::AnyRef(Some(any)) = value else {
        return Err(crate::runtime::host::type_error(
            "Number(value): value is null",
        ));
    };
    let intr = crate::runtime::intrinsic_types::build_intrinsic_types(caller.engine())?;
    let Some(st) = any.as_struct(&mut *caller)? else {
        return Err(crate::runtime::host::type_error(
            "Number(value): not a struct value",
        ));
    };
    if wasmtime::StructType::eq(&st.ty(&*caller)?, &intr.string) {
        let s = read_string_arg(caller, value, "Number(string)")?;
        return Ok(crate::runtime::number::string_to_number_js(&s));
    }
    if wasmtime::StructType::eq(&st.ty(&*caller)?, &intr.bigint) {
        use num_traits::ToPrimitive;
        let (sign, limbs) = crate::runtime::prelude::bigint::ops::read_bigint_struct(
            caller,
            value,
            "Number(bigint)",
        )?;
        return Ok(
            crate::runtime::prelude::bigint::ops::limbs_to_bigint(sign, &limbs)
                .to_f64()
                .unwrap_or(f64::INFINITY),
        );
    }
    Err(crate::runtime::host::type_error(
        "Number(value): expected a string or bigint",
    ))
}

fn constant_key(name: &str) -> MangledName {
    crate::mangle::prelude(&format!("NumberConstructor#{name}"))
}

/// The mangled name of a `globalThis` symbol (`isNaN`, `NaN`, …).
fn global_key(name: &str) -> MangledName {
    crate::mangle::prelude(name)
}

type FormatOp = fn(f64, f64) -> Result<String, String>;

/// Dispatch key for an instance method `Number#<m>` — the prelude's `Number`
/// interface mangled name extended by the method.
fn method_key(method: &str) -> MangledName {
    crate::mangle::extend(&crate::mangle::prelude("Number"), method)
}

/// Dispatch key for a `NumberConstructor` static (`Number.parseInt`, …).
fn ctor_key(method: &str) -> MangledName {
    crate::mangle::extend(&crate::mangle::prelude("NumberConstructor"), method)
}

pub fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let string_struct = intrinsic_string_type(&engine)?;
    let string_ref = ValType::Ref(RefType::new(false, HeapType::ConcreteStruct(string_struct)));

    // Instance formatters — receiver `f64` plus a digits/radix `f64` whose NaN /
    // default value is the omitted-optional sentinel the `*_js` ops interpret.
    // `to_string_radix_js` treats radix 10 (the declared default) as plain
    // `toString`.
    reg_format(
        linker,
        &engine,
        &string_ref,
        method_key("toString"),
        to_string_radix_js,
    )?;
    reg_format(
        linker,
        &engine,
        &string_ref,
        method_key("toFixed"),
        to_fixed_js,
    )?;
    reg_format(
        linker,
        &engine,
        &string_ref,
        method_key("toPrecision"),
        to_precision_js,
    )?;
    reg_format(
        linker,
        &engine,
        &string_ref,
        method_key("toExponential"),
        to_exponential_js,
    )?;

    // `Number#toJson` — no argument; non-finite values are not valid JSON and
    // render as `null`.
    let s = string_ref.clone();
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("toJson"),
        FuncType::new(&engine, [ValType::F64], [s]),
        true,
        |caller, params, results| {
            let n = read_f64(&params[0], "Number#toJson")?;
            let out = if n.is_finite() {
                format_number_js(n)
            } else {
                "null".to_string()
            };
            results[0] = string_val(caller, &out)?;
            Ok(())
        },
    )?;

    // `NumberConstructor#parseInt(s, radix)` — radix defaults to 10.
    let s = string_ref.clone();
    register_host_fn(
        linker,
        MODULE_NAME,
        ctor_key("parseInt"),
        FuncType::new(&engine, [s, ValType::F64], [ValType::F64]),
        true,
        |caller, params, results| {
            let input = read_string_arg(caller, &params[0], "Number.parseInt")?;
            let radix = read_f64(&params[1], "Number.parseInt")?;
            results[0] = Val::F64(parse_int_js(&input, to_radix_u32(radix)).to_bits());
            Ok(())
        },
    )?;

    // `NumberConstructor#parseFloat(s)`.
    let s = string_ref.clone();
    register_host_fn(
        linker,
        MODULE_NAME,
        ctor_key("parseFloat"),
        FuncType::new(&engine, [s], [ValType::F64]),
        true,
        |caller, params, results| {
            let input = read_string_arg(caller, &params[0], "Number.parseFloat")?;
            results[0] = Val::F64(parse_float_js(&input).to_bits());
            Ok(())
        },
    )?;

    // `Number(value: string | bigint)` — the conversion call-signature
    // (`Dispatch::Static`, only the boxed value arrives). Strings parse with JS
    // `Number(...)` semantics; bigints convert to f64 (∞ on overflow).
    // Non-null: the declared `string | bigint` union has no null member, so
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
        ctor_key("@call"),
        FuncType::new(&engine, [obj_param], [ValType::F64]),
        true,
        |caller, params, results| {
            results[0] = Val::F64(number_ctor_call(caller, &params[0])?.to_bits());
            Ok(())
        },
    )?;

    // `NumberConstructor` predicates and the bare `globalThis` `isNaN`/`isFinite`
    // (which share bodies). The double import of the globals with the still-Wasm
    // prelude is fine — the prelude disappears once it's fully ported.
    reg_number_predicate(linker, &engine, ctor_key("isNaN"), f64::is_nan)?;
    reg_number_predicate(linker, &engine, ctor_key("isFinite"), f64::is_finite)?;
    reg_number_predicate(linker, &engine, ctor_key("isInteger"), is_integer)?;
    reg_number_predicate(linker, &engine, ctor_key("isSafeInteger"), is_safe_integer)?;
    reg_predicate(linker, &engine, global_key("isNaN"), f64::is_nan)?;
    reg_predicate(linker, &engine, global_key("isFinite"), f64::is_finite)?;

    Ok(())
}

pub fn declare(defs: &mut PackageDeclaration) {
    let n = || Param::new("value", Type::Number);
    let s = || Param::new("s", Type::String);

    declare_method(
        defs,
        "@call",
        ctor_key("@call"),
        vec![Param::new(
            "value",
            Type::Union(vec![Type::String, Type::BigInt]),
        )],
        Type::Number,
    );

    // Instance methods: first param is the (implicit) `f64` receiver.
    declare_method(
        defs,
        "toString",
        method_key("toString"),
        vec![n(), Param::new("radix", Type::Number)],
        Type::String,
    );
    declare_method(
        defs,
        "toFixed",
        method_key("toFixed"),
        vec![n(), Param::new("digits", Type::Number)],
        Type::String,
    );
    declare_method(
        defs,
        "toPrecision",
        method_key("toPrecision"),
        vec![n(), Param::new("precision", Type::Number)],
        Type::String,
    );
    declare_method(
        defs,
        "toExponential",
        method_key("toExponential"),
        vec![n(), Param::new("fractionDigits", Type::Number)],
        Type::String,
    );
    declare_method(
        defs,
        "toJson",
        method_key("toJson"),
        vec![n()],
        Type::String,
    );

    // Statics: `Dispatch::Static` drops the receiver, so no receiver param.
    declare_method(
        defs,
        "parseInt",
        ctor_key("parseInt"),
        vec![s(), Param::new("radix", Type::Number)],
        Type::Number,
    );
    declare_method(
        defs,
        "parseFloat",
        ctor_key("parseFloat"),
        vec![s()],
        Type::Number,
    );
    for pred in ["isNaN", "isFinite", "isInteger", "isSafeInteger"] {
        declare_method(
            defs,
            pred,
            ctor_key(pred),
            vec![Param::new("value", Type::Unknown)],
            Type::Boolean,
        );
    }
    // `globalThis.isNaN` / `globalThis.isFinite`.
    declare_method(defs, "isNaN", global_key("isNaN"), vec![n()], Type::Boolean);
    declare_method(
        defs,
        "isFinite",
        global_key("isFinite"),
        vec![n()],
        Type::Boolean,
    );

    // Constants: a value symbol per constant so the guest imports the global from
    // prelude-host (see `install_constants`) rather than the prelude. Covers both
    // the `NumberConstructor#` statics and the bare `globalThis` `NaN`/`Infinity`.
    for (name, _) in NUMBER_CONSTANTS {
        declare_const(defs, name, constant_key(name));
    }
    for (name, _) in GLOBAL_CONSTANTS {
        declare_const(defs, name, global_key(name));
    }
}

/// Declare a readonly numeric constant as a value symbol under `mangled`.
fn declare_const(defs: &mut PackageDeclaration, name: &str, mangled: MangledName) {
    defs.values.insert(
        mangled.as_str().to_string(),
        ValueSymbol {
            name: name.to_string(),
            mangled_name: mangled,
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: ValueKind::Const {
                ty: Type::Number,
                doc: None,
            },
        },
    );
}

/// The `Number` statics and `globalThis` f64 constants, defined as host-owned
/// globals — the value-equivalent of a host fn (a constant has no body to run).
/// Store-bound, so this installs from the per-store path before the prelude
/// instantiates. Field names match what codegen imports from
/// [`MODULE_NAME`].
pub(crate) fn install_constants(
    linker: &mut Linker<StoreData>,
    store: &mut Store<StoreData>,
) -> wasmtime::Result<()> {
    let statics = NUMBER_CONSTANTS.iter().map(|&(n, v)| (constant_key(n), v));
    let globals = GLOBAL_CONSTANTS.iter().map(|&(n, v)| (global_key(n), v));
    for (mangled, value) in statics.chain(globals) {
        // Mutable to match the `mutable: true` global import codegen emits for a
        // value symbol (const-ness is typecheck-enforced, not Wasm-enforced).
        let gty = GlobalType::new(ValType::F64, Mutability::Var);
        let global = Global::new(&mut *store, gty, Val::F64(value.to_bits()))?;
        linker.define(&mut *store, MODULE_NAME, mangled.as_str(), global)?;
    }
    Ok(())
}

/// Register an `(f64, f64) -> $string` formatter that may reject its argument
/// (out-of-range digits/radix → a thrown `Error`).
fn reg_format(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    string_ref: &ValType,
    key: MangledName,
    op: FormatOp,
) -> wasmtime::Result<()> {
    register_host_fn(
        linker,
        MODULE_NAME,
        key,
        FuncType::new(engine, [ValType::F64, ValType::F64], [string_ref.clone()]),
        true,
        move |caller, params, results| {
            let x = read_f64(&params[0], "Number formatter")?;
            let arg = read_f64(&params[1], "Number formatter")?;
            // Formatter argument rejections (radix/digits/precision out of
            // range) are spec `RangeError`s.
            let out = op(x, arg).map_err(crate::runtime::host::range_error)?;
            results[0] = string_val(caller, &out)?;
            Ok(())
        },
    )
}

/// Inspect a boxed value without coercing non-number inputs.
fn reg_number_predicate(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    key: MangledName,
    op: fn(f64) -> bool,
) -> wasmtime::Result<()> {
    let intr = crate::runtime::intrinsic_types::build_intrinsic_types(engine)?;
    register_host_fn(
        linker,
        MODULE_NAME,
        key,
        FuncType::new(
            engine,
            [ValType::Ref(RefType::new(
                true,
                HeapType::ConcreteStruct(intr.object),
            ))],
            [ValType::I32],
        ),
        true,
        move |caller, params, results| {
            results[0] =
                Val::I32(super::value::number_value(caller, &params[0])?.is_some_and(op) as i32);
            Ok(())
        },
    )
}

/// Register an `(f64) -> boolean` predicate after caller-side numeric conversion.
fn reg_predicate(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    key: MangledName,
    op: fn(f64) -> bool,
) -> wasmtime::Result<()> {
    register_host_fn(
        linker,
        MODULE_NAME,
        key,
        FuncType::new(engine, [ValType::F64], [ValType::I32]),
        true,
        move |_caller, params, results| {
            let n = read_f64(&params[0], "Number predicate")?;
            results[0] = Val::I32(op(n) as i32);
            Ok(())
        },
    )
}

fn is_integer(n: f64) -> bool {
    n.is_finite() && n == n.trunc()
}

fn is_safe_integer(n: f64) -> bool {
    is_integer(n) && n.abs() <= MAX_SAFE_INTEGER
}

/// ToInteger on a radix argument: NaN → 0, finite → trunc, out of `[0, 36]` → 0
/// (so `parse_int_js`'s own range check returns `NaN`).
fn to_radix_u32(radix: f64) -> u32 {
    if radix.is_nan() {
        return 0;
    }
    let truncated = radix.trunc();
    if (0.0..=36.0).contains(&truncated) {
        truncated as u32
    } else {
        0
    }
}

fn read_f64(val: &Val, name: &str) -> wasmtime::Result<f64> {
    match val {
        Val::F64(bits) => Ok(f64::from_bits(*bits)),
        other => Err(crate::runtime::host::type_error(format!(
            "{name} expects f64, got {other:?}"
        ))),
    }
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
        Dispatch, MethodSig, Param, PropertySig, Span, Type, TypeKind, TypeSymbol, ValueKind,
        ValueSymbol,
    };
    use std::collections::BTreeMap;
    defs.types.insert(
        "Number".to_string(),
        TypeSymbol {
            name: "Number".to_string(),
            mangled_name: crate::mangle::prelude("Number"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface {
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
                                "/**\n * Returns this number formatted in the given base.\n * @param radix Base 2–36 (out of range throws a `RangeError`). Defaults to `10`.\n */",
                            ),
                        },
                    ),
                    (
                        "toFixed".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::with_default(
                                "digits",
                                Type::Number,
                                crate::DefaultValue::Number(0.0),
                            )],
                            ret: Type::String,
                            predicate: None,
                            doc: doc(
                                "/**\n * Fixed-point notation with exactly `digits` fraction digits — `(3.14159).toFixed(2)` is `\"3.14\"`.\n * @param digits 0–100 (out of range throws a `RangeError`). Defaults to `0`.\n */",
                            ),
                        },
                    ),
                    (
                        "toPrecision".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::with_default(
                                "precision",
                                Type::Number,
                                crate::DefaultValue::Number(f64::NAN),
                            )],
                            ret: Type::String,
                            predicate: None,
                            doc: doc(
                                "/**\n * Formats to `precision` significant digits, switching to exponential form for very large or small values.\n * @param precision 1–100 (out of range throws a `RangeError`). Omitted behaves like `toString()`.\n */",
                            ),
                        },
                    ),
                    (
                        "toExponential".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::with_default(
                                "fractionDigits",
                                Type::Number,
                                crate::DefaultValue::Number(f64::NAN),
                            )],
                            ret: Type::String,
                            predicate: None,
                            doc: doc(
                                "/**\n * Exponential notation — `(1234.5).toExponential(2)` is `\"1.23e+3\"`.\n * @param fractionDigits 0–100 fraction digits. Omitted uses as many digits as the value needs.\n */",
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
                                "/** Returns the JSON representation of this number. NaN and ±Infinity become `\"null\"` (JSON has no literal for either). */",
                            ),
                        },
                    ),
                ]),
                properties: BTreeMap::new(),
                dispatch: Dispatch::Direct,
                doc: doc("/** The IEEE-754 double-precision number type. */"),
            },
        },
    );
    defs.types.insert(
        "NumberConstructor".to_string(),
        TypeSymbol {
            name: "NumberConstructor".to_string(),
            mangled_name: crate::mangle::prelude("NumberConstructor"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface {
                generics: Vec::new(),
                methods: BTreeMap::from([
                    (
                        "@call".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new(
                                "value",
                                Type::Union(vec![Type::String, Type::BigInt]),
                            )],
                            ret: Type::Number,
                            predicate: None,
                            doc: doc(
                                "/**\n * Convert a `string` or `bigint` to a `number`.\n * - String input is parsed (empty / whitespace → 0, malformed → NaN).\n * - BigInt input is converted lossily to the nearest representable f64.\n */",
                            ),
                        },
                    ),
                    (
                        "isNaN".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("value", Type::Number)],
                            ret: Type::Boolean,
                            predicate: None,
                            doc: doc(
                                "/** Returns `true` when `value` is `NaN`. Same as the global `isNaN` (parameters are already `number`-typed; there is no coercion to differ on). */",
                            ),
                        },
                    ),
                    (
                        "isFinite".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("value", Type::Number)],
                            ret: Type::Boolean,
                            predicate: None,
                            doc: doc(
                                "/** Returns `true` when `value` is neither `NaN` nor ±`Infinity`. */",
                            ),
                        },
                    ),
                    (
                        "isInteger".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("value", Type::Number)],
                            ret: Type::Boolean,
                            predicate: None,
                            doc: doc(
                                "/** Returns `true` when `value` is a finite whole number. */",
                            ),
                        },
                    ),
                    (
                        "isSafeInteger".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("value", Type::Number)],
                            ret: Type::Boolean,
                            predicate: None,
                            doc: doc(
                                "/** Returns `true` when `value` is an integer that f64 represents exactly (|value| ≤ 2^53 − 1). */",
                            ),
                        },
                    ),
                    (
                        "parseInt".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("s", Type::String),
                                Param::with_default(
                                    "radix",
                                    Type::Number,
                                    crate::DefaultValue::Number(10.0),
                                ),
                            ],
                            ret: Type::Number,
                            predicate: None,
                            doc: doc(
                                "/**\n * Same as the global `parseInt` — parses the longest leading integer, ignoring trailing junk. Returns `NaN` when no prefix parses.\n * @param s The string to parse.\n * @param radix Base 2–36; `0` auto-detects a `0x` hex prefix. Defaults to `10`.\n */",
                            ),
                        },
                    ),
                    (
                        "parseFloat".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("s", Type::String)],
                            ret: Type::Number,
                            predicate: None,
                            doc: doc(
                                "/**\n * Same as the global `parseFloat` — parses the longest leading decimal, ignoring trailing junk. Returns `NaN` when no prefix parses.\n * @param s The string to parse.\n */",
                            ),
                        },
                    ),
                ]),
                properties: BTreeMap::from([
                    (
                        "EPSILON".to_string(),
                        PropertySig {
                            ty: Type::Number,
                            readonly: true,
                            intrinsic: false,
                            optional: false,
                            doc: doc(
                                "/** The gap between 1 and the next representable double (2^-52). */",
                            ),
                        },
                    ),
                    (
                        "MAX_VALUE".to_string(),
                        PropertySig {
                            ty: Type::Number,
                            readonly: true,
                            intrinsic: false,
                            optional: false,
                            doc: doc("/** The largest finite double (≈1.7976931348623157e308). */"),
                        },
                    ),
                    (
                        "MIN_VALUE".to_string(),
                        PropertySig {
                            ty: Type::Number,
                            readonly: true,
                            intrinsic: false,
                            optional: false,
                            doc: doc("/** The smallest positive double (5e-324, subnormal). */"),
                        },
                    ),
                    (
                        "MAX_SAFE_INTEGER".to_string(),
                        PropertySig {
                            ty: Type::Number,
                            readonly: true,
                            intrinsic: false,
                            optional: false,
                            doc: doc("/** 2^53 − 1 — the largest exactly-representable integer. */"),
                        },
                    ),
                    (
                        "MIN_SAFE_INTEGER".to_string(),
                        PropertySig {
                            ty: Type::Number,
                            readonly: true,
                            intrinsic: false,
                            optional: false,
                            doc: doc("/** −(2^53 − 1). */"),
                        },
                    ),
                    (
                        "POSITIVE_INFINITY".to_string(),
                        PropertySig {
                            ty: Type::Number,
                            readonly: true,
                            intrinsic: false,
                            optional: false,
                            doc: doc("/** Same value as the global `Infinity`. */"),
                        },
                    ),
                    (
                        "NEGATIVE_INFINITY".to_string(),
                        PropertySig {
                            ty: Type::Number,
                            readonly: true,
                            intrinsic: false,
                            optional: false,
                            doc: doc("/** Same value as `-Infinity`. */"),
                        },
                    ),
                    (
                        "NaN".to_string(),
                        PropertySig {
                            ty: Type::Number,
                            readonly: true,
                            intrinsic: false,
                            optional: false,
                            doc: doc("/** Same value as the global `NaN`. */"),
                        },
                    ),
                ]),
                dispatch: Dispatch::Static,
                doc: doc(
                    "/** Constructor object for `number`. Accessed via the global `Number` binding. */",
                ),
            },
        },
    );
    defs.values.insert(
        "Number".to_string(),
        ValueSymbol {
            name: "Number".to_string(),
            mangled_name: crate::mangle::prelude("Number"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: ValueKind::Const {
                ty: Type::prelude_interface("NumberConstructor".to_string(), Vec::new()),
                doc: doc(
                    "/** The `Number` constructor — call `Number(x)` to convert a `string` or `bigint` to a number. */",
                ),
            },
        },
    );
    defs.values.insert(
        "NaN".to_string(),
        ValueSymbol {
            name: "NaN".to_string(),
            mangled_name: crate::mangle::prelude("NaN"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: ValueKind::Const {
                ty: Type::Number,
                doc: doc(
                    "/** Not-a-Number. Compares unequal to everything including itself — test with `isNaN(x)`. */",
                ),
            },
        },
    );
    defs.values.insert(
        "Infinity".to_string(),
        ValueSymbol {
            name: "Infinity".to_string(),
            mangled_name: crate::mangle::prelude("Infinity"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: ValueKind::Const {
                ty: Type::Number,
                doc: doc("/** Positive infinity. Negate (`-Infinity`) for negative infinity. */"),
            },
        },
    );
    defs.values.insert(
        "isNaN".to_string(),
        ValueSymbol {
            name: "isNaN".to_string(),
            mangled_name: crate::mangle::prelude("isNaN"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params: vec![Param::new("value", Type::Number)],
                ret: Type::Boolean,
                type_predicate: None,
                doc: doc("/** Returns `true` when `value` is `NaN`. */"),
            },
        },
    );
    defs.values.insert(
        "isFinite".to_string(),
        ValueSymbol {
            name: "isFinite".to_string(),
            mangled_name: crate::mangle::prelude("isFinite"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params: vec![Param::new("value", Type::Number)],
                ret: Type::Boolean,
                type_predicate: None,
                doc: doc(
                    "/** Returns `true` when `value` is an ordinary number — neither `NaN` nor ±`Infinity`. */",
                ),
            },
        },
    );
}
