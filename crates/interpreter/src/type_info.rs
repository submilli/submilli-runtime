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
    Boolean,
    Number,
    String,
    NumberLiteral(LiteralF64),
    StringLiteral(String),
    BooleanLiteral(bool),
    Array { element: TypeInfoId },
    Tuple { elements: Vec<TypeInfoId> },
    Object { fields: Vec<FieldInfo> },
    Union { members: Vec<TypeInfoId> },
    Unsupported { label: String },
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
        let Type::Object { fields } = ty.peel() else {
            return false;
        };
        fields
            .values()
            .all(|field| self.supports_host_json_type(&field.ty))
    }

    pub fn supports_host_json_type(&self, ty: &Type) -> bool {
        match ty.peel() {
            Type::Null
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
            Type::Union(members) if members.iter().any(|m| matches!(m.peel(), Type::Null)) => {
                members
                    .iter()
                    .filter(|m| !matches!(m.peel(), Type::Null))
                    .count()
                    == 1
                    && members
                        .iter()
                        .find(|m| !matches!(m.peel(), Type::Null))
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
            | (TypeInfoKind::Boolean, Type::Boolean)
            | (TypeInfoKind::Number, Type::Number)
            | (TypeInfoKind::String, Type::String) => true,
            (TypeInfoKind::NumberLiteral(a), Type::NumberLiteral(b)) => a == b,
            (TypeInfoKind::StringLiteral(a), Type::StringLiteral(b)) => a == b,
            (TypeInfoKind::BooleanLiteral(a), Type::BooleanLiteral(b)) => a == b,
            (TypeInfoKind::Array { element }, Type::Array(expected)) => {
                self.type_info_matches_type(*element, expected, seen)
            }
            (TypeInfoKind::Tuple { elements }, Type::Tuple(expected)) => {
                elements.len() == expected.len()
                    && elements
                        .iter()
                        .zip(expected)
                        .all(|(id, ty)| self.type_info_matches_type(*id, ty, seen))
            }
            (TypeInfoKind::Object { fields }, Type::Object { fields: expected }) => {
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

    fn finish(self) -> TypeInfoTable {
        self.table
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
                elements: elements
                    .iter()
                    .map(|element| self.intern_type(element))
                    .collect(),
            },
            Type::Object { fields } => TypeInfoKind::Object {
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
        Shape::Object { fields } => Type::Object {
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
    fn object_type_id_finds_collected_object_shape() {
        let fields = BTreeMap::from([
            ("id".to_string(), ObjectField::required(Type::Number)),
            ("name".to_string(), ObjectField::required(Type::String)),
        ]);
        let ty = Type::Object {
            fields: fields.clone(),
        };
        let table = TypeInfoTable::collect_from_shapes("main", &[Shape::Object { fields }]);
        let id = table
            .object_type_id(&ty)
            .expect("collected object shape should have TypeInfo");

        assert!(matches!(
            table.get(id).map(|info| &info.kind),
            Some(TypeInfoKind::Object { fields }) if fields.len() == 2
        ));
    }
}
