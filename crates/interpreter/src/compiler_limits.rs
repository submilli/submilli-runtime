//! Representation limits shared by signature checking and code generation.

/// Number of declared argument slots in the closure ABI. Defaults keep their
/// slots and a packed rest array occupies one slot. The environment/receiver
/// and captured generic descriptors are carried separately.
pub const MAX_CLOSURE_ARITY: usize = u8::MAX as usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnsupportedClosureArity {
    pub actual: usize,
}

impl std::fmt::Display for UnsupportedClosureArity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "closure signature has {} parameter slots; the maximum is {MAX_CLOSURE_ARITY}",
            self.actual,
        )
    }
}

impl std::error::Error for UnsupportedClosureArity {}

/// Checks the encoded slot count, not the number of source call arguments.
/// Callers attach their compiler stage and source context to a failure.
pub fn checked_closure_arity(arity: usize) -> Result<u8, UnsupportedClosureArity> {
    u8::try_from(arity).map_err(|_| UnsupportedClosureArity { actual: arity })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closure_arity_preserves_the_supported_boundary() {
        assert_eq!(MAX_CLOSURE_ARITY, 255);
        for arity in [0, 1, 254, 255] {
            assert_eq!(usize::from(checked_closure_arity(arity).unwrap()), arity);
        }
    }

    #[test]
    fn closure_arity_rejects_excess_without_truncation() {
        for actual in [256, 257, usize::MAX] {
            let error = checked_closure_arity(actual).unwrap_err();
            assert_eq!(error.actual, actual);
            assert_eq!(
                error.to_string(),
                format!("closure signature has {actual} parameter slots; the maximum is 255"),
            );
        }
        assert_eq!(checked_closure_arity(255), Ok(255));
    }
}
