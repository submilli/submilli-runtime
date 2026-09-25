//! Primitive operators for live values whose runtime type can differ from a
//! retained TypeScript control-flow refinement.

use num_traits::{FromPrimitive, ToPrimitive, Zero};
use std::cmp::Ordering;
use wasmtime::{Caller, FuncType, HeapType, Linker, RefType, Val, ValType};

use crate::runtime::StoreData;
use crate::runtime::host::{
    range_error, register_host_fn_async, type_error, write_boxed_number_struct,
    write_submilli_string_struct_units,
};
use crate::runtime::intrinsic_types::build_intrinsic_types;
use crate::runtime::number::{format_number_js, string_to_number_js};
use crate::runtime::prelude::bigint::ops::{
    limbs_to_bigint, make_bigint_struct, read_bigint_struct,
};
use crate::runtime::prelude::{MODULE_NAME, declare_method};
use crate::{PackageDeclaration, Param, Type};

const ARITHMETIC: [&str; 6] = ["add", "sub", "mul", "div", "rem", "pow"];
const RELATIONAL: [&str; 4] = ["lt", "gt", "le", "ge"];

pub(super) fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let intr = build_intrinsic_types(&engine)?;
    let value = ValType::Ref(RefType::new(true, HeapType::ConcreteStruct(intr.object)));
    for operation in ARITHMETIC {
        register_host_fn_async(
            linker,
            MODULE_NAME,
            crate::mangle::prelude(&format!("__value_{operation}")),
            FuncType::new(&engine, [value.clone(), value.clone()], [value.clone()]),
            true,
            move |caller, params, results| {
                Box::pin(async move {
                    let lhs = primitive(caller, &params[0]).await?;
                    let rhs = primitive(caller, &params[1]).await?;
                    results[0] = arithmetic(caller, operation, lhs, rhs)?;
                    Ok(())
                })
            },
        )?;
    }
    for operation in RELATIONAL {
        register_host_fn_async(
            linker,
            MODULE_NAME,
            crate::mangle::prelude(&format!("__value_{operation}")),
            FuncType::new(&engine, [value.clone(), value.clone()], [ValType::I32]),
            true,
            move |caller, params, results| {
                Box::pin(async move {
                    let lhs = primitive(caller, &params[0]).await?;
                    let rhs = primitive(caller, &params[1]).await?;
                    let ordering = compare(&lhs, &rhs)?;
                    let answer = match operation {
                        "lt" => ordering == Some(Ordering::Less),
                        "gt" => ordering == Some(Ordering::Greater),
                        "le" => matches!(ordering, Some(Ordering::Less | Ordering::Equal)),
                        "ge" => matches!(ordering, Some(Ordering::Greater | Ordering::Equal)),
                        _ => unreachable!("registered comparison"),
                    };
                    results[0] = Val::I32(i32::from(answer));
                    Ok(())
                })
            },
        )?;
    }
    for operation in ["neg", "pos", "numeric", "inc", "dec"] {
        register_host_fn_async(
            linker,
            MODULE_NAME,
            crate::mangle::prelude(&format!("__value_{operation}")),
            FuncType::new(&engine, [value.clone()], [value.clone()]),
            true,
            move |caller, params, results| {
                Box::pin(async move {
                    let value = primitive(caller, &params[0]).await?;
                    results[0] = unary(caller, operation, value)?;
                    Ok(())
                })
            },
        )?;
    }
    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::prelude("__value_to_number"),
        FuncType::new(&engine, [value.clone()], [ValType::F64]),
        true,
        |caller, params, results| {
            Box::pin(async move {
                results[0] = Val::F64(number(primitive(caller, &params[0]).await?)?.to_bits());
                Ok(())
            })
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::prelude("__value_to_index"),
        FuncType::new(&engine, [value.clone()], [ValType::F64]),
        true,
        |caller, params, results| {
            Box::pin(async move {
                let key = primitive_with_hint(caller, &params[0], true).await?;
                let units = string(key);
                let index = number(Primitive::String(units.clone()))?;
                if format_number_js(index).encode_utf16().collect::<Vec<_>>() != units {
                    return Err(range_error("Array index must be a canonical numeric key"));
                }
                results[0] = Val::F64(index.to_bits());
                Ok(())
            })
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::prelude("__value_to_string"),
        FuncType::new(
            &engine,
            [value],
            [ValType::Ref(RefType::new(
                false,
                HeapType::ConcreteStruct(intr.string),
            ))],
        ),
        true,
        |caller, params, results| {
            Box::pin(async move {
                let value = primitive_with_hint(caller, &params[0], true).await?;
                results[0] = Val::AnyRef(Some(
                    write_submilli_string_struct_units(caller, &string(value))?.to_anyref(),
                ));
                Ok(())
            })
        },
    )?;
    Ok(())
}

pub(super) fn declare(defs: &mut PackageDeclaration) {
    for operation in ARITHMETIC {
        let name = format!("__value_{operation}");
        declare_method(
            defs,
            &name,
            crate::mangle::prelude(&name),
            vec![
                Param::new("lhs", Type::Unknown),
                Param::new("rhs", Type::Unknown),
            ],
            Type::Unknown,
        );
    }
    for operation in RELATIONAL {
        let name = format!("__value_{operation}");
        declare_method(
            defs,
            &name,
            crate::mangle::prelude(&name),
            vec![
                Param::new("lhs", Type::Unknown),
                Param::new("rhs", Type::Unknown),
            ],
            Type::Boolean,
        );
    }
    for operation in ["neg", "pos", "numeric", "inc", "dec"] {
        let name = format!("__value_{operation}");
        declare_method(
            defs,
            &name,
            crate::mangle::prelude(&name),
            vec![Param::new("value", Type::Unknown)],
            Type::Unknown,
        );
    }
    declare_method(
        defs,
        "__value_to_number",
        crate::mangle::prelude("__value_to_number"),
        vec![Param::new("value", Type::Unknown)],
        Type::Number,
    );
    declare_method(
        defs,
        "__value_to_index",
        crate::mangle::prelude("__value_to_index"),
        vec![Param::new("value", Type::Unknown)],
        Type::Number,
    );
    declare_method(
        defs,
        "__value_to_string",
        crate::mangle::prelude("__value_to_string"),
        vec![Param::new("value", Type::Unknown)],
        Type::String,
    );
}

#[derive(Clone)]
pub(super) enum Primitive {
    Null,
    Number(f64),
    Boolean(bool),
    String(Vec<u16>),
    BigInt(num_bigint::BigInt),
}

fn compare(lhs: &Primitive, rhs: &Primitive) -> wasmtime::Result<Option<Ordering>> {
    match (lhs, rhs) {
        (Primitive::String(lhs), Primitive::String(rhs)) => Ok(Some(lhs.cmp(rhs))),
        (Primitive::BigInt(lhs), Primitive::BigInt(rhs)) => Ok(Some(lhs.cmp(rhs))),
        (Primitive::BigInt(lhs), Primitive::String(rhs)) => {
            Ok(parse_bigint(rhs).map(|rhs| lhs.cmp(&rhs)))
        }
        (Primitive::String(lhs), Primitive::BigInt(rhs)) => {
            Ok(parse_bigint(lhs).map(|lhs| lhs.cmp(rhs)))
        }
        (Primitive::BigInt(lhs), rhs) => Ok(compare_bigint_number(lhs, number(rhs.clone())?)),
        (lhs, Primitive::BigInt(rhs)) => {
            Ok(compare_bigint_number(rhs, number(lhs.clone())?).map(Ordering::reverse))
        }
        _ => Ok(number(lhs.clone())?.partial_cmp(&number(rhs.clone())?)),
    }
}

fn parse_bigint(units: &[u16]) -> Option<num_bigint::BigInt> {
    let text = String::from_utf16(units).ok()?;
    let text = text.trim_matches(crate::runtime::number::is_js_whitespace);
    if text.is_empty() {
        return Some(num_bigint::BigInt::zero());
    }
    for (prefix, radix) in [
        ("0x", 16),
        ("0X", 16),
        ("0o", 8),
        ("0O", 8),
        ("0b", 2),
        ("0B", 2),
    ] {
        if let Some(digits) = text.strip_prefix(prefix) {
            return parse_bigint_digits(digits, radix);
        }
    }
    let (negative, digits) = if let Some(digits) = text.strip_prefix('-') {
        (true, digits)
    } else {
        (false, text.strip_prefix('+').unwrap_or(text))
    };
    parse_bigint_digits(digits, 10).map(|value| if negative { -value } else { value })
}

fn parse_bigint_digits(digits: &str, radix: u32) -> Option<num_bigint::BigInt> {
    if digits.is_empty() || !digits.chars().all(|digit| digit.is_digit(radix)) {
        return None;
    }
    num_bigint::BigInt::parse_bytes(digits.as_bytes(), radix)
}

fn compare_bigint_number(lhs: &num_bigint::BigInt, rhs: f64) -> Option<Ordering> {
    if rhs.is_nan() {
        return None;
    }
    if rhs == f64::INFINITY {
        return Some(Ordering::Less);
    }
    if rhs == f64::NEG_INFINITY {
        return Some(Ordering::Greater);
    }
    let integer = num_bigint::BigInt::from_f64(rhs.trunc())?;
    Some(match lhs.cmp(&integer) {
        Ordering::Equal if rhs.fract() > 0.0 => Ordering::Less,
        Ordering::Equal if rhs.fract() < 0.0 => Ordering::Greater,
        ordering => ordering,
    })
}

async fn primitive(caller: &mut Caller<'_, StoreData>, value: &Val) -> wasmtime::Result<Primitive> {
    primitive_with_hint(caller, value, false).await
}

pub(super) async fn primitive_with_hint(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
    prefers_string: bool,
) -> wasmtime::Result<Primitive> {
    if let Some(primitive) = read_primitive(caller, value)? {
        return Ok(primitive);
    }
    let methods = if prefers_string {
        ["toString", "valueOf"]
    } else {
        ["valueOf", "toString"]
    };
    for name in methods {
        let method = conversion_method(caller, value, name).await?;
        let converted = if let Some(method) = method {
            if !is_callable(caller, &method)? {
                continue;
            }
            super::closure::read(caller, &method, name)?
                .call_with_receiver(caller, *value, &[])
                .await?
        } else if name == "toString" {
            super::vtable::dispatch_vtable_slot(caller, value, 0, &[]).await?
        } else {
            // The inherited Object.valueOf returns the object itself.
            continue;
        };
        if let Some(primitive) = read_primitive(caller, &converted)? {
            return Ok(primitive);
        }
    }
    Err(type_error("Cannot convert object to primitive value"))
}

pub(super) async fn conversion_method(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
    name: &str,
) -> wasmtime::Result<Option<Val>> {
    if let Some(method) = super::collection::object_field(caller, value, name)? {
        return Ok(Some(method));
    }
    let Val::AnyRef(Some(reference)) = value else {
        return Ok(None);
    };
    let Some(object) = reference.as_struct(&mut *caller)? else {
        return Ok(None);
    };
    let Val::AnyRef(Some(vtable)) = object.field(&mut *caller, 0)? else {
        return Ok(None);
    };
    let Some(vtable) = vtable.as_struct(&mut *caller)? else {
        return Ok(None);
    };
    let intr = build_intrinsic_types(caller.engine())?;
    if !vtable.matches_ty(&*caller, &intr.class_vtable)? {
        return Ok(None);
    }
    let getter_name = format!("get {name}");
    let Some(getter) = super::collection::object_accessor(caller, value, &getter_name)? else {
        return Ok(None);
    };
    if !is_callable(caller, &getter)? {
        return Ok(None);
    }
    let method = super::closure::read(caller, &getter, &getter_name)?
        .call_with_receiver(caller, *value, &[])
        .await?;
    Ok(Some(method))
}

pub(super) fn is_callable(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
) -> wasmtime::Result<bool> {
    let Val::AnyRef(Some(reference)) = value else {
        return Ok(false);
    };
    let Some(object) = reference.as_struct(&mut *caller)? else {
        return Ok(false);
    };
    let closure = build_intrinsic_types(caller.engine())?.closure;
    object.matches_ty(&*caller, &closure)
}

pub(super) async fn to_number(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
) -> wasmtime::Result<f64> {
    number(primitive(caller, value).await?)
}

pub(super) fn truthy(caller: &mut Caller<'_, StoreData>, value: &Val) -> wasmtime::Result<bool> {
    Ok(match read_primitive(caller, value)? {
        Some(Primitive::Null) => false,
        Some(Primitive::Boolean(value)) => value,
        Some(Primitive::Number(value)) => value != 0.0 && !value.is_nan(),
        Some(Primitive::String(value)) => !value.is_empty(),
        Some(Primitive::BigInt(value)) => !value.is_zero(),
        None => true,
    })
}

fn read_primitive(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
) -> wasmtime::Result<Option<Primitive>> {
    let Val::AnyRef(Some(reference)) = value else {
        return Ok(matches!(value, Val::AnyRef(None)).then_some(Primitive::Null));
    };
    let Some(object) = reference.as_struct(&mut *caller)? else {
        return Ok(None);
    };
    let intr = build_intrinsic_types(caller.engine())?;
    if object.matches_ty(&*caller, &intr.boxed_number)? {
        let Val::F64(bits) = object.field(&mut *caller, 1)? else {
            unreachable!("boxed number payload")
        };
        return Ok(Some(Primitive::Number(f64::from_bits(bits))));
    }
    if object.matches_ty(&*caller, &intr.boxed_boolean)? {
        let Val::I32(value) = object.field(&mut *caller, 1)? else {
            unreachable!("boxed boolean payload")
        };
        return Ok(Some(Primitive::Boolean(value != 0)));
    }
    if object.matches_ty(&*caller, &intr.string)? {
        return Ok(Some(Primitive::String(super::array::read_string_units(
            caller, value,
        )?)));
    }
    if object.matches_ty(&*caller, &intr.bigint)? {
        let (sign, limbs) = read_bigint_struct(caller, value, "arithmetic")?;
        return Ok(Some(Primitive::BigInt(limbs_to_bigint(sign, &limbs))));
    }
    Ok(None)
}

fn arithmetic(
    caller: &mut Caller<'_, StoreData>,
    operation: &str,
    lhs: Primitive,
    rhs: Primitive,
) -> wasmtime::Result<Val> {
    if operation == "add"
        && (matches!(lhs, Primitive::String(_)) || matches!(rhs, Primitive::String(_)))
    {
        let mut text = string(lhs);
        text.extend(string(rhs));
        let value = write_submilli_string_struct_units(caller, &text)?;
        return Ok(Val::AnyRef(Some(value.to_anyref())));
    }
    if let (Primitive::BigInt(lhs), Primitive::BigInt(rhs)) = (&lhs, &rhs) {
        return bigint_arithmetic(caller, operation, lhs, rhs);
    }
    let lhs = number(lhs)?;
    let rhs = number(rhs)?;
    let result = match operation {
        "add" => lhs + rhs,
        "sub" => lhs - rhs,
        "mul" => lhs * rhs,
        "div" => lhs / rhs,
        "rem" => lhs % rhs,
        "pow" => crate::runtime::number::pow_js(lhs, rhs),
        _ => unreachable!("registered arithmetic operation"),
    };
    Ok(Val::AnyRef(Some(
        write_boxed_number_struct(caller, result)?.to_anyref(),
    )))
}

fn number(value: Primitive) -> wasmtime::Result<f64> {
    Ok(match value {
        Primitive::Null => 0.0,
        Primitive::Number(value) => value,
        Primitive::Boolean(value) => {
            if value {
                1.0
            } else {
                0.0
            }
        }
        // A lone surrogate cannot form a numeric literal. Reject it as NaN
        // without changing any string value or its UTF-16 representation.
        Primitive::String(units) => {
            String::from_utf16(&units).map_or(f64::NAN, |text| string_to_number_js(&text))
        }
        Primitive::BigInt(_) => return Err(type_error("Cannot mix BigInt and other types")),
    })
}

pub(super) fn string(value: Primitive) -> Vec<u16> {
    let text = match value {
        Primitive::String(units) => return units,
        Primitive::Null => "null".to_owned(),
        Primitive::Number(value) => format_number_js(value),
        Primitive::Boolean(value) => value.to_string(),
        Primitive::BigInt(value) => value.to_string(),
    };
    text.encode_utf16().collect()
}

fn bigint_arithmetic(
    caller: &mut Caller<'_, StoreData>,
    operation: &str,
    lhs: &num_bigint::BigInt,
    rhs: &num_bigint::BigInt,
) -> wasmtime::Result<Val> {
    if matches!(operation, "div" | "rem") && rhs.is_zero() {
        return Err(range_error("Division by zero"));
    }
    let result = match operation {
        "add" => lhs + rhs,
        "sub" => lhs - rhs,
        "mul" => lhs * rhs,
        "div" => lhs / rhs,
        "rem" => lhs % rhs,
        "pow" => lhs.pow(
            rhs.to_u32()
                .ok_or_else(|| range_error("BigInt exponent must fit a non-negative u32"))?,
        ),
        _ => unreachable!("registered arithmetic operation"),
    };
    make_bigint_struct(caller, result)
}

fn unary(
    caller: &mut Caller<'_, StoreData>,
    operation: &str,
    value: Primitive,
) -> wasmtime::Result<Val> {
    Ok(
        if operation != "pos"
            && let Primitive::BigInt(value) = &value
        {
            let value = match operation {
                "neg" => -value,
                "inc" => value + 1,
                "dec" => value - 1,
                _ => value.clone(),
            };
            make_bigint_struct(caller, value)?
        } else {
            let number = number(value)?;
            let number = match operation {
                "neg" => -number,
                "inc" => number + 1.0,
                "dec" => number - 1.0,
                _ => number,
            };
            Val::AnyRef(Some(write_boxed_number_struct(caller, number)?.to_anyref()))
        },
    )
}

/// Inspect a number without invoking JavaScript coercion hooks.
pub(super) fn number_value(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
) -> wasmtime::Result<Option<f64>> {
    Ok(match read_primitive(caller, value)? {
        Some(Primitive::Number(value)) => Some(value),
        _ => None,
    })
}

pub(super) async fn search_string(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
) -> wasmtime::Result<Vec<u16>> {
    let intr = build_intrinsic_types(caller.engine())?;
    if let Val::AnyRef(Some(reference)) = value
        && let Some(object) = reference.as_struct(&mut *caller)?
        && object.matches_ty(&*caller, &intr.regex)?
    {
        return Err(type_error("String search argument must not be a RegExp"));
    }
    Ok(string(primitive_with_hint(caller, value, true).await?))
}
