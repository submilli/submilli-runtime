//! The Rust port of the prelude's built-in `Math` namespace.

use std::collections::BTreeMap;
use wasmtime::{Caller, FuncType, Global, GlobalType, Linker, Mutability, Store, Val, ValType};

use crate::runtime::StoreData;
use crate::runtime::host::register_host_fn;
use crate::runtime::prelude::MODULE_NAME;
use crate::{
    MangledName, NamespaceSymbol, PackageDeclaration, Param, Span, Type, ValueKind, ValueSymbol,
};

type UnaryFn = fn(f64) -> f64;
type BinaryFn = fn(f64, f64) -> f64;

const CONSTANTS: &[(&str, f64)] = &[
    ("PI", std::f64::consts::PI),
    ("E", std::f64::consts::E),
    ("LN2", std::f64::consts::LN_2),
    ("LN10", std::f64::consts::LN_10),
    ("LOG2E", std::f64::consts::LOG2_E),
    ("LOG10E", std::f64::consts::LOG10_E),
    ("SQRT2", std::f64::consts::SQRT_2),
    ("SQRT1_2", std::f64::consts::FRAC_1_SQRT_2),
];

const UNARY: &[(&str, UnaryFn)] = &[
    ("abs", f64::abs),
    ("ceil", f64::ceil),
    ("floor", f64::floor),
    ("trunc", f64::trunc),
    ("sqrt", f64::sqrt),
    ("round", math_round),
    ("sign", math_sign),
    ("fround", math_fround),
    ("clz32", math_clz32),
    ("exp", f64::exp),
    ("expm1", f64::exp_m1),
    ("log", f64::ln),
    ("log1p", f64::ln_1p),
    ("log2", f64::log2),
    ("log10", f64::log10),
    ("cbrt", f64::cbrt),
    ("sin", f64::sin),
    ("cos", f64::cos),
    ("tan", f64::tan),
    ("asin", f64::asin),
    ("acos", f64::acos),
    ("atan", f64::atan),
    ("sinh", f64::sinh),
    ("cosh", f64::cosh),
    ("tanh", f64::tanh),
    ("asinh", f64::asinh),
    ("acosh", f64::acosh),
    ("atanh", f64::atanh),
];

const BINARY: &[(&str, BinaryFn)] = &[
    ("imul", math_imul),
    ("pow", crate::runtime::number::pow_js),
    ("atan2", f64::atan2),
];

const VARIADIC: &[&str] = &["min", "max", "hypot"];

pub(crate) fn math_key(name: &str) -> MangledName {
    crate::mangle::extend(&crate::mangle::prelude("Math"), name)
}

pub(crate) fn namespace_symbol() -> NamespaceSymbol {
    let math_prefix = crate::mangle::prelude("Math");
    let mut math = NamespaceSymbol {
        name: "Math".to_string(),
        mangled_prefix: math_prefix.clone(),
        declaration_span: Span::at(crate::FileId::MATH),
        values: BTreeMap::new(),
        types: BTreeMap::new(),
        namespaces: BTreeMap::new(),
        doc: None,
    };

    for (name, _) in CONSTANTS {
        math.values.insert(
            (*name).to_string(),
            ValueSymbol {
                name: (*name).to_string(),
                mangled_name: math_key(name),
                declaration_span: Span::at(crate::FileId::MATH),
                kind: ValueKind::Const {
                    ty: Type::Number,
                    doc: None,
                },
            },
        );
    }
    for (name, _) in UNARY {
        math.values.insert(
            (*name).to_string(),
            ValueSymbol {
                name: (*name).to_string(),
                mangled_name: math_key(name),
                declaration_span: Span::at(crate::FileId::MATH),
                kind: ValueKind::Function {
                    generics: Vec::new(),
                    params: vec![Param::new("x", Type::Number)],
                    ret: Type::Number,
                    type_predicate: None,
                    doc: None,
                },
            },
        );
    }
    math.values.insert(
        "random".to_string(),
        ValueSymbol {
            name: "random".to_string(),
            mangled_name: math_key("random"),
            declaration_span: Span::at(crate::FileId::MATH),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params: Vec::new(),
                ret: Type::Number,
                type_predicate: None,
                doc: crate::doc(
                    crate::FileId::MATH,
                    "/** Pseudo-random number in `[0, 1)`. Not suitable for cryptographic use — reach for `submilli:crypto.randomBytes` instead. */",
                ),
            },
        },
    );
    for (name, _) in BINARY {
        let params = match *name {
            "pow" => vec![
                Param::new("base", Type::Number),
                Param::new("exponent", Type::Number),
            ],
            "atan2" => vec![Param::new("y", Type::Number), Param::new("x", Type::Number)],
            "imul" => vec![Param::new("a", Type::Number), Param::new("b", Type::Number)],
            _ => vec![Param::new("a", Type::Number), Param::new("b", Type::Number)],
        };
        math.values.insert(
            (*name).to_string(),
            ValueSymbol {
                name: (*name).to_string(),
                mangled_name: math_key(name),
                declaration_span: Span::at(crate::FileId::MATH),
                kind: ValueKind::Function {
                    generics: Vec::new(),
                    params,
                    ret: Type::Number,
                    type_predicate: None,
                    doc: None,
                },
            },
        );
    }
    for name in VARIADIC {
        math.values.insert(
            (*name).to_string(),
            ValueSymbol {
                name: (*name).to_string(),
                mangled_name: math_key(name),
                declaration_span: Span::at(crate::FileId::MATH),
                kind: ValueKind::Function {
                    generics: Vec::new(),
                    params: vec![Param::rest("values", Type::Array(Box::new(Type::Number)))],
                    ret: Type::Number,
                    type_predicate: None,
                    doc: None,
                },
            },
        );
    }

    math
}

pub fn declare(defs: &mut PackageDeclaration) {
    defs.namespaces
        .insert("Math".to_string(), namespace_symbol());
}

pub(crate) fn install_constants(
    linker: &mut Linker<StoreData>,
    store: &mut Store<StoreData>,
) -> wasmtime::Result<()> {
    for (name, value) in CONSTANTS {
        let gty = GlobalType::new(ValType::F64, Mutability::Var);
        let global = Global::new(&mut *store, gty, Val::F64(value.to_bits()))?;
        let mangled = math_key(name);
        linker.define(&mut *store, MODULE_NAME, mangled.as_str(), global)?;
    }
    Ok(())
}

pub fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let unary_ty = FuncType::new(&engine, [ValType::F64], [ValType::F64]);
    let binary_ty = FuncType::new(&engine, [ValType::F64, ValType::F64], [ValType::F64]);
    let array_ref = ValType::Ref(wasmtime::RefType::new(
        false,
        wasmtime::HeapType::ConcreteStruct(
            crate::runtime::intrinsic_types::build_intrinsic_types(&engine)?.array,
        ),
    ));
    let variadic_ty = FuncType::new(&engine, [array_ref], [ValType::F64]);

    for (name, op) in UNARY {
        let op = *op;
        register_host_fn(
            linker,
            MODULE_NAME,
            math_key(name),
            unary_ty.clone(),
            /* deterministic = */ true,
            move |_caller, params, results| -> wasmtime::Result<()> {
                let x = read_f64(&params[0], "Math unary")?;
                results[0] = Val::F64(op(x).to_bits());
                Ok(())
            },
        )?;
    }

    for (name, op) in BINARY {
        let op = *op;
        register_host_fn(
            linker,
            MODULE_NAME,
            math_key(name),
            binary_ty.clone(),
            /* deterministic = */ true,
            move |_caller, params, results| -> wasmtime::Result<()> {
                let a = read_f64(&params[0], "Math binary")?;
                let b = read_f64(&params[1], "Math binary")?;
                results[0] = Val::F64(op(a, b).to_bits());
                Ok(())
            },
        )?;
    }

    for name in VARIADIC {
        let op = *name;
        register_host_fn(
            linker,
            MODULE_NAME,
            math_key(name),
            variadic_ty.clone(),
            /* deterministic = */ true,
            move |caller, params, results| -> wasmtime::Result<()> {
                let values = read_variadic_numbers(caller, &params[0], op)?;
                let result = match op {
                    "min" => math_min(&values),
                    "max" => math_max(&values),
                    "hypot" => math_hypot(&values),
                    _ => unreachable!("registered Math variadic"),
                };
                results[0] = Val::F64(result.to_bits());
                Ok(())
            },
        )?;
    }

    register_host_fn(
        linker,
        MODULE_NAME,
        math_key("random"),
        FuncType::new(&engine, [], [ValType::F64]),
        /* deterministic = */ false,
        |_caller, _params, results| -> wasmtime::Result<()> {
            let mut bytes = [0u8; 8];
            getrandom::getrandom(&mut bytes)
                .map_err(|e| wasmtime::Error::msg(format!("Math.random getrandom failed: {e}")))?;
            let raw = u64::from_le_bytes(bytes);
            let mantissa = raw & ((1u64 << 52) - 1);
            let bits = (0x3FFu64 << 52) | mantissa;
            results[0] = Val::F64((f64::from_bits(bits) - 1.0).to_bits());
            Ok(())
        },
    )?;

    Ok(())
}

fn read_f64(val: &Val, name: &str) -> wasmtime::Result<f64> {
    match val {
        Val::F64(bits) => Ok(f64::from_bits(*bits)),
        other => Err(crate::runtime::host::type_error(format!(
            "{name} expects f64, got {other:?}"
        ))),
    }
}

fn read_variadic_numbers(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    name: &str,
) -> wasmtime::Result<Vec<f64>> {
    let elements = crate::runtime::prelude::collection::read_array_vals(caller, val)?;
    let boxed_number =
        crate::runtime::intrinsic_types::build_intrinsic_types(caller.engine())?.boxed_number;
    let mut out = Vec::with_capacity(elements.len());
    for element in elements {
        let Val::AnyRef(Some(any)) = element else {
            return Err(crate::runtime::host::type_error(format!(
                "Math.{name} expects boxed number arguments"
            )));
        };
        let st = any.as_struct(&mut *caller)?.ok_or_else(|| {
            crate::runtime::host::type_error(format!("Math.{name}: expected boxed number"))
        })?;
        if !st.matches_ty(&*caller, &boxed_number)? {
            return Err(crate::runtime::host::type_error(format!(
                "Math.{name}: expected boxed number"
            )));
        }
        out.push(read_f64(&st.field(&mut *caller, 1)?, name)?);
    }
    Ok(out)
}

fn math_round(x: f64) -> f64 {
    if x.is_nan() || x == 0.0 || x.is_infinite() {
        return x;
    }
    if (-0.5..0.0).contains(&x) || x == -0.5 {
        return -0.0;
    }
    (x + 0.5).floor()
}

fn math_sign(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        x
    }
}

fn math_fround(x: f64) -> f64 {
    (x as f32) as f64
}

fn math_clz32(x: f64) -> f64 {
    f64::from(to_uint32(x).leading_zeros())
}

fn math_imul(a: f64, b: f64) -> f64 {
    let a = to_int32(a);
    let b = to_int32(b);
    f64::from(a.wrapping_mul(b))
}

fn to_uint32(x: f64) -> u32 {
    if x == 0.0 || !x.is_finite() {
        return 0;
    }
    x.trunc().rem_euclid(4_294_967_296.0) as u32
}

fn to_int32(x: f64) -> i32 {
    let u = to_uint32(x);
    if u >= 0x8000_0000 {
        (i64::from(u) - 4_294_967_296) as i32
    } else {
        u as i32
    }
}

fn math_min(values: &[f64]) -> f64 {
    let mut out = f64::INFINITY;
    for &value in values {
        if value.is_nan() {
            return f64::NAN;
        }
        if value < out || (value == 0.0 && out == 0.0 && value.is_sign_negative()) {
            out = value;
        }
    }
    out
}

fn math_max(values: &[f64]) -> f64 {
    let mut out = f64::NEG_INFINITY;
    for &value in values {
        if value.is_nan() {
            return f64::NAN;
        }
        if value > out || (value == 0.0 && out == 0.0 && value.is_sign_positive()) {
            out = value;
        }
    }
    out
}

fn math_hypot(values: &[f64]) -> f64 {
    if values.iter().any(|v| v.is_infinite()) {
        return f64::INFINITY;
    }
    if values.iter().any(|v| v.is_nan()) {
        return f64::NAN;
    }
    values.iter().fold(0.0, |acc, value| acc.hypot(*value))
}
