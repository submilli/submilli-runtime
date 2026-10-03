//! Host-side declaration of the canonical intrinsic WasmGC rec group.
//!
//! [`build_intrinsic_types`] declares the same types as
//! [`crate::codegen::intrinsics::declare_intrinsic_types`] (the bytes every
//! Submilli module emits), but via wasmtime's `RecGroupBuilder` rather than
//! recovering them from a compiled module. WasmGC structural canonicalization
//! makes the resulting `StructType`/`ArrayType`/`FuncType` handles share the
//! engine type-index the modules use, so a host-built `$string` flows into a
//! guest `(ref $string)` slot without trapping.
//!
//! The grouping must mirror `declare_intrinsic_types` exactly: one
//! `RecGroupBuilder::build()` per rec group — the 14-member `$Object`/`$VTable`
//! group, the self-referential `(rec $ClassVTable)` singleton, the 2-member
//! `(rec $Error_vtable $Error)` pair, plus one singleton per standalone array /
//! `$Object` subtype. Lumping them into one builder would form a different rec
//! group and canonicalize to different indices. The
//! `intrinsic_types_match_codegen` test pins this core mirror to the first 32
//! codegen types. Map, Set, and host backing mirrors are rebuilt by their owning
//! runtime modules from the same canonical layouts.

use std::sync::Arc;

use wasmtime::{
    ArrayType, AsContextMut, Engine, FieldType, Finality, FuncType, Mutability, RecGroupBuilder,
    RefType, StorageType, StructType, ValType,
};

use crate::runtime::StoreData;
use crate::runtime::gc_singleton::{singleton_array, singleton_struct};

/// Canonical handles for the intrinsic types, in `IntrinsicTypeIndices` order.
/// Most fields are unused today; #1 (host-owned vtables) and later per-type ports
/// consume the rest, and the guard test reads every one.
#[allow(dead_code)]
#[derive(Clone)]
pub(crate) struct IntrinsicTypes {
    pub raw_string: ArrayType,
    pub vtable: StructType,
    pub object: StructType,
    pub string: StructType,
    pub boxed_number: StructType,
    pub boxed_boolean: StructType,
    pub field_names: ArrayType,
    pub object_fields: ArrayType,
    pub object_shape: StructType,
    pub to_string_fn: FuncType,
    pub to_json_fn: FuncType,
    pub equals_fn: FuncType,
    pub hash_fn: FuncType,
    pub field_getter: FuncType,
    pub field_setter: FuncType,
    pub raw_array: ArrayType,
    pub array: StructType,
    pub raw_uint8_array: ArrayType,
    pub uint8_array: StructType,
    pub closure: StructType,
    pub class_vtable: StructType,
    pub error_vtable: StructType,
    pub error: StructType,
    pub raw_bigint: ArrayType,
    pub bigint: StructType,
    pub regex_capture_array: ArrayType,
    pub regex_match: StructType,
    pub regex: StructType,
    pub regex_match_box: StructType,
    pub temporal_instant: StructType,
    pub temporal_duration: StructType,
    pub temporal_zdt: StructType,
}

/// The intrinsic types for `store`'s engine. Building them interns every rec
/// group with the engine, which costs far more than the host call that needs
/// them, so a store builds them once and its host functions share the result.
pub(crate) fn intrinsic_types(
    mut store: impl AsContextMut<Data = StoreData>,
) -> wasmtime::Result<Arc<IntrinsicTypes>> {
    let mut ctx = store.as_context_mut();
    if let Some(types) = &ctx.data().intrinsic_types {
        return Ok(Arc::clone(types));
    }
    let types = Arc::new(build_intrinsic_types(ctx.engine())?);
    ctx.data_mut().intrinsic_types = Some(Arc::clone(&types));
    Ok(types)
}

/// Build the full intrinsic type set against `engine`, mirroring
/// `declare_intrinsic_types` group-by-group.
pub(crate) fn build_intrinsic_types(engine: &Engine) -> wasmtime::Result<IntrinsicTypes> {
    use Finality::{Final, NonFinal};
    let imm = Mutability::Const;
    let mutv = Mutability::Var;

    // $rawString — standalone `(array (mut i16))`.
    let raw_string = singleton_array(engine, Final, FieldType::new(mutv, StorageType::I16))?;

    // The closed `$Object`/`$VTable` cycle, emitted as one 14-member rec group.
    // Declare every label first so forward references (VTable -> method fn types,
    // field_names -> $string, …) resolve. Member order must match index order 1..14.
    let mut b = RecGroupBuilder::new(engine);
    let vtable = b.declare_struct();
    let object = b.declare_struct();
    let string = b.declare_struct();
    let boxed_number = b.declare_struct();
    let boxed_boolean = b.declare_struct();
    let field_names = b.declare_array();
    let object_fields = b.declare_array();
    let object_shape = b.declare_struct();
    let to_string_fn = b.declare_func();
    let to_json_fn = b.declare_func();
    let equals_fn = b.declare_func();
    let hash_fn = b.declare_func();
    let field_getter = b.declare_func();
    let field_setter = b.declare_func();

    let mut def = b.define_struct(vtable);
    def.finality(NonFinal);
    def.forward_ref_field(to_string_fn)
        .mutability(imm)
        .nullable(false)
        .finish();
    def.forward_ref_field(to_json_fn)
        .mutability(imm)
        .nullable(false)
        .finish();
    def.forward_ref_field(equals_fn)
        .mutability(imm)
        .nullable(false)
        .finish();
    def.forward_ref_field(hash_fn)
        .mutability(imm)
        .nullable(false)
        .finish();
    def.finish();

    let mut def = b.define_struct(object);
    def.finality(NonFinal);
    def.forward_ref_field(vtable)
        .mutability(imm)
        .nullable(false)
        .finish();
    def.finish();

    // `$string`'s payload is the already-built standalone `$rawString` (a concrete
    // cross-group ref), matching how the module references type index 0.
    let mut def = b.define_struct(string);
    def.finality(NonFinal);
    def.forward_supertype(object);
    def.forward_ref_field(vtable)
        .mutability(imm)
        .nullable(false)
        .finish();
    def.field(FieldType::new(
        imm,
        StorageType::ValType(ValType::Ref(RefType::new(false, raw_string.clone().into()))),
    ));
    // Zero means not hashed; nonzero stores the unsigned 32-bit hash plus one.
    def.field(FieldType::new(
        Mutability::Var,
        StorageType::ValType(ValType::I64),
    ));
    def.finish();

    let mut def = b.define_struct(boxed_number);
    def.finality(NonFinal);
    def.forward_supertype(object);
    def.forward_ref_field(vtable)
        .mutability(imm)
        .nullable(false)
        .finish();
    def.field(FieldType::new(imm, StorageType::ValType(ValType::F64)));
    def.finish();

    let mut def = b.define_struct(boxed_boolean);
    def.finality(NonFinal);
    def.forward_supertype(object);
    def.forward_ref_field(vtable)
        .mutability(imm)
        .nullable(false)
        .finish();
    def.field(FieldType::new(imm, StorageType::ValType(ValType::I32)));
    def.finish();

    let mut def = b.define_array(field_names);
    def.finality(NonFinal);
    def.forward_ref_element(string)
        .mutability(imm)
        .nullable(false)
        .finish();
    def.finish();

    let mut def = b.define_array(object_fields);
    def.finality(NonFinal);
    def.forward_ref_element(object)
        .mutability(mutv)
        .nullable(true)
        .finish();
    def.finish();

    let mut def = b.define_struct(object_shape);
    def.finality(NonFinal);
    def.forward_supertype(object);
    def.forward_ref_field(vtable)
        .mutability(imm)
        .nullable(false)
        .finish();
    def.forward_ref_field(field_names)
        .mutability(mutv)
        .nullable(false)
        .finish();
    def.forward_ref_field(object_fields)
        .mutability(mutv)
        .nullable(false)
        .finish();
    def.finish();

    let mut def = b.define_func(to_string_fn);
    def.finality(NonFinal);
    def.forward_ref_param(object).nullable(false).finish();
    def.forward_ref_result(string).nullable(false).finish();
    def.finish();

    let mut def = b.define_func(to_json_fn);
    def.finality(NonFinal);
    def.forward_ref_param(object).nullable(false).finish();
    def.forward_ref_result(string).nullable(false).finish();
    def.finish();

    let mut def = b.define_func(equals_fn);
    def.finality(NonFinal);
    def.forward_ref_param(object).nullable(false).finish();
    def.forward_ref_param(object).nullable(false).finish();
    def.result(ValType::I32);
    def.finish();

    let mut def = b.define_func(hash_fn);
    def.finality(NonFinal);
    def.forward_ref_param(object).nullable(false).finish();
    def.result(ValType::I32);
    def.finish();

    let mut def = b.define_func(field_getter);
    def.finality(NonFinal);
    def.forward_ref_param(object_shape).nullable(false).finish();
    def.forward_ref_param(string).nullable(false).finish();
    def.forward_ref_result(object).nullable(true).finish();
    def.finish();

    let mut def = b.define_func(field_setter);
    def.finality(NonFinal);
    def.forward_ref_param(object_shape).nullable(false).finish();
    def.forward_ref_param(string).nullable(false).finish();
    def.forward_ref_param(object).nullable(true).finish();
    def.finish();

    let g = b.build().map_err(crate::runtime::host::fatal_host_error)?;
    let vtable = g
        .get_struct(vtable)
        .ok_or_else(|| crate::runtime::host::fatal_host_error("vtable should be a struct"))?;
    let object = g
        .get_struct(object)
        .ok_or_else(|| crate::runtime::host::fatal_host_error("object should be a struct"))?;
    let string = g
        .get_struct(string)
        .ok_or_else(|| crate::runtime::host::fatal_host_error("string should be a struct"))?;
    let boxed_number = g
        .get_struct(boxed_number)
        .ok_or_else(|| crate::runtime::host::fatal_host_error("boxed_number should be a struct"))?;
    let boxed_boolean = g.get_struct(boxed_boolean).ok_or_else(|| {
        crate::runtime::host::fatal_host_error("boxed_boolean should be a struct")
    })?;
    let field_names = g
        .get_array(field_names)
        .ok_or_else(|| crate::runtime::host::fatal_host_error("field_names should be an array"))?;
    let object_fields = g.get_array(object_fields).ok_or_else(|| {
        crate::runtime::host::fatal_host_error("object_fields should be an array")
    })?;
    let object_shape = g
        .get_struct(object_shape)
        .ok_or_else(|| crate::runtime::host::fatal_host_error("object_shape should be a struct"))?;
    let to_string_fn = g
        .get_func(to_string_fn)
        .ok_or_else(|| crate::runtime::host::fatal_host_error("to_string_fn should be a func"))?;
    let to_json_fn = g
        .get_func(to_json_fn)
        .ok_or_else(|| crate::runtime::host::fatal_host_error("to_json_fn should be a func"))?;
    let equals_fn = g
        .get_func(equals_fn)
        .ok_or_else(|| crate::runtime::host::fatal_host_error("equals_fn should be a func"))?;
    let hash_fn = g
        .get_func(hash_fn)
        .ok_or_else(|| crate::runtime::host::fatal_host_error("hash_fn should be a func"))?;
    let field_getter = g
        .get_func(field_getter)
        .ok_or_else(|| crate::runtime::host::fatal_host_error("field_getter should be a func"))?;
    let field_setter = g
        .get_func(field_setter)
        .ok_or_else(|| crate::runtime::host::fatal_host_error("field_setter should be a func"))?;

    // Standalone types, each its own rec group. Built after the main group so they
    // can reference `$Object`/`$VTable`/`$string`/`$rawString` as concrete handles.
    let raw_array = singleton_array(
        engine,
        Final,
        FieldType::new(
            mutv,
            StorageType::ValType(ValType::Ref(RefType::new(true, object.clone().into()))),
        ),
    )?;
    let array = singleton_struct(
        engine,
        NonFinal,
        Some(object.clone()),
        vec![
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(false, vtable.clone().into()))),
            ),
            FieldType::new(
                mutv,
                StorageType::ValType(ValType::Ref(RefType::new(false, raw_array.clone().into()))),
            ),
            FieldType::new(mutv, StorageType::ValType(ValType::I32)),
        ],
    )?;
    let raw_uint8_array = singleton_array(engine, Final, FieldType::new(mutv, StorageType::I8))?;
    let uint8_array = singleton_struct(
        engine,
        NonFinal,
        Some(object.clone()),
        vec![
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(false, vtable.clone().into()))),
            ),
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(
                    false,
                    raw_uint8_array.clone().into(),
                ))),
            ),
        ],
    )?;
    let closure = singleton_struct(
        engine,
        NonFinal,
        Some(object.clone()),
        vec![FieldType::new(
            imm,
            StorageType::ValType(ValType::Ref(RefType::new(false, vtable.clone().into()))),
        )],
    )?;

    // `(rec $ClassVTable)` — the class-only vtable base: the 4 universal slots
    // plus the self-referential nominal-identity parent link, its own singleton
    // rec group (mirrors `declare_intrinsic_types`).
    let mut b = RecGroupBuilder::new(engine);
    let class_vtable_label = b.declare_struct();
    let mut def = b.define_struct(class_vtable_label);
    def.finality(NonFinal);
    def.supertype(vtable.clone());
    for slot_fn in [&to_string_fn, &to_json_fn, &equals_fn, &hash_fn] {
        def.field(FieldType::new(
            imm,
            StorageType::ValType(ValType::Ref(RefType::new(false, slot_fn.clone().into()))),
        ));
    }
    def.forward_ref_field(class_vtable_label)
        .mutability(imm)
        .nullable(true)
        .finish();
    def.finish();
    let g = b.build().map_err(crate::runtime::host::fatal_host_error)?;
    let class_vtable = g
        .get_struct(class_vtable_label)
        .ok_or_else(|| crate::runtime::host::fatal_host_error("class_vtable should be a struct"))?;

    // `(rec $Error_vtable $Error)` — the class-shaped pair, one 2-member rec
    // group mirroring the user-class emitter's output (`classes.rs`): the vtable
    // is the `$ClassVTable` prefix (universal slots + parent link, no methods);
    // the struct has the 3 `$ObjectShape` header slots and an identity ID;
    // named fields remain in the object-fields payload.
    let mut b = RecGroupBuilder::new(engine);
    let error_vtable_label = b.declare_struct();
    let error_label = b.declare_struct();

    let mut def = b.define_struct(error_vtable_label);
    def.finality(NonFinal);
    def.supertype(class_vtable.clone());
    for slot_fn in [&to_string_fn, &to_json_fn, &equals_fn, &hash_fn] {
        def.field(FieldType::new(
            imm,
            StorageType::ValType(ValType::Ref(RefType::new(false, slot_fn.clone().into()))),
        ));
    }
    def.field(FieldType::new(
        imm,
        StorageType::ValType(ValType::Ref(RefType::new(
            true,
            class_vtable.clone().into(),
        ))),
    ));
    def.finish();

    let mut def = b.define_struct(error_label);
    def.finality(NonFinal);
    def.supertype(object_shape.clone());
    def.forward_ref_field(error_vtable_label)
        .mutability(imm)
        .nullable(false)
        .finish();
    def.field(FieldType::new(
        mutv,
        StorageType::ValType(ValType::Ref(RefType::new(
            false,
            field_names.clone().into(),
        ))),
    ));
    def.field(FieldType::new(
        mutv,
        StorageType::ValType(ValType::Ref(RefType::new(
            false,
            object_fields.clone().into(),
        ))),
    ));
    def.field(FieldType::new(mutv, StorageType::ValType(ValType::I64)));
    def.finish();

    let g = b.build().map_err(crate::runtime::host::fatal_host_error)?;
    let error_vtable = g
        .get_struct(error_vtable_label)
        .ok_or_else(|| crate::runtime::host::fatal_host_error("error_vtable should be a struct"))?;
    let error = g
        .get_struct(error_label)
        .ok_or_else(|| crate::runtime::host::fatal_host_error("error should be a struct"))?;

    let raw_bigint = singleton_array(
        engine,
        Final,
        FieldType::new(mutv, StorageType::ValType(ValType::I64)),
    )?;
    let bigint = singleton_struct(
        engine,
        NonFinal,
        Some(object.clone()),
        vec![
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(false, vtable.clone().into()))),
            ),
            FieldType::new(imm, StorageType::ValType(ValType::I32)),
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(false, raw_bigint.clone().into()))),
            ),
        ],
    )?;

    let regex_capture_array = singleton_array(
        engine,
        Final,
        FieldType::new(
            mutv,
            StorageType::ValType(ValType::Ref(RefType::new(true, raw_string.clone().into()))),
        ),
    )?;
    let regex_match = singleton_struct(
        engine,
        Final,
        None::<StructType>,
        vec![
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(false, raw_string.clone().into()))),
            ),
            FieldType::new(imm, StorageType::ValType(ValType::I32)),
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(false, raw_string.clone().into()))),
            ),
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(
                    false,
                    regex_capture_array.clone().into(),
                ))),
            ),
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(
                    false,
                    regex_capture_array.clone().into(),
                ))),
            ),
            FieldType::new(imm, StorageType::ValType(ValType::I32)),
        ],
    )?;
    let regex = singleton_struct(
        engine,
        NonFinal,
        Some(object.clone()),
        vec![
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(false, vtable.clone().into()))),
            ),
            FieldType::new(imm, StorageType::ValType(ValType::EXTERNREF)),
            FieldType::new(mutv, StorageType::ValType(ValType::I32)),
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(false, string.clone().into()))),
            ),
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(false, string.clone().into()))),
            ),
            FieldType::new(imm, StorageType::ValType(ValType::I32)),
            FieldType::new(mutv, StorageType::ValType(ValType::I64)),
        ],
    )?;
    let regex_match_box = singleton_struct(
        engine,
        NonFinal,
        Some(object.clone()),
        vec![
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(false, vtable.clone().into()))),
            ),
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(false, string.clone().into()))),
            ),
            FieldType::new(imm, StorageType::ValType(ValType::I32)),
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(false, string.clone().into()))),
            ),
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(
                    false,
                    regex_capture_array.clone().into(),
                ))),
            ),
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(
                    false,
                    regex_capture_array.clone().into(),
                ))),
            ),
            FieldType::new(mutv, StorageType::ValType(ValType::I64)),
        ],
    )?;

    let temporal_instant = singleton_struct(
        engine,
        NonFinal,
        Some(object.clone()),
        vec![
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(false, vtable.clone().into()))),
            ),
            FieldType::new(imm, StorageType::ValType(ValType::I64)),
            FieldType::new(imm, StorageType::ValType(ValType::I32)),
        ],
    )?;
    let temporal_duration = singleton_struct(
        engine,
        NonFinal,
        Some(object.clone()),
        vec![
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(false, vtable.clone().into()))),
            ),
            FieldType::new(imm, StorageType::ValType(ValType::I32)),
            FieldType::new(imm, StorageType::ValType(ValType::I32)),
            FieldType::new(imm, StorageType::ValType(ValType::I32)),
            FieldType::new(imm, StorageType::ValType(ValType::I32)),
            FieldType::new(imm, StorageType::ValType(ValType::I64)),
            FieldType::new(imm, StorageType::ValType(ValType::I64)),
            FieldType::new(imm, StorageType::ValType(ValType::I64)),
            FieldType::new(imm, StorageType::ValType(ValType::I64)),
            FieldType::new(imm, StorageType::ValType(ValType::I64)),
            FieldType::new(imm, StorageType::ValType(ValType::I64)),
        ],
    )?;
    let temporal_zdt = singleton_struct(
        engine,
        NonFinal,
        Some(object.clone()),
        vec![
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(false, vtable.clone().into()))),
            ),
            FieldType::new(imm, StorageType::ValType(ValType::I64)),
            FieldType::new(imm, StorageType::ValType(ValType::I32)),
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(false, string.clone().into()))),
            ),
        ],
    )?;

    Ok(IntrinsicTypes {
        raw_string,
        vtable,
        object,
        string,
        boxed_number,
        boxed_boolean,
        field_names,
        object_fields,
        object_shape,
        to_string_fn,
        to_json_fn,
        equals_fn,
        hash_fn,
        field_getter,
        field_setter,
        raw_array,
        array,
        raw_uint8_array,
        uint8_array,
        closure,
        class_vtable,
        error_vtable,
        error,
        raw_bigint,
        bigint,
        regex_capture_array,
        regex_match,
        regex,
        regex_match_box,
        temporal_instant,
        temporal_duration,
        temporal_zdt,
    })
}

pub(crate) fn intrinsic_string_type(engine: &Engine) -> wasmtime::Result<StructType> {
    Ok(build_intrinsic_types(engine)?.string)
}

pub(crate) fn intrinsic_uint8_array_type(engine: &Engine) -> wasmtime::Result<StructType> {
    Ok(build_intrinsic_types(engine)?.uint8_array)
}

pub(crate) fn intrinsic_array_type(engine: &Engine) -> wasmtime::Result<StructType> {
    Ok(build_intrinsic_types(engine)?.array)
}

pub(crate) fn intrinsic_bigint_type(engine: &Engine) -> wasmtime::Result<StructType> {
    Ok(build_intrinsic_types(engine)?.bigint)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::intrinsics::declare_intrinsic_types;
    use wasm_encoder::{
        ConstExpr, ExportKind, ExportSection, GlobalSection, GlobalType, HeapType, Module, RefType,
        TypeSection, ValType as EncValType,
    };
    use wasmtime::Config;

    const INTRINSIC_COUNT: u32 = 32;

    /// A module that declares the intrinsics and exports `t{idx}` as a nullable
    /// global referencing each type, so the compiled module's canonical handles
    /// can be read back. This is the old type-witness pattern, kept in the test
    /// and generalized from 4 globals to all 30.
    fn witness_module_bytes() -> Vec<u8> {
        let mut module = Module::new();
        let mut types = TypeSection::new();
        let _ = declare_intrinsic_types(&mut types);
        module.section(&types);

        let mut globals = GlobalSection::new();
        let mut exports = ExportSection::new();
        for idx in 0..INTRINSIC_COUNT {
            globals.global(
                GlobalType {
                    val_type: EncValType::Ref(RefType {
                        nullable: true,
                        heap_type: HeapType::Concrete(idx),
                    }),
                    mutable: false,
                    shared: false,
                },
                &ConstExpr::ref_null(HeapType::Concrete(idx)),
            );
            exports.export(&format!("t{idx}"), ExportKind::Global, idx);
        }
        module.section(&globals);
        module.section(&exports);
        module.finish()
    }

    #[test]
    fn intrinsic_types_match_codegen() {
        let mut config = Config::new();
        config.wasm_gc(true);
        config.wasm_function_references(true);
        let engine = Engine::new(&config).unwrap();

        let intr = build_intrinsic_types(&engine).unwrap();
        let module = wasmtime::Module::new(&engine, witness_module_bytes()).unwrap();

        let heap = |idx: u32| {
            module
                .get_export(&format!("t{idx}"))
                .unwrap()
                .global()
                .unwrap()
                .content()
                .as_ref()
                .map(|r| r.heap_type().clone())
                .unwrap()
        };
        let recovered_struct = |idx: u32| heap(idx).as_concrete_struct().unwrap().clone();
        let recovered_array = |idx: u32| heap(idx).as_concrete_array().unwrap().clone();
        let recovered_func = |idx: u32| heap(idx).as_concrete_func().unwrap().clone();

        // (idx, host handle) for every intrinsic, asserted equal to the canonical
        // type the compiled module uses. `eq` compares engine type-indices, so a
        // pass proves the host built the *same* canonical type, not just a look-alike.
        assert!(ArrayType::eq(&intr.raw_string, &recovered_array(0)));
        assert!(StructType::eq(&intr.vtable, &recovered_struct(1)));
        assert!(StructType::eq(&intr.object, &recovered_struct(2)));
        assert!(StructType::eq(&intr.string, &recovered_struct(3)));
        assert!(StructType::eq(&intr.boxed_number, &recovered_struct(4)));
        assert!(StructType::eq(&intr.boxed_boolean, &recovered_struct(5)));
        assert!(ArrayType::eq(&intr.field_names, &recovered_array(6)));
        assert!(ArrayType::eq(&intr.object_fields, &recovered_array(7)));
        assert!(StructType::eq(&intr.object_shape, &recovered_struct(8)));
        assert!(FuncType::eq(&intr.to_string_fn, &recovered_func(9)));
        assert!(FuncType::eq(&intr.to_json_fn, &recovered_func(10)));
        assert!(FuncType::eq(&intr.equals_fn, &recovered_func(11)));
        assert!(FuncType::eq(&intr.hash_fn, &recovered_func(12)));
        assert!(FuncType::eq(&intr.field_getter, &recovered_func(13)));
        assert!(FuncType::eq(&intr.field_setter, &recovered_func(14)));
        assert!(ArrayType::eq(&intr.raw_array, &recovered_array(15)));
        assert!(StructType::eq(&intr.array, &recovered_struct(16)));
        assert!(ArrayType::eq(&intr.raw_uint8_array, &recovered_array(17)));
        assert!(StructType::eq(&intr.uint8_array, &recovered_struct(18)));
        assert!(StructType::eq(&intr.closure, &recovered_struct(19)));
        assert!(StructType::eq(&intr.class_vtable, &recovered_struct(20)));
        assert!(StructType::eq(&intr.error_vtable, &recovered_struct(21)));
        assert!(StructType::eq(&intr.error, &recovered_struct(22)));
        assert!(ArrayType::eq(&intr.raw_bigint, &recovered_array(23)));
        assert!(StructType::eq(&intr.bigint, &recovered_struct(24)));
        assert!(ArrayType::eq(
            &intr.regex_capture_array,
            &recovered_array(25)
        ));
        assert!(StructType::eq(&intr.regex_match, &recovered_struct(26)));
        assert!(StructType::eq(&intr.regex, &recovered_struct(27)));
        assert!(StructType::eq(&intr.regex_match_box, &recovered_struct(28)));
        assert!(StructType::eq(
            &intr.temporal_instant,
            &recovered_struct(29)
        ));
        assert!(StructType::eq(
            &intr.temporal_duration,
            &recovered_struct(30)
        ));
        assert!(StructType::eq(&intr.temporal_zdt, &recovered_struct(31)));
    }
}
