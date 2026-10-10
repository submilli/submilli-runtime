//! Compiler limits, and the native stack embedders provide for compilation.

/// Native stack a thread needs to run a [`crate::compile`] entry point or a
/// public phase API on any program within the structural limits below and in
/// [`crate::tree_height`], including types at [`MAX_TYPE_DEPTH`]. The
/// interpreter creates no threads: embedders run compilation on a thread of at
/// least this size. Only touched pages are committed. The deepest programs at
/// the limits measured about 63 MiB in unoptimized builds and 2 MiB in
/// optimized ones; each size keeps at least twice that.
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

/// Nodes in one type. Instantiating an alias or generic that mentions its
/// parameter more than once copies the argument, so nesting such instantiations
/// doubles a type's size per level; substitution stops at this bound. An alias
/// instance also keeps its arguments beside its substituted body, so each layer
/// of generic alias around a type doubles it too. The largest type in the
/// fixture and package suites has under 100 nodes. Each stored copy of a type
/// costs memory in proportion, up to about 1 KiB per node for small objects.
pub const MAX_TYPE_NODES: u64 = 1 << 16;

/// Nesting depth of one type, counting every type constructor and alias. Every
/// recursive walk over a type is bounded by it. Annotation resolution nests at
/// most [`MAX_TYPE_RESOLUTION_DEPTH`] levels, but a `[]` suffix is not one of
/// them, so a chain `type Ai = A(i-1)[]` adds two levels per alias and is cut
/// here at 254 aliases.
pub const MAX_TYPE_DEPTH: u32 = 512;

/// Steps of type work one compiler phase may spend: each type node built by
/// substitution (instantiating aliases, generic calls and members, and
/// expanding interfaces) and each step comparing types. A phase can rebuild or
/// compare types near [`MAX_TYPE_NODES`] many times over; this bounds the
/// total. The largest inference in the fixture and package suites spends under
/// 2 million.
pub const MAX_TYPE_WORK: u64 = 1 << 24;

/// Steps of inline code one runtime type check may emit: one per structural
/// test of a type, union member, field or element, and one per segment of the
/// failure path each test records. Interface checks are inlined with their
/// members' checks, so interfaces that reference others several times multiply
/// a check's code; this bounds the work of emitting one before the function's
/// locals would. The largest module in the fixture and package suites emits
/// under 4,000 tests across all its checks.
pub const MAX_INLINE_VALIDATOR_STEPS: u64 = 1 << 15;

/// Locals the Wasm engine accepts in one function (wasmparser's
/// `MAX_WASM_FUNCTION_LOCALS`). Runtime checks add locals as they are emitted,
/// so emitting stops with a located error before a function the engine would
/// reject.
pub const MAX_FUNCTION_LOCALS: u32 = 50_000;

/// Bytes the Wasm engine accepts in one function body (wasmparser's
/// `MAX_WASM_FUNCTION_SIZE`). Runtime checks stop emitting before a function
/// reaches it, which also keeps a body far below the 4 GiB the Wasm encoder can
/// represent.
pub const MAX_FUNCTION_BODY_BYTES: usize = 7_654_321;

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

/// Limits on dependency namespace metadata before recursive compiler consumers.
pub const MAX_NAMESPACE_DEPTH: usize = 128;
pub const MAX_NAMESPACE_NODES: usize = 1 << 16;
/// Total qualified namespace/type path bytes materialized during registration.
pub const MAX_NAMESPACE_PATH_BYTES: usize = 1 << 20;

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
