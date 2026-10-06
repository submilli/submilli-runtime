//! Compile-time pool for large bigint literals (those outside ±2^53 - 1).
//! Each distinct literal becomes a passive data segment of little-endian u64 limbs;
//! `BigIntPool::lookup` returns `None` for small literals that skip the pool.

use std::collections::BTreeMap;

use crate::codegen::{internal_failure, wasm_u32};
use crate::compiler_error::CompilerFailure;

#[derive(Clone, Debug)]
pub struct BigIntLiteral {
    /// Dedup key; no `n` suffix or leading sign.
    pub digits: String,
    /// Always `1`; negation is applied at runtime.
    pub sign: i8,
    pub limbs_le: Vec<u64>,
}

#[derive(Clone, Copy, Debug)]
pub struct BigIntEntry {
    /// Index passed to `array.new_data` for this literal.
    pub data_idx: u32,
    pub limb_count: u32,
    pub sign: i8,
}

#[derive(Default, Clone, Debug)]
pub struct BigIntPool {
    /// Distinct large literals in first-appearance order. Each
    /// becomes its own passive data segment.
    pub literals: Vec<BigIntLiteral>,
    text_to_idx: BTreeMap<String, usize>,
}

impl BigIntPool {
    pub(crate) fn intern_digits(&mut self, digits: &str) -> Result<Option<usize>, CompilerFailure> {
        const SAFE_INTEGER_MAX: u64 = (1u64 << 53) - 1;
        check_decimal_digits(digits)?;
        if digits
            .parse::<u64>()
            .is_ok_and(|value| value <= SAFE_INTEGER_MAX)
        {
            return Ok(None);
        }
        if let Some(&existing) = self.text_to_idx.get(digits) {
            return Ok(Some(existing));
        }
        let limbs_le = decimal_to_limbs(digits);
        let idx = self.literals.len();
        self.literals.push(BigIntLiteral {
            digits: digits.to_string(),
            sign: 1,
            limbs_le,
        });
        self.text_to_idx.insert(digits.to_string(), idx);
        Ok(Some(idx))
    }

    /// Returns `None` if the value is within the safe-integer range (wasn't pooled).
    pub fn lookup(&self, digits: &str) -> Result<Option<BigIntEntry>, CompilerFailure> {
        let Some(&idx) = self.text_to_idx.get(digits) else {
            return Ok(None);
        };
        let lit = self.literal(idx)?;
        Ok(Some(BigIntEntry {
            data_idx: wasm_u32(idx)?,
            limb_count: wasm_u32(lit.limbs_le.len())?,
            sign: lit.sign,
        }))
    }

    fn literal(&self, idx: usize) -> Result<&BigIntLiteral, CompilerFailure> {
        self.literals
            .get(idx)
            .ok_or_else(|| internal_failure("a bigint literal pool index is out of range"))
    }

    pub fn is_empty(&self) -> bool {
        self.literals.is_empty()
    }

    /// Little-endian byte payload for the literal at `idx` — body of its passive data segment.
    pub fn le_bytes(&self, idx: usize) -> Result<Vec<u8>, CompilerFailure> {
        Ok(self
            .literal(idx)?
            .limbs_le
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect())
    }
}

/// The lexer emits sign-free decimal digits; anything else is corrupt typed input.
fn check_decimal_digits(digits: &str) -> Result<(), CompilerFailure> {
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(internal_failure(
            "a bigint literal is not a sign-free decimal digit string",
        ));
    }
    Ok(())
}

// intern_digits validates a nonempty ASCII decimal string before calling; BigUint
// parsing has no digit/empty-input error for that language and no fixed-width limit.
fn decimal_to_limbs(digits: &str) -> Vec<u64> {
    use num_bigint::BigUint;
    use num_traits::Zero;
    let value: BigUint = digits
        .parse()
        .expect("validated decimal digits parse as BigUint");
    if value.is_zero() {
        return Vec::new();
    }
    value.to_u64_digits()
}

#[cfg(test)]
mod tests {
    use super::{BigIntPool, decimal_to_limbs};
    use crate::compiler_error::CompilerFailure;

    #[test]
    fn small_round_trip() {
        let limbs = decimal_to_limbs("18446744073709551616");
        assert_eq!(limbs, vec![0, 1]);
    }

    #[test]
    fn one_bit_under_two_limbs() {
        let limbs = decimal_to_limbs("18446744073709551615");
        assert_eq!(limbs, vec![u64::MAX]);
    }

    #[test]
    fn very_large() {
        let limbs = decimal_to_limbs("1267650600228229401496703205376");
        // 2^100 = 2^64 * 2^36 = limb[1] = 2^36, limb[0] = 0.
        assert_eq!(limbs, vec![0, 1u64 << 36]);
    }

    #[test]
    fn corrupt_literals_and_indices_are_internal_failures() {
        let mut pool = BigIntPool::default();
        // Signs are never part of the lexer's digits, including `i64::MIN`'s.
        for digits in ["-9223372036854775808", "", "12a", "+5"] {
            assert!(
                matches!(
                    pool.intern_digits(digits),
                    Err(CompilerFailure::Internal { .. })
                ),
                "{digits:?}"
            );
        }
        assert_eq!(pool.intern_digits("9007199254740991").unwrap(), None);
        assert_eq!(pool.intern_digits("9007199254740992").unwrap(), Some(0));
        assert!(pool.lookup("9007199254740992").unwrap().is_some());
        assert!(pool.lookup("1").unwrap().is_none());
        assert!(pool.le_bytes(1).is_err());
        pool.literals.clear();
        assert!(pool.lookup("9007199254740992").is_err());
    }
}
