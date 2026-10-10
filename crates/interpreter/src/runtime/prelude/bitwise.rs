//! Shared bitwise computations for direct and widened runtime operands.

use num_bigint::{BigInt, Sign};
use num_traits::{ToPrimitive, Zero};
use wasmtime::Caller;

use crate::runtime::host::{fatal_host_error, range_error, type_error};
use crate::runtime::number::{to_int32, to_uint32};
use crate::runtime::{StoreData, fuel};

pub(super) const OPERATIONS: [&str; 6] = ["bitand", "bitor", "bitxor", "shl", "shr", "ushr"];
// Matches the bigint exponentiation ceiling; check before allocating a result.
const MAX_RESULT_BITS: u64 = 512 * 1024 * 8;

pub(super) fn number(operation: &str, lhs: f64, rhs: f64) -> wasmtime::Result<f64> {
    let left = to_int32(lhs);
    let right = to_int32(rhs);
    let count = to_uint32(rhs) & 31;
    Ok(match operation {
        "bitand" => f64::from(left & right),
        "bitor" => f64::from(left | right),
        "bitxor" => f64::from(left ^ right),
        "shl" => f64::from(left.wrapping_shl(count)),
        "shr" => f64::from(left.wrapping_shr(count)),
        "ushr" => f64::from(to_uint32(lhs).wrapping_shr(count)),
        _ => return Err(fatal_host_error("unknown number bitwise operation")),
    })
}

pub(super) fn bigint(
    caller: &mut Caller<'_, StoreData>,
    operation: &str,
    lhs: &BigInt,
    rhs: &BigInt,
) -> wasmtime::Result<BigInt> {
    if operation == "ushr" {
        return Err(type_error(
            "BigInts have no unsigned right shift; use >> instead",
        ));
    }
    if matches!(operation, "shl" | "shr") {
        return shift(caller, operation, lhs, rhs);
    }
    charge(caller, lhs.bits().max(rhs.bits()).saturating_add(1))?;
    Ok(match operation {
        "bitand" => lhs & rhs,
        "bitor" => lhs | rhs,
        "bitxor" => lhs ^ rhs,
        _ => return Err(fatal_host_error("unknown bigint bitwise operation")),
    })
}

pub(super) fn complement(
    caller: &mut Caller<'_, StoreData>,
    value: &BigInt,
) -> wasmtime::Result<BigInt> {
    charge(caller, value.bits().saturating_add(1))?;
    Ok(!value)
}

fn shift(
    caller: &mut Caller<'_, StoreData>,
    operation: &str,
    value: &BigInt,
    count: &BigInt,
) -> wasmtime::Result<BigInt> {
    let expanding = (operation == "shl") != (count.sign() == Sign::Minus);
    let magnitude = count.magnitude().to_u64();
    fuel::charge_host_fuel(
        &mut *caller,
        fuel::ELEM.cost(count.bits().div_ceil(64).max(1)),
    )?;
    if value.is_zero() {
        return Ok(BigInt::zero());
    }
    if !expanding && magnitude.is_none_or(|count| count >= value.bits()) {
        charge(caller, 1)?;
        return Ok(BigInt::from(if value.sign() == Sign::Minus {
            -1
        } else {
            0
        }));
    }
    let count = magnitude.ok_or_else(|| range_error("bigint shift: result too large"))?;
    let result_bits = if expanding {
        value
            .bits()
            .checked_add(count)
            .ok_or_else(|| range_error("bigint shift: result too large"))?
    } else {
        value.bits()
    };
    charge(caller, result_bits)?;
    let count = usize::try_from(count).map_err(|_| range_error("bigint shift: count too large"))?;
    Ok(if expanding {
        value << count
    } else {
        value >> count
    })
}

fn charge(caller: &mut Caller<'_, StoreData>, bits: u64) -> wasmtime::Result<()> {
    if bits > MAX_RESULT_BITS {
        return Err(range_error("bigint bitwise operation: result too large"));
    }
    fuel::charge_host_fuel(caller, fuel::ELEM.cost(bits.div_ceil(64).max(1)))
}
