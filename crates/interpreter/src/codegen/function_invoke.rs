//! Hidden exports used when the host directly invokes a `@functions/<name>`
//! module's exported `run` entry point with positional JSON arguments:
//!
//! - `<pkg>#function:argument:<i>`: `(unknown) -> unknown` per castable
//!   parameter, validating the parsed JSON value as `value as T` and returning
//!   the boxed runtime value before `run` executes.
//! - `<pkg>#function:invoke`: `(unknown, …) -> (ref null $string)`, coercing
//!   each validated argument into `run`'s parameter slot, calling `run`, and
//!   encoding the result to JSON (`null` ref when `undefined`).

use wasm_encoder::{Function, HeapType, Instruction, RefType, ValType};

use super::{CodegenCtx, function_emitter::FunctionEmitter, symbol_table::SymbolTable};
use crate::{
    ExportKind, FinalizerArgumentCheck, FunctionArgumentCheck, FunctionInvokePlan, Ident, Span,
    Type, TypedAst,
};

pub(super) struct FunctionInvokeExports<'a> {
    pub arguments: Vec<FunctionArgumentExport<'a>>,
    pub invoke_type_idx: u32,
    pub invoke_func_idx: u32,
    pub run_mangled: crate::MangledName,
    pub source_return_type: &'a Type,
}

pub(super) struct FunctionArgumentExport<'a> {
    pub index: usize,
    pub check: &'a FinalizerArgumentCheck,
    pub type_idx: u32,
    pub func_idx: u32,
}

pub(super) fn allocate<'a>(
    ta: &'a TypedAst,
    plan: &'a FunctionInvokePlan,
    types: &mut wasm_encoder::TypeSection,
    symbols: &SymbolTable,
    next_type: &mut u32,
    next_function: &mut u32,
) -> Result<FunctionInvokeExports<'a>, crate::compiler_error::CompilerFailure> {
    let FunctionInvokePlan {
        arguments,
        return_type: source_return_type,
    } = plan;

    let public_run = crate::mangle::package_symbol(&ta.package_name, "run");
    let Some(export_entry) = ta
        .exports
        .iter()
        .find(|entry| entry.kind == ExportKind::Function && entry.public_name == public_run)
    else {
        return Err(crate::codegen::internal_failure(
            "callable function invoke plan requires an exported `run` function",
        ));
    };

    let unknown = unknown_type(symbols)?;
    let intrinsics = symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsic types registered"))?;

    let mut allocated_args = Vec::new();
    for (index, argument) in arguments.iter().enumerate() {
        let FunctionArgumentCheck::Checked(check) = argument else {
            continue;
        };
        let type_idx = *next_type;
        crate::codegen::next_index(next_type)?;
        types.ty().function([unknown], [unknown]);
        let func_idx = *next_function;
        crate::codegen::next_index(next_function)?;
        allocated_args.push(FunctionArgumentExport {
            index,
            check,
            type_idx,
            func_idx,
        });
    }

    let invoke_type_idx = *next_type;
    crate::codegen::next_index(next_type)?;
    types.ty().function(
        vec![unknown; arguments.len()],
        [ValType::Ref(RefType {
            nullable: true,
            heap_type: HeapType::Concrete(intrinsics.string),
        })],
    );
    let invoke_func_idx = *next_function;
    crate::codegen::next_index(next_function)?;

    Ok(FunctionInvokeExports {
        arguments: allocated_args,
        invoke_type_idx,
        invoke_func_idx,
        run_mangled: export_entry.target.clone(),
        source_return_type,
    })
}

pub(super) fn argument_body(
    ctx: &CodegenCtx,
    argument: &FunctionArgumentExport,
) -> Result<Function, crate::compiler_error::CompilerFailure> {
    let FinalizerArgumentCheck { target, check } = argument.check;
    ctx.checking_standalone(target, || {
        let params = [(
            Ident {
                name: "value".into(),
                span: Span::at(ctx.file),
            },
            unknown_type(ctx.symbols)?,
        )];
        let mut emitter = FunctionEmitter::new(ctx, &params)?;
        emitter.instruction(Instruction::LocalGet(0));
        super::cast_check::emit_shape_cast_on_stack(&mut emitter, ctx, target, check)?;
        super::function_emitter::cast::emit_box(&mut emitter, ctx, target)?;
        emitter.build()
    })
}

pub(super) fn invoke_body(
    ctx: &CodegenCtx,
    invoke: &FunctionInvokeExports,
) -> Result<Function, crate::compiler_error::CompilerFailure> {
    let run_func = ctx
        .ta
        .functions
        .iter()
        .find(|func| func.mangled_name == invoke.run_mangled)
        .ok_or_else(|| {
            crate::codegen::internal_failure("`run` function exists in lowered TypedAst")
        })?;
    let run_func_idx = ctx
        .symbols
        .top_level_fn(&invoke.run_mangled)
        .map(|meta| meta.wasm_idx)
        .ok_or_else(|| {
            crate::codegen::internal_failure("`run` function recorded in symbol table")
        })?;

    let unknown = unknown_type(ctx.symbols)?;
    let params: Vec<(Ident, ValType)> = run_func
        .params
        .iter()
        .map(|param| (param.name.clone(), unknown))
        .collect();
    let mut emitter = FunctionEmitter::new(ctx, &params)?;

    for (index, param) in run_func.params.iter().enumerate() {
        let local = crate::codegen::wasm_u32(index)?;
        emitter.instruction(Instruction::LocalGet(local));
        super::function_emitter::cast::emit_coerce_to_slot(
            &mut emitter,
            ctx,
            &Type::Unknown,
            &param.ty,
        )?;
    }

    emitter.instruction(Instruction::Call(run_func_idx));
    super::function_emitter::json::emit_function_json_return(
        &mut emitter,
        ctx,
        &run_func.return_type,
        invoke.source_return_type,
    )?;
    emitter.build()
}

fn unknown_type(symbols: &SymbolTable) -> Result<ValType, crate::compiler_error::CompilerFailure> {
    let intrinsics = symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsic types registered"))?;
    Ok(ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(intrinsics.object),
    }))
}
