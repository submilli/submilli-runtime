//! Injected codegen-state failures; these are not guest-input reproductions.
use super::{
    CodegenCtx, SymbolTable,
    function_emitter::{FunctionEmitter, cast},
};
use crate::{FileId, LineIndex, Type, TypedAst, compiler_error::CompilerFailure};
use wasm_encoder::ValType;

pub(super) fn with_context<T>(
    ta: &TypedAst,
    symbols: &SymbolTable,
    action: impl FnOnce(&CodegenCtx<'_>) -> T,
) -> T {
    let strings = super::StringPool::default();
    let bigints = super::BigIntPool::default();
    let line_index = LineIndex::new("").unwrap();
    let validator_bodies = super::recursive_validators::ValidatorBodies::collect(ta, []);
    let type_info = crate::TypeInfoTable::default();
    action(&CodegenCtx {
        ta,
        strings: &strings,
        bigints: &bigints,
        symbols,
        source: "",
        line_index: &line_index,
        file: FileId(0),
        validator_bodies: &validator_bodies,
        type_info: &type_info,
        package_string_global_idx: None,
        failure: std::cell::Cell::new(None),
        validator_steps_left: std::cell::Cell::new(
            crate::compiler_limits::MAX_INLINE_VALIDATOR_STEPS,
        ),
        validator_root: std::cell::Cell::new(None),
        check_is_standalone: std::cell::Cell::new(false),
    })
}

pub(super) fn assert_internal(error: CompilerFailure) {
    assert!(matches!(error, CompilerFailure::Internal { .. }), "{error}");
}

#[test]
fn invalid_parameter_slots_and_representation_coercions_return_errors() {
    with_context(&TypedAst::new(), &SymbolTable::default(), |ctx| {
        let mut emitter = FunctionEmitter::new(ctx, &[]);
        assert_internal(emitter.emit_boxed_param_prologue(&[], &[0]).unwrap_err());
        emitter.emit_boxed_param_prologue(&[], &[]).unwrap();
        for target in [ValType::I32, ValType::I64] {
            assert_internal(
                cast::emit_coerce_to_wasm_slot(&mut emitter, ctx, &Type::Number, target)
                    .unwrap_err(),
            );
        }
        cast::emit_coerce_to_wasm_slot(&mut emitter, ctx, &Type::Number, ValType::F64).unwrap();
    });
}

#[test]
fn descriptor_closure_requires_every_companion_registration() {
    let ta = TypedAst::new();
    let mut symbols = super::tests::mock_symbols_with_intrinsics();
    for step in 0..3 {
        with_context(&ta, &symbols, |ctx| {
            let mut emitter = FunctionEmitter::new(ctx, &[]);
            assert_internal(
                super::runtime_descriptors::environment(&mut emitter, ctx, &[Type::Number])
                    .unwrap_err(),
            );
        });
        match step {
            0 => {
                symbols.type_descriptor_functions.insert(Type::Number, 1);
            }
            1 => symbols.set_closure_vtable_global(2),
            2 => symbols.record_closure_struct_type(super::field_guards::signature(), 3),
            _ => unreachable!(),
        }
    }
    with_context(&ta, &symbols, |ctx| {
        let mut emitter = FunctionEmitter::new(ctx, &[]);
        super::runtime_descriptors::environment(&mut emitter, ctx, &[Type::Number]).unwrap();
    });
}

#[test]
fn receiver_binding_requires_an_environment_registration() {
    with_context(
        &TypedAst::new(),
        &super::tests::mock_symbols_with_intrinsics(),
        |ctx| {
            let mut emitter = FunctionEmitter::new(ctx, &[]);
            assert_internal(super::this_binding::load_receiver(&mut emitter, ctx).unwrap_err());
            assert_internal(super::this_binding::wrap(&mut emitter, ctx).unwrap_err());
            assert_internal(super::this_binding::bind(&mut emitter, ctx, 0).unwrap_err());
        },
    );
}

fn assert_limit(error: CompilerFailure) {
    assert!(matches!(error, CompilerFailure::Limit { .. }), "{error}");
}

#[test]
fn index_spaces_and_wasm_sizes_are_checked() {
    let mut counter = u32::MAX - 1;
    assert_eq!(super::next_index(&mut counter).unwrap(), u32::MAX - 1);
    assert_limit(super::next_index(&mut counter).unwrap_err());
    assert_eq!(counter, u32::MAX);
    assert_eq!(super::wasm_u32(u32::MAX as usize).unwrap(), u32::MAX);
    if let Some(too_large) = (u32::MAX as usize).checked_add(1) {
        assert_limit(super::wasm_u32(too_large).unwrap_err());
    }

    let mut counter = u32::MAX - 5;
    assert_limit(super::user_subtypes::allocate_methods(&Type::Number, &mut counter).unwrap_err());
    assert!(super::parameter_local(usize::MAX).is_err());
    assert!(super::classes::method_vtable_slot(usize::MAX).is_err());
}

#[test]
fn symbol_lowering_requires_its_registrations() {
    let symbols = SymbolTable::default();
    assert_internal(symbols.optional_field_name_type().unwrap_err());

    let mut symbols = SymbolTable::default();
    let parent = crate::mangle::package_symbol("main", "Parent");
    let child = crate::mangle::package_symbol("main", "Child");
    assert_internal(
        symbols
            .record_class_guard_layout(child.clone(), Some(&parent), 0, false)
            .unwrap_err(),
    );
    assert_internal(symbols.recorded_class_guard_layout(&child).unwrap_err());
    symbols
        .record_class_guard_layout(parent.clone(), None, 0, true)
        .unwrap();
    symbols
        .record_class_guard_layout(child.clone(), Some(&parent), 2, false)
        .unwrap();
    let layout = symbols.recorded_class_guard_layout(&child).unwrap();
    assert_eq!(layout.inheritance_depth, 1);
    assert_eq!(layout.named_payload_len, 2);

    let generic_key = |args| Type::AliasRef {
        mangled: crate::mangle::package_symbol("main", "Box"),
        package: crate::Package("main".into()),
        name: "Box".into(),
        args,
    };
    let mut symbols = SymbolTable::default();
    symbols.record_runtime_validator(generic_key(vec![Type::TypeVar("T".into())]), 7);
    let concrete = generic_key(vec![Type::Number]);
    assert_eq!(
        symbols.generic_runtime_validator(&concrete),
        Some((&["T".to_string()][..], 7))
    );
    let mut symbols = SymbolTable::default();
    symbols.record_runtime_validator(
        generic_key(vec![Type::TypeVar("T".into()), Type::Number]),
        8,
    );
    assert_eq!(symbols.generic_runtime_validator(&concrete), None);
}

fn structural_symbols(with_walk_guard: bool) -> SymbolTable {
    let mut symbols = super::tests::mock_symbols_with_intrinsics();
    if with_walk_guard {
        symbols.record_func(crate::mangle::prelude("vtable_walk_enter"), 3);
        symbols.record_func(crate::mangle::prelude("vtable_walk_leave"), 4);
    }
    symbols.record_global(crate::mangle::prelude("string_vtable"), 0);
    symbols.record_func(crate::mangle::prelude("string_concat"), 0);
    symbols.record_func(crate::mangle::prelude("string_eq"), 1);
    symbols.record_func(crate::mangle::prelude("ObjectConstructor##toJson"), 2);
    symbols.record_optional_field_name_type(99);
    symbols
}

fn subtype(ty: Type) -> super::user_subtypes::UserSubtype {
    let mut next = 0;
    super::user_subtypes::allocate_methods(&ty, &mut next).unwrap()
}

#[test]
fn structural_subtypes_reject_unrepresentable_layouts() {
    let emit = |symbols: &SymbolTable, subtype| {
        super::user_subtypes::emit_method_bodies(
            &mut wasm_encoder::CodeSection::new(),
            &[subtype],
            symbols,
            &crate::TypeInfoTable::default(),
            &crate::TypeInfoIndex::default(),
            None,
        )
    };
    let object = |ty| Type::Object {
        fields: [("a".to_string(), crate::ObjectField::required(ty))].into(),
        index: None,
    };
    assert_internal(emit(&SymbolTable::default(), subtype(object(Type::Number))).unwrap_err());
    // A missing depth guard import fails instead of emitting an unguarded body.
    assert_internal(emit(&structural_symbols(false), subtype(object(Type::Number))).unwrap_err());
    let symbols = structural_symbols(true);
    emit(&symbols, subtype(object(Type::Number))).unwrap();
    assert_internal(emit(&symbols, subtype(Type::Number)).unwrap_err());
    for field in [Type::Void, Type::Never, Type::Error] {
        assert_internal(emit(&symbols, subtype(object(field))).unwrap_err());
    }
}

#[test]
fn literal_pools_and_call_metadata_require_their_registrations() {
    let ta = TypedAst::new();
    with_context(&ta, &super::tests::mock_symbols_with_intrinsics(), |ctx| {
        let mut emitter = FunctionEmitter::new(ctx, &[]);
        assert_internal(super::call_arguments::wrap(&mut emitter, ctx, "[]").unwrap_err());
        assert_internal(super::call_arguments::unwrap(&mut emitter, ctx).unwrap_err());
        // Text the analysis pass never interned has no pool entry to reference.
        assert_internal(
            super::function_emitter::emit_const_string_by_text(&mut emitter, ctx, "absent")
                .unwrap_err(),
        );
        assert_internal(
            super::field_names::emit_instance_names(&mut emitter, ctx, &[], |_| false).unwrap_err(),
        );
        assert_internal(super::field_names::emit_name_presence(&mut emitter, ctx).unwrap_err());
    });
    // Emitters that cannot return errors yet latch the failure, so the module
    // is discarded at the codegen boundary instead of carrying partial code.
    with_context(&ta, &super::tests::mock_symbols_with_intrinsics(), |ctx| {
        let mut emitter = FunctionEmitter::new(ctx, &[]);
        super::throw::emit_type_error_throw(&mut emitter, ctx, "absent");
        assert_internal(ctx.check_failure().unwrap_err());
        ctx.check_failure().unwrap();
    });
}
