use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::types::LiteralF64;
use crate::{Shape, Type, TypedAst};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TypeInfoId(pub u32);

impl TypeInfoId {
    pub fn as_u32(self) -> u32 {
        self.0
    }

    pub fn as_usize(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TypeInfoTable {
    pub package_name: String,
    pub types: Vec<TypeInfo>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TypeInfo {
    pub id: TypeInfoId,
    pub kind: TypeInfoKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TypeInfoKind {
    Null,
    Undefined,
    Boolean,
    Number,
    String,
    NumberLiteral(LiteralF64),
    StringLiteral(String),
    BooleanLiteral(bool),
    Array {
        element: TypeInfoId,
    },
    Tuple {
        elements: Vec<TypeInfoId>,
        optional: usize,
    },
    Object {
        fields: Vec<FieldInfo>,
    },
    Union {
        members: Vec<TypeInfoId>,
    },
    Unsupported {
        label: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FieldInfo {
    pub name: String,
    pub optional: bool,
    pub type_id: TypeInfoId,
}

impl TypeInfoTable {
    pub fn collect(package_name: impl Into<String>, ast: &TypedAst) -> Self {
        Self::collect_from_shapes(package_name, &ast.shapes)
    }

    pub fn collect_from_shapes(package_name: impl Into<String>, shapes: &[Shape]) -> Self {
        Self::collect_from_shapes_and_types(package_name, shapes, &[])
    }

    pub fn collect_from_shapes_and_types(
        package_name: impl Into<String>,
        shapes: &[Shape],
        types: &[Type],
    ) -> Self {
        Self::collect_indexed(package_name, shapes, types).0
    }

    /// [`collect_from_shapes_and_types`](Self::collect_from_shapes_and_types),
    /// plus the id each collected type was interned under.
    pub fn collect_indexed(
        package_name: impl Into<String>,
        shapes: &[Shape],
        types: &[Type],
    ) -> (Self, TypeInfoIndex) {
        let mut builder = TypeInfoBuilder::new(package_name.into());
        for shape in shapes {
            builder.intern_type(&canonical_type(shape));
        }
        for ty in types {
            builder.intern_type(ty);
        }
        builder.finish()
    }

    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
    }

    pub fn get(&self, id: TypeInfoId) -> Option<&TypeInfo> {
        self.types.get(id.as_usize()).filter(|info| info.id == id)
    }

    pub fn object_type_id(&self, ty: &Type) -> Option<TypeInfoId> {
        let Type::Object { .. } = ty.peel() else {
            return None;
        };
        self.type_id(ty).filter(|id| {
            matches!(
                self.get(*id).map(|info| &info.kind),
                Some(TypeInfoKind::Object { .. })
            )
        })
    }

    pub fn type_id(&self, ty: &Type) -> Option<TypeInfoId> {
        self.types.iter().find_map(|info| {
            let mut seen = BTreeSet::new();
            self.type_info_matches_type(info.id, ty, &mut seen)
                .then_some(info.id)
        })
    }

    pub fn supports_host_json_object(&self, ty: &Type) -> bool {
        let Type::Object {
            fields,
            index: None,
        } = ty.peel()
        else {
            return false;
        };
        fields
            .values()
            .all(|field| self.supports_host_json_type(&field.ty))
    }

    pub fn supports_host_json_type(&self, ty: &Type) -> bool {
        match ty.peel() {
            Type::Null
            | Type::Undefined
            | Type::Boolean
            | Type::BooleanLiteral(_)
            | Type::Number
            | Type::NumberLiteral(_)
            | Type::String
            | Type::StringLiteral(_) => true,
            Type::Object { .. } => self.supports_host_json_object(ty),
            Type::Array(element) => self.supports_host_json_type(element),
            Type::Tuple(elements) => elements
                .iter()
                .all(|element| self.supports_host_json_type(element)),
            Type::Union(members)
                if members
                    .iter()
                    .any(|m| matches!(m.peel(), Type::Null | Type::Undefined)) =>
            {
                members
                    .iter()
                    .filter(|m| !matches!(m.peel(), Type::Null | Type::Undefined))
                    .count()
                    == 1
                    && members
                        .iter()
                        .find(|m| !matches!(m.peel(), Type::Null | Type::Undefined))
                        .is_some_and(|m| self.supports_host_json_type(m))
            }
            _ => false,
        }
    }

    fn type_info_matches_type(
        &self,
        id: TypeInfoId,
        ty: &Type,
        seen: &mut BTreeSet<(TypeInfoId, Type)>,
    ) -> bool {
        if !seen.insert((id, ty.clone())) {
            return true;
        }
        let Some(info) = self.get(id) else {
            return false;
        };
        match (&info.kind, ty.peel()) {
            (TypeInfoKind::Null, Type::Null)
            | (TypeInfoKind::Undefined, Type::Undefined)
            | (TypeInfoKind::Boolean, Type::Boolean)
            | (TypeInfoKind::Number, Type::Number)
            | (TypeInfoKind::String, Type::String) => true,
            (TypeInfoKind::NumberLiteral(a), Type::NumberLiteral(b)) => a == b,
            (TypeInfoKind::StringLiteral(a), Type::StringLiteral(b)) => a == b,
            (TypeInfoKind::BooleanLiteral(a), Type::BooleanLiteral(b)) => a == b,
            (TypeInfoKind::Array { element }, Type::Array(expected)) => {
                self.type_info_matches_type(*element, expected, seen)
            }
            (TypeInfoKind::Tuple { elements, optional }, Type::Tuple(expected)) => {
                *optional == expected.optional
                    && elements.len() == expected.len()
                    && elements
                        .iter()
                        .zip(expected)
                        .all(|(id, ty)| self.type_info_matches_type(*id, ty, seen))
            }
            (
                TypeInfoKind::Object { fields },
                Type::Object {
                    fields: expected, ..
                },
            ) => {
                fields.len() == expected.len()
                    && fields.iter().zip(expected).all(|(info, (name, field))| {
                        info.name == *name
                            && info.optional == field.optional
                            && self.type_info_matches_type(info.type_id, &field.ty, seen)
                    })
            }
            (TypeInfoKind::Union { members }, Type::Union(expected)) => {
                members.len() == expected.len()
                    && members
                        .iter()
                        .zip(expected)
                        .all(|(id, ty)| self.type_info_matches_type(*id, ty, seen))
            }
            _ => false,
        }
    }
}

/// The id each type was interned under when a [`TypeInfoTable`] was collected.
/// [`TypeInfoTable::type_id`] finds a type by comparing it with every entry in
/// turn, which is quadratic over a program's types; a collected type is found
/// here directly.
#[derive(Debug, Default)]
pub struct TypeInfoIndex {
    by_type: BTreeMap<Type, TypeInfoId>,
}

impl TypeInfoIndex {
    /// [`TypeInfoTable::object_type_id`], looking a collected type up directly.
    pub fn object_type_id(&self, table: &TypeInfoTable, ty: &Type) -> Option<TypeInfoId> {
        let Some(id) = self.by_type.get(ty) else {
            return table.object_type_id(ty);
        };
        matches!(
            table.get(*id).map(|info| &info.kind),
            Some(TypeInfoKind::Object { .. })
        )
        .then_some(*id)
    }
}

struct TypeInfoBuilder {
    table: TypeInfoTable,
    by_type: BTreeMap<Type, TypeInfoId>,
}

impl TypeInfoBuilder {
    fn new(package_name: String) -> Self {
        Self {
            table: TypeInfoTable {
                package_name,
                types: Vec::new(),
            },
            by_type: BTreeMap::new(),
        }
    }

    fn finish(self) -> (TypeInfoTable, TypeInfoIndex) {
        (
            self.table,
            TypeInfoIndex {
                by_type: self.by_type,
            },
        )
    }

    fn intern_type(&mut self, ty: &Type) -> TypeInfoId {
        if let Some(id) = self.by_type.get(ty) {
            return *id;
        }

        let id = TypeInfoId(self.table.types.len() as u32);
        self.by_type.insert(ty.clone(), id);
        self.table.types.push(TypeInfo {
            id,
            kind: TypeInfoKind::Unsupported {
                label: ty.to_string(),
            },
        });

        let kind = self.kind_for_type(ty);
        self.table.types[id.as_usize()].kind = kind;
        id
    }

    fn kind_for_type(&mut self, ty: &Type) -> TypeInfoKind {
        match ty.peel() {
            Type::Null => TypeInfoKind::Null,
            Type::Undefined => TypeInfoKind::Undefined,
            Type::Boolean => TypeInfoKind::Boolean,
            Type::Number => TypeInfoKind::Number,
            Type::String => TypeInfoKind::String,
            Type::NumberLiteral(n) => TypeInfoKind::NumberLiteral(*n),
            Type::StringLiteral(s) => TypeInfoKind::StringLiteral(s.clone()),
            Type::BooleanLiteral(b) => TypeInfoKind::BooleanLiteral(*b),
            Type::Array(element) => TypeInfoKind::Array {
                element: self.intern_type(element),
            },
            Type::Tuple(elements) => TypeInfoKind::Tuple {
                optional: elements.optional,
                elements: elements
                    .iter()
                    .map(|element| self.intern_type(element))
                    .collect(),
            },
            Type::Object { fields, .. } => TypeInfoKind::Object {
                fields: fields
                    .iter()
                    .map(|(name, field)| FieldInfo {
                        name: name.clone(),
                        optional: field.optional,
                        type_id: self.intern_type(&field.ty),
                    })
                    .collect(),
            },
            Type::Union(members) => TypeInfoKind::Union {
                members: members
                    .iter()
                    .map(|member| self.intern_type(member))
                    .collect(),
            },
            other => TypeInfoKind::Unsupported {
                label: other.to_string(),
            },
        }
    }
}

fn canonical_type(shape: &Shape) -> Type {
    match shape {
        Shape::Object { fields, index } => Type::Object {
            index: index.clone(),
            fields: fields.clone(),
        },
        Shape::Array(elem) => Type::Array(elem.clone()),
        Shape::Tuple(elements) => Type::Tuple(elements.clone()),
        Shape::Union(members) => Type::Union(members.clone()),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::{ObjectField, Shape, Type, TypeInfoKind, TypeInfoTable};

    #[test]
    fn index_finds_the_same_object_type_id_as_the_scan() {
        let inner = BTreeMap::from([("v".to_string(), ObjectField::required(Type::Number))]);
        let inner_ty = Type::Object {
            index: None,
            fields: inner.clone(),
        };
        let outer =
            BTreeMap::from([("inner".to_string(), ObjectField::required(inner_ty.clone()))]);
        let outer_ty = Type::Object {
            index: None,
            fields: outer.clone(),
        };
        let (table, index) = TypeInfoTable::collect_indexed(
            "main",
            &[
                Shape::Object {
                    index: None,
                    fields: outer,
                },
                Shape::Array(Box::new(Type::String)),
            ],
            std::slice::from_ref(&inner_ty),
        );
        for ty in [&outer_ty, &inner_ty] {
            let id = index.object_type_id(&table, ty);
            assert!(id.is_some(), "{ty} has type info");
            assert_eq!(id, table.object_type_id(ty));
        }
        let array = Type::Array(Box::new(Type::String));
        assert_eq!(index.object_type_id(&table, &array), None);
        // A type the table never collected falls back to the structural scan.
        let uncollected = Type::Object {
            index: None,
            fields: BTreeMap::from([("w".to_string(), ObjectField::required(Type::Number))]),
        };
        assert_eq!(index.object_type_id(&table, &uncollected), None);
    }

    #[test]
    fn object_type_id_finds_collected_object_shape() {
        let fields = BTreeMap::from([
            ("id".to_string(), ObjectField::required(Type::Number)),
            ("name".to_string(), ObjectField::required(Type::String)),
        ]);
        let ty = Type::Object {
            index: None,
            fields: fields.clone(),
        };
        let table = TypeInfoTable::collect_from_shapes(
            "main",
            &[Shape::Object {
                index: None,
                fields,
            }],
        );
        let id = table
            .object_type_id(&ty)
            .expect("collected object shape should have TypeInfo");

        assert!(matches!(
            table.get(id).map(|info| &info.kind),
            Some(TypeInfoKind::Object { fields }) if fields.len() == 2
        ));
    }
}
