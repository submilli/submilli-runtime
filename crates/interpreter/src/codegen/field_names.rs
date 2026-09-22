//! Per-module canonical-shape field-names registry.

use std::collections::BTreeSet;

use wasm_encoder::{
    ConstExpr, GlobalSection, GlobalType, HeapType, Instruction, RefType, StorageType, ValType,
};

use crate::Type;
use crate::codegen::intrinsics::IntrinsicTypeIndices;
use crate::codegen::symbol_table::SymbolTable;

/// Optional field names use a nominal string subtype. The name's text stays
/// unchanged, while `in` can recover optionality after receiver-type erasure.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FieldName {
    pub name: String,
    pub optional: bool,
}

pub fn declare_optional_name_type(
    types: &mut wasm_encoder::TypeSection,
    symbols: &mut SymbolTable,
    next_type_idx: &mut u32,
    intrinsics: IntrinsicTypeIndices,
) {
    let fields = [intrinsics.vtable, intrinsics.raw_string].map(|index| wasm_encoder::FieldType {
        element_type: StorageType::Val(ValType::Ref(RefType {
            nullable: false,
            heap_type: HeapType::Concrete(index),
        })),
        mutable: false,
    });
    types.ty().subtype(&wasm_encoder::SubType {
        is_final: true,
        supertype_idx: Some(intrinsics.string),
        composite_type: wasm_encoder::CompositeType {
            inner: wasm_encoder::CompositeInnerType::Struct(wasm_encoder::StructType {
                fields: fields.into(),
            }),
            shared: false,
            descriptor: None,
            describes: None,
        },
    });
    symbols.record_optional_field_name_type(*next_type_idx);
    *next_type_idx += 1;
}

pub fn collect(shapes: &[Type]) -> Vec<Vec<FieldName>> {
    let mut keys: BTreeSet<Vec<FieldName>> = BTreeSet::new();
    for shape in shapes {
        if let Type::Object { fields } = shape {
            keys.insert(
                fields
                    .iter()
                    .map(|(name, field)| FieldName {
                        name: name.clone(),
                        optional: field.optional,
                    })
                    .collect(),
            );
        }
    }
    keys.into_iter().collect()
}

pub fn emit(
    shapes: &[Vec<FieldName>],
    globals: &mut GlobalSection,
    symbols: &mut SymbolTable,
    next_global_idx: &mut u32,
    intrinsics: IntrinsicTypeIndices,
    string_vtable_global_idx: u32,
) {
    let field_names_val_type = ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.field_names),
    });
    for shape in shapes {
        let init = build_init_expr(
            shape,
            intrinsics,
            string_vtable_global_idx,
            symbols.optional_field_name_type(),
        );
        globals.global(
            GlobalType {
                val_type: field_names_val_type,
                mutable: false,
                shared: false,
            },
            &init,
        );
        symbols.record_field_names_global(shape.clone(), *next_global_idx);
        *next_global_idx += 1;
    }
}

fn build_init_expr(
    shape: &[FieldName],
    intrinsics: IntrinsicTypeIndices,
    string_vtable_global_idx: u32,
    optional_name_type: u32,
) -> ConstExpr {
    let mut instrs: Vec<Instruction<'_>> = Vec::new();
    for name in shape {
        let code_units: Vec<u16> = name.name.encode_utf16().collect();
        instrs.push(Instruction::GlobalGet(string_vtable_global_idx));
        for unit in &code_units {
            instrs.push(Instruction::I32Const(*unit as i32));
        }
        instrs.push(Instruction::ArrayNewFixed {
            array_type_index: intrinsics.raw_string,
            array_size: code_units.len() as u32,
        });
        instrs.push(Instruction::StructNew(if name.optional {
            optional_name_type
        } else {
            intrinsics.string
        }));
    }
    instrs.push(Instruction::ArrayNewFixed {
        array_type_index: intrinsics.field_names,
        array_size: shape.len() as u32,
    });
    let _ = StorageType::Val(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.field_names),
    }));
    ConstExpr::extended(instrs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn collect_dedupes_same_shape_with_different_field_types() {
        use crate::ObjectField;
        let mut a = BTreeMap::new();
        a.insert("x".to_string(), ObjectField::required(Type::Number));
        let mut b = BTreeMap::new();
        b.insert("x".to_string(), ObjectField::required(Type::String));
        let shapes = vec![Type::Object { fields: a }, Type::Object { fields: b }];
        let keys = collect(&shapes);
        assert_eq!(keys.len(), 1);
        assert_eq!(
            keys[0],
            vec![FieldName {
                name: "x".to_string(),
                optional: false
            }]
        );
    }

    #[test]
    fn collect_separates_shapes_with_different_field_names() {
        use crate::ObjectField;
        let mut a = BTreeMap::new();
        a.insert("x".to_string(), ObjectField::required(Type::Number));
        let mut b = BTreeMap::new();
        b.insert("y".to_string(), ObjectField::required(Type::Number));
        let shapes = vec![Type::Object { fields: a }, Type::Object { fields: b }];
        let keys = collect(&shapes);
        assert_eq!(keys.len(), 2);
        assert_eq!(
            keys[0],
            vec![FieldName {
                name: "x".to_string(),
                optional: false
            }]
        );
        assert_eq!(
            keys[1],
            vec![FieldName {
                name: "y".to_string(),
                optional: false
            }]
        );
    }
}
