//! host-side BigInt runtime — `submilli:bigint.*` plus the
//! `submilli:number.fromBigInt` arm.

use wasmtime::{
    ArrayRef, ArrayRefPre, ArrayType, AsContextMut, Caller, Engine, FieldType, FuncType, HeapType,
    Linker, Mutability, RefType, Rooted, StorageType, StructRef, StructRefPre, Val, ValType,
};

use crate::runtime::host::{
    range_error, read_string_arg, register_host_fn, type_error, write_submilli_string,
};
use crate::runtime::intrinsic_types::intrinsic_types;
use crate::runtime::{NUMBER_MODULE_NAME, StoreData};

pub const BIGINT_MODULE_NAME: &str = "submilli:bigint";

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
            let s = read_string_arg(&mut *caller, &params[0], "bigint.fromString")?;
            let trimmed = s.trim();
            let parsed: num_bigint::BigInt = trimmed.parse().map_err(|_| {
                crate::runtime::host::syntax_error(format!(
                    "bigint.fromString: invalid bigint literal: {trimmed:?}",
                ))
            })?;
            let (sign, magnitude) = parsed.into_parts();
            let limbs = magnitude.to_u64_digits();
            let arr = write_limbs(&mut *caller, &limbs)?;
            results[0] = Val::I32(sign_to_i32(sign));
            results[1] = Val::AnyRef(Some(arr.to_anyref()));
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
            let n = match params[0] {
                Val::F64(bits) => f64::from_bits(bits),
                ref other => {
                    return Err(type_error(format!(
                        "bigint.fromNumber expects f64, got {other:?}"
                    )));
                }
            };
            if !n.is_finite() {
                return Err(range_error(format!(
                    "bigint.fromNumber: cannot convert non-finite number to bigint ({n})",
                )));
            }
            if n.fract() != 0.0 {
                return Err(range_error(format!(
                    "bigint.fromNumber: cannot convert non-integer number to bigint ({n})",
                )));
            }
            // For integer doubles up to 2^53 the cast is exact; beyond,
            // the f64 itself already lost precision (matches JS
            // `BigInt(N)` for unsafe-range N).
            let parsed = num_bigint::BigInt::from(n as i128);
            let (sign, magnitude) = parsed.into_parts();
            let limbs = magnitude.to_u64_digits();
            let arr = write_limbs(&mut *caller, &limbs)?;
            results[0] = Val::I32(sign_to_i32(sign));
            results[1] = Val::AnyRef(Some(arr.to_anyref()));
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
            let value = read_bigint_arg(&mut *caller, &params[0], &params[1], "bigint.toString")?;
            let formatted = value.to_str_radix(10);
            let arr = write_submilli_string(&mut *caller, &formatted)?;
            results[0] = Val::AnyRef(Some(arr.to_anyref()));
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
            let value =
                read_bigint_arg(&mut *caller, &params[0], &params[1], "bigint.toStringRadix")?;
            let radix = match params[2] {
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
            let formatted = value.to_str_radix(truncated as u32);
            let arr = write_submilli_string(&mut *caller, &formatted)?;
            results[0] = Val::AnyRef(Some(arr.to_anyref()));
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
    for (name, op) in [
        ("add", BinOp::Add),
        ("sub", BinOp::Sub),
        ("mul", BinOp::Mul),
    ] {
        register_host_fn(
            linker,
            BIGINT_MODULE_NAME,
            crate::mangle::host(BIGINT_MODULE_NAME, name),
            binop_ty.clone(),
            /* deterministic = */ true,
            move |caller, params, results| -> wasmtime::Result<()> {
                run_binop(&mut *caller, params, results, name, |a, b| {
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
                let divisor =
                    read_bigint_arg(caller, &params[2], &params[3], &format!("{name} rhs"))?;
                if divisor.is_zero() {
                    return Err(range_error("Division by zero"));
                }
                run_binop(caller, params, results, name, |a, b| {
                    Ok(match kind {
                        DivKind::Div => a / b,
                        DivKind::Rem => a % b,
                    })
                })
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
            run_binop(&mut *caller, params, results, "pow", |base, exp| {
                use num_traits::{Signed, ToPrimitive};
                if exp.is_negative() {
                    return Err(range_error("bigint.pow: exponent must be non-negative"));
                }
                let exp_u32 = exp
                    .to_u32()
                    .ok_or_else(|| range_error("bigint.pow: exponent too large to fit in u32"))?;
                Ok(base.pow(exp_u32))
            })
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
            let a = read_bigint_arg(&mut *caller, &params[0], &params[1], "bigint.cmp lhs")?;
            let b = read_bigint_arg(&mut *caller, &params[2], &params[3], "bigint.cmp rhs")?;
            results[0] = Val::I32(match a.cmp(&b) {
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
            let sign = match params[0] {
                Val::I32(v) => v,
                ref other => {
                    return Err(type_error(format!(
                        "bigint.neg expects i32 sign, got {other:?}"
                    )));
                }
            };
            let limbs = read_limbs_arg(&mut *caller, &params[1], "bigint.neg")?;
            let arr = write_limbs(&mut *caller, &limbs)?;
            results[0] = Val::I32(-sign);
            results[1] = Val::AnyRef(Some(arr.to_anyref()));
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
            let value = read_bigint_arg(&mut *caller, &params[0], &params[1], "number.fromBigInt")?;
            let n = value.to_f64().unwrap_or(f64::INFINITY);
            results[0] = Val::F64(n.to_bits());
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

fn run_binop(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
    results: &mut [Val],
    op_name: &str,
    op: impl FnOnce(num_bigint::BigInt, num_bigint::BigInt) -> wasmtime::Result<num_bigint::BigInt>,
) -> wasmtime::Result<()> {
    let a = read_bigint_arg(caller, &params[0], &params[1], &format!("{op_name} lhs"))?;
    let b = read_bigint_arg(caller, &params[2], &params[3], &format!("{op_name} rhs"))?;
    let r = op(a, b)?;
    let (sign, magnitude) = r.into_parts();
    let limbs = magnitude.to_u64_digits();
    let arr = write_limbs(caller, &limbs)?;
    results[0] = Val::I32(sign_to_i32(sign));
    results[1] = Val::AnyRef(Some(arr.to_anyref()));
    Ok(())
}

pub(crate) fn read_bigint_arg(
    caller: &mut Caller<'_, StoreData>,
    sign_val: &Val,
    limbs_val: &Val,
    name: &str,
) -> wasmtime::Result<num_bigint::BigInt> {
    let sign = match sign_val {
        Val::I32(v) => *v,
        other => {
            return Err(type_error(format!(
                "{name}: expected i32 sign, got {other:?}"
            )));
        }
    };
    let limbs = read_limbs_arg(caller, limbs_val, name)?;
    Ok(limbs_to_bigint(sign, &limbs))
}

/// Reconstruct a `num_bigint::BigInt` from the canonical `$bigint` payload —
/// `sign` ∈ {−1, 0, 1} and little-endian u64 `limbs`. Shared by the host
/// arithmetic ABI and the host-owned `$bigint` vtable.
pub(crate) fn limbs_to_bigint(sign: i32, limbs: &[u64]) -> num_bigint::BigInt {
    let mut u32s = Vec::with_capacity(limbs.len() * 2);
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
    num_bigint::BigInt::from_biguint(signum, magnitude)
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
    let sign = match st.field(&mut *caller, 1)? {
        Val::I32(v) => v,
        other => wasmtime::bail!("{name}: sign field is {other:?}, not i32"),
    };
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
    let mut out = Vec::with_capacity(len as usize);
    for i in 0..len {
        let elem = arr.get(&mut *caller, i)?;
        let limb = match elem {
            Val::I64(v) => v as u64,
            other => {
                return Err(type_error(format!(
                    "{name} limb {i}: expected i64, got {other:?}"
                )));
            }
        };
        out.push(limb);
    }
    Ok(out)
}

pub(crate) fn write_limbs(
    mut ctx: impl AsContextMut,
    limbs: &[u64],
) -> wasmtime::Result<Rooted<ArrayRef>> {
    let array_ty = limbs_array_type(ctx.as_context().engine());
    let pre = ArrayRefPre::new(&mut ctx, array_ty);
    let units: Vec<Val> = limbs.iter().map(|w| Val::I64(*w as i64)).collect();
    ArrayRef::new_fixed(&mut ctx, &pre, &units)
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
