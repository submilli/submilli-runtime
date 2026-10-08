//! Per-user-subtype Wasm subtype, vtable, and method emission for structural object shapes.

use std::collections::BTreeSet;

use wasm_encoder::{
    BlockType, ConstExpr, Function, GlobalSection, GlobalType, HeapType, Instruction, RefType,
    ValType,
};

use crate::codegen::intrinsics::IntrinsicTypeIndices;
use crate::codegen::symbol_table::SymbolTable;
use crate::codegen::{GuardedBodies, internal_failure, next_index, wasm_u32};
use crate::compiler_error::CompilerFailure;
use crate::{ObjectField, Shape, Type, TypeInfoIndex, TypeInfoTable};

pub fn collect_object_shapes<'a>(
    dependency_shapes: impl IntoIterator<Item = &'a Shape>,
    own_shapes: &[Shape],
) -> Vec<Type> {
    // Dependency entries are walked first so they win the dedup race against own-side duplicates.
    let mut entries: BTreeSet<Type> = BTreeSet::new();
    for shape in dependency_shapes {
        entries.insert(canonical_type(shape));
    }
    for shape in own_shapes {
        entries.insert(canonical_type(shape));
    }

    // Topo sort: each Shape's deps (Object → field types; Array →
    // element type) must precede it. Post-order DFS gives us that.
    let mut sorted: Vec<Type> = Vec::new();
    let mut visited: BTreeSet<Type> = BTreeSet::new();
    let keys: Vec<Type> = entries.iter().cloned().collect();
    for ty in &keys {
        topo_visit_kind(ty, &entries, &mut visited, &mut sorted);
    }
    sorted
        .into_iter()
        .filter(|ty| matches!(ty, Type::Object { .. }))
        .collect()
}

fn canonical_type(shape: &Shape) -> Type {
    match shape {
        Shape::Object { fields, index } => Type::Object {
            index: index.clone(),
            fields: fields.clone(),
        },
        Shape::Array(elem) => Type::Array(elem.clone()),
        Shape::Tuple(elements) => Type::Tuple(elements.clone()),
        // No discriminator struct — runtime distinguishes union members via subtype identity.
        Shape::Union(members) => Type::Union(members.clone()),
    }
}

fn topo_visit_kind(
    ty: &Type,
    entries: &BTreeSet<Type>,
    visited: &mut BTreeSet<Type>,
    sorted: &mut Vec<Type>,
) {
    if visited.contains(ty) {
        return;
    }
    let Some(ty) = entries.get(ty) else {
        // Primitive types (Number, String, etc.) have no entry; value_type lowers them directly.
        return;
    };
    visited.insert(ty.clone());
    // Peel so aliased child types (`type Inner = { … }`) hit the entries map lookup.
    match ty {
        Type::Object { fields, .. } => {
            for inner in fields.values() {
                topo_visit_kind(inner.ty.peel(), entries, visited, sorted);
            }
        }
        Type::Array(elem) => {
            topo_visit_kind(elem.peel(), entries, visited, sorted);
        }
        Type::Tuple(elements) => {
            for inner in elements {
                topo_visit_kind(inner.peel(), entries, visited, sorted);
            }
        }
        Type::Union(members) => {
            for m in members {
                topo_visit_kind(m.peel(), entries, visited, sorted);
            }
        }
        _ => {}
    }
    sorted.push(ty.clone());
}

/// Emitted artifacts for one user subtype. The Wasm struct type is shared by arity;
/// the vtable global is shape-specialized and carries the shape-specific behavior.
#[derive(Clone, Debug)]
pub struct UserSubtype {
    pub ty: Type,
    pub to_string_func: u32,
    pub to_json_func: u32,
    pub equals_func: u32,
    pub hash_func: u32,
    pub guarded_bodies: GuardedBodies,
    pub vtable_global_idx: u32,
}

impl UserSubtype {
    fn fields(&self) -> Result<&std::collections::BTreeMap<String, ObjectField>, CompilerFailure> {
        match &self.ty {
            Type::Object { fields, .. } => Ok(fields),
            _ => Err(internal_failure(
                "a structural subtype was allocated for a non-object type",
            )),
        }
    }
}

/// Pre-allocate method indices so vtable globals can reference them via `ref.func`.
pub fn allocate_methods(
    ty: &Type,
    next_func_idx: &mut u32,
) -> Result<UserSubtype, CompilerFailure> {
    Ok(UserSubtype {
        ty: ty.clone(),
        to_string_func: next_index(next_func_idx)?,
        to_json_func: next_index(next_func_idx)?,
        equals_func: next_index(next_func_idx)?,
        hash_func: next_index(next_func_idx)?,
        // Field order is allocation order, which the body emission follows.
        guarded_bodies: GuardedBodies {
            to_json: next_index(next_func_idx)?,
            equals: next_index(next_func_idx)?,
            hash: next_index(next_func_idx)?,
        },
        vtable_global_idx: 0, // filled in by emit_vtable_globals
    })
}

pub fn needs_host_object_to_json_adapter(types: &[Type]) -> bool {
    types.iter().any(|ty| match ty.peel() {
        Type::Object { fields, .. } => !fields.contains_key("toJson"),
        _ => false,
    })
}

/// Must be called after all other function section entries so the pre-allocated indices match.
pub fn emit_method_function_entries(
    functions: &mut wasm_encoder::FunctionSection,
    subtypes: &[UserSubtype],
    intrinsics: IntrinsicTypeIndices,
) {
    for _ in subtypes {
        functions.function(intrinsics.to_string_fn);
        functions.function(intrinsics.to_json_fn);
        functions.function(intrinsics.equals_fn);
        functions.function(intrinsics.hash_fn);
        functions.function(intrinsics.to_json_fn);
        functions.function(intrinsics.equals_fn);
        functions.function(intrinsics.hash_fn);
    }
}

pub fn emit_vtable_globals(
    globals: &mut GlobalSection,
    subtypes: &mut [UserSubtype],
    symbols: &mut SymbolTable,
    next_global_idx: &mut u32,
    intrinsics: IntrinsicTypeIndices,
) -> Result<(), CompilerFailure> {
    for subtype in subtypes.iter_mut() {
        let init = ConstExpr::extended([
            Instruction::RefFunc(subtype.to_string_func),
            Instruction::RefFunc(subtype.to_json_func),
            Instruction::RefFunc(subtype.equals_func),
            Instruction::RefFunc(subtype.hash_func),
            Instruction::StructNew(intrinsics.vtable),
        ]);
        globals.global(
            GlobalType {
                val_type: ref_to(intrinsics.vtable),
                mutable: false,
                shared: false,
            },
            &init,
        );
        let idx = next_index(next_global_idx)?;
        subtype.vtable_global_idx = idx;
        symbols.record_vtable_global(subtype.ty.clone(), idx);
    }
    Ok(())
}

pub fn emit_method_bodies(
    code: &mut wasm_encoder::CodeSection,
    subtypes: &[UserSubtype],
    symbols: &SymbolTable,
    type_info: &TypeInfoTable,
    type_info_index: &TypeInfoIndex,
    pkg_string_global_idx: Option<u32>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let intrinsics = symbols
        .intrinsic_type_indices()
        .ok_or_else(|| internal_failure("intrinsic types are not declared"))?;
    let string_vtable_global_idx = symbols
        .prelude_global_idx("string_vtable")
        .ok_or_else(|| internal_failure("string_vtable is not imported from the prelude"))?;
    let string_concat_func_idx = symbols
        .prelude_func_idx("string_concat")
        .ok_or_else(|| internal_failure("string_concat is not imported from the prelude"))?;
    let string_eq_func_idx = symbols
        .prelude_func_idx("string_eq")
        .ok_or_else(|| internal_failure("string_eq is not imported from the prelude"))?;

    let object_vtable_global = symbols
        .prelude_global_idx("object_vtable")
        .ok_or_else(|| internal_failure("object_vtable is not imported from the prelude"))?;
    for subtype in subtypes {
        code.function(&emit_subtype_to_string_body(
            subtype,
            intrinsics,
            symbols,
            string_vtable_global_idx,
        )?);

        let bodies = subtype.guarded_bodies;
        for (body, params, result) in [
            (bodies.to_json, 1, ref_to(intrinsics.string)),
            (bodies.equals, 2, ValType::I32),
            (bodies.hash, 1, ValType::I32),
        ] {
            code.function(&super::vtable_walk::guarded_body(
                body, params, result, symbols,
            )?);
        }
        // The host serializes an object whose shape has type info; others walk
        // their vtable.
        let host_json_type = type_info_index
            .object_type_id(type_info, &subtype.ty)
            .filter(|_| type_info.supports_host_json_object(&subtype.ty));
        code.function(&emit_subtype_to_json_body(
            subtype,
            intrinsics,
            symbols,
            host_json_type,
            pkg_string_global_idx,
            string_concat_func_idx,
            string_vtable_global_idx,
        )?);

        code.function(&emit_subtype_equals_body(
            subtype,
            intrinsics,
            symbols,
            string_eq_func_idx,
            object_vtable_global,
        )?);

        code.function(&emit_subtype_hash_body(
            subtype,
            intrinsics,
            symbols,
            object_vtable_global,
        )?);
    }
    Ok(())
}

fn emit_subtype_to_string_body(
    subtype: &UserSubtype,
    intrinsics: IntrinsicTypeIndices,
    symbols: &SymbolTable,
    string_vtable_global_idx: u32,
) -> Result<Function, CompilerFailure> {
    let fields = subtype.fields()?;

    if !fields.contains_key("toString") {
        let mut f = Function::new(std::iter::empty());
        crate::codegen::intrinsics::push_string_literal(
            &mut f,
            intrinsics,
            string_vtable_global_idx,
            "[object Object]",
        )?;
        f.instruction(&Instruction::End);
        return Ok(f);
    }

    // The typechecker guarantees a `toString` field is `() => string`. It may be
    // optional, and an absent one falls back to `[object Object]`, as in
    // JavaScript, where the property lookup reaches `Object.prototype`.
    let to_string_sig = crate::codegen::closures::ClosureSig {
        arity: 0,
        is_void: false,
    };
    let closure_struct_idx = symbols
        .closure_struct_type_idx(to_string_sig)
        .ok_or_else(|| {
            crate::codegen::internal_failure(
                "closure struct for `() => string` registered when any shape declares a toString \
         override (collect_from_dependencies walks every shape field type)",
            )
        })?;
    let closure_func_type_idx = symbols
        .closure_func_type_idx(to_string_sig)
        .ok_or_else(|| {
            crate::codegen::internal_failure(
                "closure funcref type for `() => string` registered alongside its struct",
            )
        })?;

    let object_shape_ref = ref_to(intrinsics.object_shape);
    let closure_ref = ref_to(closure_struct_idx);

    // Locals (after 1 param: self=0):
    //   1: self_t  (ref $object_shape)
    //   2: closure (ref $Closure_string)
    let locals: Vec<(u32, ValType)> = vec![(1, object_shape_ref), (1, closure_ref)];
    let mut f = Function::new(locals);
    let self_t = 1u32;
    let closure = 2u32;

    f.instruction(&Instruction::LocalGet(0));
    f.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(
        intrinsics.object_shape,
    )));
    f.instruction(&Instruction::LocalSet(self_t));

    // The outer block yields the result string; the inner one is left for the
    // `[object Object]` fallback below it when the field slot is null.
    f.instruction(&Instruction::Block(BlockType::Result(ref_to(
        intrinsics.string,
    ))));
    f.instruction(&Instruction::Block(BlockType::Empty));
    // The field slot is `(ref null $Object)`; null when an optional `toString`
    // is absent.
    let to_string_slot = field_index(&subtype.ty, "toString")?
        .ok_or_else(|| internal_failure("an object shape lost its toString field slot"))?;
    f.instruction(&Instruction::LocalGet(self_t));
    f.instruction(&Instruction::StructGet {
        struct_type_index: intrinsics.object_shape,
        field_index: 2,
    });
    f.instruction(&Instruction::I32Const(to_string_slot.cast_signed()));
    f.instruction(&Instruction::ArrayGet(intrinsics.object_fields));
    f.instruction(&Instruction::BrOnNull(0));
    f.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(
        closure_struct_idx,
    )));
    f.instruction(&Instruction::LocalSet(closure));

    f.instruction(&Instruction::LocalGet(closure));
    f.instruction(&Instruction::StructGet {
        struct_type_index: closure_struct_idx,
        field_index: 2,
    });
    f.instruction(&Instruction::LocalGet(closure));
    f.instruction(&Instruction::StructGet {
        struct_type_index: closure_struct_idx,
        field_index: 1,
    });
    f.instruction(&Instruction::CallRef(closure_func_type_idx));

    // The closure returns its result boxed as `(ref $Object)`; unbox
    // to `(ref $string)` to match `$toStringFn`'s return type.
    f.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(
        intrinsics.string,
    )));
    f.instruction(&Instruction::Br(1));
    f.instruction(&Instruction::End);
    crate::codegen::intrinsics::push_string_literal(
        &mut f,
        intrinsics,
        string_vtable_global_idx,
        "[object Object]",
    )?;
    f.instruction(&Instruction::End);
    f.instruction(&Instruction::End);
    Ok(f)
}

fn emit_subtype_to_json_body(
    subtype: &UserSubtype,
    intrinsics: IntrinsicTypeIndices,
    symbols: &SymbolTable,
    host_json_type: Option<crate::TypeInfoId>,
    pkg_string_global_idx: Option<u32>,
    string_concat_func_idx: u32,
    string_vtable_global_idx: u32,
) -> Result<Function, CompilerFailure> {
    let fields = subtype.fields()?;

    if fields.contains_key("toJson") {
        return emit_subtype_to_json_override_body(subtype, intrinsics, symbols);
    }

    let Some(type_id) = host_json_type else {
        return emit_subtype_to_json_vtable_body(
            subtype,
            intrinsics,
            string_concat_func_idx,
            string_vtable_global_idx,
            symbols,
        );
    };
    let pkg_string_global_idx = pkg_string_global_idx
        .ok_or_else(|| internal_failure("host object serialization needs the package string"))?;
    let stringify_func_idx = symbols
        .func_idx(&crate::mangle::host(
            crate::runtime::JSON_MODULE_NAME,
            "stringifyTypedObject",
        ))
        .ok_or_else(|| internal_failure("submilli:json.stringifyTypedObject is not imported"))?;

    let mut f = Function::new(std::iter::empty());

    f.instruction(&Instruction::GlobalGet(string_vtable_global_idx));

    f.instruction(&Instruction::GlobalGet(pkg_string_global_idx));
    f.instruction(&Instruction::RefAsNonNull);
    f.instruction(&Instruction::StructGet {
        struct_type_index: intrinsics.string,
        field_index: 1,
    });
    f.instruction(&Instruction::I32Const(type_id.as_u32().cast_signed()));
    f.instruction(&Instruction::LocalGet(0));
    f.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(
        intrinsics.object_shape,
    )));
    f.instruction(&Instruction::Call(stringify_func_idx));
    f.instruction(&Instruction::I64Const(0));
    f.instruction(&Instruction::StructNew(intrinsics.string));
    f.instruction(&Instruction::End);
    Ok(f)
}

/// Static serializers fall back to the dynamic walker after shape growth.
pub(super) fn emit_grown_object_to_json(
    function: &mut Function,
    intrinsics: IntrinsicTypeIndices,
    symbols: &SymbolTable,
    original_len: u32,
) -> Result<(), CompilerFailure> {
    let dynamic_serializer = symbols
        .prelude_func_idx("ObjectConstructor##toJson")
        .ok_or_else(|| internal_failure("the dynamic object serializer is not imported"))?;
    function.instruction(&Instruction::LocalGet(0));
    function.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(
        intrinsics.object_shape,
    )));
    function.instruction(&Instruction::StructGet {
        struct_type_index: intrinsics.object_shape,
        field_index: 1,
    });
    function.instruction(&Instruction::ArrayLen);
    function.instruction(&Instruction::I32Const(original_len.cast_signed()));
    function.instruction(&Instruction::I32GtU);
    function.instruction(&Instruction::If(BlockType::Empty));
    function.instruction(&Instruction::LocalGet(0));
    function.instruction(&Instruction::Call(dynamic_serializer));
    function.instruction(&Instruction::Return);
    function.instruction(&Instruction::End);
    Ok(())
}

fn emit_subtype_to_json_vtable_body(
    subtype: &UserSubtype,
    intrinsics: IntrinsicTypeIndices,
    string_concat_func_idx: u32,
    string_vtable_global_idx: u32,
    symbols: &SymbolTable,
) -> Result<Function, CompilerFailure> {
    let fields = subtype.fields()?;

    let object_shape_ref = ref_to(intrinsics.object_shape);
    let string_ref = ref_to(intrinsics.string);
    let object_null_ref = ref_null(intrinsics.object);
    let to_json_fn_ref = ref_to(intrinsics.to_json_fn);

    let locals: Vec<(u32, ValType)> = vec![
        (1, object_shape_ref),
        (1, string_ref),
        (1, object_null_ref),
        (1, to_json_fn_ref),
        (1, ValType::I32), // Whether any serializable field has been emitted.
    ];
    let mut f = Function::new(locals);
    emit_grown_object_to_json(&mut f, intrinsics, symbols, wasm_u32(fields.len())?)?;
    let self_t = 1u32;
    let acc = 2u32;
    let elem = 3u32;
    let tj_fn = 4u32;
    let first_local = 5u32;

    f.instruction(&Instruction::LocalGet(0));
    f.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(
        intrinsics.object_shape,
    )));
    f.instruction(&Instruction::LocalSet(self_t));

    if fields.is_empty() {
        push_inline_string(
            &mut f,
            "{}",
            intrinsics.string,
            intrinsics.raw_string,
            string_vtable_global_idx,
        )?;
        f.instruction(&Instruction::End);
        return Ok(f);
    }

    push_inline_string(
        &mut f,
        "{",
        intrinsics.string,
        intrinsics.raw_string,
        string_vtable_global_idx,
    )?;
    f.instruction(&Instruction::LocalSet(acc));

    f.instruction(&Instruction::I32Const(1));
    f.instruction(&Instruction::LocalSet(first_local));

    for (idx, (field_name, field)) in fields.iter().enumerate() {
        let idx = wasm_u32(idx)?;
        let key_no_comma = format!("\"{}\":", json_escape_key(field_name));
        let key_with_comma = format!(",\"{}\":", json_escape_key(field_name));

        f.instruction(&Instruction::LocalGet(self_t));
        f.instruction(&Instruction::StructGet {
            struct_type_index: intrinsics.object_shape,
            field_index: 2,
        });
        f.instruction(&Instruction::I32Const(idx.cast_signed()));
        f.instruction(&Instruction::ArrayGet(intrinsics.object_fields));
        f.instruction(&Instruction::LocalSet(elem));

        f.instruction(&Instruction::LocalGet(elem));
        f.instruction(&Instruction::RefTestNonNull(HeapType::Concrete(
            intrinsics.closure,
        )));
        f.instruction(&Instruction::I32Eqz);
        f.instruction(&Instruction::If(BlockType::Empty));

        if field.optional {
            super::field_names::emit_optional_presence(
                &mut f,
                intrinsics,
                symbols.optional_field_name_type()?,
                self_t,
                idx,
                elem,
            );
            f.instruction(&Instruction::If(BlockType::Empty));
        }

        f.instruction(&Instruction::LocalGet(acc));
        f.instruction(&Instruction::LocalGet(first_local));
        f.instruction(&Instruction::If(BlockType::Result(string_ref)));
        push_inline_string(
            &mut f,
            &key_no_comma,
            intrinsics.string,
            intrinsics.raw_string,
            string_vtable_global_idx,
        )?;
        f.instruction(&Instruction::Else);
        push_inline_string(
            &mut f,
            &key_with_comma,
            intrinsics.string,
            intrinsics.raw_string,
            string_vtable_global_idx,
        )?;
        f.instruction(&Instruction::End);
        f.instruction(&Instruction::Call(string_concat_func_idx));
        f.instruction(&Instruction::LocalSet(acc));

        // Every field is null-checked: the shape's field type is the object
        // literal's own, and a binding with a wider type (`{ v: number | null }`
        // holding `{ v: 1 }`) can later store `null` in it.
        f.instruction(&Instruction::LocalGet(acc));
        f.instruction(&Instruction::LocalGet(elem));
        f.instruction(&Instruction::RefIsNull);
        f.instruction(&Instruction::If(BlockType::Result(string_ref)));
        push_inline_string(
            &mut f,
            "null",
            intrinsics.string,
            intrinsics.raw_string,
            string_vtable_global_idx,
        )?;
        f.instruction(&Instruction::Else);
        emit_field_value_to_json(&mut f, elem, tj_fn, intrinsics);
        f.instruction(&Instruction::End);
        f.instruction(&Instruction::Call(string_concat_func_idx));
        f.instruction(&Instruction::LocalSet(acc));

        f.instruction(&Instruction::I32Const(0));
        f.instruction(&Instruction::LocalSet(first_local));

        if field.optional {
            f.instruction(&Instruction::End);
        }
        f.instruction(&Instruction::End);
    }

    f.instruction(&Instruction::LocalGet(acc));
    push_inline_string(
        &mut f,
        "}",
        intrinsics.string,
        intrinsics.raw_string,
        string_vtable_global_idx,
    )?;
    f.instruction(&Instruction::Call(string_concat_func_idx));
    f.instruction(&Instruction::End);
    Ok(f)
}

fn emit_subtype_to_json_override_body(
    subtype: &UserSubtype,
    intrinsics: IntrinsicTypeIndices,
    symbols: &SymbolTable,
) -> Result<Function, CompilerFailure> {
    let to_json_sig = crate::codegen::closures::ClosureSig {
        arity: 0,
        is_void: false,
    };
    let closure_struct_idx = symbols
        .closure_struct_type_idx(to_json_sig)
        .ok_or_else(|| {
            crate::codegen::internal_failure(
                "closure struct for `() => string` registered when any shape declares a toJson \
         override (collect_from_dependencies walks every shape field type)",
            )
        })?;
    let closure_func_type_idx = symbols.closure_func_type_idx(to_json_sig).ok_or_else(|| {
        crate::codegen::internal_failure(
            "closure funcref type for `() => string` registered alongside its struct",
        )
    })?;

    let object_shape_ref = ref_to(intrinsics.object_shape);
    let closure_ref = ref_to(closure_struct_idx);

    // Locals (after 1 param: self=0):
    //   1: self_t  (ref $object_shape)
    //   2: closure (ref $Closure_string)
    let locals: Vec<(u32, ValType)> = vec![(1, object_shape_ref), (1, closure_ref)];
    let mut f = Function::new(locals);
    let self_t = 1u32;
    let closure = 2u32;

    f.instruction(&Instruction::LocalGet(0));
    f.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(
        intrinsics.object_shape,
    )));
    f.instruction(&Instruction::LocalSet(self_t));

    // The typechecker guarantees `toJson` is non-optional `() => string`.
    let to_json_slot = field_index(&subtype.ty, "toJson")?
        .ok_or_else(|| internal_failure("an object shape lost its toJson field slot"))?;
    f.instruction(&Instruction::LocalGet(self_t));
    f.instruction(&Instruction::StructGet {
        struct_type_index: intrinsics.object_shape,
        field_index: 2,
    });
    f.instruction(&Instruction::I32Const(to_json_slot.cast_signed()));
    f.instruction(&Instruction::ArrayGet(intrinsics.object_fields));
    f.instruction(&Instruction::RefAsNonNull);
    f.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(
        closure_struct_idx,
    )));
    f.instruction(&Instruction::LocalSet(closure));

    f.instruction(&Instruction::LocalGet(closure));
    f.instruction(&Instruction::StructGet {
        struct_type_index: closure_struct_idx,
        field_index: 2,
    });
    f.instruction(&Instruction::LocalGet(closure));
    f.instruction(&Instruction::StructGet {
        struct_type_index: closure_struct_idx,
        field_index: 1,
    });
    f.instruction(&Instruction::CallRef(closure_func_type_idx));
    f.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(
        intrinsics.string,
    )));
    f.instruction(&Instruction::End);
    Ok(f)
}

/// The index records whether insertion changed the shape. Such values use the
/// same dynamic equality/hash hooks as objects created by JSON.parse.
fn emit_indexed_object_dispatch(
    body: &mut Function,
    intrinsics: IntrinsicTypeIndices,
    vtable_global: u32,
    slot: u32,
) {
    let operands = if slot == 2 { 2 } else { 1 };
    for operand in 0..operands {
        body.instruction(&Instruction::LocalGet(operand));
        body.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(
            intrinsics.object_shape,
        )));
        body.instruction(&Instruction::StructGet {
            struct_type_index: intrinsics.object_shape,
            field_index: 3,
        });
        body.instruction(&Instruction::RefIsNull);
        body.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
        body.instruction(&Instruction::I32Const(0));
        body.instruction(&Instruction::Else);
        body.instruction(&Instruction::LocalGet(operand));
        body.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(
            intrinsics.object_shape,
        )));
        body.instruction(&Instruction::StructGet {
            struct_type_index: intrinsics.object_shape,
            field_index: 3,
        });
        body.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(
            intrinsics.raw_index_array,
        )));
        body.instruction(&Instruction::I32Const(1));
        body.instruction(&Instruction::ArrayGet(intrinsics.raw_index_array));
        body.instruction(&Instruction::End);
    }
    if operands == 2 {
        body.instruction(&Instruction::I32Or);
    }
    body.instruction(&Instruction::If(BlockType::Empty));
    for operand in 0..operands {
        body.instruction(&Instruction::LocalGet(operand));
    }
    body.instruction(&Instruction::GlobalGet(vtable_global));
    body.instruction(&Instruction::StructGet {
        struct_type_index: intrinsics.vtable,
        field_index: slot,
    });
    body.instruction(&Instruction::CallRef(if slot == 2 {
        intrinsics.equals_fn
    } else {
        intrinsics.hash_fn
    }));
    body.instruction(&Instruction::Return);
    body.instruction(&Instruction::End);
}

fn emit_subtype_equals_body(
    subtype: &UserSubtype,
    intrinsics: IntrinsicTypeIndices,
    symbols: &SymbolTable,
    string_eq_func_idx: u32,
    object_vtable_global: u32,
) -> Result<Function, CompilerFailure> {
    let fields = subtype.fields()?;

    let object_shape_ref = ref_to(intrinsics.object_shape);
    let object_ref = ref_to(intrinsics.object);
    let object_null_ref = ref_null(intrinsics.object);
    let equals_fn_ref = ref_to(intrinsics.equals_fn);

    // Locals layout (after 2 params):
    //   2: $a_t            (ref $arity)
    //   3: $b_t            (ref $arity)
    //   4: $field_lhs      (ref $Object)
    //   5: $eq_fn          (ref $equalsFn)
    //   6: $field_lhs_null (ref null $Object)
    //   7: $field_rhs_null (ref null $Object)
    let locals: Vec<(u32, ValType)> = vec![
        (2, object_shape_ref),
        (1, object_ref),
        (1, equals_fn_ref),
        (2, object_null_ref),
    ];
    let mut f = Function::new(locals);
    let (a, b) = (0u32, 1u32);
    let (a_t, b_t) = (2u32, 3u32);
    let field_lhs = 4u32;
    let eq_fn = 5u32;
    let field_lhs_null = 6u32;
    let field_rhs_null = 7u32;

    // Dispatched off a union value, `other` may not be a same-arity object; a
    // mismatched runtime shape is unequal, not a cast trap.
    emit_equals_type_guard(&mut f, intrinsics.object_shape);

    // Nominal values (class instances, `$Error`) are `$ObjectShape` subtypes
    // too, but never equal structural values — their own equals bodies guard
    // by vtable identity, and this direction must agree. Rejecting them here
    // also prevents an out-of-bounds payload read when the class payload is
    // shorter than this shape.
    f.instruction(&Instruction::LocalGet(1));
    f.instruction(&Instruction::StructGet {
        struct_type_index: intrinsics.object,
        field_index: 0,
    });
    f.instruction(&Instruction::RefTestNonNull(HeapType::Concrete(
        intrinsics.class_vtable,
    )));
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::Return);
    f.instruction(&Instruction::End);

    f.instruction(&Instruction::LocalGet(a));
    f.instruction(&Instruction::LocalGet(b));
    f.instruction(&Instruction::RefEq);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::I32Const(1));
    f.instruction(&Instruction::Return);
    f.instruction(&Instruction::End);

    f.instruction(&Instruction::LocalGet(a));
    f.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(
        intrinsics.object_shape,
    )));
    f.instruction(&Instruction::LocalSet(a_t));
    f.instruction(&Instruction::LocalGet(b));
    f.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(
        intrinsics.object_shape,
    )));
    f.instruction(&Instruction::LocalSet(b_t));

    emit_indexed_object_dispatch(&mut f, intrinsics, object_vtable_global, 2);

    emit_shape_guard(
        &mut f,
        wasm_u32(fields.len())?,
        intrinsics,
        string_eq_func_idx,
        (a_t, b_t),
    );

    for (slot, field) in fields.values().enumerate() {
        let field_index = wasm_u32(slot)?;

        for object in [a_t, b_t] {
            emit_field_presence(
                &mut f,
                intrinsics,
                symbols.optional_field_name_type()?,
                object,
                field_index,
            );
        }
        f.instruction(&Instruction::I32Ne);
        f.instruction(&Instruction::If(BlockType::Empty));
        f.instruction(&Instruction::I32Const(0));
        f.instruction(&Instruction::Return);
        f.instruction(&Instruction::End);

        emit_field_compare(
            &mut f,
            intrinsics.object_shape,
            field_index,
            &field.ty,
            intrinsics,
            (a_t, b_t, field_lhs, eq_fn, field_lhs_null, field_rhs_null),
        )?;

        f.instruction(&Instruction::I32Eqz);
        f.instruction(&Instruction::If(BlockType::Empty));
        f.instruction(&Instruction::I32Const(0));
        f.instruction(&Instruction::Return);
        f.instruction(&Instruction::End);
    }

    f.instruction(&Instruction::I32Const(1));
    f.instruction(&Instruction::End);
    Ok(f)
}

/// Presence belongs to the field-name marker, while a non-null payload also
/// implies presence. The other operand may use an ordinary required name.
fn emit_field_presence(
    body: &mut Function,
    intrinsics: IntrinsicTypeIndices,
    optional_name_type: u32,
    object: u32,
    slot: u32,
) {
    let load_name = |body: &mut Function| {
        body.instruction(&Instruction::LocalGet(object));
        body.instruction(&Instruction::StructGet {
            struct_type_index: intrinsics.object_shape,
            field_index: 1,
        });
        body.instruction(&Instruction::I32Const(slot.cast_signed()));
        body.instruction(&Instruction::ArrayGet(intrinsics.field_names));
    };
    load_name(body);
    body.instruction(&Instruction::RefTestNonNull(HeapType::Concrete(
        optional_name_type,
    )));
    body.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    load_name(body);
    body.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(
        optional_name_type,
    )));
    body.instruction(&Instruction::StructGet {
        struct_type_index: optional_name_type,
        field_index: 3,
    });
    body.instruction(&Instruction::Else);
    body.instruction(&Instruction::I32Const(1));
    body.instruction(&Instruction::End);
    body.instruction(&Instruction::LocalGet(object));
    body.instruction(&Instruction::StructGet {
        struct_type_index: intrinsics.object_shape,
        field_index: 2,
    });
    body.instruction(&Instruction::I32Const(slot.cast_signed()));
    body.instruction(&Instruction::ArrayGet(intrinsics.object_fields));
    body.instruction(&Instruction::RefIsNull);
    body.instruction(&Instruction::I32Eqz);
    body.instruction(&Instruction::I32Or);
}

/// Compares one field slot of `a_t` and `b_t`, pushing `1` when equal.
///
/// Every field goes through the null-aware vtable dispatch rather than the
/// shape's field type: the shape's type is the object literal's own, and a
/// binding with a wider type (`{ v: number | null }` holding `{ v: 1 }`) can
/// later store `null` or another type in the slot. The dynamic `Object#equals`
/// hook compares the same way, so a grown object agrees with this body.
fn emit_field_compare(
    f: &mut Function,
    object_shape_idx: u32,
    field_index: u32,
    field_ty: &Type,
    intrinsics: IntrinsicTypeIndices,
    locals: (u32, u32, u32, u32, u32, u32),
) -> Result<(), CompilerFailure> {
    let (a_t, b_t, field_lhs, eq_fn, field_lhs_null, field_rhs_null) = locals;
    reject_unrepresentable_field(field_ty)?;
    emit_nullable_field_compare(
        f,
        object_shape_idx,
        field_index,
        intrinsics,
        (field_lhs, eq_fn, field_lhs_null, field_rhs_null, a_t, b_t),
    );
    Ok(())
}

/// FNV-1a-32 field-by-field hash. Must use same field order and dispatch as `equals`.
fn emit_subtype_hash_body(
    subtype: &UserSubtype,
    intrinsics: IntrinsicTypeIndices,
    symbols: &SymbolTable,
    object_vtable_global: u32,
) -> Result<Function, CompilerFailure> {
    let fields = subtype.fields()?;

    let object_shape_ref = ref_to(intrinsics.object_shape);
    let object_ref = ref_to(intrinsics.object);
    let object_null_ref = ref_null(intrinsics.object);
    let hash_fn_ref = ref_to(intrinsics.hash_fn);

    // Locals (after 1 param self=0):
    //   1: self_t    (ref $arity)
    //   2: hash      (i32) — running accumulator
    //   3: field_obj (ref $Object)
    //   4: hash_fn   (ref $hashFn)
    //   5: f_null    (ref null $Object)
    let locals: Vec<(u32, ValType)> = vec![
        (1, object_shape_ref),
        (1, ValType::I32),
        (1, object_ref),
        (1, hash_fn_ref),
        (1, object_null_ref),
    ];
    let mut f = Function::new(locals);
    let self_param = 0u32;
    let self_t = 1u32;
    let hash = 2u32;
    let field_obj = 3u32;
    let hash_fn = 4u32;
    let f_null = 5u32;
    f.instruction(&Instruction::LocalGet(self_param));
    f.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(
        intrinsics.object_shape,
    )));
    f.instruction(&Instruction::LocalSet(self_t));

    emit_indexed_object_dispatch(&mut f, intrinsics, object_vtable_global, 3);

    // hash = FNV-1a offset basis
    f.instruction(&Instruction::I32Const(0x811c9dc5u32.cast_signed()));
    f.instruction(&Instruction::LocalSet(hash));

    for (slot, (name, field)) in fields.iter().enumerate() {
        let field_index = wasm_u32(slot)?;
        if field.optional {
            f.instruction(&Instruction::LocalGet(self_t));
            f.instruction(&Instruction::StructGet {
                struct_type_index: intrinsics.object_shape,
                field_index: 2,
            });
            f.instruction(&Instruction::I32Const(field_index.cast_signed()));
            f.instruction(&Instruction::ArrayGet(intrinsics.object_fields));
            f.instruction(&Instruction::LocalSet(f_null));
            super::field_names::emit_optional_presence(
                &mut f,
                intrinsics,
                symbols.optional_field_name_type()?,
                self_t,
                field_index,
                f_null,
            );
            f.instruction(&Instruction::If(BlockType::Empty));
        }
        emit_field_hash(
            &mut f,
            intrinsics.object_shape,
            field_index,
            &field.ty,
            intrinsics,
            (self_t, field_obj, hash_fn, f_null),
        )?;
        // Match the dynamic hook's order-independent name/value combination.
        let name_hash = crate::runtime::prelude::vtable::hash_utf16_units(
            crate::literal_units::literal_units(name),
        );
        f.instruction(&Instruction::I32Const(13));
        f.instruction(&Instruction::I32Rotl);
        f.instruction(&Instruction::I32Const(name_hash.cast_signed()));
        f.instruction(&Instruction::I32Xor);
        f.instruction(&Instruction::LocalGet(hash));
        f.instruction(&Instruction::I32Add);
        f.instruction(&Instruction::LocalSet(hash));
        if field.optional {
            f.instruction(&Instruction::End);
        }
    }

    f.instruction(&Instruction::LocalGet(hash));
    f.instruction(&Instruction::End);
    Ok(f)
}

/// Hashes one field slot of `self_t`, by the same dispatch as
/// [`emit_field_compare`], so equal objects hash alike.
fn emit_field_hash(
    f: &mut Function,
    object_shape_idx: u32,
    field_index: u32,
    field_ty: &Type,
    intrinsics: IntrinsicTypeIndices,
    locals: (u32, u32, u32, u32),
) -> Result<(), CompilerFailure> {
    reject_unrepresentable_field(field_ty)?;
    emit_nullable_field_hash(f, object_shape_idx, field_index, intrinsics, locals);
    Ok(())
}

/// Peeled, typechecked object fields never have these types; one reaching
/// codegen means an earlier phase broke that invariant.
fn reject_unrepresentable_field(field_ty: &Type) -> Result<(), CompilerFailure> {
    let field_ty = field_ty.peel();
    match field_ty {
        Type::Void
        | Type::Error
        | Type::Alias { .. }
        | Type::Refined { .. }
        | Type::Readonly(_) => Err(internal_failure(format!(
            "an object field of type `{field_ty}` reached structural equality or hashing"
        ))),
        _ => Ok(()),
    }
}

/// Null-aware vtable-hash dispatch for one field. Pushes a single `i32`:
/// `0` if the slot is null, otherwise the value's `vtable.hash(value)`.
fn emit_nullable_field_hash(
    f: &mut Function,
    object_shape_idx: u32,
    field_index: u32,
    intrinsics: IntrinsicTypeIndices,
    locals: (u32, u32, u32, u32),
) {
    let (self_t, field_obj, hash_fn, f_null) = locals;
    f.instruction(&Instruction::LocalGet(self_t));
    f.instruction(&Instruction::StructGet {
        struct_type_index: object_shape_idx,
        field_index: 2,
    });
    f.instruction(&Instruction::I32Const(field_index.cast_signed()));
    f.instruction(&Instruction::ArrayGet(intrinsics.object_fields));
    f.instruction(&Instruction::LocalSet(f_null));

    f.instruction(&Instruction::LocalGet(f_null));
    f.instruction(&Instruction::RefIsNull);
    f.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::Else);
    f.instruction(&Instruction::LocalGet(f_null));
    f.instruction(&Instruction::RefAsNonNull);
    f.instruction(&Instruction::LocalTee(field_obj));
    f.instruction(&Instruction::StructGet {
        struct_type_index: intrinsics.object,
        field_index: 0,
    });
    f.instruction(&Instruction::StructGet {
        struct_type_index: intrinsics.vtable,
        field_index: 3,
    });
    f.instruction(&Instruction::LocalSet(hash_fn));
    f.instruction(&Instruction::LocalGet(field_obj));
    f.instruction(&Instruction::LocalGet(hash_fn));
    f.instruction(&Instruction::CallRef(intrinsics.hash_fn));
    f.instruction(&Instruction::End);
}

/// Null-aware vtable-equals dispatch for one field. Pushes i32: 1=equal, 0=unequal.
fn emit_nullable_field_compare(
    f: &mut Function,
    object_shape_idx: u32,
    field_index: u32,
    intrinsics: IntrinsicTypeIndices,
    locals: (u32, u32, u32, u32, u32, u32),
) {
    let (field_lhs, eq_fn, field_lhs_null, field_rhs_null, a_t, b_t) = locals;
    f.instruction(&Instruction::LocalGet(a_t));
    f.instruction(&Instruction::StructGet {
        struct_type_index: object_shape_idx,
        field_index: 2,
    });
    f.instruction(&Instruction::I32Const(field_index.cast_signed()));
    f.instruction(&Instruction::ArrayGet(intrinsics.object_fields));
    f.instruction(&Instruction::LocalSet(field_lhs_null));

    f.instruction(&Instruction::LocalGet(b_t));
    f.instruction(&Instruction::StructGet {
        struct_type_index: object_shape_idx,
        field_index: 2,
    });
    f.instruction(&Instruction::I32Const(field_index.cast_signed()));
    f.instruction(&Instruction::ArrayGet(intrinsics.object_fields));
    f.instruction(&Instruction::LocalSet(field_rhs_null));

    f.instruction(&Instruction::LocalGet(field_lhs_null));
    f.instruction(&Instruction::RefIsNull);
    f.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    f.instruction(&Instruction::LocalGet(field_rhs_null));
    f.instruction(&Instruction::RefIsNull);
    f.instruction(&Instruction::Else);
    f.instruction(&Instruction::LocalGet(field_rhs_null));
    f.instruction(&Instruction::RefIsNull);
    f.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::Else);
    f.instruction(&Instruction::LocalGet(field_lhs_null));
    f.instruction(&Instruction::RefAsNonNull);
    f.instruction(&Instruction::LocalTee(field_lhs));
    f.instruction(&Instruction::StructGet {
        struct_type_index: intrinsics.object,
        field_index: 0,
    });
    f.instruction(&Instruction::StructGet {
        struct_type_index: intrinsics.vtable,
        field_index: 2,
    });
    f.instruction(&Instruction::LocalSet(eq_fn));
    f.instruction(&Instruction::LocalGet(field_lhs));
    f.instruction(&Instruction::LocalGet(field_rhs_null));
    f.instruction(&Instruction::RefAsNonNull);
    f.instruction(&Instruction::LocalGet(eq_fn));
    f.instruction(&Instruction::CallRef(intrinsics.equals_fn));
    f.instruction(&Instruction::End);
    f.instruction(&Instruction::End);
}

/// Returns all method function indices; must be declared in the element section for `ref.func` to be valid.
pub fn declared_method_funcs(subtypes: &[UserSubtype]) -> Vec<u32> {
    subtypes
        .iter()
        .flat_map(|s| [s.to_string_func, s.to_json_func, s.equals_func, s.hash_func])
        .collect()
}

/// Payload-array index for a named field.
pub fn field_index(ty: &Type, field_name: &str) -> Result<Option<u32>, CompilerFailure> {
    let Type::Object { fields, .. } = ty else {
        return Ok(None);
    };
    fields
        .keys()
        .position(|k| k == field_name)
        .map(wasm_u32)
        .transpose()
}

fn emit_field_value_to_json(
    f: &mut Function,
    elem: u32,
    tj_fn: u32,
    intrinsics: IntrinsicTypeIndices,
) {
    f.instruction(&Instruction::LocalGet(elem));
    f.instruction(&Instruction::RefAsNonNull);
    f.instruction(&Instruction::StructGet {
        struct_type_index: intrinsics.object,
        field_index: 0,
    });
    f.instruction(&Instruction::StructGet {
        struct_type_index: intrinsics.vtable,
        field_index: 1,
    });
    f.instruction(&Instruction::LocalSet(tj_fn));

    f.instruction(&Instruction::LocalGet(elem));
    f.instruction(&Instruction::RefAsNonNull);
    f.instruction(&Instruction::LocalGet(tj_fn));
    f.instruction(&Instruction::CallRef(intrinsics.to_json_fn));
}

fn push_inline_string(
    f: &mut Function,
    s: &str,
    string_idx: u32,
    raw_string_idx: u32,
    string_vtable_global_idx: u32,
) -> Result<(), CompilerFailure> {
    f.instruction(&Instruction::GlobalGet(string_vtable_global_idx));
    for b in s.bytes() {
        f.instruction(&Instruction::I32Const(i32::from(b)));
    }
    f.instruction(&Instruction::ArrayNewFixed {
        array_type_index: raw_string_idx,
        array_size: wasm_u32(s.len())?,
    });
    f.instruction(&Instruction::I64Const(0));
    f.instruction(&Instruction::StructNew(string_idx));
    Ok(())
}

pub(crate) fn json_escape_key(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out
}

fn ref_to(idx: u32) -> ValType {
    ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(idx),
    })
}

fn ref_null(idx: u32) -> ValType {
    ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(idx),
    })
}

/// Prepend `if (other is not `type_idx`) return 0` to a vtable `equals` body.
/// `equals(self, other)` is dispatched off `self`'s vtable, so when `self` is a
/// union value its `other` may be any `$Object` subtype; a different runtime type
/// is unequal, never a cast trap. (Param 1 is always `other`.)
fn emit_equals_type_guard(f: &mut Function, type_idx: u32) {
    f.instruction(&Instruction::LocalGet(1));
    f.instruction(&Instruction::RefTestNonNull(HeapType::Concrete(type_idx)));
    f.instruction(&Instruction::I32Eqz);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::Return);
    f.instruction(&Instruction::End);
}

/// `if (other's field-name list differs from self's shape) return 0`, emitted
/// before the per-slot value loop. Guards both wrongness modes of a shape-blind
/// compare: same-arity different names reading equal (`{a: 1} === {b: 1}` via
/// `unknown`), and a shorter `other` payload trapping out-of-bounds.
///
/// Field-name arrays are per-module canonical globals, so `ref.eq` decides the
/// intra-module case. A same-shape value built in another module carries a
/// different (equal-content) array — the miss path compares arity, then each
/// name via `string_eq`.
fn emit_shape_guard(
    f: &mut Function,
    n_fields: u32,
    intrinsics: IntrinsicTypeIndices,
    string_eq_func_idx: u32,
    (a_t, b_t): (u32, u32),
) {
    let load_names = |f: &mut Function, side: u32| {
        f.instruction(&Instruction::LocalGet(side));
        f.instruction(&Instruction::StructGet {
            struct_type_index: intrinsics.object_shape,
            field_index: 1,
        });
    };
    let load_name_at = |f: &mut Function, side: u32, i: u32| {
        load_names(f, side);
        f.instruction(&Instruction::I32Const(i.cast_signed()));
        f.instruction(&Instruction::ArrayGet(intrinsics.field_names));
    };

    load_names(f, a_t);
    load_names(f, b_t);
    f.instruction(&Instruction::RefEq);
    f.instruction(&Instruction::I32Eqz);
    f.instruction(&Instruction::If(BlockType::Empty));

    load_names(f, b_t);
    f.instruction(&Instruction::ArrayLen);
    f.instruction(&Instruction::I32Const(n_fields.cast_signed()));
    f.instruction(&Instruction::I32Ne);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::Return);
    f.instruction(&Instruction::End);

    for i in 0..n_fields {
        load_name_at(f, a_t, i);
        load_name_at(f, b_t, i);
        f.instruction(&Instruction::Call(string_eq_func_idx));
        f.instruction(&Instruction::I32Eqz);
        f.instruction(&Instruction::If(BlockType::Empty));
        f.instruction(&Instruction::I32Const(0));
        f.instruction(&Instruction::Return);
        f.instruction(&Instruction::End);
    }

    f.instruction(&Instruction::End);
}
