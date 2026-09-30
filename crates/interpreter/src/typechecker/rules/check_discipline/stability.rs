//! Which static types hold values that cannot change after they are read.

use crate::Type;

/// Whether every value of type `ty` is immutable and runs no code when read.
pub(super) fn is_primitive(ty: &Type) -> bool {
    match ty.peel() {
        Type::Number
        | Type::NumberLiteral(_)
        | Type::BigInt
        | Type::String
        | Type::StringLiteral(_)
        | Type::Boolean
        | Type::BooleanLiteral(_)
        | Type::Null
        | Type::Void
        | Type::Never
        | Type::NumberEnum { .. }
        | Type::StringEnum { .. }
        // A diagnostic already reports the broken type.
        | Type::Error => true,
        Type::Union(members) => members.iter().all(is_primitive),
        Type::Uint8Array
        | Type::Unknown
        | Type::Function { .. }
        | Type::Object { .. }
        | Type::Array(_)
        | Type::Tuple(_)
        | Type::TypeVar(_)
        | Type::GenericParam { .. }
        | Type::InterfaceRef { .. }
        | Type::ClassRef { .. }
        | Type::AliasRef { .. }
        // `peel` has removed these.
        | Type::Readonly(_)
        | Type::Refined { .. }
        | Type::Alias { .. } => false,
    }
}
