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
