//! Per-module canonical-shape field-names registry.

use std::collections::BTreeSet;

use wasm_encoder::{
    ConstExpr, GlobalSection, GlobalType, HeapType, Instruction, RefType, StorageType, ValType,
};

use crate::Type;
use crate::codegen::intrinsics::IntrinsicTypeIndices;
use crate::codegen::symbol_table::SymbolTable;
use crate::codegen::{internal_failure, next_index, wasm_u32};
use crate::compiler_error::CompilerFailure;

/// Optional field names use a nominal string subtype. The name's text stays
/// unchanged, while `in` can recover optionality after receiver-type erasure.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FieldName {
    pub name: String,
    pub optional: bool,
    pub is_accessor: bool,
    pub is_private: bool,
}

pub fn declare_optional_name_type(
    types: &mut wasm_encoder::TypeSection,
    symbols: &mut SymbolTable,
    next_type_idx: &mut u32,
    intrinsics: IntrinsicTypeIndices,
) -> Result<(), CompilerFailure> {
    let type_idx = next_index(next_type_idx)?;
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
    fields.push(wasm_encoder::FieldType {
        element_type: StorageType::Val(ValType::I32),
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
    symbols.record_optional_field_name_type(type_idx);
    Ok(())
}

pub fn collect(shapes: &[Type]) -> Vec<Vec<FieldName>> {
    let mut keys: BTreeSet<Vec<FieldName>> = BTreeSet::new();
    for shape in shapes {
        if let Type::Object { fields, .. } = shape {
            keys.insert(
                fields
                    .iter()
                    .map(|(name, field)| FieldName {
                        name: name.clone(),
                        optional: field.optional,
                        is_accessor: false,
                        is_private: false,
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
) -> Result<(), CompilerFailure> {
    let field_names_val_type = ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.field_names),
    });
    let optional_name_type = symbols.optional_field_name_type()?;
    for shape in shapes {
        let init = build_init_expr(
            shape,
            intrinsics,
            string_vtable_global_idx,
            optional_name_type,
        )?;
        let global_idx = next_index(next_global_idx)?;
        globals.global(
            GlobalType {
                val_type: field_names_val_type,
                mutable: false,
                shared: false,
            },
            &init,
        );
        symbols.record_field_names_global(shape.clone(), global_idx);
    }
    Ok(())
}

fn build_init_expr(
    shape: &[FieldName],
    intrinsics: IntrinsicTypeIndices,
    string_vtable_global_idx: u32,
    optional_name_type: u32,
) -> Result<ConstExpr, CompilerFailure> {
    let mut instrs: Vec<Instruction<'_>> = Vec::new();
    for name in shape {
        let code_units: Vec<u16> = name.name.encode_utf16().collect();
        instrs.push(Instruction::GlobalGet(string_vtable_global_idx));
        for unit in &code_units {
            instrs.push(Instruction::I32Const(i32::from(*unit)));
        }
        instrs.push(Instruction::ArrayNewFixed {
            array_type_index: intrinsics.raw_string,
            array_size: wasm_u32(code_units.len())?,
        });
        if name.optional || name.is_accessor || name.is_private {
            instrs.push(Instruction::I32Const(if name.is_accessor {
                -1
            } else if name.optional {
                0
            } else {
                1
            }));
            instrs.push(Instruction::I32Const(i32::from(name.is_private)));
        }
        instrs.push(Instruction::StructNew(
            if name.optional || name.is_accessor || name.is_private {
                optional_name_type
            } else {
                intrinsics.string
            },
        ));
    }
    instrs.push(Instruction::ArrayNewFixed {
        array_type_index: intrinsics.field_names,
        array_size: wasm_u32(shape.len())?,
    });
    Ok(ConstExpr::extended(instrs))
}

/// Optional names carry per-instance presence separately from the value slot.
/// Required names remain shared immutable strings.
pub(crate) fn emit_instance_names(
    emitter: &mut super::function_emitter::FunctionEmitter,
    ctx: &super::CodegenCtx,
    names: &[FieldName],
    present: impl Fn(&str) -> bool,
) -> Result<(), CompilerFailure> {
    let global = ctx
        .symbols
        .field_names_global_idx(names)
        .ok_or_else(|| internal_failure("object field names were not collected"))?;
    if !names.iter().any(|name| name.optional) {
        emitter.instruction(Instruction::GlobalGet(global));
        return Ok(());
    }
    let intrinsics = intrinsics(ctx)?;
    let optional_name_type = ctx.symbols.optional_field_name_type()?;
    for (index, name) in names.iter().enumerate() {
        let index = wasm_u32(index)?.cast_signed();
        if name.optional {
            for field_index in [0, 1] {
                emitter.instruction(Instruction::GlobalGet(global));
                emitter.instruction(Instruction::I32Const(index));
                emitter.instruction(Instruction::ArrayGet(intrinsics.field_names));
                emitter.instruction(Instruction::StructGet {
                    struct_type_index: intrinsics.string,
                    field_index,
                });
            }
            emitter.instruction(Instruction::I32Const(i32::from(present(&name.name))));
            emitter.instruction(Instruction::I32Const(i32::from(name.is_private)));
            emitter.instruction(Instruction::StructNew(optional_name_type));
        } else {
            emitter.instruction(Instruction::GlobalGet(global));
            emitter.instruction(Instruction::I32Const(index));
            emitter.instruction(Instruction::ArrayGet(intrinsics.field_names));
        }
    }
    emitter.instruction(Instruction::ArrayNewFixed {
        array_type_index: intrinsics.field_names,
        array_size: wasm_u32(names.len())?,
    });
    Ok(())
}

/// Push whether the name on the stack marks an internal accessor payload slot.
pub(crate) fn emit_name_is_accessor(
    emitter: &mut super::function_emitter::FunctionEmitter,
    ctx: &super::CodegenCtx,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let name = emitter.add_anonymous_local(ctx.symbols.value_type(&Type::String)?)?;
    let marked = ctx.symbols.optional_field_name_type()?;
    emitter.instruction(Instruction::LocalTee(name));
    emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(marked)));
    emitter.emit_if(wasm_encoder::BlockType::Result(ValType::I32));
    emitter.instruction(Instruction::LocalGet(name));
    emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(marked)));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: marked,
        field_index: 2,
    });
    emitter.instruction(Instruction::I32Const(-1));
    emitter.instruction(Instruction::I32Eq);
    emitter.emit_else();
    emitter.instruction(Instruction::I32Const(0));
    emitter.emit_end();

    Ok(())
}

/// Push the presence flag for a field name already on the stack.
pub(crate) fn emit_name_presence(
    emitter: &mut super::function_emitter::FunctionEmitter,
    ctx: &super::CodegenCtx,
) -> Result<(), CompilerFailure> {
    let intrinsics = intrinsics(ctx)?;
    let optional = ctx.symbols.optional_field_name_type()?;
    let name = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.string),
    }))?;
    emitter.instruction(Instruction::LocalTee(name));
    emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(optional)));
    emitter.emit_if(wasm_encoder::BlockType::Result(ValType::I32));
    emitter.instruction(Instruction::LocalGet(name));
    emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(optional)));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: optional,
        field_index: 2,
    });
    emitter.instruction(Instruction::I32Eqz);
    emitter.instruction(Instruction::I32Eqz);
    emitter.emit_else();
    emitter.instruction(Instruction::I32Const(1));
    emitter.emit_end();
    Ok(())
}

/// The object and index are locals; callers mark a successful store as present.
pub(crate) fn emit_mark_present(
    emitter: &mut super::function_emitter::FunctionEmitter,
    ctx: &super::CodegenCtx,
    object: u32,
    index: u32,
) -> Result<(), CompilerFailure> {
    let intrinsics = intrinsics(ctx)?;
    let optional = ctx.symbols.optional_field_name_type()?;
    let name = emitter.add_anonymous_local(ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.string),
    }))?;
    emitter.instruction(Instruction::LocalGet(object));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: intrinsics.object_shape,
        field_index: 1,
    });
    emitter.instruction(Instruction::LocalGet(index));
    emitter.instruction(Instruction::ArrayGet(intrinsics.field_names));
    emitter.instruction(Instruction::LocalTee(name));
    emitter.instruction(Instruction::RefTestNonNull(HeapType::Concrete(optional)));
    emitter.emit_if(wasm_encoder::BlockType::Empty);
    emitter.instruction(Instruction::LocalGet(name));
    emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(optional)));
    emitter.instruction(Instruction::StructGet {
        struct_type_index: optional,
        field_index: 2,
    });
    emitter.instruction(Instruction::I32Eqz);
    emitter.emit_if(wasm_encoder::BlockType::Empty);
    emitter.instruction(Instruction::LocalGet(name));
    emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(optional)));
    emitter.instruction(Instruction::I32Const(1));
    emitter.instruction(Instruction::StructSet {
        struct_type_index: optional,
        field_index: 2,
    });
    emitter.emit_end();
    emitter.emit_end();
    Ok(())
}

fn intrinsics(ctx: &super::CodegenCtx) -> Result<IntrinsicTypeIndices, CompilerFailure> {
    ctx.symbols
        .intrinsic_type_indices()
        .ok_or_else(|| internal_failure("intrinsic types are not declared"))
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
        Instruction::I32Const(index.cast_signed()),
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
        let keys = collect(&shapes);
        assert_eq!(keys.len(), 1);
        assert_eq!(
            keys[0],
            vec![FieldName {
                name: "x".to_string(),
                optional: false,
                is_accessor: false,
                is_private: false,
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
        let keys = collect(&shapes);
        assert_eq!(keys.len(), 2);
        assert_eq!(
            keys[0],
            vec![FieldName {
                name: "x".to_string(),
                optional: false,
                is_accessor: false,
                is_private: false,
            }]
        );
        assert_eq!(
            keys[1],
            vec![FieldName {
                name: "y".to_string(),
                optional: false,
                is_accessor: false,
                is_private: false,
            }]
        );
    }
}
