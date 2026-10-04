//! host-side BigInt runtime — `submilli:bigint.*` plus the
//! `submilli:number.fromBigInt` arm.

use crate::runtime::host::{abi_arg, abi_result};
use wasmtime::{
    ArrayRef, ArrayRefPre, ArrayType, AsContextMut, Caller, Engine, FieldType, FuncType, HeapType,
    Linker, Mutability, RefType, Rooted, StorageType, StructRef, StructRefPre, Val, ValType,
};

use crate::runtime::host::{
    range_error, read_string_arg, register_host_fn, type_error, write_submilli_string,
};
use crate::runtime::intrinsic_types::intrinsic_types;
use crate::runtime::{NUMBER_MODULE_NAME, StoreData, fuel};

pub const BIGINT_MODULE_NAME: &str = "submilli:bigint";

/// A finite integer double has at most 1024 magnitude bits. Convert its exact
/// represented value rather than saturating it through a fixed-width integer.
pub(super) fn integer_number(n: f64, name: &str) -> wasmtime::Result<num_bigint::BigInt> {
    if !n.is_finite() {
        return Err(range_error(format!(
            "{name}: cannot convert non-finite number to bigint ({n})"
        )));
    }
    if n.fract() != 0.0 {
        return Err(range_error(format!(
            "{name}: cannot convert non-integer number to bigint ({n})"
        )));
    }
    <num_bigint::BigInt as num_traits::FromPrimitive>::from_f64(n).ok_or_else(|| {
        crate::runtime::host::fatal_host_error("BigInt conversion refused a finite integer double")
    })
}

/// Standalone so cross-module canonicalization aligns with the consumer's `$rawBigInt`.
pub(crate) fn limbs_array_type(engine: &Engine) -> ArrayType {
    ArrayType::new(
        engine,
        FieldType::new(Mutability::Var, StorageType::ValType(ValType::I64)),
    )
}

pub(crate) fn install(
    linker: &mut Linker<StoreData>,
    string_type: &ArrayType,
) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let raw_string_param = ValType::Ref(RefType::new(false, HeapType::from(string_type.clone())));
    let raw_string_result = raw_string_param.clone();
    let limbs_array_ty = limbs_array_type(&engine);
    let raw_bigint_result =
        ValType::Ref(RefType::new(false, HeapType::from(limbs_array_ty.clone())));
    let raw_bigint_param = raw_bigint_result.clone();

    let from_string_ty = FuncType::new(
        &engine,
        [raw_string_param.clone()],
        [ValType::I32, raw_bigint_result.clone()],
    );
    register_host_fn(
        linker,
        BIGINT_MODULE_NAME,
        crate::mangle::host(BIGINT_MODULE_NAME, "fromString"),
        from_string_ty,
        /* deterministic = */ true,
        |caller, params, results| -> wasmtime::Result<()> {
            let s = read_string_arg(&mut *caller, abi_arg(params, 0)?, "bigint.fromString")?;
            let parsed = parse_decimal(caller, &s, "bigint.fromString")?;
            let (sign, magnitude) = parsed.into_parts();
            let limbs = magnitude.to_u64_digits();
            let arr = write_limbs(&mut *caller, &limbs)?;
            *abi_result(results, 0)? = Val::I32(sign_to_i32(sign));
            *abi_result(results, 1)? = Val::AnyRef(Some(arr.to_anyref()));
            Ok(())
        },
    )?;

    let from_number_ty = FuncType::new(
        &engine,
        [ValType::F64],
        [ValType::I32, raw_bigint_result.clone()],
    );
    register_host_fn(
        linker,
        BIGINT_MODULE_NAME,
        crate::mangle::host(BIGINT_MODULE_NAME, "fromNumber"),
        from_number_ty,
        /* deterministic = */ true,
        |caller, params, results| -> wasmtime::Result<()> {
            let n = match *abi_arg(params, 0)? {
                Val::F64(bits) => f64::from_bits(bits),
                ref other => {
                    return Err(type_error(format!(
                        "bigint.fromNumber expects f64, got {other:?}"
                    )));
                }
            };
            let parsed = integer_number(n, "bigint.fromNumber")?;
            let (sign, magnitude) = parsed.into_parts();
            let limbs = magnitude.to_u64_digits();
            let arr = write_limbs(&mut *caller, &limbs)?;
            *abi_result(results, 0)? = Val::I32(sign_to_i32(sign));
            *abi_result(results, 1)? = Val::AnyRef(Some(arr.to_anyref()));
            Ok(())
        },
    )?;

    let to_string_ty = FuncType::new(
        &engine,
        [ValType::I32, raw_bigint_param.clone()],
        [raw_string_result.clone()],
    );
    register_host_fn(
        linker,
        BIGINT_MODULE_NAME,
        crate::mangle::host(BIGINT_MODULE_NAME, "toString"),
        to_string_ty,
        /* deterministic = */ true,
        |caller, params, results| -> wasmtime::Result<()> {
            let value = read_bigint_arg(
                &mut *caller,
                abi_arg(params, 0)?,
                abi_arg(params, 1)?,
                "bigint.toString",
            )?;
            let formatted = format_bigint(caller, &value, 10)?;
            let arr = write_submilli_string(&mut *caller, &formatted)?;
            *abi_result(results, 0)? = Val::AnyRef(Some(arr.to_anyref()));
            Ok(())
        },
    )?;

    let to_string_radix_ty = FuncType::new(
        &engine,
        [ValType::I32, raw_bigint_param.clone(), ValType::F64],
        [raw_string_result.clone()],
    );
    register_host_fn(
        linker,
        BIGINT_MODULE_NAME,
        crate::mangle::host(BIGINT_MODULE_NAME, "toStringRadix"),
        to_string_radix_ty,
        /* deterministic = */ true,
        |caller, params, results| -> wasmtime::Result<()> {
            let value = read_bigint_arg(
                &mut *caller,
                abi_arg(params, 0)?,
                abi_arg(params, 1)?,
                "bigint.toStringRadix",
            )?;
            let radix = match *abi_arg(params, 2)? {
                Val::F64(bits) => f64::from_bits(bits),
                ref other => {
                    return Err(type_error(format!(
                        "bigint.toStringRadix expects f64 radix, got {other:?}"
                    )));
                }
            };
            let truncated = radix.trunc();
            if radix.is_nan() || !(2.0..=36.0).contains(&truncated) {
                return Err(range_error("toString radix must be between 2 and 36"));
            }
            let formatted = format_bigint(caller, &value, truncated as u32)?;
            let arr = write_submilli_string(&mut *caller, &formatted)?;
            *abi_result(results, 0)? = Val::AnyRef(Some(arr.to_anyref()));
            Ok(())
        },
    )?;

    let binop_ty = FuncType::new(
        &engine,
        [
            ValType::I32,
            raw_bigint_param.clone(),
            ValType::I32,
            raw_bigint_param.clone(),
        ],
        [ValType::I32, raw_bigint_result.clone()],
    );
    for (name, op, cost) in [
        ("add", BinOp::Add, Cost::Linear),
        ("sub", BinOp::Sub, Cost::Linear),
        ("mul", BinOp::Mul, Cost::Product),
    ] {
        register_host_fn(
            linker,
            BIGINT_MODULE_NAME,
            crate::mangle::host(BIGINT_MODULE_NAME, name),
            binop_ty.clone(),
            /* deterministic = */ true,
            move |caller, params, results| -> wasmtime::Result<()> {
                run_binop(&mut *caller, params, results, name, cost, |a, b| {
                    Ok(match op {
                        BinOp::Add => a + b,
                        BinOp::Sub => a - b,
                        BinOp::Mul => a * b,
                    })
                })
            },
        )?;
    }
    for (name, kind) in [("div", DivKind::Div), ("mod", DivKind::Rem)] {
        register_host_fn(
            linker,
            BIGINT_MODULE_NAME,
            crate::mangle::host(BIGINT_MODULE_NAME, name),
            binop_ty.clone(),
            /* deterministic = */ true,
            move |caller, params, results| -> wasmtime::Result<()> {
                use num_traits::Zero;
                let divisor = read_bigint_arg(
                    caller,
                    abi_arg(params, 2)?,
                    abi_arg(params, 3)?,
                    &format!("{name} rhs"),
                )?;
                if divisor.is_zero() {
                    return Err(range_error("Division by zero"));
                }
                let dividend =
                    read_bigint_arg(caller, &params[0], &params[1], &format!("{name} lhs"))?;
                fuel::charge_host_fuel(&mut *caller, Cost::Product.of(&dividend, &divisor)?)?;
                let result = match kind {
                    DivKind::Div => dividend / divisor,
                    DivKind::Rem => dividend % divisor,
                };
                write_binop_result(caller, results, result)
            },
        )?;
    }

    // num_bigint::pow takes u32, so that's the practical ceiling on the exponent.
    register_host_fn(
        linker,
        BIGINT_MODULE_NAME,
        crate::mangle::host(BIGINT_MODULE_NAME, "pow"),
        binop_ty.clone(),
        /* deterministic = */ true,
        |caller, params, results| -> wasmtime::Result<()> {
            run_binop(
                &mut *caller,
                params,
                results,
                "pow",
                Cost::Pow,
                |base, exp| {
                    use num_traits::{Signed, ToPrimitive};
                    if exp.is_negative() {
                        return Err(range_error("bigint.pow: exponent must be non-negative"));
                    }
                    let exp_u32 = exp.to_u32().ok_or_else(|| {
                        range_error("bigint.pow: exponent too large to fit in u32")
                    })?;
                    Ok(base.pow(exp_u32))
                },
            )
        },
    )?;

    let cmp_ty = FuncType::new(
        &engine,
        [
            ValType::I32,
            raw_bigint_param.clone(),
            ValType::I32,
            raw_bigint_param.clone(),
        ],
        [ValType::I32],
    );
    register_host_fn(
        linker,
        BIGINT_MODULE_NAME,
        crate::mangle::host(BIGINT_MODULE_NAME, "cmp"),
        cmp_ty,
        /* deterministic = */ true,
        |caller, params, results| -> wasmtime::Result<()> {
            let a = read_bigint_arg(
                &mut *caller,
                abi_arg(params, 0)?,
                abi_arg(params, 1)?,
                "bigint.cmp lhs",
            )?;
            let b = read_bigint_arg(
                &mut *caller,
                abi_arg(params, 2)?,
                abi_arg(params, 3)?,
                "bigint.cmp rhs",
            )?;
            *abi_result(results, 0)? = Val::I32(match a.cmp(&b) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            });
            Ok(())
        },
    )?;

    let neg_ty = FuncType::new(
        &engine,
        [ValType::I32, raw_bigint_param.clone()],
        [ValType::I32, raw_bigint_result.clone()],
    );
    register_host_fn(
        linker,
        BIGINT_MODULE_NAME,
        crate::mangle::host(BIGINT_MODULE_NAME, "neg"),
        neg_ty,
        /* deterministic = */ true,
        |caller, params, results| -> wasmtime::Result<()> {
            let sign = read_sign(abi_arg(params, 0)?)?;
            let limbs = read_limbs_arg(&mut *caller, abi_arg(params, 1)?, "bigint.neg")?;
            let arr = write_limbs(&mut *caller, &limbs)?;
            *abi_result(results, 0)? = Val::I32(-sign);
            *abi_result(results, 1)? = Val::AnyRef(Some(arr.to_anyref()));
            Ok(())
        },
    )?;

    let from_bigint_ty = FuncType::new(&engine, [ValType::I32, raw_bigint_param], [ValType::F64]);
    register_host_fn(
        linker,
        NUMBER_MODULE_NAME,
        crate::mangle::host(NUMBER_MODULE_NAME, "fromBigInt"),
        from_bigint_ty,
        /* deterministic = */ true,
        |caller, params, results| -> wasmtime::Result<()> {
            use num_traits::ToPrimitive;
            let value = read_bigint_arg(
                &mut *caller,
                abi_arg(params, 0)?,
                abi_arg(params, 1)?,
                "number.fromBigInt",
            )?;
            let n = value.to_f64().unwrap_or(f64::INFINITY);
            *abi_result(results, 0)? = Val::F64(n.to_bits());
            Ok(())
        },
    )?;

    Ok(())
}

#[derive(Copy, Clone)]
enum BinOp {
    Add,
    Sub,
    Mul,
}

#[derive(Copy, Clone)]
enum DivKind {
    Div,
    Rem,
}

/// Bounds conversion work independently of the program's total fuel budget.
pub(crate) const MAX_DECIMAL_INPUT_BYTES: usize = 65_536;
const MAX_FORMAT_LIMBS: u64 = 4_096;

pub(crate) fn parse_decimal(
    caller: &mut Caller<'_, StoreData>,
    text: &str,
    operation: &str,
) -> wasmtime::Result<num_bigint::BigInt> {
    if text.len() > MAX_DECIMAL_INPUT_BYTES {
        return Err(range_error(format!(
            "{operation}: decimal input exceeds {MAX_DECIMAL_INPUT_BYTES} bytes; use a smaller integer"
        )));
    }
    let trimmed = text.trim();
    let digits = trimmed.len() as u64;
    // Decimal conversion uses repeated multiplication, unlike the faster
    // multiplication algorithms priced by bigint_product_cost.
    fuel::charge(
        &mut *caller,
        fuel::ELEM,
        digits.saturating_mul(digits.div_ceil(19)),
    )?;
    trimmed.parse().map_err(|_| {
        crate::runtime::host::syntax_error(format!(
            "{operation}: invalid bigint literal: {trimmed:?}"
        ))
    })
}

/// Power-of-two radices extract bits linearly; other radices repeatedly divide
/// the remaining magnitude and need a quadratic work bound.
pub(crate) fn format_bigint(
    caller: &mut Caller<'_, StoreData>,
    value: &num_bigint::BigInt,
    radix: u32,
) -> wasmtime::Result<String> {
    if !(2..=36).contains(&radix) {
        return Err(range_error("toString radix must be between 2 and 36"));
    }
    let limbs = value.bits().div_ceil(64);
    if limbs > MAX_FORMAT_LIMBS {
        return Err(range_error(format!(
            "BigInt.toString: exceeds {MAX_FORMAT_LIMBS} limbs; format a smaller integer"
        )));
    }
    let work = if radix.is_power_of_two() {
        limbs.max(1)
    } else {
        limbs.max(1).saturating_mul(limbs.max(1))
    };
    fuel::charge(&mut *caller, fuel::ELEM, work)?;
    Ok(value.to_str_radix(radix))
}

/// Limbs of the magnitude: the size variable of every BigInt cost.
fn limbs_of(value: &num_bigint::BigInt) -> u64 {
    value.magnitude().to_u64_digits().len() as u64
}

/// The largest BigInt `pow` may produce, in limbs: 512 KiB of magnitude. The
/// result size is known before the work, so an oversized one is refused
/// instead of built in host memory the store limit does not see.
const MAX_POW_LIMBS: u64 = 1 << 16;

/// How a binary operation's work scales with its operands.
#[derive(Copy, Clone)]
enum Cost {
    /// A pass over the longer operand: add, sub.
    Linear,
    /// A limb pair per step: mul, div, mod.
    Product,
    /// Repeated squaring up to the result size.
    Pow,
}

impl Cost {
    /// The fuel for `a op b`, sized before the work. `Pow` refuses a result
    /// over the cap here, before any of it is built.
    fn of(self, a: &num_bigint::BigInt, b: &num_bigint::BigInt) -> wasmtime::Result<u64> {
        let (la, lb) = (limbs_of(a), limbs_of(b));
        Ok(match self {
            Cost::Linear => fuel::ELEM.cost(la.max(lb)),
            Cost::Product => fuel::bigint_product_cost(la, lb),
            Cost::Pow => {
                use num_traits::ToPrimitive;
                // An exponent outside u32 is refused by the operation itself,
                // with its own message; it costs nothing here.
                match b.to_u32() {
                    Some(exp) => pow_cost(a, exp)?,
                    None => 0,
                }
            }
        })
    }
}

/// The work of `base ** exp`, dominated by the squarings near the result
/// size; the result has `bits(base) * exp / 64` limbs. A base of magnitude 0
/// or 1 has a one-limb result whatever the exponent.
fn pow_cost(base: &num_bigint::BigInt, exp: u32) -> wasmtime::Result<u64> {
    let bits = base.bits();
    if bits <= 1 {
        return Ok(fuel::ELEM.cost(1));
    }
    let result_limbs = bits.saturating_mul(u64::from(exp)).div_ceil(64);
    if result_limbs > MAX_POW_LIMBS {
        return Err(range_error("bigint.pow: result too large"));
    }
    Ok(fuel::bigint_product_cost(result_limbs, result_limbs))
}

fn run_binop(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
    results: &mut [Val],
    op_name: &str,
    cost: Cost,
    op: impl FnOnce(num_bigint::BigInt, num_bigint::BigInt) -> wasmtime::Result<num_bigint::BigInt>,
) -> wasmtime::Result<()> {
    let a = read_bigint_arg(
        caller,
        abi_arg(params, 0)?,
        abi_arg(params, 1)?,
        &format!("{op_name} lhs"),
    )?;
    let b = read_bigint_arg(
        caller,
        abi_arg(params, 2)?,
        abi_arg(params, 3)?,
        &format!("{op_name} rhs"),
    )?;
    fuel::charge_host_fuel(&mut *caller, cost.of(&a, &b)?)?;
    let r = op(a, b)?;
    write_binop_result(caller, results, r)
}

fn write_binop_result(
    caller: &mut Caller<'_, StoreData>,
    results: &mut [Val],
    value: num_bigint::BigInt,
) -> wasmtime::Result<()> {
    let (sign, magnitude) = value.into_parts();
    let limbs = magnitude.to_u64_digits();
    let arr = write_limbs(caller, &limbs)?;
    *abi_result(results, 0)? = Val::I32(sign_to_i32(sign));
    *abi_result(results, 1)? = Val::AnyRef(Some(arr.to_anyref()));
    Ok(())
}

pub(crate) fn read_bigint_arg(
    caller: &mut Caller<'_, StoreData>,
    sign_val: &Val,
    limbs_val: &Val,
    name: &str,
) -> wasmtime::Result<num_bigint::BigInt> {
    let sign = read_sign(sign_val)?;
    let limbs = read_limbs_arg(caller, limbs_val, name)?;
    limbs_to_bigint(sign, &limbs)
}

/// Reconstruct a `num_bigint::BigInt` from the canonical `$bigint` payload —
/// `sign` ∈ {−1, 0, 1} and little-endian u64 `limbs`. Shared by the host
/// arithmetic ABI and the host-owned `$bigint` vtable.
pub(crate) fn limbs_to_bigint(sign: i32, limbs: &[u64]) -> wasmtime::Result<num_bigint::BigInt> {
    let count = limbs
        .len()
        .checked_mul(2)
        .ok_or_else(|| crate::runtime::host::fatal_host_error("BigInt limb count overflow"))?;
    let mut u32s = Vec::new();
    u32s.try_reserve_exact(count)
        .map_err(crate::runtime::host::fatal_host_error)?;
    for w in limbs {
        u32s.push((*w & 0xFFFF_FFFF) as u32);
        u32s.push((*w >> 32) as u32);
    }
    let magnitude = num_bigint::BigUint::from_slice(&u32s);
    let signum = match sign {
        v if v > 0 => num_bigint::Sign::Plus,
        v if v < 0 => num_bigint::Sign::Minus,
        _ => num_bigint::Sign::NoSign,
    };
    Ok(num_bigint::BigInt::from_biguint(signum, magnitude))
}

/// Read a `$bigint` struct ref's `sign` (field 1) and little-endian u64 `limbs`
/// (field 2). The host-owned `$bigint` vtable and the prelude-host method ABI
/// both take a typed receiver this way (vs. the arithmetic ABI, which receives
/// `sign` and `limbs` as separate stack values via [`read_bigint_arg`]).
pub(crate) fn read_bigint_struct(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    name: &str,
) -> wasmtime::Result<(i32, Vec<u64>)> {
    let st: Rooted<StructRef> = match val {
        Val::AnyRef(Some(any)) => any
            .as_struct(&mut *caller)?
            .ok_or_else(|| type_error(format!("{name}: receiver is not a struct")))?,
        other => {
            return Err(type_error(format!(
                "{name}: expected (ref $bigint), got {other:?}"
            )));
        }
    };
    let sign = read_sign(&st.field(&mut *caller, 1)?)?;
    let limbs_val = st.field(&mut *caller, 2)?;
    let limbs = read_limbs_arg(caller, &limbs_val, name)?;
    Ok((sign, limbs))
}

fn read_limbs_arg(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    name: &str,
) -> wasmtime::Result<Vec<u64>> {
    let arr = match val {
        Val::AnyRef(Some(any)) => any.unwrap_array(&mut *caller)?,
        Val::AnyRef(None) => {
            return Err(type_error(format!("{name} limbs arg is null")));
        }
        other => {
            return Err(type_error(format!(
                "{name} expects (ref $rawBigInt), got {other:?}",
            )));
        }
    };
    let len = arr.len(&mut *caller)?;
    fuel::charge(&mut *caller, fuel::COPY, u64::from(len).saturating_mul(8))?;
    let mut out = Vec::new();
    out.try_reserve_exact(len as usize)
        .map_err(crate::runtime::host::fatal_host_error)?;
    out.resize(len as usize, 0);
    arr.copy_to_i64_slice(&*caller, &mut out)?;
    Ok(out)
}

pub(crate) fn write_limbs(
    mut ctx: impl AsContextMut<Data = StoreData>,
    limbs: &[u64],
) -> wasmtime::Result<Rooted<ArrayRef>> {
    fuel::charge(&mut ctx, fuel::COPY, (limbs.len() as u64).saturating_mul(8))?;
    let array_ty = limbs_array_type(ctx.as_context().engine());
    let pre = ArrayRefPre::new(&mut ctx, array_ty);
    ArrayRef::new_from_i64_slice(&mut ctx, &pre, limbs)
}

pub(crate) fn make_bigint_struct(
    caller: &mut Caller<'_, StoreData>,
    value: num_bigint::BigInt,
) -> wasmtime::Result<Val> {
    let (sign, magnitude) = value.into_parts();
    let limbs = write_limbs(&mut *caller, &magnitude.to_u64_digits())?;
    let intr = intrinsic_types(&mut *caller)?;
    let vtable = crate::runtime::host::host_bigint_vtable(caller)?;
    let pre = StructRefPre::new(&mut *caller, intr.bigint.clone());
    let st = StructRef::new(
        &mut *caller,
        &pre,
        &[
            vtable,
            Val::I32(sign_to_i32(sign)),
            Val::AnyRef(Some(limbs.to_anyref())),
        ],
    )?;
    Ok(Val::AnyRef(Some(st.to_anyref())))
}

pub(crate) fn sign_to_i32(sign: num_bigint::Sign) -> i32 {
    match sign {
        num_bigint::Sign::Minus => -1,
        num_bigint::Sign::NoSign => 0,
        num_bigint::Sign::Plus => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `div` and `mod` must stay on the synchronous wrapper. Nothing else pins
    /// this: an async registration behaves identically end-to-end, so every
    /// fixture would still pass if these two moved back. Calling the import
    /// synchronously is the only way an embedder can observe its kind — an async
    /// one refuses the call before the body ever runs.
    #[test]
    fn division_and_modulo_are_registered_synchronously() {
        let cfg = crate::runtime::RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let data = StoreData::with_vfs(crate::runtime::Vfs::tempdir().expect("tempdir"));
        let mut store = cfg.store_async(&engine, data).expect("store");
        let mut linker = Linker::<StoreData>::new(&engine);
        crate::runtime::host::install_host_functions(&mut linker).expect("install");

        // A zero divisor short-circuits before any arithmetic, so an empty
        // magnitude on both operands is enough to reach the guard.
        let zero = write_limbs(&mut store, &[]).expect("limbs");
        let args = [
            Val::I32(0),
            Val::AnyRef(Some(zero.to_anyref())),
            Val::I32(0),
            Val::AnyRef(Some(zero.to_anyref())),
        ];

        for name in ["div", "mod"] {
            let mangled = crate::mangle::host(BIGINT_MODULE_NAME, name);
            let func = match linker
                .get(&mut store, BIGINT_MODULE_NAME, mangled.as_str())
                .unwrap_or_else(|e| panic!("{name} resolves: {e}"))
            {
                wasmtime::Extern::Func(func) => func,
                other => panic!("expected a func for {name}, got {other:?}"),
            };
            let mut results = [Val::I32(0), Val::AnyRef(None)];
            let err = func
                .call(&mut store, &args, &mut results)
                .expect_err("divide by zero");
            assert!(
                format!("{err}").contains("Division by zero"),
                "{name} must run synchronously; got: {err}"
            );
        }
    }
}

fn read_sign(value: &Val) -> wasmtime::Result<i32> {
    match value {
        Val::I32(sign @ -1..=1) => Ok(*sign),
        _ => Err(crate::runtime::host::invariant_trap(
            "bigint: invalid canonical sign",
        )),
    }
}

#[cfg(test)]
mod sign_tests {
    use super::*;
    #[test]
    fn malformed_signs_trap_before_negation() {
        for value in [
            Val::I32(i32::MIN),
            Val::I32(i32::MAX),
            Val::I32(-2),
            Val::I32(2),
            Val::F64(0),
        ] {
            assert!(read_sign(&value).unwrap_err().is::<wasmtime::Trap>());
        }
        for sign in [-1, 0, 1] {
            assert_eq!(-read_sign(&Val::I32(sign)).unwrap(), -sign);
        }
    }
}
