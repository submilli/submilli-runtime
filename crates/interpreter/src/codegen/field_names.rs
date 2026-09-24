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
    let mut fields = [intrinsics.vtable, intrinsics.raw_string]
        .map(|index| wasm_encoder::FieldType {
            element_type: StorageType::Val(ValType::Ref(RefType {
                nullable: false,
                heap_type: HeapType::Concrete(index),
            })),
            mutable: false,
        })
        .to_vec();
    fields.push(wasm_encoder::FieldType {
        element_type: StorageType::Val(ValType::I32),
        mutable: true,
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
        if name.optional {
            instrs.push(Instruction::I32Const(0));
        }
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

/// Optional names carry per-instance presence separately from the value slot.
/// Required names remain shared immutable strings.
pub(crate) fn emit_instance_names(
    emitter: &mut super::function_emitter::FunctionEmitter,
    ctx: &super::CodegenCtx,
    names: &[FieldName],
    present: impl Fn(&str) -> bool,
) {
    let global = ctx
        .symbols
        .field_names_global_idx(names)
        .expect("field names collected");
    if !names.iter().any(|name| name.optional) {
        emitter.instruction(Instruction::GlobalGet(global));
        return;
    }
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsics declared");
    for (index, name) in names.iter().enumerate() {
        if name.optional {
            for field_index in [0, 1] {
                emitter.instruction(Instruction::GlobalGet(global));
                emitter.instruction(Instruction::I32Const(index as i32));
                emitter.instruction(Instruction::ArrayGet(intrinsics.field_names));
                emitter.instruction(Instruction::StructGet {
                    struct_type_index: intrinsics.string,
                    field_index,
                });
            }
            emitter.instruction(Instruction::I32Const(i32::from(present(&name.name))));
            emitter.instruction(Instruction::StructNew(
                ctx.symbols.optional_field_name_type(),
            ));
        } else {
            emitter.instruction(Instruction::GlobalGet(global));
            emitter.instruction(Instruction::I32Const(index as i32));
            emitter.instruction(Instruction::ArrayGet(intrinsics.field_names));
        }
    }
    emitter.instruction(Instruction::ArrayNewFixed {
        array_type_index: intrinsics.field_names,
        array_size: names.len() as u32,
    });
}

/// Push the presence flag for a field name already on the stack.
pub(crate) fn emit_name_presence(
    emitter: &mut super::function_emitter::FunctionEmitter,
    ctx: &super::CodegenCtx,
) {
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsics declared");
    let name = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.string),
    }));
    emitter.instruction(Instruction::LocalTee(name));
    let optional = ctx.symbols.optional_field_name_type();
    emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(optional)));
    emitter.emit_if(wasm_encoder::BlockType::Result(ValType::I32));
    emitter.instruction(Instruction::LocalGet(name));
    emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(optional)));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: optional,
        field_index: 2,
    });
    emitter.emit_else();
    emitter.instruction(Instruction::I32Const(1));
    emitter.emit_end();
}

/// The object and index are locals; callers mark a successful store as present.
pub(crate) fn emit_set_presence(
    emitter: &mut super::function_emitter::FunctionEmitter,
    ctx: &super::CodegenCtx,
    object: u32,
    index: u32,
    present: bool,
) {
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .expect("intrinsics declared");
    let name = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.string),
    }));
    emitter.instruction(Instruction::LocalGet(object));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: intrinsics.object_shape,
        field_index: 1,
    });
    emitter.instruction(Instruction::LocalGet(index));
    emitter.instruction(Instruction::ArrayGet(intrinsics.field_names));
    emitter.instruction(Instruction::LocalTee(name));
    let optional = ctx.symbols.optional_field_name_type();
    emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(optional)));
    emitter.emit_if(wasm_encoder::BlockType::Empty);
    emitter.instruction(Instruction::LocalGet(name));
    emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(optional)));
    emitter.instruction(Instruction::I32Const(i32::from(present)));
    emitter.instruction(Instruction::StructSet {
        struct_type_index: optional,
        field_index: 2,
    });
    emitter.emit_end();
}

/// Serializer fast path for an optional slot in its own declared layout.
pub(crate) fn emit_optional_presence(
    function: &mut wasm_encoder::Function,
    intrinsics: IntrinsicTypeIndices,
    optional_name_type: u32,
    object: u32,
    index: u32,
    value: u32,
) {
    for instruction in [
        Instruction::LocalGet(object),
        Instruction::RefCastNonNull(HeapType::Concrete(intrinsics.object_shape)),
        Instruction::StructGet {
            struct_type_index: intrinsics.object_shape,
            field_index: 1,
        },
        Instruction::I32Const(index as i32),
        Instruction::ArrayGet(intrinsics.field_names),
        Instruction::RefCastNonNull(HeapType::Concrete(optional_name_type)),
        Instruction::StructGet {
            struct_type_index: optional_name_type,
            field_index: 2,
        },
        Instruction::LocalGet(value),
        Instruction::RefIsNull,
        Instruction::I32Eqz,
        Instruction::I32Or,
    ] {
        function.instruction(&instruction);
    }
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
