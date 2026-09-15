//! Compile-time pool for large bigint literals (those outside ±2^53 - 1).
//! Each distinct literal becomes a passive data segment of little-endian u64 limbs;
//! `BigIntPool::lookup` returns `None` for small literals that skip the pool.

use std::collections::BTreeMap;

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
    pub(crate) fn intern_digits(&mut self, digits: &str) -> Option<usize> {
        const SAFE_INTEGER_MAX: i64 = (1i64 << 53) - 1;
        if let Ok(v) = digits.parse::<i64>()
            && v.abs() <= SAFE_INTEGER_MAX
        {
            return None;
        }
        if let Some(&existing) = self.text_to_idx.get(digits) {
            return Some(existing);
        }
        let limbs_le = decimal_to_limbs(digits);
        let idx = self.literals.len();
        self.literals.push(BigIntLiteral {
            digits: digits.to_string(),
            sign: 1,
            limbs_le,
        });
        self.text_to_idx.insert(digits.to_string(), idx);
        Some(idx)
    }

    /// Returns `None` if the value is within the safe-integer range (wasn't pooled).
    pub fn lookup(&self, digits: &str) -> Option<BigIntEntry> {
        let idx = *self.text_to_idx.get(digits)?;
        let lit = &self.literals[idx];
        Some(BigIntEntry {
            data_idx: idx as u32,
            limb_count: lit.limbs_le.len() as u32,
            sign: lit.sign,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.literals.is_empty()
    }

    /// Little-endian byte payload for the literal at `idx` — body of its passive data segment.
    pub fn le_bytes(&self, idx: usize) -> Vec<u8> {
        self.literals[idx]
            .limbs_le
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect()
    }
}

fn decimal_to_limbs(digits: &str) -> Vec<u64> {
    use num_bigint::BigUint;
    use num_traits::Zero;
    let value: BigUint = digits.parse().expect("lexer guarantees a decimal string");
    if value.is_zero() {
        return Vec::new();
    }
    value.to_u64_digits()
}

#[cfg(test)]
mod tests {
    use super::decimal_to_limbs;

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
}
