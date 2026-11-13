//! The built-in utility types (`Partial`, `Pick`, …) by their prelude
//! identity, and when an application of one must wait for type arguments.

use crate::Type;
use crate::mangle::MangledName;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Utility {
    Partial,
    Required,
    Readonly,
    Pick,
    Omit,
    Exclude,
    Extract,
    NonNullable,
    Parameters,
    ReturnType,
    NoInfer,
    Uppercase,
    Lowercase,
    Capitalize,
    Uncapitalize,
}

/// The utility a prelude alias names, keyed on its mangled identity so a user
/// type of the same name never matches.
pub fn utility(mangled: &MangledName) -> Option<Utility> {
    let name = mangled
        .as_str()
        .strip_prefix(crate::mangle::PRELUDE_PACKAGE)?
        .strip_prefix(crate::mangle::SEP)?;
    Utility::ALL
        .into_iter()
        .find(|utility| utility.name() == name)
}

impl Utility {
    pub const ALL: [Utility; 15] = [
        Utility::Partial,
        Utility::Required,
        Utility::Readonly,
        Utility::Pick,
        Utility::Omit,
        Utility::Exclude,
        Utility::Extract,
        Utility::NonNullable,
        Utility::Parameters,
        Utility::ReturnType,
        Utility::NoInfer,
        Utility::Uppercase,
        Utility::Lowercase,
        Utility::Capitalize,
        Utility::Uncapitalize,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Utility::Partial => "Partial",
            Utility::Required => "Required",
            Utility::Readonly => "Readonly",
            Utility::Pick => "Pick",
            Utility::Omit => "Omit",
            Utility::Exclude => "Exclude",
            Utility::Extract => "Extract",
            Utility::NonNullable => "NonNullable",
            Utility::Parameters => "Parameters",
            Utility::ReturnType => "ReturnType",
            Utility::NoInfer => "NoInfer",
            Utility::Uppercase => "Uppercase",
            Utility::Lowercase => "Lowercase",
            Utility::Capitalize => "Capitalize",
            Utility::Uncapitalize => "Uncapitalize",
        }
    }

    pub fn is_string_mapping(self) -> bool {
        matches!(
            self,
            Utility::Uppercase | Utility::Lowercase | Utility::Capitalize | Utility::Uncapitalize
        )
    }
}

/// The utility and arguments of an application that still mentions a type
/// parameter.
pub fn deferred_application(ty: &Type) -> Option<(Utility, &[Type])> {
    let Type::AliasRef { mangled, args, .. } = ty else {
        return None;
    };
    let utility = utility(mangled)?;
    args.iter()
        .any(holds_type_parameter)
        .then_some((utility, args.as_slice()))
}

/// Whether a type parameter occurs anywhere in `ty`, either a signature
/// `TypeVar` or a body `GenericParam`.
pub fn holds_type_parameter(ty: &Type) -> bool {
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        if matches!(ty, Type::TypeVar(_) | Type::GenericParam { .. }) {
            return true;
        }
        crate::type_size::for_each_child(ty, |child| pending.push(child));
    }
    false
}
