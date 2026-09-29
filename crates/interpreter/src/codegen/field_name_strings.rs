//! Per-field-name `(ref $string)` Wasm globals.
//!
//! Sharing a single global per name makes `ref.eq` correctly identify fields
//! at runtime — identity equality on the `$string` allocation.

use std::collections::BTreeSet;

use wasm_encoder::{ConstExpr, GlobalSection, GlobalType, HeapType, Instruction, RefType, ValType};

use crate::Type;
use crate::codegen::intrinsics::IntrinsicTypeIndices;
use crate::codegen::symbol_table::SymbolTable;
use crate::codegen::{next_index, wasm_u32};
use crate::compiler_error::CompilerFailure;

/// `extra_names` injects VTable interface method names (Iterator, Iterable, …)
/// that don't appear in any object literal — without them dispatch chains can't
/// find the per-name string global they expect.
pub fn collect(shapes: &[Type], extra_names: &[String]) -> Vec<String> {
    let mut names: BTreeSet<String> = BTreeSet::new();
    for shape in shapes {
        if let Type::Object { fields, .. } = shape {
            for k in fields.keys() {
                names.insert(k.clone());
            }
        }
    }
    for name in extra_names {
        names.insert(name.clone());
    }
    names.into_iter().collect()
}

pub fn emit(
    names: &[String],
    globals: &mut GlobalSection,
    symbols: &mut SymbolTable,
    next_global_idx: &mut u32,
    intrinsics: IntrinsicTypeIndices,
    string_vtable_global_idx: u32,
) -> Result<(), CompilerFailure> {
    let string_val_type = ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.string),
    });
    for name in names {
        let init = build_init_expr(name, intrinsics, string_vtable_global_idx)?;
        let global_idx = next_index(next_global_idx)?;
        globals.global(
            GlobalType {
                val_type: string_val_type,
                mutable: false,
                shared: false,
            },
            &init,
        );
        symbols.record_field_name_string_global(name.clone(), global_idx);
    }
    Ok(())
}

fn build_init_expr(
    name: &str,
    intrinsics: IntrinsicTypeIndices,
    string_vtable_global_idx: u32,
) -> Result<ConstExpr, CompilerFailure> {
    let mut instrs: Vec<Instruction<'_>> = Vec::new();
    let code_units: Vec<u16> = name.encode_utf16().collect();
    instrs.push(Instruction::GlobalGet(string_vtable_global_idx));
    for unit in &code_units {
        instrs.push(Instruction::I32Const(i32::from(*unit)));
    }
    instrs.push(Instruction::ArrayNewFixed {
        array_type_index: intrinsics.raw_string,
        array_size: wasm_u32(code_units.len())?,
    });
    instrs.push(Instruction::StructNew(intrinsics.string));
    Ok(ConstExpr::extended(instrs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn collect_union_of_field_names_alphabetical() {
        use crate::ObjectField;
        let mut a = BTreeMap::new();
        a.insert("kind".to_string(), ObjectField::required(Type::Number));
        a.insert("radius".to_string(), ObjectField::required(Type::Number));
        let mut b = BTreeMap::new();
        b.insert("kind".to_string(), ObjectField::required(Type::Number));
        b.insert("height".to_string(), ObjectField::required(Type::Number));
        b.insert("width".to_string(), ObjectField::required(Type::Number));
        let shapes = vec![
            Type::Object {
                index: None,
                fields: a,
            },
            Type::Object {
                index: None,
                fields: b,
            },
        ];
        let names = collect(&shapes, &[]);
        assert_eq!(
            names,
            vec![
                "height".to_string(),
                "kind".to_string(),
                "radius".to_string(),
                "width".to_string(),
            ]
        );
    }
}
