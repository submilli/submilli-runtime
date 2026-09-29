//! Compiler limits, and the native stack embedders provide for compilation.

/// Native stack a thread needs to run a [`crate::compile`] entry point or a
/// public phase API on any program within the structural limits below and in
/// [`crate::tree_height`]. The interpreter creates no threads: embedders run
/// compilation on a thread of at least this size. Only touched pages are
/// committed. The deepest programs at the limits measured about 63 MiB in
/// unoptimized builds and 2 MiB in optimized ones; each size keeps at least
/// twice that.
pub const COMPILER_STACK_BYTES: usize = if cfg!(debug_assertions) {
    128 * 1024 * 1024
} else {
    16 * 1024 * 1024
};

/// Number of declared argument slots in the closure ABI. Defaults keep their
/// slots and a packed rest array occupies one slot. The environment/receiver
/// and captured generic descriptors are carried separately.
pub const MAX_CLOSURE_ARITY: usize = u8::MAX as usize;

/// Nested type annotation and alias-body resolutions. Each alias reference
/// costs a level, and so does each type constructor (object, array, tuple,
/// union, function, generic type or `readonly`) wrapping the next reference in an alias
/// body: over a primitive base, plain renames chain 254 aliases and
/// `{ v: Previous }` bodies 127. Resolved alias bodies are inlined, so this also
/// bounds the depth of alias-expanded types.
pub const MAX_TYPE_RESOLUTION_DEPTH: u32 = 256;

/// Optional spreads that may override one field of an object literal, each
/// over the value an earlier member supplied. Each link is a
/// boxed fallback walked recursively; kept well below the typed-tree height
/// limit so this limit, not the generic one, names the cause.
pub const MAX_SPREAD_FALLBACK_CHAIN: u32 = 512;

/// Classes in one inheritance chain, including the class itself and library
/// ancestors such as `Error`. Each class struct subtypes its parent's, the root
/// class subtypes the intrinsic object struct, and WasmGC validation rejects
/// subtype depths above 63.
pub const MAX_CLASS_CHAIN_LEN: usize = 62;

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
