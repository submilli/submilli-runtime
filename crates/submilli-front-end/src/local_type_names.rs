//! The keys block-local types get when lifted to module scope, and how
//! diagnostics show them.

/// Separates a local type's source name from the rest of its key. No
/// identifier, package or mangled name contains it.
pub const MARKER: char = '%';

/// The source name of a type: a local type's key without its suffix.
pub fn display_type_name(name: &str) -> &str {
    name.split_once(MARKER).map_or(name, |(source, _)| source)
}

/// The type arguments, or parameters, of the type named `name` that the source
/// writes: all but those lifted from a local type's enclosing scope.
pub fn written_type_args<'a, T>(name: &str, args: &'a [T]) -> &'a [T] {
    args.get(lifted_type_params(name)..).unwrap_or_default()
}

/// Whether `name` is the key of a type declared in a block.
pub fn is_local_type_name(name: &str) -> bool {
    name.contains(MARKER)
}

fn lifted_type_params(name: &str) -> usize {
    name.split(MARKER)
        .nth(2)
        .and_then(|count| count.parse().ok())
        .unwrap_or(0)
}
