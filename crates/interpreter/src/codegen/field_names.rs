//! Per-module canonical-shape field-names registry.

use std::collections::BTreeSet;

use wasm_encoder::{
    ConstExpr, GlobalSection, GlobalType, HeapType, Instruction, RefType, StorageType, ValType,
};

use crate::Type;
use crate::codegen::intrinsics::IntrinsicTypeIndices;
use crate::codegen::symbol_table::SymbolTable;

pub fn collect(shapes: &[Type]) -> Vec<Vec<String>> {
    let mut keys: BTreeSet<Vec<String>> = BTreeSet::new();
    for shape in shapes {
        if let Type::Object { fields } = shape {
            keys.insert(fields.keys().cloned().collect());
        }
    }
    keys.into_iter().collect()
}

pub fn emit(
    shapes: &[Vec<String>],
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
        let init = build_init_expr(shape, intrinsics, string_vtable_global_idx);
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
    shape: &[String],
    intrinsics: IntrinsicTypeIndices,
    string_vtable_global_idx: u32,
) -> ConstExpr {
    let mut instrs: Vec<Instruction<'_>> = Vec::new();
    for name in shape {
        let code_units: Vec<u16> = name.encode_utf16().collect();
        instrs.push(Instruction::GlobalGet(string_vtable_global_idx));
        for unit in &code_units {
            instrs.push(Instruction::I32Const(*unit as i32));
        }
        instrs.push(Instruction::ArrayNewFixed {
            array_type_index: intrinsics.raw_string,
            array_size: code_units.len() as u32,
        });
        instrs.push(Instruction::StructNew(intrinsics.string));
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
        assert_eq!(keys[0], vec!["x".to_string()]);
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
        assert_eq!(keys[0], vec!["x".to_string()]);
        assert_eq!(keys[1], vec!["y".to_string()]);
    }
}
