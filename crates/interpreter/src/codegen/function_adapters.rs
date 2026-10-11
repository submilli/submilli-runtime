//! Collects top-level functions used in value position and emits closure-shaped adapter bodies for them.

use std::collections::{BTreeMap, BTreeSet};

use wasm_encoder::{
    CodeSection, ConstExpr, EntityType, GlobalSection, GlobalType, HeapType, ImportSection,
    Instruction, RefType, ValType,
};

use crate::codegen::CodegenCtx;
use crate::codegen::function_emitter::{FunctionEmitter, cast};
use crate::codegen::symbol_table::SymbolTable;
use crate::{Ident, MangledName, Span, Type, TypedAst};

#[derive(Clone, Debug)]
pub struct AdapterMeta {
    pub name: String,
    pub mangled: crate::MangledName,
    pub signature: Type,
}

/// The closure caches this module exports, from the name a consumer knows the
/// function by to the function it defines: its non-generic exported functions,
/// under each name they are exported as, and its classes' public static
/// methods. A consumer that reads one as a value imports the cache instead of
/// keeping its own, so the function is one closure across packages, as in
/// JavaScript.
pub fn exported_closure_caches(ta: &TypedAst) -> BTreeMap<MangledName, MangledName> {
    let defined: BTreeSet<&MangledName> = ta
        .functions
        .iter()
        .filter(|function| function.generics.is_empty())
        .map(|function| &function.mangled_name)
        .collect();
    let functions = ta
        .exports
        .iter()
        .filter(|entry| {
            entry.kind == crate::ExportKind::Function && defined.contains(&entry.target)
        })
        .map(|entry| (entry.public_name.clone(), entry.target.clone()));
    let static_methods = ta.types.iter().flat_map(|decl| {
        let crate::TypedTypeDecl::Class(class) = decl else {
            return Vec::new();
        };
        class
            .static_methods
            .iter()
            .filter(|(_, visibility)| **visibility == crate::Visibility::Public)
            .map(|(name, _)| {
                let method = crate::mangle::static_member(&class.mangled_name, name);
                (method.clone(), method)
            })
            .collect()
    });
    functions.chain(static_methods).collect()
}

/// A closure cache another module may fill: typed `anyref`, because each
/// module reads it as the closure struct of its own view of the signature.
fn shared_cache_type() -> GlobalType {
    GlobalType {
        val_type: ValType::Ref(RefType {
            nullable: true,
            heap_type: HeapType::ANY,
        }),
        mutable: true,
        shared: false,
    }
}

/// Imports the closure cache of each dependency function this module reads as
/// a value, where the function's package exports one.
pub fn import_shared_closure_caches(
    metas: &[AdapterMeta],
    dependencies: &[&crate::PackageDeclaration],
    imports: &mut ImportSection,
    symbols: &mut SymbolTable,
    next_global_idx: &mut u32,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    for meta in metas {
        let Some(package) = dependencies
            .iter()
            .find(|package| package.closure_caches.contains(&meta.mangled))
        else {
            continue;
        };
        imports.import(
            &package.package_name,
            crate::mangle::closure_cache(&meta.mangled).as_str(),
            EntityType::Global(shared_cache_type()),
        );
        symbols.record_shared_closure_global_idx(meta.mangled.clone(), *next_global_idx);
        crate::codegen::next_index(next_global_idx)?;
    }
    Ok(())
}

/// Allocates the globals that cache a function's closure, so every read of a
/// top-level function yields the same value and `f === f` holds. The closure
/// has no environment, so one instance serves every read. Each starts null and
/// is filled on first read. Each function this module exports gets one shared
/// `anyref` cache; any other function read here gets a typed cache of its own,
/// unless it imported a shared one. Returns how many globals were added.
pub fn allocate_closure_globals(
    metas: &[AdapterMeta],
    exported: &BTreeMap<MangledName, MangledName>,
    globals: &mut GlobalSection,
    symbols: &mut SymbolTable,
    next_global_idx: &mut u32,
) -> Result<u32, crate::compiler_error::CompilerFailure> {
    let shared: BTreeSet<&MangledName> = exported.values().collect();
    let mut count = 0usize;
    for meta in metas {
        let has_imported_cache = symbols.adapter_closure_global_idx(&meta.mangled).is_some();
        if shared.contains(&meta.mangled) || has_imported_cache {
            continue;
        }
        let closure_struct_idx = symbols
            .closure_struct_type_idx(super::closures::classify(&meta.signature)?)
            .ok_or_else(|| {
                crate::codegen::internal_failure(
                    "closure struct type registered for every function-as-value",
                )
            })?;
        let heap_type = HeapType::Concrete(closure_struct_idx);
        globals.global(
            GlobalType {
                val_type: ValType::Ref(RefType {
                    nullable: true,
                    heap_type,
                }),
                mutable: true,
                shared: false,
            },
            &ConstExpr::ref_null(heap_type),
        );
        symbols.record_adapter_closure_global_idx(meta.mangled.clone(), *next_global_idx);
        crate::codegen::next_index(next_global_idx)?;
        count += 1;
    }
    for mangled in shared {
        globals.global(shared_cache_type(), &ConstExpr::ref_null(HeapType::ANY));
        symbols.record_shared_closure_global_idx(mangled.clone(), *next_global_idx);
        crate::codegen::next_index(next_global_idx)?;
        count += 1;
    }
    crate::codegen::wasm_u32(count)
}

pub fn emit_bodies(
    metas: &[AdapterMeta],
    code: &mut CodeSection,
    ctx: &CodegenCtx<'_>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let intrinsics = ctx
        .symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared by codegen entry"))?;
    // adapter erased-arg slots are nullable to match the
    // closure ABI.
    let object_ref = ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(intrinsics.object),
    });
    let any_ref = ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::ANY,
    });
    for meta in metas {
        let target_idx = ctx.symbols.func_idx(&meta.mangled).ok_or_else(|| {
            crate::codegen::internal_failure(
                "adapter target function recorded during user-function pre-pass",
            )
        })?;
        let Type::Function { params, .. } = &meta.signature else {
            return Err(super::internal_failure(
                "adapter signature requires a function type",
            ));
        };

        super::closures::classify(&meta.signature)?;

        // names are unused; adapter bodies access params by Wasm slot index
        let env_name = Ident {
            name: "$__env__".to_string(),
            span: Span::at(ctx.file),
        };
        let mut wasm_params: Vec<(Ident, ValType)> = vec![(env_name, any_ref)];
        for (i, _) in params.iter().enumerate() {
            wasm_params.push((
                Ident {
                    name: format!("$__arg{i}__"),
                    span: Span::at(ctx.file),
                },
                object_ref,
            ));
        }
        let mut emitter = FunctionEmitter::new(ctx, &wasm_params)?;

        let target = ctx
            .symbols
            .top_level_fn(&meta.mangled)
            .ok_or_else(|| crate::codegen::internal_failure("adapter target signature recorded"))?;
        if target.params.len() != params.len() {
            return Err(super::internal_failure(
                "adapter signature disagrees with its target",
            ));
        }
        for (i, p_ty) in target.params.iter().enumerate() {
            emitter.instruction(Instruction::LocalGet(super::parameter_local(i)?));
            let default = ctx
                .symbols
                .host_call_defaults(target_idx, target.params.len())
                .get(i)
                .and_then(Option::as_ref);
            super::argument_defaults::emit_argument(
                &mut emitter,
                ctx,
                &Type::Unknown,
                object_ref,
                default,
            )?;
            crate::codegen::cast_check::emit_checked_parameter_cast_on_stack(
                &mut emitter,
                ctx,
                &crate::Type::Unknown,
                p_ty,
            )?;
        }
        emitter.instruction(Instruction::Call(target_idx));
        if ctx.symbols.resultless_functions.contains(&target_idx) {
            super::function_emitter::expr::emit_undefined(&mut emitter, ctx)?;
        } else {
            cast::emit_box(&mut emitter, ctx, &target.ret)?;
        }
        let built = emitter.build()?;
        code.function(&built);
    }
    Ok(())
}
