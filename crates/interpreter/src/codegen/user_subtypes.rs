//! Per-user-subtype Wasm subtype, vtable, and method emission for structural object shapes.

use std::collections::BTreeSet;

use wasm_encoder::{
    BlockType, ConstExpr, Function, GlobalSection, GlobalType, HeapType, Instruction, RefType,
    ValType,
};

use crate::codegen::intrinsics::IntrinsicTypeIndices;
use crate::codegen::symbol_table::{SymbolTable, may_hold_null};
use crate::{Shape, Type, TypeInfoTable};

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
    pub vtable_global_idx: u32,
}

/// Pre-allocate method indices so vtable globals can reference them via `ref.func`.
pub fn allocate_methods(ty: &Type, next_func_idx: &mut u32) -> UserSubtype {
    let to_string_func = take(next_func_idx);
    let to_json_func = take(next_func_idx);
    let equals_func = take(next_func_idx);
    let hash_func = take(next_func_idx);
    *next_func_idx += 3; // JSON, equals, and hash implementation bodies.
    UserSubtype {
        ty: ty.clone(),
        to_string_func,
        to_json_func,
        equals_func,
        hash_func,
        vtable_global_idx: 0, // filled in by emit_vtable_globals
    }
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
) {
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
        let idx = take(next_global_idx);
        subtype.vtable_global_idx = idx;
        symbols.record_vtable_global(subtype.ty.clone(), idx);
    }
}

pub fn emit_method_bodies(
    code: &mut wasm_encoder::CodeSection,
    subtypes: &[UserSubtype],
    symbols: &SymbolTable,
    type_info: &TypeInfoTable,
    pkg_string_global_idx: Option<u32>,
) {
    let intrinsics = symbols
        .intrinsic_type_indices()
        .expect("intrinsics declared by codegen entry");
    let string_vtable_global_idx = symbols
        .prelude_global_idx("string_vtable")
        .expect("string_vtable imported from prelude");
    let string_concat_func_idx = symbols
        .prelude_func_idx("string_concat")
        .expect("string_concat imported from prelude");
    let string_eq_func_idx = symbols
        .prelude_func_idx("string_eq")
        .expect("string_eq imported from prelude");

    for subtype in subtypes {
        code.function(&emit_subtype_to_string_body(
            subtype,
            intrinsics,
            symbols,
            string_vtable_global_idx,
        ));

        for (body, params, result) in [
            (subtype.hash_func + 1, 1, ref_to(intrinsics.string)),
            (subtype.hash_func + 2, 2, ValType::I32),
            (subtype.hash_func + 3, 1, ValType::I32),
        ] {
            code.function(&super::vtable_walk::guarded_body(
                body, params, result, symbols,
            ));
        }
        code.function(&emit_subtype_to_json_body(
            subtype,
            intrinsics,
            symbols,
            type_info,
            pkg_string_global_idx,
            string_concat_func_idx,
            string_vtable_global_idx,
        ));

        code.function(&emit_subtype_equals_body(
            subtype,
            intrinsics,
            string_eq_func_idx,
        ));

        code.function(&emit_subtype_hash_body(subtype, intrinsics));
    }
}

fn emit_subtype_to_string_body(
    subtype: &UserSubtype,
    intrinsics: IntrinsicTypeIndices,
    symbols: &SymbolTable,
    string_vtable_global_idx: u32,
) -> Function {
    let Type::Object { fields, .. } = &subtype.ty else {
        unreachable!(
            "emit_subtype_to_string_body called on non-Object subtype: {:?}",
            subtype.ty,
        );
    };

    if !fields.contains_key("toString") {
        let mut f = Function::new(std::iter::empty());
        crate::codegen::intrinsics::push_string_literal(
            &mut f,
            intrinsics,
            string_vtable_global_idx,
            "[object Object]",
        );
        f.instruction(&Instruction::End);
        return f;
    }

    // The typechecker guarantees `toString` field is non-optional `() => string`, so the slot is non-null.
    let to_string_sig = crate::codegen::closures::ClosureSig {
        arity: 0,
        is_void: false,
    };
    let closure_struct_idx = symbols.closure_struct_type_idx(to_string_sig).expect(
        "closure struct for `() => string` registered when any shape declares a toString \
         override (collect_from_dependencies walks every shape field type)",
    );
    let closure_func_type_idx = symbols
        .closure_func_type_idx(to_string_sig)
        .expect("closure funcref type for `() => string` registered alongside its struct");

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

    // The field slot is `(ref null $Object)`; ref.as_non_null before the closure cast.
    let to_string_slot = field_index(&subtype.ty, "toString")
        .expect("toString field slot exists when shape carries the field");
    f.instruction(&Instruction::LocalGet(self_t));
    f.instruction(&Instruction::StructGet {
        struct_type_index: intrinsics.object_shape,
        field_index: 2,
    });
    f.instruction(&Instruction::I32Const(to_string_slot as i32));
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

    // The closure returns its result boxed as `(ref $Object)`; unbox
    // to `(ref $string)` to match `$toStringFn`'s return type.
    f.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(
        intrinsics.string,
    )));
    f.instruction(&Instruction::End);
    f
}

fn emit_subtype_to_json_body(
    subtype: &UserSubtype,
    intrinsics: IntrinsicTypeIndices,
    symbols: &SymbolTable,
    type_info: &TypeInfoTable,
    pkg_string_global_idx: Option<u32>,
    string_concat_func_idx: u32,
    string_vtable_global_idx: u32,
) -> Function {
    let Type::Object { fields, .. } = &subtype.ty else {
        unreachable!(
            "emit_subtype_to_json_body called on non-Object subtype: {:?}",
            subtype.ty,
        );
    };

    if fields.contains_key("toJson") {
        return emit_subtype_to_json_override_body(subtype, intrinsics, symbols);
    }

    let type_id = type_info
        .object_type_id(&subtype.ty)
        .filter(|_| type_info.supports_host_json_object(&subtype.ty));
    let Some(type_id) = type_id else {
        return emit_subtype_to_json_vtable_body(
            subtype,
            intrinsics,
            string_concat_func_idx,
            string_vtable_global_idx,
            symbols,
        );
    };
    let pkg_string_global_idx =
        pkg_string_global_idx.expect("host object toJson requires package string global");
    let stringify_func_idx = symbols
        .func_idx(&crate::mangle::host(
            crate::runtime::JSON_MODULE_NAME,
            "stringifyTypedObject",
        ))
        .expect("submilli:json.stringifyTypedObject imported during codegen");

    let mut f = Function::new(std::iter::empty());

    f.instruction(&Instruction::GlobalGet(string_vtable_global_idx));

    f.instruction(&Instruction::GlobalGet(pkg_string_global_idx));
    f.instruction(&Instruction::RefAsNonNull);
    f.instruction(&Instruction::StructGet {
        struct_type_index: intrinsics.string,
        field_index: 1,
    });
    f.instruction(&Instruction::I32Const(type_id.as_u32() as i32));
    f.instruction(&Instruction::LocalGet(0));
    f.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(
        intrinsics.object_shape,
    )));
    f.instruction(&Instruction::Call(stringify_func_idx));
    f.instruction(&Instruction::StructNew(intrinsics.string));
    f.instruction(&Instruction::End);
    f
}

/// Static serializers fall back to the dynamic walker after shape growth.
pub(super) fn emit_grown_object_to_json(
    function: &mut Function,
    intrinsics: IntrinsicTypeIndices,
    symbols: &SymbolTable,
    original_len: u32,
) {
    function.instruction(&Instruction::LocalGet(0));
    function.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(
        intrinsics.object_shape,
    )));
    function.instruction(&Instruction::StructGet {
        struct_type_index: intrinsics.object_shape,
        field_index: 1,
    });
    function.instruction(&Instruction::ArrayLen);
    function.instruction(&Instruction::I32Const(original_len as i32));
    function.instruction(&Instruction::I32GtU);
    function.instruction(&Instruction::If(BlockType::Empty));
    function.instruction(&Instruction::LocalGet(0));
    function.instruction(&Instruction::Call(
        symbols
            .prelude_func_idx("ObjectConstructor##toJson")
            .expect("dynamic object serializer imported"),
    ));
    function.instruction(&Instruction::Return);
    function.instruction(&Instruction::End);
}

fn emit_subtype_to_json_vtable_body(
    subtype: &UserSubtype,
    intrinsics: IntrinsicTypeIndices,
    string_concat_func_idx: u32,
    string_vtable_global_idx: u32,
    symbols: &SymbolTable,
) -> Function {
    let Type::Object { fields, .. } = &subtype.ty else {
        unreachable!(
            "emit_subtype_to_json_vtable_body called on non-Object subtype: {:?}",
            subtype.ty,
        );
    };

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
    emit_grown_object_to_json(&mut f, intrinsics, symbols, fields.len() as u32);
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
        );
        f.instruction(&Instruction::End);
        return f;
    }

    push_inline_string(
        &mut f,
        "{",
        intrinsics.string,
        intrinsics.raw_string,
        string_vtable_global_idx,
    );
    f.instruction(&Instruction::LocalSet(acc));

    f.instruction(&Instruction::I32Const(1));
    f.instruction(&Instruction::LocalSet(first_local));

    for (idx, (field_name, field)) in fields.iter().enumerate() {
        let key_no_comma = format!("\"{}\":", json_escape_key(field_name));
        let key_with_comma = format!(",\"{}\":", json_escape_key(field_name));
        // A present optional field may hold a written `null` its declared type lacks.
        let field_nullable = field.optional || may_hold_null(&field.ty);

        f.instruction(&Instruction::LocalGet(self_t));
        f.instruction(&Instruction::StructGet {
            struct_type_index: intrinsics.object_shape,
            field_index: 2,
        });
        f.instruction(&Instruction::I32Const(idx as i32));
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
                symbols.optional_field_name_type(),
                self_t,
                idx as u32,
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
        );
        f.instruction(&Instruction::Else);
        push_inline_string(
            &mut f,
            &key_with_comma,
            intrinsics.string,
            intrinsics.raw_string,
            string_vtable_global_idx,
        );
        f.instruction(&Instruction::End);
        f.instruction(&Instruction::Call(string_concat_func_idx));
        f.instruction(&Instruction::LocalSet(acc));

        f.instruction(&Instruction::LocalGet(acc));
        if field_nullable {
            f.instruction(&Instruction::LocalGet(elem));
            f.instruction(&Instruction::RefIsNull);
            f.instruction(&Instruction::If(BlockType::Result(string_ref)));
            push_inline_string(
                &mut f,
                "null",
                intrinsics.string,
                intrinsics.raw_string,
                string_vtable_global_idx,
            );
            f.instruction(&Instruction::Else);
            emit_field_value_to_json(&mut f, elem, tj_fn, intrinsics);
            f.instruction(&Instruction::End);
        } else {
            emit_field_value_to_json(&mut f, elem, tj_fn, intrinsics);
        }
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
    );
    f.instruction(&Instruction::Call(string_concat_func_idx));
    f.instruction(&Instruction::End);
    f
}

fn emit_subtype_to_json_override_body(
    subtype: &UserSubtype,
    intrinsics: IntrinsicTypeIndices,
    symbols: &SymbolTable,
) -> Function {
    let to_json_sig = crate::codegen::closures::ClosureSig {
        arity: 0,
        is_void: false,
    };
    let closure_struct_idx = symbols.closure_struct_type_idx(to_json_sig).expect(
        "closure struct for `() => string` registered when any shape declares a toJson \
         override (collect_from_dependencies walks every shape field type)",
    );
    let closure_func_type_idx = symbols
        .closure_func_type_idx(to_json_sig)
        .expect("closure funcref type for `() => string` registered alongside its struct");

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
    let to_json_slot = field_index(&subtype.ty, "toJson")
        .expect("toJson field slot exists when shape carries the field");
    f.instruction(&Instruction::LocalGet(self_t));
    f.instruction(&Instruction::StructGet {
        struct_type_index: intrinsics.object_shape,
        field_index: 2,
    });
    f.instruction(&Instruction::I32Const(to_json_slot as i32));
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
    f
}

fn emit_subtype_equals_body(
    subtype: &UserSubtype,
    intrinsics: IntrinsicTypeIndices,
    string_eq_func_idx: u32,
) -> Function {
    let Type::Object { fields, .. } = &subtype.ty else {
        unreachable!(
            "emit_subtype_equals_body called on non-Object subtype: {:?}",
            subtype.ty
        );
    };
    // Peel before the Union check: an aliased union field must trigger null-aware locals,
    // because emit_field_compare also peels — mismatched locals cause Wasm validation failure.
    let any_dispatch = fields.values().any(|f| {
        is_ref_dispatch_field(&f.ty) || matches!(f.ty.peel(), Type::Union(_)) || f.optional
    });
    let any_union = fields
        .values()
        .any(|f| matches!(f.ty.peel(), Type::Union(_)) || f.optional);

    let object_shape_ref = ref_to(intrinsics.object_shape);
    let object_ref = ref_to(intrinsics.object);
    let object_null_ref = ref_null(intrinsics.object);
    let equals_fn_ref = ref_to(intrinsics.equals_fn);

    // Locals layout (after 2 params):
    //   2: $a_t            (ref $arity)
    //   3: $b_t            (ref $arity)
    //   4: $field_lhs      (ref $Object)        — any dispatch
    //   5: $eq_fn          (ref $equalsFn)      — any dispatch
    //   6: $field_lhs_null (ref null $Object)   — any union field
    //   7: $field_rhs_null (ref null $Object)   — any union field
    let mut locals: Vec<(u32, ValType)> = vec![(2, object_shape_ref)];
    if any_dispatch {
        locals.push((1, object_ref));
        locals.push((1, equals_fn_ref));
    }
    if any_union {
        locals.push((2, object_null_ref));
    }
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

    emit_shape_guard(
        &mut f,
        fields.len() as u32,
        intrinsics,
        string_eq_func_idx,
        (a_t, b_t),
    );

    for (slot, field) in fields.values().enumerate() {
        let field_index = slot as u32;

        emit_field_compare(
            &mut f,
            intrinsics.object_shape,
            field_index,
            &field.ty,
            field.optional,
            intrinsics,
            (a_t, b_t, field_lhs, eq_fn, field_lhs_null, field_rhs_null),
        );

        f.instruction(&Instruction::I32Eqz);
        f.instruction(&Instruction::If(BlockType::Empty));
        f.instruction(&Instruction::I32Const(0));
        f.instruction(&Instruction::Return);
        f.instruction(&Instruction::End);
    }

    f.instruction(&Instruction::I32Const(1));
    f.instruction(&Instruction::End);
    f
}

fn emit_field_compare(
    f: &mut Function,
    object_shape_idx: u32,
    field_index: u32,
    field_ty: &Type,
    field_optional: bool,
    intrinsics: IntrinsicTypeIndices,
    locals: (u32, u32, u32, u32, u32, u32),
) {
    let (a_t, b_t, field_lhs, eq_fn, field_lhs_null, field_rhs_null) = locals;
    // Peel so aliased unions (`type Maybe = T | null`) route through null-aware dispatch, not optional.
    if field_optional && !matches!(field_ty.peel(), Type::Union(_)) {
        emit_nullable_field_compare(
            f,
            object_shape_idx,
            field_index,
            intrinsics,
            (field_lhs, eq_fn, field_lhs_null, field_rhs_null, a_t, b_t),
        );
        return;
    }
    // The `self` side is provably this shape (dispatched off its own vtable), so
    // its non-optional slots honor the typechecker's non-null invariant. `other`
    // only shares this shape's field *names* — via `unknown`, a same-name shape
    // can hold a different type or null in any slot, so `other`-side reads test
    // before casting and treat a mismatch as unequal, never a trap.
    let load_slot_nullable = |f: &mut Function, side: u32| {
        f.instruction(&Instruction::LocalGet(side));
        f.instruction(&Instruction::StructGet {
            struct_type_index: object_shape_idx,
            field_index: 2,
        });
        f.instruction(&Instruction::I32Const(field_index as i32));
        f.instruction(&Instruction::ArrayGet(intrinsics.object_fields));
    };
    let load_slot_as_object = |f: &mut Function, side: u32| {
        load_slot_nullable(f, side);
        f.instruction(&Instruction::RefAsNonNull);
    };
    let boxed_compare = |f: &mut Function, box_idx: u32, eq: Instruction<'static>| {
        load_slot_nullable(f, b_t);
        f.instruction(&Instruction::RefTestNonNull(HeapType::Concrete(box_idx)));
        f.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
        load_slot_as_object(f, a_t);
        f.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(box_idx)));
        f.instruction(&Instruction::StructGet {
            struct_type_index: box_idx,
            field_index: 1,
        });
        load_slot_as_object(f, b_t);
        f.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(box_idx)));
        f.instruction(&Instruction::StructGet {
            struct_type_index: box_idx,
            field_index: 1,
        });
        f.instruction(&eq);
        f.instruction(&Instruction::Else);
        f.instruction(&Instruction::I32Const(0));
        f.instruction(&Instruction::End);
    };
    let field_ty = field_ty.peel();
    match field_ty {
        Type::Number | Type::NumberLiteral(_) => {
            boxed_compare(f, intrinsics.boxed_number, Instruction::F64Eq);
        }
        Type::Boolean | Type::BooleanLiteral(_) => {
            boxed_compare(f, intrinsics.boxed_boolean, Instruction::I32Eq);
        }
        Type::String
        | Type::StringLiteral(_)
        | Type::BigInt
        | Type::Object { .. }
        | Type::Array(_)
        | Type::Tuple(_)
        | Type::Uint8Array
        | Type::TypeVar(_)
        | Type::GenericParam { .. }
        | Type::Unknown
        | Type::NumberEnum { .. }
        | Type::StringEnum { .. }
        | Type::Function { .. }
        | Type::InterfaceRef { .. }
        // A class instance is an `$Object` subtype carrying a vtable — same
        // `vtable.equals` dispatch as `InterfaceRef`.
        | Type::ClassRef { .. }
        // A recursion back-edge's value is an `$Object` subtype with its
        // own vtable — same `vtable.equals` dispatch as `InterfaceRef`.
        | Type::AliasRef { .. } => {
            // Tuples lower to $Array (element-wise equals); Unknown/TypeVar/generic fields also
            // carry vtables, so vtable dispatch works for all of these. The callee's own
            // type guard makes a mismatched `other` slot unequal, so only null needs
            // rejecting here.
            load_slot_nullable(f, b_t);
            f.instruction(&Instruction::RefIsNull);
            f.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
            f.instruction(&Instruction::I32Const(0));
            f.instruction(&Instruction::Else);
            load_slot_as_object(f, a_t);
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
            load_slot_as_object(f, b_t);
            f.instruction(&Instruction::LocalGet(eq_fn));
            f.instruction(&Instruction::CallRef(intrinsics.equals_fn));
            f.instruction(&Instruction::End);
        }
        Type::Null => {
            // Statically null on the `self` side — equal iff `other`'s slot is null too.
            load_slot_nullable(f, b_t);
            f.instruction(&Instruction::RefIsNull);
        }
        Type::Void | Type::Error | Type::Never => {
            unreachable!(
                "field of type {field_ty:?} should not appear in a typechecked Object",
            );
        }
        Type::Union(_) => {
            // null-aware union equality: both-null → equal; one-null → unequal; both non-null → vtable.equals.
            f.instruction(&Instruction::LocalGet(a_t));
            f.instruction(&Instruction::StructGet {
                struct_type_index: object_shape_idx,
                field_index: 2,
            });
            f.instruction(&Instruction::I32Const(field_index as i32));
            f.instruction(&Instruction::ArrayGet(intrinsics.object_fields));
            f.instruction(&Instruction::LocalSet(field_lhs_null));

            f.instruction(&Instruction::LocalGet(b_t));
            f.instruction(&Instruction::StructGet {
                struct_type_index: object_shape_idx,
                field_index: 2,
            });
            f.instruction(&Instruction::I32Const(field_index as i32));
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
        Type::Alias { .. } | Type::Refined { .. } | Type::Readonly(_) => unreachable!("peel guarantees no alias here (SUB-242)"),
    }
}

/// FNV-1a-32 field-by-field hash. Must use same field order and dispatch as `equals`.
fn emit_subtype_hash_body(subtype: &UserSubtype, intrinsics: IntrinsicTypeIndices) -> Function {
    let Type::Object { fields, .. } = &subtype.ty else {
        unreachable!(
            "emit_subtype_hash_body called on non-Object subtype: {:?}",
            subtype.ty,
        );
    };

    // Peel before Union check — aliased unions must trigger null-aware dispatch prelude.
    let any_dispatch = fields.values().any(|f| {
        is_ref_dispatch_field(&f.ty) || matches!(f.ty.peel(), Type::Union(_)) || f.optional
    });
    let any_union = fields
        .values()
        .any(|f| matches!(f.ty.peel(), Type::Union(_)) || f.optional);

    let object_shape_ref = ref_to(intrinsics.object_shape);
    let object_ref = ref_to(intrinsics.object);
    let object_null_ref = ref_null(intrinsics.object);
    let hash_fn_ref = ref_to(intrinsics.hash_fn);

    // Locals (after 1 param self=0):
    //   1: self_t    (ref $arity)
    //   2: hash      (i32) — running accumulator
    //   3: field_obj (ref $Object) — any dispatch
    //   4: hash_fn   (ref $hashFn) — any dispatch
    //   5: f_null    (ref null $Object) — any union/optional field
    //   6: f_bits    (i64) — number-field unboxing
    let mut locals: Vec<(u32, ValType)> = vec![(1, object_shape_ref), (1, ValType::I32)];
    if any_dispatch {
        locals.push((1, object_ref));
        locals.push((1, hash_fn_ref));
    }
    if any_union {
        locals.push((1, object_null_ref));
    }
    let any_number_field = fields.values().any(|f| {
        // peel so `type N = number; { x: N }` still
        // triggers the f_bits scratch allocation.
        matches!(f.ty.peel(), Type::Number | Type::NumberLiteral(_))
    });
    if any_number_field {
        locals.push((1, ValType::I64));
    }

    let mut f = Function::new(locals);
    let self_param = 0u32;
    let self_t = 1u32;
    let hash = 2u32;
    let field_obj = 3u32;
    let hash_fn = 4u32;
    let f_null = if any_dispatch { 5u32 } else { 3u32 };
    let f_bits = match (any_dispatch, any_union) {
        (true, true) => 6u32,
        (true, false) => 5u32,
        (false, true) => 4u32,
        (false, false) => 3u32,
    };

    f.instruction(&Instruction::LocalGet(self_param));
    f.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(
        intrinsics.object_shape,
    )));
    f.instruction(&Instruction::LocalSet(self_t));

    // hash = FNV-1a offset basis
    f.instruction(&Instruction::I32Const(0x811c9dc5u32 as i32));
    f.instruction(&Instruction::LocalSet(hash));

    for (slot, field) in fields.values().enumerate() {
        let field_index = slot as u32;
        emit_field_hash(
            &mut f,
            intrinsics.object_shape,
            field_index,
            &field.ty,
            field.optional,
            intrinsics,
            (self_t, field_obj, hash_fn, f_null, f_bits),
        );
        // hash = (hash XOR field_hash) * 0x01000193
        f.instruction(&Instruction::LocalGet(hash));
        f.instruction(&Instruction::I32Xor);
        f.instruction(&Instruction::I32Const(0x01000193));
        f.instruction(&Instruction::I32Mul);
        f.instruction(&Instruction::LocalSet(hash));
    }

    f.instruction(&Instruction::LocalGet(hash));
    f.instruction(&Instruction::End);
    f
}

fn emit_field_hash(
    f: &mut Function,
    object_shape_idx: u32,
    field_index: u32,
    field_ty: &Type,
    field_optional: bool,
    intrinsics: IntrinsicTypeIndices,
    locals: (u32, u32, u32, u32, u32),
) {
    let (self_t, field_obj, hash_fn, f_null, f_bits) = locals;

    let field_ty = field_ty.peel();

    if field_optional && !matches!(field_ty, Type::Union(_)) {
        emit_nullable_field_hash(
            f,
            object_shape_idx,
            field_index,
            intrinsics,
            (self_t, field_obj, hash_fn, f_null),
        );
        return;
    }

    let load_slot_as_object = |f: &mut Function| {
        f.instruction(&Instruction::LocalGet(self_t));
        f.instruction(&Instruction::StructGet {
            struct_type_index: object_shape_idx,
            field_index: 2,
        });
        f.instruction(&Instruction::I32Const(field_index as i32));
        f.instruction(&Instruction::ArrayGet(intrinsics.object_fields));
        f.instruction(&Instruction::RefAsNonNull);
    };

    match field_ty {
        Type::Number | Type::NumberLiteral(_) => {
            // Unbox f64, reinterpret bits, fold high/low halves.
            load_slot_as_object(f);
            f.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(
                intrinsics.boxed_number,
            )));
            f.instruction(&Instruction::StructGet {
                struct_type_index: intrinsics.boxed_number,
                field_index: 1,
            });
            f.instruction(&Instruction::I64ReinterpretF64);
            f.instruction(&Instruction::LocalSet(f_bits));
            f.instruction(&Instruction::LocalGet(f_bits));
            f.instruction(&Instruction::I32WrapI64);
            f.instruction(&Instruction::LocalGet(f_bits));
            f.instruction(&Instruction::I64Const(32));
            f.instruction(&Instruction::I64ShrU);
            f.instruction(&Instruction::I32WrapI64);
            f.instruction(&Instruction::I32Xor);
        }
        Type::Boolean | Type::BooleanLiteral(_) => {
            load_slot_as_object(f);
            f.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(
                intrinsics.boxed_boolean,
            )));
            f.instruction(&Instruction::StructGet {
                struct_type_index: intrinsics.boxed_boolean,
                field_index: 1,
            });
        }
        Type::String
        | Type::StringLiteral(_)
        | Type::BigInt
        | Type::Object { .. }
        | Type::Array(_)
        | Type::Tuple(_)
        | Type::Uint8Array
        | Type::TypeVar(_)
        | Type::GenericParam { .. }
        | Type::Unknown
        | Type::NumberEnum { .. }
        | Type::StringEnum { .. }
        | Type::Function { .. }
        | Type::InterfaceRef { .. }
        // A class instance is an `$Object` subtype with a `$hashFn` slot —
        // same vtable dispatch as `InterfaceRef`.
        | Type::ClassRef { .. }
        // A recursion back-edge's value is an `$Object` subtype with its
        // own `$hashFn` slot — same vtable dispatch as `InterfaceRef`.
        | Type::AliasRef { .. } => {
            // Vtable dispatch through slot 3 ($hashFn).
            load_slot_as_object(f);
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
        }
        Type::Null => {
            f.instruction(&Instruction::I32Const(0));
        }
        Type::Void | Type::Error | Type::Never => {
            unreachable!(
                "field of type {field_ty:?} should not appear in a typechecked Object",
            );
        }
        Type::Union(_) => {
            emit_nullable_field_hash(
                f,
                object_shape_idx,
                field_index,
                intrinsics,
                (self_t, field_obj, hash_fn, f_null),
            );
        }
        Type::Alias { .. } | Type::Refined { .. } | Type::Readonly(_) => unreachable!("peel guarantees no alias here (SUB-242)"),
    }
}

/// Null-aware vtable-hash dispatch shared by the Union arm and
/// optional fields. Pushes a single `i32` — `0` if the slot is null,
/// otherwise the value's `vtable.hash(value)`..
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
    f.instruction(&Instruction::I32Const(field_index as i32));
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

/// Null-aware vtable-equals dispatch for Union and optional fields. Pushes i32: 1=equal, 0=unequal.
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
    f.instruction(&Instruction::I32Const(field_index as i32));
    f.instruction(&Instruction::ArrayGet(intrinsics.object_fields));
    f.instruction(&Instruction::LocalSet(field_lhs_null));

    f.instruction(&Instruction::LocalGet(b_t));
    f.instruction(&Instruction::StructGet {
        struct_type_index: object_shape_idx,
        field_index: 2,
    });
    f.instruction(&Instruction::I32Const(field_index as i32));
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

fn is_ref_dispatch_field(ty: &Type) -> bool {
    // Must peel aliases and must mirror the vtable-dispatch arm of
    // emit_field_compare/emit_field_hash exactly (incl. Unknown, BigInt,
    // ClassRef, AliasRef): those arms use the dispatch locals. If a type is
    // absent here, `any_dispatch` is false, the locals are never allocated,
    // and the Wasm validator rejects with "unknown local 4".
    matches!(
        ty.peel(),
        Type::String
            | Type::StringLiteral(_)
            | Type::BigInt
            | Type::Object { .. }
            | Type::Array(_)
            | Type::Tuple(_)
            | Type::Uint8Array
            | Type::TypeVar(_)
            | Type::GenericParam { .. }
            | Type::Unknown
            | Type::NumberEnum { .. }
            | Type::StringEnum { .. }
            | Type::Function { .. }
            | Type::InterfaceRef { .. }
            | Type::ClassRef { .. }
            | Type::AliasRef { .. },
    )
}

/// Returns all method function indices; must be declared in the element section for `ref.func` to be valid.
pub fn declared_method_funcs(subtypes: &[UserSubtype]) -> Vec<u32> {
    let mut out = Vec::with_capacity(subtypes.len() * 4);
    for s in subtypes {
        out.push(s.to_string_func);
        out.push(s.to_json_func);
        out.push(s.equals_func);
        out.push(s.hash_func);
    }
    out
}

/// Payload-array index for a named field.
pub fn field_index(ty: &Type, field_name: &str) -> Option<u32> {
    let Type::Object { fields, .. } = ty else {
        return None;
    };
    let pos = fields.keys().position(|k| k == field_name)?;
    Some(pos as u32)
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
) {
    f.instruction(&Instruction::GlobalGet(string_vtable_global_idx));
    for b in s.bytes() {
        f.instruction(&Instruction::I32Const(i32::from(b)));
    }
    f.instruction(&Instruction::ArrayNewFixed {
        array_type_index: raw_string_idx,
        array_size: s.len() as u32,
    });
    f.instruction(&Instruction::StructNew(string_idx));
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
            c if c.is_control() => {
                use std::fmt::Write;
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
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

fn take(counter: &mut u32) -> u32 {
    let v = *counter;
    *counter += 1;
    v
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
        f.instruction(&Instruction::I32Const(i as i32));
        f.instruction(&Instruction::ArrayGet(intrinsics.field_names));
    };

    load_names(f, a_t);
    load_names(f, b_t);
    f.instruction(&Instruction::RefEq);
    f.instruction(&Instruction::I32Eqz);
    f.instruction(&Instruction::If(BlockType::Empty));

    load_names(f, b_t);
    f.instruction(&Instruction::ArrayLen);
    f.instruction(&Instruction::I32Const(n_fields as i32));
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
