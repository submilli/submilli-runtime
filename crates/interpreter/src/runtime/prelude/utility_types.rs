//! TypeScript's built-in utility types (`Partial`, `Pick`, `ReturnType`, …).
//!
//! Each is a prelude type alias whose body is an application of itself. The
//! typechecker evaluates an application in Rust once its arguments are
//! concrete (`typechecker::infer::utility_types`); a path that substitutes the
//! body without evaluating it still yields the application itself, never a
//! silently wrong type.

use std::collections::BTreeMap;

use crate::runtime::prelude::declaration::doc;
use crate::{GenericParameters, Package, Span, Type, TypeKind, TypeSymbol};

/// A utility's name, its type parameters with their bounds, the
/// `lib.es5.d.ts` declaration shown in its docs, and its doc comment.
struct Utility {
    name: &'static str,
    params: &'static [&'static str],
    declaration: &'static str,
    doc: &'static str,
}

const UTILITIES: &[Utility] = &[
    Utility {
        name: "Partial",
        params: &["T"],
        declaration: "type Partial<T> = { [P in keyof T]?: T[P] };",
        doc: "/** Make all properties in `T` optional. */",
    },
    Utility {
        name: "Required",
        params: &["T"],
        declaration: "type Required<T> = { [P in keyof T]-?: T[P] };",
        doc: "/** Make all properties in `T` required. */",
    },
    Utility {
        name: "Readonly",
        params: &["T"],
        declaration: "type Readonly<T> = { readonly [P in keyof T]: T[P] };",
        doc: "/** Make all properties in `T` readonly. */",
    },
    Utility {
        name: "Pick",
        params: &["T", "K"],
        declaration: "type Pick<T, K extends keyof T> = { [P in K]: T[P] };",
        doc: "/** From `T`, pick a set of properties whose keys are in the union `K`. */",
    },
    Utility {
        name: "Omit",
        params: &["T", "K"],
        declaration: "type Omit<T, K extends string | number> = Pick<T, Exclude<keyof T, K>>;",
        doc: "/** Construct a type with the properties of `T` except for those in type `K`. */",
    },
    Utility {
        name: "Exclude",
        params: &["T", "U"],
        declaration: "type Exclude<T, U> = T extends U ? never : T;",
        doc: "/** Exclude from `T` those types that are assignable to `U`. */",
    },
    Utility {
        name: "Extract",
        params: &["T", "U"],
        declaration: "type Extract<T, U> = T extends U ? T : never;",
        doc: "/** Extract from `T` those types that are assignable to `U`. */",
    },
    Utility {
        name: "NonNullable",
        params: &["T"],
        declaration: "type NonNullable<T> = T & {};",
        doc: "/** Exclude `null` and `undefined` from `T`. */",
    },
    Utility {
        name: "Parameters",
        params: &["T"],
        declaration: "type Parameters<T extends (...args: never[]) => unknown> = T extends (...args: infer P) => unknown ? P : never;",
        doc: "/** Obtain the parameters of a function type in a tuple. */",
    },
    Utility {
        name: "ReturnType",
        params: &["T"],
        declaration: "type ReturnType<T extends (...args: never[]) => unknown> = T extends (...args: never[]) => infer R ? R : never;",
        doc: "/** Obtain the return type of a function type. */",
    },
    Utility {
        name: "NoInfer",
        params: &["T"],
        declaration: "type NoInfer<T> = intrinsic;",
        doc: "/** Marker for non-inference type position: `T` is not inferred from it. */",
    },
    Utility {
        name: "Uppercase",
        params: &["S"],
        declaration: "type Uppercase<S extends string> = intrinsic;",
        doc: "/** Convert string literal type to uppercase. */",
    },
    Utility {
        name: "Lowercase",
        params: &["S"],
        declaration: "type Lowercase<S extends string> = intrinsic;",
        doc: "/** Convert string literal type to lowercase. */",
    },
    Utility {
        name: "Capitalize",
        params: &["S"],
        declaration: "type Capitalize<S extends string> = intrinsic;",
        doc: "/** Convert first character of string literal type to uppercase. */",
    },
    Utility {
        name: "Uncapitalize",
        params: &["S"],
        declaration: "type Uncapitalize<S extends string> = intrinsic;",
        doc: "/** Convert first character of string literal type to lowercase. */",
    },
];

/// The `lib.es5.d.ts` declaration of the utility type `name`, which its
/// self-referencing alias body cannot spell.
pub(crate) fn declaration_text(mangled: &crate::MangledName) -> Option<&'static str> {
    UTILITIES
        .iter()
        .find(|utility| &crate::mangle::prelude(utility.name) == mangled)
        .map(|utility| utility.declaration)
}

/// The bound of each constrained utility parameter.
fn constraints(name: &str) -> BTreeMap<String, Type> {
    let callable = || Type::Function {
        params: vec![Type::Array(Box::new(Type::Never))],
        optional: 0,
        ret: Box::new(Type::Unknown),
        predicate: None,
        has_rest: true,
    };
    match name {
        "Pick" => BTreeMap::from([(
            "K".to_string(),
            Type::KeyOf(Box::new(Type::TypeVar("T".to_string()))),
        )]),
        "Omit" => BTreeMap::from([(
            "K".to_string(),
            Type::union(vec![Type::String, Type::Number]),
        )]),
        "Parameters" | "ReturnType" => BTreeMap::from([("T".to_string(), callable())]),
        "Uppercase" | "Lowercase" | "Capitalize" | "Uncapitalize" => {
            BTreeMap::from([("S".to_string(), Type::String)])
        }
        _ => BTreeMap::new(),
    }
}

pub(crate) fn declare_types(defs: &mut crate::PackageDeclaration) {
    for utility in UTILITIES {
        let names: Vec<String> = utility.params.iter().map(ToString::to_string).collect();
        let mangled = crate::mangle::prelude(utility.name);
        let ty = Type::alias_ref(
            Package::prelude(),
            utility.name,
            mangled.clone(),
            names.iter().cloned().map(Type::TypeVar).collect(),
        );
        defs.types.insert(
            utility.name.to_string(),
            TypeSymbol {
                name: utility.name.to_string(),
                mangled_name: mangled,
                declaration_span: Span::at(crate::FileId::PRELUDE),
                kind: TypeKind::Alias {
                    generics: GenericParameters {
                        names,
                        constraints: Box::new(constraints(utility.name)),
                    },
                    ty,
                    doc: doc(utility.doc),
                },
            },
        );
    }
}
