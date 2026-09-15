//! Concrete value shapes that need a Wasm subtype emission.

use std::collections::BTreeMap;
use std::fmt;

use crate::{ObjectField, Type};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Shape {
    Object {
        fields: BTreeMap<String, ObjectField>,
    },
    Array(Box<Type>),
    /// Lowers to `$Array`; no distinct Wasm type is emitted.
    Tuple(Vec<Type>),
    Union(Vec<Type>),
}

impl Shape {
    pub fn from_type(ty: &Type) -> Option<Shape> {
        match ty {
            Type::Object { fields } => Some(Shape::Object {
                fields: fields.clone(),
            }),
            Type::Array(elem) => Some(Shape::Array(elem.clone())),
            Type::Tuple(elements) => Some(Shape::Tuple(elements.clone())),
            Type::Union(members) => Some(Shape::Union(members.clone())),
            _ => None,
        }
    }

    /// Deduplication key — matches `Type`'s display so identical shapes across modules compare equal.
    pub fn canonical_display(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for Shape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Shape::Object { fields } => {
                if fields.is_empty() {
                    return f.write_str("{}");
                }
                f.write_str("{ ")?;
                for (i, (name, field)) in fields.iter().enumerate() {
                    if i > 0 {
                        f.write_str("; ")?;
                    }
                    let marker = if field.optional { "?" } else { "" };
                    write!(f, "{}{}: {}", name, marker, field.ty)?;
                }
                f.write_str(" }")
            }
            // Mirrors `Type`'s array arm — `canonical_display` is a dedup key
            // compared against it.
            Shape::Array(elem) => match &**elem {
                Type::Union(_) | Type::Function { .. } => write!(f, "({elem})[]"),
                _ => write!(f, "{elem}[]"),
            },
            Shape::Tuple(elements) => {
                f.write_str("[")?;
                for (i, t) in elements.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{t}")?;
                }
                f.write_str("]")
            }
            Shape::Union(members) => {
                for (i, m) in members.iter().enumerate() {
                    if i > 0 {
                        f.write_str(" | ")?;
                    }
                    match m {
                        Type::Function { .. } => write!(f, "({m})")?,
                        _ => write!(f, "{m}")?,
                    }
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn object_shape_round_trip() {
        let mut fields = BTreeMap::new();
        fields.insert("x".to_string(), ObjectField::required(Type::Number));
        fields.insert("y".to_string(), ObjectField::required(Type::Number));
        let ty = Type::Object {
            fields: fields.clone(),
        };
        let shape = Shape::from_type(&ty).expect("Object is a shape");
        assert_eq!(shape, Shape::Object { fields });
        assert_eq!(shape.canonical_display(), "{ x: number; y: number }");
    }

    #[test]
    fn object_shape_preserves_optional_flag() {
        let mut fields = BTreeMap::new();
        fields.insert("x".to_string(), ObjectField::required(Type::Number));
        fields.insert("y".to_string(), ObjectField::optional(Type::String));
        let ty = Type::Object {
            fields: fields.clone(),
        };
        let shape = Shape::from_type(&ty).expect("Object is a shape");
        assert_eq!(shape, Shape::Object { fields });
        assert_eq!(shape.canonical_display(), "{ x: number; y?: string }");
    }

    #[test]
    fn array_shape_round_trip() {
        let ty = Type::Array(Box::new(Type::String));
        let shape = Shape::from_type(&ty).expect("Array is a shape");
        assert_eq!(shape, Shape::Array(Box::new(Type::String)));
        assert_eq!(shape.canonical_display(), "string[]");
    }

    #[test]
    fn tuple_shape_round_trip() {
        let ty = Type::Tuple(vec![Type::String, Type::Number]);
        let shape = Shape::from_type(&ty).expect("Tuple is a shape");
        assert_eq!(shape, Shape::Tuple(vec![Type::String, Type::Number]));
        assert_eq!(shape.canonical_display(), "[string, number]");
    }

    #[test]
    fn union_shape_round_trip() {
        let ty = Type::union(vec![Type::Number, Type::String]);
        let shape = Shape::from_type(&ty).expect("Union is a shape");
        assert!(matches!(shape, Shape::Union(_)));
        // Canonical sort puts Boolean < Null < Number < String per `Ord`
        // derive on Type (variant order). Just check display includes
        // both members.
        let display = shape.canonical_display();
        assert!(display.contains("number"));
        assert!(display.contains("string"));
        assert!(display.contains(" | "));
    }

    #[test]
    fn non_shape_types_return_none() {
        assert!(Shape::from_type(&Type::Number).is_none());
        assert!(Shape::from_type(&Type::String).is_none());
        assert!(Shape::from_type(&Type::Boolean).is_none());
        assert!(Shape::from_type(&Type::Null).is_none());
        assert!(Shape::from_type(&Type::Void).is_none());
        assert!(Shape::from_type(&Type::Error).is_none());
        assert!(Shape::from_type(&Type::TypeVar("T".to_string())).is_none());
        assert!(
            Shape::from_type(&Type::GenericParam {
                id: 0,
                name: "T".to_string()
            })
            .is_none()
        );
        assert!(
            Shape::from_type(&Type::InterfaceRef {
                mangled: crate::mangle::package_symbol(crate::mangle::USER_PACKAGE, "Foo"),
                package: crate::Package::user(),
                name: "Foo".to_string(),
                args: Vec::new(),
            })
            .is_none()
        );
        assert!(
            Shape::from_type(&Type::NumberEnum {
                mangled: crate::mangle::package_symbol(crate::mangle::USER_PACKAGE, "D"),
                package: crate::Package::user(),
                name: "D".to_string()
            })
            .is_none()
        );
        assert!(
            Shape::from_type(&Type::StringEnum {
                mangled: crate::mangle::package_symbol(crate::mangle::USER_PACKAGE, "C"),
                package: crate::Package::user(),
                name: "C".to_string()
            })
            .is_none()
        );
        assert!(
            Shape::from_type(&Type::Function {
                params: vec![Type::Number],
                ret: Box::new(Type::Number),
                predicate: None,
                has_rest: false,
            })
            .is_none()
        );
    }

    #[test]
    fn btreeset_dedup_by_structural_equality() {
        let mut set: BTreeSet<Shape> = BTreeSet::new();
        let mut fields_a = BTreeMap::new();
        fields_a.insert("x".to_string(), ObjectField::required(Type::Number));
        let mut fields_b = BTreeMap::new();
        fields_b.insert("x".to_string(), ObjectField::required(Type::Number));
        set.insert(Shape::Object { fields: fields_a });
        set.insert(Shape::Object { fields: fields_b });
        set.insert(Shape::Array(Box::new(Type::Number)));
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn canonical_display_matches_type_display_for_equivalent_shape() {
        let mut fields = BTreeMap::new();
        fields.insert("x".to_string(), ObjectField::required(Type::Number));
        let ty = Type::Object {
            fields: fields.clone(),
        };
        let shape = Shape::Object { fields };
        assert_eq!(shape.canonical_display(), ty.to_string());

        let ty = Type::Array(Box::new(Type::String));
        let shape = Shape::Array(Box::new(Type::String));
        assert_eq!(shape.canonical_display(), ty.to_string());
    }
}
