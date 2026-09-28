//! Codegen — translates a typechecked Typed AST into a Wasm module.

pub mod analysis;
pub mod bigint_pool;
pub mod bounds;
pub mod box_types;
mod call_arguments;
pub mod cast_check;
mod cast_diagnostics;
pub mod classes;
mod closure_coercions;
pub(crate) use closure_coercions::ADAPTER_ORIGINAL_FIELD;
pub mod closures;
pub mod dependency_usage;
pub mod dwarf;
mod field_guards;
mod field_lookup;
pub mod field_name_strings;
pub mod field_names;
pub mod function_adapters;
pub mod function_emitter;
pub mod imported_classes;
pub mod intrinsics;
pub mod recursive_validators;
mod runtime_descriptors;
mod runtime_values;
pub mod string_pool;
pub mod symbol_table;
mod this_binding;
pub mod throw;
pub mod user_subtypes;
mod vtable_walk;

use crate::compiler_error::{CompilerFailure, CompilerStage};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use wasm_encoder::{
    AbstractHeapType, CodeSection, ConstExpr, CustomSection, DataCountSection, DataSection,
    EntityType, ExportKind as WasmExportKind, ExportSection, FunctionSection, GlobalSection,
    GlobalType, HeapType, Ieee64, ImportSection, Instruction, Module, RefType, StartSection,
    TagKind, TagType, TypeSection, ValType,
};

use crate::{
    LineIndex, MangledName, PackageDeclaration, Param, StmtId, Type, TypeKind, TypedAst, ValueKind,
    ValueSymbol,
};
use analysis::CodegenAnalysis;
use bigint_pool::BigIntPool;
use function_emitter::FunctionEmitter;
use function_emitter::stmt::emit_statement;
use string_pool::StringPool;

/// The `Dispatch::Static` interface `ty` names in `defs`, if any. Constructor
/// and namespace receiver bindings (`console`, `Map`, `Temporal.Instant`) are
/// typed by these; every call site drops the receiver, so the bindings are
/// inert — they lower to a typed null and import no global.
fn static_interface_of<'a>(
    defs: &'a PackageDeclaration,
    ty: &Type,
) -> Option<&'a crate::TypeSymbol> {
    fn find<'a>(
        types: &'a std::collections::BTreeMap<String, crate::TypeSymbol>,
        namespaces: &'a std::collections::BTreeMap<String, crate::NamespaceSymbol>,
        mangled: &crate::MangledName,
    ) -> Option<&'a crate::TypeSymbol> {
        types
            .values()
            .find(|t| t.mangled_name == *mangled)
            .or_else(|| {
                namespaces
                    .values()
                    .find_map(|ns| find(&ns.types, &ns.namespaces, mangled))
            })
    }
    let Type::InterfaceRef { mangled, .. } = ty.peel() else {
        return None;
    };
    find(&defs.types, &defs.namespaces, mangled).filter(|ts| {
        matches!(
            &ts.kind,
            TypeKind::Interface {
                dispatch: crate::Dispatch::Static,
                ..
            }
        )
    })
}

#[allow(clippy::too_many_arguments)]
fn import_value_symbol(
    name: &str,
    value: &ValueSymbol,
    defs: &PackageDeclaration,
    intrinsics: &intrinsics::IntrinsicTypeIndices,
    next_type_idx: &mut u32,
    next_func_idx: &mut u32,
    next_global_idx: &mut u32,
    types: &mut wasm_encoder::TypeSection,
    import_section: &mut wasm_encoder::ImportSection,
    symbols: &mut SymbolTable,
) {
    use wasm_encoder::{EntityType, GlobalType, ValType};
    match &value.kind {
        ValueKind::Function { params, ret, .. } => {
            let sig_idx = *next_type_idx;
            *next_type_idx += 1;
            // `submilli:json` is the last host package on the raw ABI: its host
            // fns take/return `$rawString` (codegen re-wraps at the boundary).
            // Every other host package speaks the real `$string`/`$Array` ABI,
            // so it goes through the normal `value_type` path, same as user modules.
            let raw_string_abi = defs.package_name.as_str() == crate::runtime::JSON_MODULE_NAME;
            let (mut param_types, result_types): (Vec<ValType>, Vec<ValType>) = if raw_string_abi {
                json_host_signature(name, params, symbols, intrinsics)
            } else {
                (
                    params.iter().map(|p| symbols.value_type(&p.ty)).collect(),
                    symbols.wasm_result(ret),
                )
            };
            if defs.runtime_generics.contains(&value.mangled_name) {
                symbols
                    .runtime_generic_functions
                    .insert(value.mangled_name.clone());
                param_types.push(runtime_descriptors::environment_type(symbols));
            }
            types.ty().function(param_types, result_types);
            import_section.import(
                defs.package_name.as_str(),
                value.mangled_name.as_str(),
                EntityType::Function(sig_idx),
            );
            if let Some(metadata) = call_arguments::metadata(
                params
                    .iter()
                    .map(|param| (param.default.as_ref(), param.rest)),
            ) {
                symbols
                    .function_argument_metadata
                    .insert(value.mangled_name.clone(), metadata);
            }
            symbols.record_imported_fn(
                value.mangled_name.clone(),
                *next_func_idx,
                params.iter().map(|p| p.ty.clone()).collect(),
                ret.clone(),
                raw_string_abi,
            );
            *next_func_idx += 1;
        }
        ValueKind::Let { ty, .. } | ValueKind::Const { ty, .. } => {
            // Static-interface receiver bindings are inert (see
            // `static_interface_of`): record the dispatch so the emitter lowers
            // references to a typed null, and import nothing.
            if let Some(ts) = static_interface_of(defs, ty) {
                symbols.record_iface_dispatch(ts.mangled_name.clone(), crate::Dispatch::Static);
                return;
            }
            // Every exported value-global is Wasm-mutable across the board — a
            // codegen-compiled package needs its `_start` to initialize them, and
            // the prelude/host packages match that so the import type lines up.
            // Const immutability is typechecker-enforced, not Wasm-enforced.
            let global_ty = GlobalType {
                val_type: global_val_type(ty, symbols),
                mutable: true,
                shared: false,
            };
            import_section.import(
                defs.package_name.as_str(),
                value.mangled_name.as_str(),
                EntityType::Global(global_ty),
            );
            symbols.record_typed_global(value.mangled_name.clone(), *next_global_idx, ty.clone());
            *next_global_idx += 1;
        }
    }
}

/// The physical Wasm signature a Direct/Static-dispatch interface method is
/// called through, as its user-arg slots and result (the receiver is excluded).
///
/// Two declarations can describe one wrapper. A host-implemented method was
/// imported by the value-symbol loop from the declaration sitting beside its
/// Rust impl; the interface's `MethodSig` is the *language*-level view of the
/// same method, and the two can lower differently — Temporal's `with(fields)`
/// declares an interface-typed bag (`(ref null $Object)`) where the host takes
/// a structural object (`(ref $ObjectShape)`). The imported signature is the one
/// the call actually has to satisfy, so it wins.
///
/// `has_receiver` is false for `Dispatch::Static`, whose value symbols carry no
/// receiver param; every other value symbol declares the receiver as param 0
/// (`declare_method`'s contract). The user-arg count is the only cross-check
/// available: the receiver's *own* slot can't serve as one, because the two
/// import paths lower it differently — this loop pushes `receiver_wasm`
/// (`(ref $Object)`), while `import_value_symbol` runs the declared receiver
/// through `value_type`, which for an `InterfaceRef` yields the nullable
/// `(ref null $Object)`. A declaration that dropped the receiver while keeping
/// the count would slip through, so keep param 0 the receiver.
fn declared_wrapper_abi(
    symbols: &SymbolTable,
    mangled: &crate::MangledName,
    sig: &crate::MethodSig,
    has_receiver: bool,
) -> symbol_table::MethodSlotAbi {
    let receiver_offset = usize::from(has_receiver);
    if let Some(imported) = symbols.top_level_fn(mangled)
        && imported.params.len() == sig.params.len() + receiver_offset
    {
        return symbol_table::MethodSlotAbi {
            params: imported
                .params
                .iter()
                .skip(receiver_offset)
                .map(|ty| symbols.value_type(ty))
                .collect(),
            ret: symbols.wasm_result(&imported.ret).first().copied(),
        };
    }
    symbol_table::MethodSlotAbi {
        params: sig
            .params
            .iter()
            .map(|p| symbols.value_type(&p.ty))
            .collect(),
        ret: symbols.wasm_result(&sig.ret).first().copied(),
    }
}

#[allow(clippy::too_many_arguments)]
fn import_json_host_function(
    name: &str,
    intrinsics: &intrinsics::IntrinsicTypeIndices,
    next_type_idx: &mut u32,
    next_func_idx: &mut u32,
    types: &mut wasm_encoder::TypeSection,
    import_section: &mut wasm_encoder::ImportSection,
    symbols: &mut SymbolTable,
) {
    let mangled = crate::mangle::host(crate::runtime::JSON_MODULE_NAME, name);
    if symbols.func_idx(&mangled).is_some() {
        return;
    }

    let sig_idx = *next_type_idx;
    *next_type_idx += 1;
    let (param_types, result_types) = json_host_signature(name, &[], symbols, intrinsics);
    types.ty().function(param_types, result_types);
    import_section.import(
        crate::runtime::JSON_MODULE_NAME,
        mangled.as_str(),
        EntityType::Function(sig_idx),
    );
    symbols.record_func(mangled, *next_func_idx);
    *next_func_idx += 1;
}

pub use symbol_table::SymbolTable;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodegenError {
    message: &'static str,
    span: crate::Span,
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(out, "internal code generation error: {}", self.message)
    }
}

impl std::error::Error for CodegenError {}

impl From<CodegenError> for CompilerFailure {
    fn from(error: CodegenError) -> Self {
        Self::Internal {
            stage: CompilerStage::Codegen,
            span: Some(error.span),
            message: error.message.to_owned(),
        }
    }
}

impl CodegenError {
    pub fn diagnostic(self) -> crate::Diagnostic {
        crate::Diagnostic {
            severity: crate::Severity::Error,
            span: self.span,
            message: self.to_string(),
            help: vec!["report this compiler error with the source program".into()],
            notes: Vec::new(),
        }
    }
}

pub struct CodegenCtx<'a> {
    pub ta: &'a TypedAst,
    pub strings: &'a StringPool,
    pub bigints: &'a BigIntPool,
    pub symbols: &'a SymbolTable,
    pub source: &'a str,
    pub line_index: &'a LineIndex,
    /// The module being compiled; codegen-generated nodes (closure envs, adapter
    /// params, runtime validators) anchor their placeholder spans to it.
    pub file: crate::FileId,
    /// Runtime-validator bodies for expanding recursive aliases and data interfaces.
    pub validator_bodies: &'a recursive_validators::ValidatorBodies,
    pub type_info: &'a crate::TypeInfoTable,
    pub package_string_global_idx: Option<u32>,
    failure: std::cell::Cell<Option<CodegenError>>,
}

impl CodegenCtx<'_> {
    /// Emission accumulates bytes locally; an internal failure discards the whole
    /// module at the codegen boundary, never returning partial Wasm to a caller.
    fn require<T>(&self, value: Option<T>, message: &'static str) -> Option<T> {
        if value.is_none() {
            self.fail(message);
        }
        value
    }

    fn check_failure(&self) -> Result<(), CodegenError> {
        match self.failure.take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn fail(&self, message: &'static str) {
        let first = self.failure.take().unwrap_or(CodegenError {
            message,
            span: crate::Span::at(self.file),
        });
        self.failure.set(Some(first));
    }
}

pub fn codegen(
    source: &str,
    filename: &str,
    file: crate::FileId,
    ta: &TypedAst,
    dependencies: &[&PackageDeclaration],
) -> Result<Vec<u8>, CompilerFailure> {
    Ok(codegen_with_type_info(source, filename, file, ta, dependencies)?.wasm)
}

/// Like [`codegen_with_type_info`], but names the module after `owning_package` instead of
/// [`TypedAst::package_name`].
///
/// The two differ for a package's test file. It compiles as a script, so its entry point must
/// stay mangled as `main` for the runner to find it, but the code is the package's and a gated
/// call from it must be attributed to the package — not to `main`, which `secrets.get` refuses
/// outright.
pub fn codegen_owned_by(
    owning_package: &str,
    source: &str,
    filename: &str,
    file: crate::FileId,
    ta: &TypedAst,
    dependencies: &[&PackageDeclaration],
) -> Result<GeneratedModule, CompilerFailure> {
    codegen_inner(
        source,
        filename,
        file,
        ta,
        dependencies,
        owning_package,
        None,
    )
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneratedModule {
    pub wasm: Vec<u8>,
    pub type_info: crate::TypeInfoTable,
    pub runtime_functions: BTreeMap<crate::MangledName, crate::RuntimeFunction>,
    pub runtime_globals: BTreeMap<crate::MangledName, Type>,
}

pub fn codegen_with_type_info(
    source: &str,
    filename: &str,
    file: crate::FileId,
    ta: &TypedAst,
    dependencies: &[&PackageDeclaration],
) -> Result<GeneratedModule, CompilerFailure> {
    codegen_inner(
        source,
        filename,
        file,
        ta,
        dependencies,
        &ta.package_name,
        None,
    )
}

pub fn codegen_package_with_type_info(
    sources: &crate::Sources,
    file: crate::FileId,
    ta: &TypedAst,
    dependencies: &[&PackageDeclaration],
) -> Result<GeneratedModule, CompilerFailure> {
    let source = sources.get(file).ok_or_else(|| {
        crate::source::SourceError::UnknownFile { file }
            .into_compiler_failure(CompilerStage::Codegen)
    })?;
    codegen_inner(
        source.text(),
        source.path.as_str(),
        file,
        ta,
        dependencies,
        &ta.package_name,
        Some(sources),
    )
}

fn codegen_inner(
    source: &str,
    filename: &str,
    file: crate::FileId,
    ta: &TypedAst,
    dependencies: &[&PackageDeclaration],
    owning_package: &str,
    sources: Option<&crate::Sources>,
) -> Result<GeneratedModule, CompilerFailure> {
    u32::try_from(source.len()).map_err(|_| CompilerFailure::Limit {
        stage: CompilerStage::Codegen,
        span: None,
        message: "source exceeds the 32-bit source-offset limit".into(),
        help: vec!["split the source into smaller modules".into()],
    })?;
    let line_index = LineIndex::new(source)
        .map_err(|error| error.into_compiler_failure(CompilerStage::Codegen))?;
    let debug_sources = dwarf::DebugSources {
        file,
        filename,
        index: &line_index,
        sources,
    };
    for id in ta
        .expr_ids()
        .map_err(|error| error.into_compiler_failure(CompilerStage::Codegen))?
    {
        let expr = ta
            .try_expr(id)
            .map_err(|error| error.into_compiler_failure(CompilerStage::Codegen))?;
        debug_sources.location(expr.span)?;
    }
    for id in ta
        .stmt_ids()
        .map_err(|error| error.into_compiler_failure(CompilerStage::Codegen))?
    {
        let stmt = ta
            .try_stmt(id)
            .map_err(|error| error.into_compiler_failure(CompilerStage::Codegen))?;
        debug_sources.location(stmt.span)?;
    }
    let source_main_return_ty = ta
        .functions
        .iter()
        .find(|function| function.name.name == "main")
        .map(|function| function.return_type.clone());
    let lowered = runtime_values::lower(ta, dependencies);
    let ta = &lowered;
    let lowered_dependencies: Vec<_> = dependencies
        .iter()
        .map(|defs| runtime_values::lower_declaration(defs))
        .collect();
    let dependencies: Vec<_> = lowered_dependencies.iter().collect();
    let dependencies = dependencies.as_slice();
    let analysis = CodegenAnalysis::collect(ta, dependencies);
    let pool = analysis.string_pool;
    let bigint_pool = analysis.bigint_pool;
    let dependency_usage = analysis.dependency_usage.finish(dependencies);

    let mut module = Module::new();
    let mut types = TypeSection::new();
    let mut import_section = ImportSection::new();
    let mut symbols = SymbolTable::default();
    let mut next_type_idx: u32 = 0;
    let mut next_func_idx: u32 = 0;

    // Intrinsic types hard-coded locally in every consumer module; WasmGC canonicalization
    // unifies them across modules at instantiation. Instance-interface types (e.g. $TextEncoder)
    // stay in their owning module — consumers see them as (ref null $Object) via InterfaceRef.
    let intrinsics = intrinsics::declare_intrinsic_types(&mut types);
    symbols.set_intrinsic_type_indices(intrinsics);
    next_type_idx += intrinsics::INTRINSIC_TYPE_COUNT;
    field_names::declare_optional_name_type(
        &mut types,
        &mut symbols,
        &mut next_type_idx,
        intrinsics,
    );

    // The exception tag — host-owned (created per store, linker-defined under
    // `submilli:prelude`) and imported unconditionally: every module can
    // throw via bounds/cast/assert even when the source never mentions `Error`.
    // There is exactly one tag, by design (only `Error`s are throwable), so it's
    // fixed codegen plumbing, not a declaration entry. The
    // engine matches tag imports by exact canonical type identity, which holds
    // because the payload is an intrinsic every module declares byte-identically.
    // Sole tag import -> tag index 0.
    let error_tag_func_type_idx = next_type_idx;
    types.ty().function(
        [ValType::Ref(RefType {
            nullable: false,
            heap_type: HeapType::Concrete(intrinsics.error),
        })],
        Vec::<ValType>::new(),
    );
    next_type_idx += 1;
    import_section.import(
        crate::runtime::prelude::MODULE_NAME,
        crate::mangle::prelude("__error_tag").as_str(),
        EntityType::Tag(TagType {
            kind: TagKind::Exception,
            func_type_idx: error_tag_func_type_idx,
        }),
    );
    symbols.set_error_tag_idx(0);

    // Recursive cast and narrowed-field targets compile to validator functions;
    // discover them up front so their internal object shapes contribute field-name
    // globals even when no literal of that shape appears in the program.
    let dependency_types = dependency_usage.dependency_types(dependencies);
    let validator_bodies =
        recursive_validators::ValidatorBodies::collect(ta, dependency_types.iter());
    let recursive_validators =
        recursive_validators::discover(ta, dependency_types.iter(), &validator_bodies);
    let mut all_shapes = ta.shapes.clone();
    all_shapes.extend(recursive_validators.extra_shapes.iter().cloned());

    let dependency_shapes = dependency_usage.dependency_shapes(dependencies);
    let user_emitted_types: Vec<Type> =
        user_subtypes::collect_object_shapes(dependency_shapes.iter().copied(), &all_shapes);
    let type_info = crate::TypeInfoTable::collect_from_shapes_and_types(
        ta.package_name.clone(),
        &all_shapes,
        &user_emitted_types,
    );
    let needs_host_object_to_json =
        user_subtypes::needs_host_object_to_json_adapter(&user_emitted_types);

    // Closure types emitted before boxes: value_type(Type::Function) needs the closure type index.
    // Env structs come after boxes since their fields may reference (ref $box_T).
    let closure_metas = analysis.closure_metas;
    let adapter_metas = analysis.adapter_metas;
    let mut mentioned_closure_sigs = analysis.mentioned_closure_sigs;
    // Validator discovery can require predicate closures even when source
    // expressions never construct or call a closure of this signature.
    if !recursive_validators.descriptor_types.is_empty() {
        mentioned_closure_sigs.push(field_guards::signature());
    }
    let dependency_values = dependency_usage.dependency_values(dependencies);
    let hof_dependency_sigs = closures::collect_from_dependencies(
        dependency_values.iter().map(|value| value.symbol),
        dependency_shapes.iter().copied(),
        dependency_types.iter(),
        &dependency_usage,
    );
    let class_member_sigs = closures::class_member_sigs(ta, dependencies);
    // The closures this module builds, then the three collectors for shapes it
    // only names — see the `closures` module doc for why each is needed.
    closures::emit_func_and_struct_types(
        closure_metas
            .iter()
            .map(|m| closures::classify(&m.signature))
            .chain(
                adapter_metas
                    .iter()
                    .map(|a| closures::classify(&a.signature)),
            )
            .chain(hof_dependency_sigs.iter().copied())
            .chain(class_member_sigs.iter().copied())
            .chain(mentioned_closure_sigs.iter().copied()),
        &mut types,
        &mut symbols,
        &mut next_type_idx,
    );

    // Box types emitted after user subtypes and closure types so their field refs resolve.
    let boxed_value_types = box_types::collect(ta, &symbols);
    box_types::emit(
        &boxed_value_types,
        &mut types,
        &mut symbols,
        &mut next_type_idx,
    );

    // Env structs must come after box_types::emit.
    closures::emit_env_types(&closure_metas, &mut types, &mut symbols, &mut next_type_idx);

    // Wasm uses separate index spaces for functions and globals.
    let mut next_global_idx: u32 = 0;

    // Reconstruct imported classes (their rec groups + function imports + vtable
    // globals) before the local class plan, so a local subclass can subtype an
    // imported parent and lay its fields out after the parent's (SUB-488).
    let imported_class_layouts = imported_classes::reconstruct(
        dependencies,
        ta,
        &dependency_usage,
        intrinsics,
        file,
        &mut types,
        &mut next_type_idx,
        &mut import_section,
        &mut next_func_idx,
        &mut next_global_idx,
        &mut symbols,
    );

    // Class rec group last among types so class field slots can resolve string /
    // array / closure / box field types via `value_type`.
    let mut class_plan = classes::ClassPlan::collect(ta, &imported_class_layouts);
    class_plan.reserve_and_emit_types(&mut types, &mut next_type_idx, &mut symbols, ta, intrinsics);
    for value in &dependency_values {
        let defs = value.package;
        // `@mcp/<server>` tools aren't per-tool Wasm imports: every call lowers to
        // the single `submilli:mcp.call` host fn (see `emit_mcp_call`), so skip
        // emitting imports for them. Their structural shapes are still collected
        // via `defs.shapes`.
        if defs.mcp_server.is_some() {
            continue;
        }
        import_value_symbol(
            value.export_name.as_ref(),
            value.symbol,
            defs,
            &intrinsics,
            &mut next_type_idx,
            &mut next_func_idx,
            &mut next_global_idx,
            &mut types,
            &mut import_section,
            &mut symbols,
        );
    }

    // VTable globals are not in PackageDeclaration — there is no language-level Type for $VTable.
    let vtable_ref = ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.vtable),
    });
    // The object-identity and boxed-primitive vtables are host-owned; the rest
    // are still prelude-built. The mangled lookup key stays `prelude(name)`
    // either way, so downstream consumers are unaffected by the source module.
    for (module, name) in [
        (crate::runtime::prelude::MODULE_NAME, "string_vtable"),
        (crate::runtime::prelude::MODULE_NAME, "boxed_number_vtable"),
        (crate::runtime::prelude::MODULE_NAME, "boxed_boolean_vtable"),
        (crate::runtime::prelude::MODULE_NAME, "array_vtable"),
    ] {
        let mangled = crate::mangle::prelude(name);
        import_section.import(
            module,
            mangled.as_str(),
            EntityType::Global(GlobalType {
                val_type: vtable_ref,
                mutable: false,
                shared: false,
            }),
        );
        symbols.record_global(crate::mangle::prelude(name), next_global_idx);
        next_global_idx += 1;
    }
    if dependency_usage.uses_bigint() {
        let mangled = crate::mangle::prelude("bigint_vtable");
        import_section.import(
            crate::runtime::prelude::MODULE_NAME,
            mangled.as_str(),
            EntityType::Global(GlobalType {
                val_type: vtable_ref,
                mutable: false,
                shared: false,
            }),
        );
        symbols.record_global(crate::mangle::prelude("bigint_vtable"), next_global_idx);
        next_global_idx += 1;
    }

    // Bigint host calls use `(ref $rawBigInt) + i32 sign`, which is not expressible
    // in the language-level Type/Param model. Import only the calls this module emits.
    let raw_bigint_val_type = ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.raw_bigint),
    });
    let mut bigint_binop_sig_idx = None;
    let mut bigint_unary_sig_idx = None;
    let mut bigint_cmp_sig_idx = None;
    let mut bigint_from_number_sig_idx = None;
    let mut bigint_from_string_sig_idx = None;
    let mut bigint_to_string_sig_idx = None;
    let mut number_from_bigint_sig_idx = None;
    let mut import_bigint_host = |name: &str, sig_idx: u32| {
        let mangled = crate::mangle::host(crate::runtime::BIGINT_MODULE_NAME, name);
        import_section.import(
            crate::runtime::BIGINT_MODULE_NAME,
            mangled.as_str(),
            EntityType::Function(sig_idx),
        );
        symbols.record_func(mangled, next_func_idx);
        next_func_idx += 1;
    };
    for name in ["add", "sub", "mul", "div", "mod", "pow"] {
        if dependency_usage.is_host_value_used(crate::runtime::BIGINT_MODULE_NAME, name) {
            let sig_idx = *bigint_binop_sig_idx.get_or_insert_with(|| {
                let sig_idx = next_type_idx;
                next_type_idx += 1;
                types.ty().function(
                    [
                        ValType::I32,
                        raw_bigint_val_type,
                        ValType::I32,
                        raw_bigint_val_type,
                    ],
                    [ValType::I32, raw_bigint_val_type],
                );
                sig_idx
            });
            import_bigint_host(name, sig_idx);
        }
    }
    if dependency_usage.is_host_value_used(crate::runtime::BIGINT_MODULE_NAME, "cmp") {
        let sig_idx = *bigint_cmp_sig_idx.get_or_insert_with(|| {
            let sig_idx = next_type_idx;
            next_type_idx += 1;
            types.ty().function(
                [
                    ValType::I32,
                    raw_bigint_val_type,
                    ValType::I32,
                    raw_bigint_val_type,
                ],
                [ValType::I32],
            );
            sig_idx
        });
        import_bigint_host("cmp", sig_idx);
    }
    if dependency_usage.is_host_value_used(crate::runtime::BIGINT_MODULE_NAME, "neg") {
        let sig_idx = *bigint_unary_sig_idx.get_or_insert_with(|| {
            let sig_idx = next_type_idx;
            next_type_idx += 1;
            types.ty().function(
                [ValType::I32, raw_bigint_val_type],
                [ValType::I32, raw_bigint_val_type],
            );
            sig_idx
        });
        import_bigint_host("neg", sig_idx);
    }
    if dependency_usage.is_host_value_used(crate::runtime::BIGINT_MODULE_NAME, "fromNumber") {
        let sig_idx = *bigint_from_number_sig_idx.get_or_insert_with(|| {
            let sig_idx = next_type_idx;
            next_type_idx += 1;
            types
                .ty()
                .function([ValType::F64], [ValType::I32, raw_bigint_val_type]);
            sig_idx
        });
        import_bigint_host("fromNumber", sig_idx);
    }
    if dependency_usage.is_host_value_used(crate::runtime::BIGINT_MODULE_NAME, "fromString") {
        let raw_string_val_type = ValType::Ref(RefType {
            nullable: false,
            heap_type: HeapType::Concrete(intrinsics.raw_string),
        });
        let sig_idx = *bigint_from_string_sig_idx.get_or_insert_with(|| {
            let sig_idx = next_type_idx;
            next_type_idx += 1;
            types
                .ty()
                .function([raw_string_val_type], [ValType::I32, raw_bigint_val_type]);
            sig_idx
        });
        import_bigint_host("fromString", sig_idx);
    }
    if dependency_usage.is_host_value_used(crate::runtime::BIGINT_MODULE_NAME, "toString") {
        let raw_string_val_type = ValType::Ref(RefType {
            nullable: false,
            heap_type: HeapType::Concrete(intrinsics.raw_string),
        });
        let sig_idx = *bigint_to_string_sig_idx.get_or_insert_with(|| {
            let sig_idx = next_type_idx;
            next_type_idx += 1;
            types
                .ty()
                .function([ValType::I32, raw_bigint_val_type], [raw_string_val_type]);
            sig_idx
        });
        import_bigint_host("toString", sig_idx);
    }
    if dependency_usage.is_host_value_used(crate::runtime::NUMBER_MODULE_NAME, "fromBigInt") {
        let sig_idx = *number_from_bigint_sig_idx.get_or_insert_with(|| {
            let sig_idx = next_type_idx;
            next_type_idx += 1;
            types
                .ty()
                .function([ValType::I32, raw_bigint_val_type], [ValType::F64]);
            sig_idx
        });
        let from_bigint = crate::mangle::host(crate::runtime::NUMBER_MODULE_NAME, "fromBigInt");
        import_section.import(
            crate::runtime::NUMBER_MODULE_NAME,
            from_bigint.as_str(),
            EntityType::Function(sig_idx),
        );
        symbols.record_func(from_bigint, next_func_idx);
        next_func_idx += 1;
    }

    let intrinsic_string_ref = ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.string),
    });
    let intrinsic_object_ref = ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.object),
    });
    for dependency_type in &dependency_types {
        let defs = dependency_type.package;
        let iface_name = dependency_type.name;
        let ty_sym = dependency_type.symbol;
        let TypeKind::Interface {
            dispatch,
            methods,
            properties,
            ..
        } = &ty_sym.kind
        else {
            continue;
        };
        if *dispatch == crate::Dispatch::VTable {
            continue;
        }
        // Record dispatch so emit_method_call can branch without re-walking dependencies.
        symbols.record_iface_dispatch(ty_sym.mangled_name.clone(), *dispatch);
        let intrinsic_bigint_ref = ValType::Ref(RefType {
            nullable: false,
            heap_type: HeapType::Concrete(intrinsics.bigint),
        });
        let receiver_wasm: Option<ValType> = match dispatch {
            crate::Dispatch::Static => None,
            crate::Dispatch::Direct => Some(match iface_name {
                "Number" => ValType::F64,
                "Boolean" => ValType::I32,
                "String" => intrinsic_string_ref,
                "BigInt" => intrinsic_bigint_ref,
                _ => intrinsic_object_ref,
            }),
            crate::Dispatch::VTable => unreachable!(),
        };
        for (method_name, sig) in methods {
            if !dependency_usage.is_interface_member_used(ty_sym, method_name) {
                continue;
            }
            // TODO(reified generics): `Response#json` is recognised by the typechecker
            // and lowered to `JSON.parse(body)`; it has no host export, so skip its
            // import (declaring it would import an unresolvable symbol and fail link).
            // Remove once reified generics let the shim host a real per-call validator
            // (SUB-363).
            if defs.package_name.as_str() == crate::stdlib::http::MODULE_NAME
                && iface_name == "Response"
                && method_name == "json"
            {
                continue;
            }
            let mangled = crate::mangle::extend(&ty_sym.mangled_name, method_name);
            // Recorded above the host-implemented `continue` below, not after it:
            // the methods that take that branch are precisely the ones whose call
            // sites have no other record of the erased slots.
            let abi = declared_wrapper_abi(&symbols, &mangled, sig, receiver_wasm.is_some());
            symbols.record_iface_method_abi(mangled.clone(), abi.clone());
            // Routing invariant: a method whose dispatch key resolved in the
            // earlier value-symbol loop is host-implemented (every prelude
            // method is — its value symbol is declared beside the Rust impl in
            // `runtime::prelude`); only interface members without one
            // (stdlib shim exports like `File#close`) import here.
            if symbols.func_idx(&mangled).is_some() {
                continue;
            }
            let mut params: Vec<ValType> = Vec::new();
            if let Some(recv) = receiver_wasm {
                params.push(recv);
            }
            params.extend(abi.params.iter().copied());
            let results: Vec<ValType> = abi.ret.into_iter().collect();
            let sig_idx = next_type_idx;
            next_type_idx += 1;
            types.ty().function(params, results);
            import_section.import(
                defs.package_name.as_str(),
                mangled.as_str(),
                EntityType::Function(sig_idx),
            );
            symbols.record_func(mangled, next_func_idx);
            next_func_idx += 1;
        }
        for (prop_name, sig) in properties {
            // An intrinsic member is emitted inline by codegen (`String#length`
            // → payload `struct.get` + `array.len`) — there is no getter to
            // import; record it so the emitter picks its inline lowering.
            if sig.intrinsic {
                symbols.record_intrinsic_member(crate::mangle::extend(
                    &ty_sym.mangled_name,
                    prop_name,
                ));
                continue;
            }
            if !dependency_usage.is_interface_member_used(ty_sym, prop_name) {
                continue;
            }
            let mangled = crate::mangle::extend(&ty_sym.mangled_name, prop_name);
            // Same routing invariant as the method loop above, for property
            // getters (`Set#size`) and static constants (`Number.EPSILON`) —
            // resolved as a func or a global respectively.
            if symbols.func_idx(&mangled).is_some() || symbols.global_idx(&mangled).is_some() {
                continue;
            }
            let Some(recv) = receiver_wasm else {
                // Static-interface property (`Number.EPSILON`): there is
                // no receiver to dispatch on, so it imports as a constant
                // global rather than a getter function. Mutable to match the
                // prelude's value-global exports (const-ness is typecheck-enforced).
                import_section.import(
                    defs.package_name.as_str(),
                    mangled.as_str(),
                    EntityType::Global(wasm_encoder::GlobalType {
                        val_type: symbols.value_type(&sig.ty),
                        mutable: true,
                        shared: false,
                    }),
                );
                symbols.record_typed_global(mangled, next_global_idx, sig.ty.clone());
                next_global_idx += 1;
                continue;
            };
            let params = vec![recv];
            let results = symbols.wasm_result(&sig.ty);
            let sig_idx = next_type_idx;
            next_type_idx += 1;
            types.ty().function(params, results);
            import_section.import(
                defs.package_name.as_str(),
                mangled.as_str(),
                EntityType::Function(sig_idx),
            );
            symbols.record_func(mangled, next_func_idx);
            next_func_idx += 1;
        }
    }
    if needs_host_object_to_json {
        import_json_host_function(
            "stringifyTypedObject",
            &intrinsics,
            &mut next_type_idx,
            &mut next_func_idx,
            &mut types,
            &mut import_section,
            &mut symbols,
        );
    }
    import_json_host_function(
        "parse",
        &intrinsics,
        &mut next_type_idx,
        &mut next_func_idx,
        &mut types,
        &mut import_section,
        &mut symbols,
    );

    // _start is always emitted even when empty, for uniform module shape.
    let start_type_idx = next_type_idx;
    next_type_idx += 1;
    types
        .ty()
        .function(Vec::<ValType>::new(), Vec::<ValType>::new());
    let start_func_idx = next_func_idx;
    next_func_idx += 1;

    // Allocate function indices before emitting bodies, enabling forward references.
    let mut user_funcs: Vec<UserFunc> = Vec::new();
    let mut user_func_type_idx: BTreeMap<MangledName, u32> = BTreeMap::new();
    for f in &ta.functions {
        let sig_idx = next_type_idx;
        next_type_idx += 1;
        let mut params: Vec<_> = f.params.iter().map(|p| symbols.value_type(&p.ty)).collect();
        if !f.generics.is_empty() {
            symbols
                .runtime_generic_functions
                .insert(f.mangled_name.clone());
            params.push(runtime_descriptors::environment_type(&symbols));
        }
        types
            .ty()
            .function(params, symbols.wasm_result(&f.return_type));
        let func_idx = next_func_idx;
        next_func_idx += 1;
        let param_types: Vec<Type> = f.params.iter().map(|p| p.ty.clone()).collect();
        if let Some(metadata) = call_arguments::typed_metadata(&f.params) {
            symbols
                .function_argument_metadata
                .insert(f.mangled_name.clone(), metadata);
        }
        symbols.record_local_fn(
            f.mangled_name.clone(),
            func_idx,
            param_types,
            f.return_type.clone(),
        );
        user_func_type_idx.insert(f.mangled_name.clone(), sig_idx);
        user_funcs.push(UserFunc {
            name: f.name.name.clone(),
            decl_span: f.name.span,
            type_idx: sig_idx,
            func_idx,
            body: f.body,
            return_type: f.return_type.clone(),
            params: f.params.clone(),
            generics: f.generics.clone(),
        });
    }

    // Coercions can construct closures even when this module has no literals.
    let needs_closure_coercions = !mentioned_closure_sigs.is_empty();
    let closure_methods = if closure_metas.is_empty()
        && adapter_metas.is_empty()
        && class_member_sigs.is_empty()
        && !needs_closure_coercions
    {
        None
    } else {
        Some(closures::allocate_methods(&mut next_func_idx))
    };

    for meta in &closure_metas {
        let func_idx = next_func_idx;
        next_func_idx += 1;
        symbols.record_closure_func_idx(meta.expr_id, func_idx);
    }

    for meta in &adapter_metas {
        let func_idx = next_func_idx;
        next_func_idx += 1;
        symbols.record_adapter_func_idx(meta.mangled.clone(), func_idx);
    }

    let closure_coercion_targets = if closure_methods.is_some() {
        closure_coercions::allocate(&mut symbols, &mut next_func_idx)
    } else {
        Vec::new()
    };

    // Allocated after user functions; vtable globals reference these via ref.func.
    let mut user_subtypes_alloc: Vec<user_subtypes::UserSubtype> = Vec::new();
    for ty in &user_emitted_types {
        if let Type::Object { .. } = ty {
            user_subtypes_alloc.push(user_subtypes::allocate_methods(ty, &mut next_func_idx));
        }
    }

    // Class method stubs + per-class getter/setter (vtable/header globals ref.func these).
    class_plan.allocate_funcs(&mut next_func_idx, &mut symbols);
    let instance_field_guards = field_guards::allocate(ta, &mut symbols, &mut next_func_idx);
    let type_descriptors = runtime_descriptors::allocate(
        ta,
        &recursive_validators.descriptor_types,
        &mut symbols,
        &mut next_func_idx,
    );

    let field_lookup_signature = field_lookup::allocate(
        &mut types,
        &mut symbols,
        &mut next_type_idx,
        &mut next_func_idx,
    );

    // Recursive runtime validators. Allocate signatures and indices, then
    // register each back-edge key so structural checks can call its plan.
    let object_ref_null = ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(intrinsics.object),
    });
    let raw_array_ref = ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.raw_array),
    });
    let raw_index_array_ref = ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.raw_index_array),
    });
    let mut runtime_validator_sigs: Vec<u32> = Vec::with_capacity(recursive_validators.plans.len());
    for plan in &recursive_validators.plans {
        let sig_idx = next_type_idx;
        next_type_idx += 1;
        types.ty().function(
            [
                object_ref_null,
                raw_array_ref,
                raw_index_array_ref,
                ValType::I32,
                runtime_descriptors::environment_type(&symbols),
            ],
            [ValType::I32],
        );
        let func_idx = next_func_idx;
        next_func_idx += 1;
        symbols.record_runtime_validator(plan.key.clone(), func_idx);
        runtime_validator_sigs.push(sig_idx);
    }

    // `main`'s result is encoded to its output `$string` entirely in wasm by the
    // `__main_output` shim (`() -> (ref $string)`): scalars render via `toString`
    // (a `string` passes through verbatim — no JSON quoting), and structured
    // returns route through the same `toJson` machinery as `JSON.stringify`. Codegen
    // — which has the typed return type — is the single source of truth for the
    // encoding; the host reads the shim's `$string` verbatim. Only `void` (no
    // output) skips the shim; `never`/`error` never produce a value to encode.
    let main_func = ta.functions.iter().find(|f| f.name.name == "main");
    let main_return_ty = main_func.map(|f| f.return_type.clone());
    let main_output_shim = match main_return_ty.as_ref().map(Type::peel) {
        None | Some(Type::Void | Type::Never | Type::Error) => None,
        Some(_) => {
            let sig_idx = next_type_idx;
            types.ty().function(
                Vec::<ValType>::new(),
                [ValType::Ref(RefType {
                    nullable: false,
                    heap_type: HeapType::Concrete(intrinsics.string),
                })],
            );
            // Last consumer of the counter — nothing is emitted after this shim.
            let func_idx = next_func_idx;
            Some((sig_idx, func_idx))
        }
    };

    let pkg_string_global_idx = if needs_host_object_to_json {
        let idx = next_global_idx;
        next_global_idx += 1;
        Some(idx)
    } else {
        None
    };

    let main_mangled = main_func.map(|f| f.mangled_name.clone());
    let main_func_idx = main_mangled.as_ref().map(|mangled| {
        symbols
            .func_idx(mangled)
            .expect("`main` recorded during user-function pre-pass")
    });

    module.section(&types);
    module.section(&import_section);

    let mut functions = FunctionSection::new();
    functions.function(start_type_idx);
    for f in &user_funcs {
        functions.function(f.type_idx);
    }
    if closure_methods.is_some() {
        closures::emit_method_function_entries(&mut functions, intrinsics);
    }
    for meta in &closure_metas {
        let sig_idx = symbols
            .closure_func_type_idx(closures::classify(&meta.signature))
            .expect("closures::emit_func_and_struct_types registered the funcref type");
        functions.function(sig_idx);
    }
    for meta in &adapter_metas {
        let sig_idx = symbols
            .closure_func_type_idx(closures::classify(&meta.signature))
            .expect("closures::emit_func_and_struct_types registered the adapter signature");
        functions.function(sig_idx);
    }
    closure_coercions::emit_entries(&closure_coercion_targets, &mut functions, &symbols);
    user_subtypes::emit_method_function_entries(&mut functions, &user_subtypes_alloc, intrinsics);
    class_plan.emit_function_entries(&mut functions, &symbols, intrinsics);
    for _ in 0..instance_field_guards.len() + type_descriptors.len() {
        functions.function(
            symbols
                .closure_func_type_idx(field_guards::signature())
                .expect("guard signature registered"),
        );
    }
    functions.function(field_lookup_signature);
    for &sig_idx in &runtime_validator_sigs {
        functions.function(sig_idx);
    }
    if let Some((sig_idx, _)) = main_output_shim {
        functions.function(sig_idx);
    }
    module.section(&functions);

    // Consumer-local globals are mutable so _start can initialize them; const immutability is typechecker-enforced.
    let mut globals = GlobalSection::new();
    let mut globals_count: u32 = 0;
    if pkg_string_global_idx.is_some() {
        globals.global(
            GlobalType {
                val_type: ValType::Ref(RefType {
                    nullable: true,
                    heap_type: HeapType::Concrete(intrinsics.string),
                }),
                mutable: true,
                shared: false,
            },
            &ConstExpr::ref_null(HeapType::Concrete(intrinsics.string)),
        );
        globals_count += 1;
    }
    for g in &ta.globals {
        let val_type = global_val_type(&g.ty, &symbols);
        globals.global(
            GlobalType {
                val_type,
                mutable: true,
                shared: false,
            },
            &default_const_expr(val_type),
        );
        symbols.record_typed_global(g.mangled_name.clone(), next_global_idx, g.ty.clone());
        next_global_idx += 1;
        globals_count += 1;
    }
    let vtable_globals_count = user_subtypes_alloc.len() as u32;
    user_subtypes::emit_vtable_globals(
        &mut globals,
        &mut user_subtypes_alloc,
        &mut symbols,
        &mut next_global_idx,
        intrinsics,
    );
    let mut field_names_shapes = field_names::collect(&user_emitted_types);
    // Classes carry a field-names array global too (header slot 1); add each
    // class's declaration-order field-name list, deduping against object shapes.
    for class_fields in class_plan.field_name_lists() {
        if !field_names_shapes.contains(&class_fields) {
            field_names_shapes.push(class_fields);
        }
    }
    let string_vtable_global_idx = symbols
        .prelude_global_idx("string_vtable")
        .expect("string_vtable imported from prelude in the hard-coded import bootstrap");
    let field_names_globals_count = field_names_shapes.len() as u32;
    field_names::emit(
        &field_names_shapes,
        &mut globals,
        &mut symbols,
        &mut next_global_idx,
        intrinsics,
        string_vtable_global_idx,
    );
    // Names that need a per-name `$string` global even when no object literal
    // of that shape appears in this module — field access and name-based
    // dispatch read the global by name. Object-literal field names come from
    // `user_emitted_types`; the cases below do not.
    let mut extra_field_names = analysis.extra_field_names;
    for dependency_type in &dependency_types {
        let ty_sym = dependency_type.symbol;
        if let TypeKind::Interface {
            methods,
            properties,
            dispatch: crate::Dispatch::VTable,
            ..
        } = &ty_sym.kind
        {
            for method_name in methods.keys() {
                if dependency_usage.is_interface_member_used(ty_sym, method_name) {
                    extra_field_names.push(method_name.clone());
                }
            }
            for property_name in properties.keys() {
                if dependency_usage.is_interface_member_used(ty_sym, property_name) {
                    extra_field_names.push(property_name.clone());
                }
            }
        } else if let TypeKind::Class { accessors, .. } = &ty_sym.kind {
            // An imported class's accessor getter/setter names + the property name
            // are scanned at runtime by the dynamic property path, so the consumer
            // needs their `$string` globals too.
            for acc in accessors {
                extra_field_names.push(acc.name().to_string());
                match acc {
                    crate::AccessorSig::Getter { name, .. } => {
                        extra_field_names.push(crate::codegen::classes::accessor_getter_name(name));
                    }
                    crate::AccessorSig::Setter { name, .. } => {
                        extra_field_names.push(crate::codegen::classes::accessor_setter_name(name));
                    }
                }
            }
        }
    }
    // Class field names need per-name `$string` globals for the getter's `ref.eq`/
    // `string_eq` comparisons.
    for class_fields in class_plan.field_name_lists() {
        extra_field_names.extend(class_fields.into_iter().map(|field| field.name));
    }
    // Accessor property names are scanned at runtime by the dynamic property path
    // even though they back no data slot, so they need their own `$string` globals.
    extra_field_names.extend(class_plan.accessor_property_names());
    let field_name_strings = field_name_strings::collect(&user_emitted_types, &extra_field_names);
    let field_name_strings_count = field_name_strings.len() as u32;
    field_name_strings::emit(
        &field_name_strings,
        &mut globals,
        &mut symbols,
        &mut next_global_idx,
        intrinsics,
        string_vtable_global_idx,
    );
    // Per-class header singletons.
    class_plan.emit_globals(&mut globals, &mut symbols, &mut next_global_idx, intrinsics);
    let mut closure_methods_for_emit = closure_methods;
    let closure_vtable_count = if let Some(methods) = closure_methods_for_emit.as_mut() {
        closures::emit_vtable_global(
            &mut globals,
            methods,
            &mut symbols,
            &mut next_global_idx,
            intrinsics,
        );
        1
    } else {
        0
    };
    let descriptor_globals_count =
        runtime_descriptors::allocate_globals(&mut globals, &mut symbols, &mut next_global_idx);
    if descriptor_globals_count
        + globals_count
        + vtable_globals_count
        + field_names_globals_count
        + field_name_strings_count
        + closure_vtable_count
        > 0
    {
        module.section(&globals);
    }

    let mut exports = ExportSection::new();
    if let Some(main_func_idx) = main_func_idx {
        exports.export("main", WasmExportKind::Func, main_func_idx);
    }
    if let Some((_, func_idx)) = main_output_shim {
        exports.export("__main_output", WasmExportKind::Func, func_idx);
    }
    for entry in &ta.exports {
        match entry.kind {
            crate::ExportKind::Function => {
                let func_idx = symbols
                    .top_level_fn(&entry.target)
                    .map(|f| f.wasm_idx)
                    .expect("checked package export function target exists");
                exports.export(entry.public_name.as_str(), WasmExportKind::Func, func_idx);
            }
            crate::ExportKind::Global => {
                let global_idx = symbols
                    .global_idx(&entry.target)
                    .expect("checked package export global target exists");
                exports.export(
                    entry.public_name.as_str(),
                    WasmExportKind::Global,
                    global_idx,
                );
            }
            crate::ExportKind::Type => {}
        }
    }
    // Export each exported class's entry points (constructor, ctor-init, method
    // bodies) so another package can construct, dispatch, and `extends` it. A
    // class export is an `ExportKind::Type` entry whose target is the class's
    // mangled name (SUB-488).
    // Hidden implementations also export their runtime entry points. Their
    // declarations stay in compiler-only metadata, outside the source API.
    let exported_class_mangles: BTreeSet<&MangledName> = ta
        .types
        .iter()
        .filter_map(|decl| match decl {
            crate::TypedTypeDecl::Class(class) => Some(&class.mangled_name),
            _ => None,
        })
        .collect();
    for (name, func_idx) in
        class_plan.exported_funcs(&symbols, |m| exported_class_mangles.contains(m))
    {
        exports.export(name.as_str(), WasmExportKind::Func, func_idx);
    }
    for (name, global_idx) in
        class_plan.exported_vtable_globals(&symbols, |m| exported_class_mangles.contains(m))
    {
        exports.export(name.as_str(), WasmExportKind::Global, global_idx);
    }
    module.section(&exports);

    module.section(&StartSection {
        function_index: start_func_idx,
    });

    // Wasm requires declarative element coverage for any function referenced by ref.func outside element segments.
    let mut declared = user_subtypes::declared_method_funcs(&user_subtypes_alloc);
    if let Some(methods) = closure_methods_for_emit {
        declared.extend(closures::declared_funcs(methods));
    }
    for meta in &closure_metas {
        let idx = symbols
            .closure_func_idx(meta.expr_id)
            .expect("closure func index allocated");
        declared.push(idx);
    }
    for meta in &adapter_metas {
        let idx = symbols
            .adapter_func_idx(&meta.mangled)
            .expect("adapter func index allocated");
        declared.push(idx);
    }
    declared.extend(
        closure_coercion_targets
            .iter()
            .map(|&sig| symbols.closure_coercion(sig).expect("coercion allocated")),
    );
    declared.extend(class_plan.declared_funcs(&symbols));
    declared.extend(instance_field_guards.iter().map(|guard| guard.function));
    declared.extend(type_descriptors.iter().map(|(_, function)| *function));
    if !declared.is_empty() {
        let mut elements = wasm_encoder::ElementSection::new();
        elements.declared(wasm_encoder::Elements::Functions(Cow::Owned(declared)));
        module.section(&elements);
    }

    // DataCount must precede Code per Wasm spec.
    let total_data_segments = pool.strings.len() + bigint_pool.literals.len();
    if total_data_segments > 0 {
        module.section(&DataCountSection {
            count: total_data_segments as u32,
        });
    }

    let ctx = CodegenCtx {
        ta,
        strings: &pool,
        bigints: &bigint_pool,
        symbols: &symbols,
        source,
        line_index: &line_index,
        file,
        validator_bodies: &validator_bodies,
        type_info: &type_info,
        package_string_global_idx: pkg_string_global_idx,
        failure: std::cell::Cell::new(None),
    };

    let mut code = CodeSection::new();
    let mut start_emitter = FunctionEmitter::new(&ctx, &[]);
    if let Some(pkg_string_global_idx) = pkg_string_global_idx {
        emit_package_string_init(
            &mut start_emitter,
            ta.package_name.as_str(),
            pkg_string_global_idx,
            intrinsics.raw_string,
            intrinsics.string,
            string_vtable_global_idx,
        );
    }
    for &stmt_id in &ctx.ta.top_level_statements {
        emit_statement(&mut start_emitter, &ctx, stmt_id);
        ctx.check_failure()?;
    }
    // CodeSection::byte_len excludes the leading vec-count LEB128; adjust to get Code-section-content offsets for DWARF.
    let start_body = start_emitter.build();
    code.function(&start_body);

    let mut user_func_ranges: Vec<(u64, u64)> = Vec::with_capacity(user_funcs.len());
    let mut user_func_lines: Vec<Vec<(u64, crate::Span)>> = Vec::with_capacity(user_funcs.len());
    for func in &user_funcs {
        let (built, lines) = function_emitter::emit_function(
            &func.generics,
            &ctx,
            &func.params,
            func.body,
            &func.return_type,
        );
        ctx.check_failure()?;
        let body_len = built.byte_len() as u64;
        code.function(&built);
        let low_in_buf = code.byte_len() as u64 - body_len;
        user_func_ranges.push((low_in_buf, body_len));
        user_func_lines.push(lines);
    }

    if let Some(methods) = closure_methods_for_emit {
        closures::emit_method_bodies(&mut code, methods, &symbols);
    }

    for meta in &closure_metas {
        let (built, _lines) = function_emitter::emit_closure_function(&ctx, meta);
        ctx.check_failure()?;
        code.function(&built);
    }

    function_adapters::emit_bodies(&adapter_metas, &mut code, &ctx);
    closure_coercions::emit_bodies(&closure_coercion_targets, &mut code, &ctx);

    user_subtypes::emit_method_bodies(
        &mut code,
        &user_subtypes_alloc,
        &symbols,
        &type_info,
        pkg_string_global_idx,
    );

    class_plan.emit_bodies(&mut code, &ctx);
    for guard in &instance_field_guards {
        code.function(&field_guards::body(&ctx, guard));
    }
    for (ty, _) in &type_descriptors {
        code.function(&runtime_descriptors::body(&ctx, ty));
    }

    code.function(&field_lookup::body(&ctx));
    for (validator_id, plan) in recursive_validators.plans.iter().enumerate() {
        code.function(&cast_check::emit_runtime_validator_body(
            &ctx,
            &plan.key,
            &plan.body,
            plan.rejects_polymorphic_edge,
            validator_id as i32,
        ));
    }

    if let (Some(_), Some(main_func_idx), Some(main_return_ty)) =
        (main_output_shim, main_func_idx, main_return_ty.as_ref())
    {
        code.function(&function_emitter::json::emit_main_output_shim(
            &ctx,
            main_func_idx,
            main_return_ty,
            source_main_return_ty.as_ref().unwrap_or(main_return_ty),
        ));
    }

    ctx.check_failure()?;
    module.section(&code);

    // DWARF sections appear after Code, before Data — LLVM/Emscripten convention.
    let vec_count_size = leb128_u32_size(code.len()) as u64;
    let code_content_size = vec_count_size + code.byte_len() as u64;

    let funcs: Vec<dwarf::FuncDebugInfo> = user_funcs
        .iter()
        .zip(user_func_ranges.iter())
        .zip(user_func_lines)
        .map(|((uf, (low_in_buf, body_len)), lines)| {
            let abs_low = low_in_buf + vec_count_size;
            dwarf::FuncDebugInfo {
                name: uf.name.clone(),
                low_pc: abs_low,
                body_len: *body_len,
                decl_span: uf.decl_span,
                lines: lines
                    .into_iter()
                    .map(|(off, span)| (abs_low + off, span))
                    .collect(),
            }
        })
        .collect();
    for (sect_name, bytes) in
        dwarf::build_dwarf(&funcs, code_content_size, filename, &debug_sources)?
    {
        module.section(&CustomSection {
            name: Cow::Borrowed(sect_name),
            data: Cow::Owned(bytes),
        });
    }

    // The owning package, for the runtime to read back off the innermost wasm frame. This is
    // the sole source of caller identity for capability checks, so it ships on every module —
    // a module without it resolves to no principal and its gated calls are refused.
    {
        let mut names = wasm_encoder::NameSection::new();
        names.module(owning_package);
        module.section(&names);
    }

    if total_data_segments > 0 {
        let mut data = DataSection::new();
        for i in 0..pool.strings.len() {
            data.passive(pool.utf16_le_bytes(i));
        }
        // Bigint segments indexed from pool.strings.len() at each array.new_data site.
        for i in 0..bigint_pool.literals.len() {
            data.passive(bigint_pool.le_bytes(i));
        }
        module.section(&data);
    }

    Ok(GeneratedModule {
        wasm: module.finish(),
        type_info,
        runtime_functions: runtime_values::signatures(ta),
        runtime_globals: runtime_values::global_types(ta),
    })
}

struct UserFunc {
    name: String,
    decl_span: crate::Span,
    type_idx: u32,
    #[allow(dead_code)]
    func_idx: u32,
    body: StmtId,
    return_type: Type,
    params: Vec<crate::TypedParam>,
    generics: Vec<String>,
}

/// Converts `CodeSection::byte_len` (excludes leading vec-count) into Code-section-content offsets for DWARF.
fn leb128_u32_size(mut v: u32) -> usize {
    let mut size = 0;
    loop {
        size += 1;
        v >>= 7;
        if v == 0 {
            return size;
        }
    }
}

fn json_host_signature(
    name: &str,
    params: &[Param],
    symbols: &SymbolTable,
    intrinsics: &intrinsics::IntrinsicTypeIndices,
) -> (Vec<wasm_encoder::ValType>, Vec<wasm_encoder::ValType>) {
    use wasm_encoder::{HeapType, RefType, ValType};

    let raw_string_ref = ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(intrinsics.raw_string),
    });
    match name {
        // json.parse returns the language's `unknown` slot.
        "parse" => (
            vec![raw_string_ref],
            vec![ValType::Ref(RefType {
                nullable: true,
                heap_type: HeapType::Concrete(intrinsics.object),
            })],
        ),
        "stringify" => (vec![raw_string_ref], vec![raw_string_ref]),
        "stringifyTypedObject" => (
            vec![
                raw_string_ref,
                ValType::I32,
                ValType::Ref(RefType {
                    nullable: false,
                    heap_type: HeapType::Abstract {
                        shared: false,
                        ty: AbstractHeapType::Struct,
                    },
                }),
            ],
            vec![raw_string_ref],
        ),
        "stringifyPrettyNumber" => (vec![raw_string_ref, ValType::F64], vec![raw_string_ref]),
        "stringifyPrettyString" => (vec![raw_string_ref, raw_string_ref], vec![raw_string_ref]),
        _ => (
            params
                .iter()
                .map(|p| symbols.host_value_type(&p.ty))
                .collect(),
            vec![],
        ),
    }
}

/// Reference-typed globals are nullable so the `ref.null` initializer validates;
/// `_start` fills the slot before user code runs and reads reattach `ref.as_non_null`.
///
/// Every consumer that *imports* such a global must declare it the same way:
/// the engine's import check is invariant on a mutable global's content type,
/// so `(mut (ref null $string))` and `(mut (ref $string))` do not link.
fn global_val_type(ty: &Type, im: &SymbolTable) -> ValType {
    match im.value_type(ty) {
        ValType::Ref(r) => ValType::Ref(RefType {
            nullable: true,
            ..r
        }),
        other => other,
    }
}

/// Zero/null const-expr placeholder — value is overwritten by _start before any user code runs.
fn default_const_expr(val_type: ValType) -> ConstExpr {
    match val_type {
        ValType::F64 => ConstExpr::f64_const(Ieee64::from(0.0_f64)),
        ValType::I32 => ConstExpr::i32_const(0),
        ValType::Ref(RefType { heap_type, .. }) => ConstExpr::ref_null(heap_type),
        other => unimplemented!("default const-expr for {other:?} arrives in a later codegen task"),
    }
}

fn emit_package_string_init(
    emitter: &mut FunctionEmitter<'_>,
    package_name: &str,
    pkg_string_global_idx: u32,
    raw_string_type_idx: u32,
    string_type_idx: u32,
    string_vtable_global_idx: u32,
) {
    emitter.instruction(Instruction::GlobalGet(string_vtable_global_idx));
    for code_unit in package_name.encode_utf16() {
        emitter.instruction(Instruction::I32Const(code_unit as i32));
    }
    emitter.instruction(Instruction::ArrayNewFixed {
        array_type_index: raw_string_type_idx,
        array_size: package_name.encode_utf16().count() as u32,
    });
    emitter.instruction(Instruction::StructNew(string_type_idx));
    emitter.instruction(Instruction::GlobalSet(pkg_string_global_idx));
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{SymbolTable, ValueSymbol, codegen, codegen_with_type_info};
    use crate::runtime::prelude;
    use crate::{
        Asi, ModulePath, NamespaceSymbol, ObjectField, PackageDeclaration, Sources, Token,
        TokenKind, Type, TypedAst, capture, check, desugar, infer, infer_package, lower_patterns,
        parse,
    };
    use wasmparser::{Parser, Payload};

    #[test]
    fn missing_record_import_returns_an_internal_error_without_wasm() {
        let source = "function main(): number | null { const d: Record<string, number> = {}; const key: string = 'x'; return d[key]; }";
        let ta = type_check(source);
        let (prelude_defs, host_defs, internal_defs) =
            prelude::cached_runtime_package_declarations();
        let mut dependencies: Vec<_> = prelude_defs
            .iter()
            .chain(host_defs.iter())
            .chain(internal_defs.iter())
            .cloned()
            .collect();
        for declaration in &mut dependencies {
            declaration.values.retain(|_, value| {
                !value
                    .mangled_name
                    .as_str()
                    .ends_with("ObjectConstructor##getField")
            });
        }
        let refs: Vec<_> = dependencies.iter().collect();
        let error = codegen_with_type_info(source, "record.ts", crate::FileId(0), &ta, &refs)
            .expect_err("missing internal import must not return a module");
        assert!(
            error.to_string().contains("dynamic read imported"),
            "{error}"
        );
        super::tests::compile("function main(): number { return 42; }");
    }

    fn type_check(source: &str) -> TypedAst {
        type_check_with_packages(source, &[])
    }

    fn type_check_with_packages(source: &str, packages: &[&PackageDeclaration]) -> TypedAst {
        let mut asi = Asi::new(source, crate::FileId(0));
        let mut tokens: Vec<Token> = Vec::new();
        loop {
            let tok = asi.next_token();
            let is_eof = matches!(tok.kind, TokenKind::Eof);
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        let lex_diags = asi.into_diagnostics();
        assert!(
            lex_diags.is_empty(),
            "unexpected lexer diags: {lex_diags:?}"
        );
        let (ast, parse_diags) = parse(source, tokens, crate::FileId(0));
        assert!(
            parse_diags.is_empty(),
            "unexpected parser diags: {parse_diags:?}"
        );
        let packages = runtime_packages(packages);
        let (mut ta, mut diags) = infer(source, "main", &ast, &packages);
        diags.extend(check(&ta));
        capture(&mut ta);
        desugar(&mut ta, crate::FileId(0));
        assert!(diags.is_empty(), "unexpected typecheck diags: {diags:?}");
        ta
    }

    fn runtime_packages<'a>(packages: &[&'a PackageDeclaration]) -> Vec<&'a PackageDeclaration> {
        let (prelude_defs, host_defs, _) = prelude::cached_runtime_package_declarations();
        let mut out = Vec::with_capacity(prelude_defs.len() + host_defs.len() + packages.len());
        out.extend(prelude_defs.iter());
        out.extend(host_defs.iter());
        out.extend(packages.iter().copied());
        out
    }

    fn infer_with_runtime_packages(source: &str, ast: &crate::Ast) -> Vec<crate::Diagnostic> {
        let packages = runtime_packages(&[]);
        let (_, diags) = infer(source, "main", ast, &packages);
        diags
    }

    fn parse_module(
        sources: &mut Sources,
        module: &str,
        source: &str,
    ) -> (ModulePath, crate::FileId, crate::Ast) {
        let file = sources.add(module.to_string(), source).unwrap();
        let mut asi = Asi::new(source, file);
        let mut tokens: Vec<Token> = Vec::new();
        loop {
            let tok = asi.next_token();
            let is_eof = matches!(tok.kind, TokenKind::Eof);
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        let lex_diags = asi.into_diagnostics();
        assert!(
            lex_diags.is_empty(),
            "unexpected lexer diags: {lex_diags:?}"
        );
        let (mut ast, parse_diags) = parse(source, tokens, file);
        assert!(
            parse_diags.is_empty(),
            "unexpected parser diags: {parse_diags:?}"
        );
        lower_patterns(&mut ast);
        (ModulePath::from(module), file, ast)
    }

    pub(crate) fn compile_package_modules(
        package_name: &str,
        modules: &[(&str, &str)],
        packages: &[&PackageDeclaration],
    ) -> (Vec<u8>, PackageDeclaration, crate::TypeInfoTable) {
        let mut sources = Sources::new();
        let owned_modules: Vec<_> = modules
            .iter()
            .map(|(module, source)| parse_module(&mut sources, module, source))
            .collect();
        let module_refs: Vec<_> = owned_modules
            .iter()
            .map(|(module, file, ast)| (module.clone(), *file, ast))
            .collect();
        let stdlib_defs = crate::runtime::stdlib_package_declarations();
        let mut external_packages: std::collections::BTreeMap<String, PackageDeclaration> =
            stdlib_defs
                .into_iter()
                .map(|defs| (defs.package_name.clone(), defs))
                .collect();
        let (prelude_defs, host_defs, _) = prelude::cached_runtime_package_declarations();
        for defs in prelude_defs {
            external_packages.insert(defs.package_name.clone(), defs.clone());
        }
        for defs in host_defs {
            external_packages.insert(defs.package_name.clone(), defs.clone());
        }
        for defs in packages {
            external_packages.insert(defs.package_name.clone(), (*defs).clone());
        }
        let (mut ta, mut package, diags) = infer_package(
            package_name,
            ModulePath::from("lib"),
            module_refs,
            &sources,
            external_packages,
            std::collections::BTreeMap::new(),
        );
        assert!(diags.is_empty(), "unexpected package diags: {diags:?}");
        capture(&mut ta);
        let root_file = owned_modules
            .iter()
            .find(|(module, _, _)| module.as_str() == "lib")
            .map(|(_, file, _)| *file)
            .expect("test package has lib module");
        desugar(&mut ta, root_file);

        let (prelude_defs, host_defs, internal_defs) =
            prelude::cached_runtime_package_declarations();
        let stdlib_defs = crate::runtime::stdlib_package_declarations();
        let mut dependencies: Vec<&PackageDeclaration> = prelude_defs.iter().collect();
        dependencies.extend(host_defs.iter());
        dependencies.extend(internal_defs.iter());
        dependencies.extend(stdlib_defs.iter());
        dependencies.extend_from_slice(packages);
        let generated =
            super::codegen_package_with_type_info(&sources, root_file, &ta, &dependencies)
                .expect("code generation");
        package.runtime_functions = generated.runtime_functions;
        package.runtime_globals = generated.runtime_globals;
        (generated.wasm, package, generated.type_info)
    }

    pub(crate) fn compile(source: &str) -> Vec<u8> {
        crate::compile::compile_script(source, "script.subm", crate::FileId(0), &[], &[])
            .expect("test-only compile expects no diagnostics")
            .wasm
    }

    #[test]
    fn compiled_type_info_includes_stringify_object_shapes() {
        let source = r#"
function main(): string {
  const original = "{\"id\":1,\"name\":\"alice\"}";
  const u = JSON.parse(original) as { id: number, name: string };
  return JSON.stringify(u);
}
"#;
        let compiled =
            crate::compile::compile_script(source, "script.subm", crate::FileId(0), &[], &[])
                .expect("script should compile");
        let ty = Type::Object {
            index: None,
            fields: std::collections::BTreeMap::from([
                ("id".to_string(), ObjectField::required(Type::Number)),
                ("name".to_string(), ObjectField::required(Type::String)),
            ]),
        };

        let id = compiled
            .type_info
            .object_type_id(&ty)
            .expect("compiled TypeInfo should contain the object stringify target");
        let info = compiled.type_info.get(id).expect("TypeInfo id is valid");
        assert!(matches!(
            &info.kind,
            crate::TypeInfoKind::Object { fields } if fields.len() == 2
        ));

        let ta = type_check(source);
        let emitted = super::user_subtypes::collect_object_shapes(std::iter::empty(), &ta.shapes);
        assert!(
            emitted
                .iter()
                .all(|ty| compiled.type_info.object_type_id(ty).is_some()),
            "every emitted object subtype should have TypeInfo"
        );
    }

    fn build_symbol_table(source: &str) -> SymbolTable {
        let _ta = type_check(source);
        let prelude_defs = prelude::prelude_package_declaration();
        let dependencies: [&crate::PackageDeclaration; 1] = [&prelude_defs];
        let mut map = SymbolTable::default();
        let mut next_type_idx: u32 = 0;
        let mut next_func_idx: u32 = 0;
        let mut next_global_idx: u32 = 0;
        map.set_intrinsic_type_indices(super::intrinsics::IntrinsicTypeIndices {
            raw_string: 0,
            vtable: 1,
            object: 2,
            string: 3,
            boxed_number: 4,
            boxed_boolean: 5,
            field_names: 6,
            object_fields: 7,
            object_shape: 8,
            to_string_fn: 9,
            to_json_fn: 10,
            equals_fn: 11,
            hash_fn: 12,
            field_getter: 13,
            field_setter: 14,
            raw_array: 15,
            array: 16,
            raw_uint8_array: 17,
            uint8_array: 18,
            closure: 19,
            class_vtable: 20,
            error_vtable: 21,
            error: 22,
            raw_bigint: 23,
            bigint: 24,
            regex_capture_array: 25,
            regex_match: 26,
            regex: 27,
            regex_match_box: 28,
            temporal_instant: 29,
            temporal_duration: 30,
            temporal_zdt: 31,
            raw_index_array: 32,
            map: 33,
            set: 34,
            url: 35,
            fs_stat: 41,
            fs_peek: 42,
            fs_dir_entry: 43,
            fs_info: 44,
            fs_file_writer: 45,
            http_response: 46,
            http_download_result: 47,
            session_entry: 48,
            session_page: 49,

            temporal_plain_date: 36,
            temporal_plain_time: 37,
            temporal_plain_date_time: 38,
            temporal_plain_year_month: 39,
            temporal_plain_month_day: 40,
        });
        next_type_idx += super::intrinsics::INTRINSIC_TYPE_COUNT;
        let _ = next_type_idx;
        for defs in &dependencies {
            for value in defs.values.values() {
                record(&mut map, value, &mut next_func_idx, &mut next_global_idx);
            }
            walk_test_namespaces(
                &defs.namespaces,
                &mut map,
                &mut next_func_idx,
                &mut next_global_idx,
            );
        }
        map
    }

    fn record(
        map: &mut SymbolTable,
        value: &ValueSymbol,
        next_func_idx: &mut u32,
        next_global_idx: &mut u32,
    ) {
        match &value.kind {
            crate::ValueKind::Function { .. } => {
                map.record_func(value.mangled_name.clone(), *next_func_idx);
                *next_func_idx += 1;
            }
            crate::ValueKind::Let { .. } | crate::ValueKind::Const { .. } => {
                map.record_global(value.mangled_name.clone(), *next_global_idx);
                *next_global_idx += 1;
            }
        }
    }

    fn walk_test_namespaces(
        map_in: &std::collections::BTreeMap<String, NamespaceSymbol>,
        map: &mut SymbolTable,
        next_func_idx: &mut u32,
        next_global_idx: &mut u32,
    ) {
        for ns in map_in.values() {
            for value in ns.values.values() {
                record(map, value, next_func_idx, next_global_idx);
            }
            walk_test_namespaces(&ns.namespaces, map, next_func_idx, next_global_idx);
        }
    }

    fn function_count(bytes: &[u8]) -> usize {
        for payload in Parser::new(0).parse_all(bytes) {
            if let Payload::FunctionSection(reader) = payload.expect("payload") {
                return reader.count() as usize;
            }
        }
        0
    }

    fn imports(bytes: &[u8]) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for payload in Parser::new(0).parse_all(bytes) {
            if let Payload::ImportSection(reader) = payload.expect("payload") {
                for entry in reader {
                    match entry.expect("import") {
                        wasmparser::Imports::Single(_, imp) => {
                            out.push((imp.module.to_string(), imp.name.to_string()));
                        }
                        other => panic!("unexpected import form {other:?}"),
                    }
                }
            }
        }
        out
    }

    fn global_count(bytes: &[u8]) -> usize {
        for payload in Parser::new(0).parse_all(bytes) {
            if let Payload::GlobalSection(reader) = payload.expect("payload") {
                return reader.count() as usize;
            }
        }
        0
    }

    fn global_decls(bytes: &[u8]) -> Vec<(bool, &'static str)> {
        let mut out = Vec::new();
        for payload in Parser::new(0).parse_all(bytes) {
            if let Payload::GlobalSection(reader) = payload.expect("payload") {
                for g in reader {
                    let g = g.expect("global");
                    let kind = match g.ty.content_type {
                        wasmparser::ValType::F64 => "f64",
                        wasmparser::ValType::I32 => "i32",
                        wasmparser::ValType::Ref(_) => "ref",
                        _ => "other",
                    };
                    out.push((g.ty.mutable, kind));
                }
            }
        }
        out
    }

    fn start_function_idx(bytes: &[u8]) -> Option<u32> {
        for payload in Parser::new(0).parse_all(bytes) {
            if let Payload::StartSection { func, .. } = payload.expect("payload") {
                return Some(func);
            }
        }
        None
    }

    fn data_count(bytes: &[u8]) -> Option<u32> {
        for payload in Parser::new(0).parse_all(bytes) {
            if let Payload::DataCountSection { count, .. } = payload.expect("payload") {
                return Some(count);
            }
        }
        None
    }

    fn data_segments(bytes: &[u8]) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        for payload in Parser::new(0).parse_all(bytes) {
            if let Payload::DataSection(reader) = payload.expect("payload") {
                for d in reader {
                    let d = d.expect("data segment");
                    out.push(d.data.to_vec());
                }
            }
        }
        out
    }

    /// The `name` section's module subsection — the identity the runtime attributes a gated
    /// call to. Read through the engine rather than by hand, so these tests fail if the
    /// accessor and the emitter ever disagree.
    fn module_name(bytes: &[u8]) -> Option<String> {
        let engine = crate::runtime::RuntimeConfig::default()
            .engine()
            .expect("engine");
        let module = wasmtime::Module::new(&engine, bytes).expect("module validates");
        module.name().map(str::to_string)
    }

    #[test]
    fn package_module_is_named_for_its_package() {
        let (bytes, _decl, _ti) = compile_package_modules(
            "test:pkg",
            &[(
                "lib",
                "/** Identity. */\nexport function noop(x: number): number { return x; }",
            )],
            &[],
        );
        assert_eq!(module_name(&bytes).as_deref(), Some("test:pkg"));
    }

    #[test]
    fn script_module_is_named_main() {
        let bytes = compile("function main(): number { return 1; }");
        assert_eq!(module_name(&bytes).as_deref(), Some("main"));
    }

    /// A package's test file compiles as a script — its entry point stays mangled as `main` so
    /// the runner can find it — but the code is the package's, so a gated call from it must
    /// attribute to the package. Without this, `secrets.get` in a package's own tests is
    /// refused outright by the unconditional deny of `secrets.get` to `main`.
    #[test]
    fn test_file_module_is_named_for_its_owning_package() {
        let source = "function main(): number { return 1; }";
        let compiled = crate::compile::compile_script_owned_by(
            "test:pkg",
            source,
            "tests/thing.test.ts",
            crate::FileId(0),
            &[],
            &[],
        )
        .expect("compiles");
        assert_eq!(module_name(&compiled.wasm).as_deref(), Some("test:pkg"));

        assert!(
            main_export_func_idx(&compiled.wasm).is_some(),
            "entry point must stay exported as `main` — the module name is identity, not mangling"
        );
    }

    /// Records every attribution the policy engine is asked about and allows all of them, so
    /// the only thing that can refuse `main` is the unconditional `secrets.get` rule.
    /// A test-local deny would mask which mechanism actually held.
    struct RecordingAllowAll {
        seen: std::sync::Mutex<Vec<(String, String)>>,
    }

    impl crate::runtime::SecurityCheck for RecordingAllowAll {
        fn check(
            &self,
            caller: &str,
            capability: &str,
            _context: &serde_json::Value,
        ) -> crate::runtime::CheckOutcome {
            self.seen
                .lock()
                .expect("mutex")
                .push((caller.to_string(), capability.to_string()));
            crate::runtime::CheckOutcome::Allow
        }
    }

    struct FixedSecret;

    impl crate::runtime::SecretProvider for FixedSecret {
        fn get<'a>(
            &'a self,
            _name: &'a str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Option<String>, String>> + Send + 'a>,
        > {
            Box::pin(async { Ok(Some("SECRET-VALUE".to_string())) })
        }
    }

    /// The escalation reported in PR #4, and the isolation guarantee in
    /// `docs/semantic-security.md` that it breaks: a caller gets its own permission set and
    /// inherits nothing from the frame above it.
    ///
    /// `JSON.stringify` of a caller-supplied value dispatches that value's own `toJson`. That
    /// method is `main`'s code, but it runs while the package sits on top of the caller stack,
    /// so its gated calls are attributed to the package — and `main` reads a secret it is
    /// unconditionally denied.
    #[tokio::test]
    async fn main_authored_code_keeps_main_identity_inside_a_package_call() {
        let (lib_bytes, lib_decl, lib_type_info) = compile_package_modules(
            "test:vuln",
            &[(
                "lib",
                r#"
                /** Pass-through JSON encoder. */
                export function passthrough(value: unknown): string {
                    return JSON.stringify(value);
                }
                "#,
            )],
            &[],
        );

        let recording = std::sync::Arc::new(RecordingAllowAll {
            seen: std::sync::Mutex::new(Vec::new()),
        });
        let cfg = crate::runtime::RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let mut data =
            crate::runtime::StoreData::with_vfs(crate::runtime::Vfs::tempdir().expect("tempdir"));
        data.install_type_info(lib_type_info);
        data.security_check = recording.clone();
        data.secret_provider = std::sync::Arc::new(FixedSecret);
        let mut store = cfg.store(&engine, data).expect("store");
        let mut linker = wasmtime::Linker::<crate::runtime::StoreData>::new(&engine);
        crate::runtime::install_runtime_async(&mut linker, &mut store)
            .await
            .expect("install runtime");
        let lib_module = wasmtime::Module::new(&engine, &lib_bytes).expect("library module");
        let lib_inst = linker
            .instantiate_async(&mut store, &lib_module)
            .await
            .expect("instantiate library");
        linker
            .instance(&mut store, "test:vuln", lib_inst)
            .expect("register library instance");
        let public_name = crate::mangle::package_symbol("test:vuln", "passthrough");
        let func = lib_inst
            .get_func(&mut store, public_name.as_str())
            .expect("library public export");
        linker
            .define(&mut store, "test:vuln", "passthrough", func)
            .expect("plain package import alias");

        // Control: main reading the secret directly is refused ahead of any policy.
        let direct = crate::compile::compile_script(
            r#"
            import { get } from "submilli:secrets";
            function main(): void { const _ = get("TOKEN"); }
            "#,
            "direct.subm",
            crate::FileId(0),
            &[&lib_decl],
            &[],
        )
        .expect("direct consumer compiles");
        store.data_mut().install_type_info(direct.type_info.clone());
        let direct_module = wasmtime::Module::new(&engine, &direct.wasm).expect("direct module");
        let direct_inst = linker
            .instantiate_async(&mut store, &direct_module)
            .await
            .expect("instantiate direct consumer");
        let err = crate::runtime::dispatch_main_async(&mut store, &direct_inst)
            .await
            .expect_err("direct read is denied");
        assert!(
            err.to_string().contains("permission denied"),
            "expected denial, got: {err}",
        );

        // Escalation: the same read, reached through the package's stringify.
        let escalated = crate::compile::compile_script(
            r#"
            import { passthrough } from "test:vuln";
            import { get } from "submilli:secrets";

            class Exfil {
                stolen: string;
                constructor() { this.stolen = "none"; }
                toJson(): string {
                    const token = get("TOKEN");
                    if (token !== null) { this.stolen = token; }
                    return "\"ok\"";
                }
            }

            function main(): string {
                const payload = new Exfil();
                const _ = passthrough(payload);
                return payload.stolen;
            }
            "#,
            "escalated.subm",
            crate::FileId(0),
            &[&lib_decl],
            &[],
        )
        .expect("escalating consumer compiles");
        store
            .data_mut()
            .install_type_info(escalated.type_info.clone());
        let escalated_module =
            wasmtime::Module::new(&engine, &escalated.wasm).expect("escalated module");
        let escalated_inst = linker
            .instantiate_async(&mut store, &escalated_module)
            .await
            .expect("instantiate escalating consumer");
        let result = crate::runtime::dispatch_main_async(&mut store, &escalated_inst).await;
        let seen = recording.seen.lock().expect("mutex").clone();

        // The sharp assertion: nothing `main` authored may reach the policy engine wearing the
        // package's name. Under the bug this holds `("test:vuln", "secrets.get")`.
        assert!(
            !seen.iter().any(|(caller, _)| caller == "test:vuln"),
            "main-authored code was attributed to the package it was handed to: {seen:?}",
        );
        assert!(
            result.is_err(),
            "main must not obtain a secret it is unconditionally denied; got {result:?}",
        );
    }

    /// Links `lib_bytes` under `package`, runs `consumer_src` as the script, and returns every
    /// (caller, capability) pair the policy engine was asked about, plus whether `main` ran to
    /// completion. A denial is a legitimate outcome — for the escalation shapes it is the
    /// point — so the caller decides which it expects.
    async fn attributions_for(
        package: &str,
        lib_bytes: &[u8],
        lib_decl: &PackageDeclaration,
        lib_type_info: crate::TypeInfoTable,
        consumer_src: &str,
    ) -> (Vec<(String, String)>, bool) {
        let recording = std::sync::Arc::new(RecordingAllowAll {
            seen: std::sync::Mutex::new(Vec::new()),
        });
        let cfg = crate::runtime::RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let mut data =
            crate::runtime::StoreData::with_vfs(crate::runtime::Vfs::tempdir().expect("tempdir"));
        data.install_type_info(lib_type_info);
        data.security_check = recording.clone();
        data.secret_provider = std::sync::Arc::new(FixedSecret);
        let mut store = cfg.store(&engine, data).expect("store");
        let mut linker = wasmtime::Linker::<crate::runtime::StoreData>::new(&engine);
        crate::runtime::install_runtime_async(&mut linker, &mut store)
            .await
            .expect("install runtime");
        let lib_module = wasmtime::Module::new(&engine, lib_bytes).expect("library module");
        let lib_inst = linker
            .instantiate_async(&mut store, &lib_module)
            .await
            .expect("instantiate library");
        linker
            .instance(&mut store, package, lib_inst)
            .expect("register library instance");

        let consumer = crate::compile::compile_script(
            consumer_src,
            "consumer.subm",
            crate::FileId(0),
            &[lib_decl],
            &[],
        )
        .expect("consumer compiles");
        store
            .data_mut()
            .install_type_info(consumer.type_info.clone());
        let consumer_module = wasmtime::Module::new(&engine, &consumer.wasm).expect("module");
        let inst = linker
            .instantiate_async(&mut store, &consumer_module)
            .await
            .expect("instantiate consumer");
        let ran_ok = crate::runtime::dispatch_main_async(&mut store, &inst)
            .await
            .is_ok();
        (recording.seen.lock().expect("mutex").clone(), ran_ok)
    }

    /// The escalation's route is `JSON.stringify` reaching a caller-supplied `toJson`, and it
    /// does not need the value handed over directly — an element inside a collection the
    /// package serializes dispatches the same way.
    #[tokio::test]
    async fn main_authored_tojson_inside_an_array_keeps_main_identity() {
        let (lib_bytes, lib_decl, lib_type_info) = compile_package_modules(
            "test:arr",
            &[(
                "lib",
                r#"
                /** Pass-through JSON encoder for a collection. */
                export function passthroughAll(values: unknown[]): string {
                    return JSON.stringify(values);
                }
                "#,
            )],
            &[],
        );
        let (seen, ran_ok) = attributions_for(
            "test:arr",
            &lib_bytes,
            &lib_decl,
            lib_type_info,
            r#"
            import { passthroughAll } from "test:arr";
            import { get } from "submilli:secrets";

            class Exfil {
                stolen: string;
                constructor() { this.stolen = "none"; }
                toJson(): string {
                    const t = get("TOKEN");
                    if (t !== null) { this.stolen = t; }
                    return "\"ok\"";
                }
            }

            function main(): string {
                const payload = new Exfil();
                const _ = passthroughAll([payload]);
                return payload.stolen;
            }
            "#,
        )
        .await;
        assert!(
            !seen.iter().any(|(caller, _)| caller == "test:arr"),
            "an element's toJson is main's code wherever the package serializes it: {seen:?}",
        );
        assert!(
            !ran_ok,
            "identified as main, the read must be refused rather than completing",
        );
    }

    /// The report names exported class members as a second route with the same root cause:
    /// constructors and methods carry no identity wrapper, so their gated calls were
    /// attributed to whoever called them. The practical consequence was that a
    /// credential-resolving API had to be a plain exported function — as a class method it
    /// would be attributed to `main` and refused.
    #[tokio::test]
    async fn a_packages_exported_class_method_is_attributed_to_the_package() {
        let (lib_bytes, lib_decl, lib_type_info) = compile_package_modules(
            "test:cls",
            &[(
                "lib",
                r#"
                import { get } from "submilli:secrets";

                /** Resolves a credential from inside a class method. */
                export class Client {
                    token: string;
                    constructor() { this.token = "none"; }

                    /** Reads the token. */
                    load(): string {
                        const t = get("TOKEN");
                        return t === null ? "none" : t;
                    }
                }
                "#,
            )],
            &[],
        );
        let (seen, ran_ok) = attributions_for(
            "test:cls",
            &lib_bytes,
            &lib_decl,
            lib_type_info,
            r#"
            import { Client } from "test:cls";
            function main(): string {
                const c = new Client();
                return c.load();
            }
            "#,
        )
        .await;
        assert!(ran_ok, "the package is permitted, so main must complete");
        assert_eq!(
            seen,
            vec![("test:cls".to_string(), "secrets.get".to_string())],
            "a class method's gated call belongs to the package that defines it",
        );
    }

    /// A package's module-level initializers run in its own `_start`, so they were already
    /// correct while a frame was pushed around instantiation. Under frame-derived identity
    /// they are correct by construction — that code genuinely executes in the package's own
    /// module. Asserted rather than assumed.
    #[tokio::test]
    async fn a_packages_initializer_is_attributed_to_the_package() {
        let (lib_bytes, lib_decl, lib_type_info) = compile_package_modules(
            "test:init",
            &[(
                "lib",
                r#"
                import { get } from "submilli:secrets";

                const TOKEN: string | null = get("TOKEN");

                /** Reports whether the module-level read succeeded. */
                export function loaded(): boolean {
                    return TOKEN !== null;
                }
                "#,
            )],
            &[],
        );
        let (seen, ran_ok) = attributions_for(
            "test:init",
            &lib_bytes,
            &lib_decl,
            lib_type_info,
            r#"
            import { loaded } from "test:init";
            function main(): boolean { return loaded(); }
            "#,
        )
        .await;
        assert!(ran_ok, "the package is permitted, so main must complete");
        assert_eq!(
            seen,
            vec![("test:init".to_string(), "secrets.get".to_string())],
            "a package initializer's gated call belongs to the package",
        );
    }

    fn custom_section(bytes: &[u8], name: &str) -> Option<Vec<u8>> {
        for payload in Parser::new(0).parse_all(bytes) {
            if let Payload::CustomSection(reader) = payload.expect("payload")
                && reader.name() == name
            {
                return Some(reader.data().to_vec());
            }
        }
        None
    }

    fn export_names(bytes: &[u8]) -> Vec<String> {
        let mut names = Vec::new();
        for payload in Parser::new(0).parse_all(bytes) {
            if let Payload::ExportSection(reader) = payload.expect("payload") {
                for export in reader {
                    names.push(export.expect("export").name.to_string());
                }
            }
        }
        names
    }

    fn export_entries(bytes: &[u8]) -> Vec<(String, wasmparser::ExternalKind, u32)> {
        let mut exports = Vec::new();
        for payload in Parser::new(0).parse_all(bytes) {
            if let Payload::ExportSection(reader) = payload.expect("payload") {
                for export in reader {
                    let export = export.expect("export");
                    exports.push((export.name.to_string(), export.kind, export.index));
                }
            }
        }
        exports
    }

    fn imported_func_count(bytes: &[u8]) -> u32 {
        let mut count = 0;
        for payload in Parser::new(0).parse_all(bytes) {
            if let Payload::ImportSection(reader) = payload.expect("payload") {
                for entry in reader {
                    if let wasmparser::Imports::Single(_, imp) = entry.expect("import")
                        && matches!(imp.ty, wasmparser::TypeRef::Func(_))
                    {
                        count += 1;
                    }
                }
            }
        }
        count
    }

    /// Function-space index of the imported func whose linker field is `field`.
    fn imported_func_idx(bytes: &[u8], field: &str) -> u32 {
        let mut count = 0;
        for payload in Parser::new(0).parse_all(bytes) {
            if let Payload::ImportSection(reader) = payload.expect("payload") {
                for entry in reader {
                    if let wasmparser::Imports::Single(_, imp) = entry.expect("import")
                        && matches!(imp.ty, wasmparser::TypeRef::Func(_))
                    {
                        if imp.name == field {
                            return count;
                        }
                        count += 1;
                    }
                }
            }
        }
        panic!("no func import named {field}");
    }

    fn body_call_targets(bytes: &[u8], body_idx: usize) -> Vec<u32> {
        let mut bodies_seen = 0;
        let mut targets = Vec::new();
        for payload in Parser::new(0).parse_all(bytes) {
            if let Payload::CodeSectionEntry(body) = payload.expect("payload") {
                if bodies_seen == body_idx {
                    let mut reader = body
                        .get_operators_reader()
                        .expect("body operators readable");
                    while !reader.eof() {
                        if let wasmparser::Operator::Call { function_index } =
                            reader.read().expect("operator")
                        {
                            targets.push(function_index);
                        }
                    }
                    return targets;
                }
                bodies_seen += 1;
            }
        }
        targets
    }

    fn main_body_call_targets(bytes: &[u8]) -> Vec<u32> {
        let main_idx = main_export_func_idx(bytes).expect("main exported");
        let body_idx = (main_idx - imported_func_count(bytes)) as usize;
        body_call_targets(bytes, body_idx)
    }

    fn main_export_func_idx(bytes: &[u8]) -> Option<u32> {
        for payload in Parser::new(0).parse_all(bytes) {
            if let Payload::ExportSection(reader) = payload.expect("payload") {
                for export in reader {
                    let export = export.expect("export");
                    if export.name == "main"
                        && matches!(export.kind, wasmparser::ExternalKind::Func)
                    {
                        return Some(export.index);
                    }
                }
            }
        }
        None
    }

    fn instantiate_against_prelude(consumer_bytes: &[u8]) {
        let _ = link_consumer(consumer_bytes);
    }

    fn link_consumer(
        consumer_bytes: &[u8],
    ) -> (
        wasmtime::Store<crate::runtime::StoreData>,
        wasmtime::Instance,
    ) {
        let cfg = crate::runtime::RuntimeConfig::default();
        link_consumer_with(&cfg, consumer_bytes)
    }

    fn link_consumer_with(
        cfg: &crate::runtime::RuntimeConfig,
        consumer_bytes: &[u8],
    ) -> (
        wasmtime::Store<crate::runtime::StoreData>,
        wasmtime::Instance,
    ) {
        let engine = cfg.engine().expect("engine builds with default config");
        let mut store = cfg
            .store(
                &engine,
                crate::runtime::StoreData::with_tempdir()
                    .expect("tempdir allocates for codegen test VFS"),
            )
            .expect("store builds with default config");
        let consumer_module =
            wasmtime::Module::new(&engine, consumer_bytes).expect("consumer module");
        let mut linker = wasmtime::Linker::<crate::runtime::StoreData>::new(&engine);
        // Async install/instantiate (the linker holds async http/fs host fns);
        // driven to completion here since no IO happens during setup. `main` is a
        // pure-compute func, so the tests still call it synchronously.
        let consumer_inst = pollster::block_on(async {
            crate::runtime::install_runtime_async(&mut linker, &mut store)
                .await
                .expect("runtime installs the prelude + host functions");
            linker
                .instantiate_async(&mut store, &consumer_module)
                .await
                .expect("consumer instantiates against the runtime")
        });
        (store, consumer_inst)
    }

    fn run_main_f64(source: &str) -> f64 {
        run_main_number(&compile(source))
    }

    fn run_main_number(bytes: &[u8]) -> f64 {
        let (mut store, inst) = link_consumer(bytes);
        let main = inst.get_func(&mut store, "main").expect("main export");
        let mut results = [wasmtime::Val::F64(0)];
        pollster::block_on(main.call_async(&mut store, &[], &mut results))
            .expect("main does not trap");
        match &results[0] {
            wasmtime::Val::F64(bits) => f64::from_bits(*bits),
            wasmtime::Val::AnyRef(Some(value)) => {
                let boxed = value
                    .as_struct(&mut store)
                    .expect("read result")
                    .expect("boxed number");
                boxed
                    .field(&mut store, 1)
                    .expect("number payload")
                    .f64()
                    .expect("f64 payload")
            }
            other => panic!("expected numeric result, got {other:?}"),
        }
    }

    fn run_main_i32(source: &str) -> i32 {
        let bytes = compile(source);
        let (mut store, inst) = link_consumer(&bytes);
        let main = inst
            .get_typed_func::<(), i32>(&mut store, "main")
            .expect("main is exported as `() -> i32`");
        pollster::block_on(main.call_async(&mut store, ())).expect("main does not trap")
    }

    fn run_main_expecting_error(source: &str) -> String {
        let bytes = compile(source);
        let (mut store, inst) = link_consumer(&bytes);
        let main = inst
            .get_typed_func::<(), ()>(&mut store, "main")
            .expect("main signature is `() -> void` in expecting-error fixtures");
        let result = pollster::block_on(main.call_async(&mut store, ()));
        // Same uncaught-exception reshaping `dispatch_main_async` applies, so a
        // throw renders with its stashed backtrace like a trap does.
        let err = crate::runtime::exec::map_uncaught_exception(&mut store, result)
            .expect_err("expected main() to trap or throw");
        let (sources, file) = crate::Sources::single("script.subm", source).unwrap();
        crate::render_backtrace(&err, &sources, file, crate::BacktraceMode::Full)
            .expect("backtrace empty — was wasm_backtrace_details enabled?")
    }

    fn dump_dwarf_dies(bytes: &[u8]) -> String {
        use gimli::LittleEndian;
        use gimli::read::{AttributeValue, DebugAbbrev, DebugInfo, DebugStr};
        use std::fmt::Write;

        let info_bytes = custom_section(bytes, ".debug_info").expect(".debug_info");
        let abbrev_bytes = custom_section(bytes, ".debug_abbrev").expect(".debug_abbrev");
        let str_bytes = custom_section(bytes, ".debug_str").expect(".debug_str");
        let debug_info = DebugInfo::new(&info_bytes, LittleEndian);
        let debug_abbrev = DebugAbbrev::new(&abbrev_bytes, LittleEndian);
        let debug_str = DebugStr::new(&str_bytes, LittleEndian);

        let mut out = String::new();
        let mut units = debug_info.units();
        while let Some(header) = units.next().expect("unit") {
            let abbreviations = header.abbreviations(&debug_abbrev).expect("abbreviations");
            let mut entries = header.entries(&abbreviations);
            while let Some(entry) = entries.next_dfs().expect("entry") {
                let tag = entry.tag().static_string().unwrap_or("DW_TAG_<unknown>");
                writeln!(out, "<{:#x}> {}", entry.offset().0, tag).unwrap();
                for attr in entry.attrs() {
                    let name = attr.name().static_string().unwrap_or("DW_AT_<unknown>");
                    let value_repr = match attr.value() {
                        AttributeValue::Addr(a) => format!("Addr({a:#x})"),
                        AttributeValue::Udata(u) => format!("Udata({u})"),
                        AttributeValue::DebugStrRef(off) => {
                            let s = debug_str.get_str(off).expect("str");
                            format!("Str({:?})", std::str::from_utf8(s.slice()).unwrap_or("?"))
                        }
                        AttributeValue::Language(lang) => format!(
                            "Language({})",
                            lang.static_string().unwrap_or("DW_LANG_<unknown>")
                        ),
                        other => format!("{other:?}"),
                    };
                    writeln!(out, "  {name} = {value_repr}").unwrap();
                }
            }
        }
        out
    }

    fn dump_dwarf_lines(bytes: &[u8]) -> String {
        use gimli::LittleEndian;
        use gimli::read::{DebugAbbrev, DebugInfo, DebugLine};
        use std::fmt::Write;

        let info_bytes = custom_section(bytes, ".debug_info").expect(".debug_info");
        let abbrev_bytes = custom_section(bytes, ".debug_abbrev").expect(".debug_abbrev");
        let line_bytes = custom_section(bytes, ".debug_line").expect(".debug_line");
        let debug_info = DebugInfo::new(&info_bytes, LittleEndian);
        let debug_abbrev = DebugAbbrev::new(&abbrev_bytes, LittleEndian);
        let debug_line = DebugLine::new(&line_bytes, LittleEndian);

        let mut out = String::new();
        let header = debug_info
            .units()
            .next()
            .expect("units")
            .expect("at least one unit");
        let _ = header.abbreviations(&debug_abbrev).expect("abbrevs");
        let program = debug_line
            .program(gimli::DebugLineOffset(0), header.address_size(), None, None)
            .expect("line program");
        let (program, sequences) = program.sequences().expect("sequences");
        for seq in sequences {
            writeln!(out, "sequence start={:#x} end={:#x}", seq.start, seq.end,).unwrap();
            let cloned = program.clone();
            let mut state = cloned.resume_from(&seq);
            while let Some((header, row)) = state.next_row().expect("next_row") {
                let _ = header;
                let line = row.line().map_or(0, std::num::NonZero::get);
                writeln!(
                    out,
                    "  addr={:#x} file={} line={} col={:?} stmt={}{}",
                    row.address(),
                    row.file_index(),
                    line,
                    row.column(),
                    row.is_stmt(),
                    if row.end_sequence() {
                        " end_sequence"
                    } else {
                        ""
                    },
                )
                .unwrap();
            }
        }
        out
    }

    #[test]
    fn empty_main_links_with_prelude() {
        instantiate_against_prelude(&compile("function main(): void { }"));
    }

    #[test]
    fn main_with_number_return_links_with_prelude() {
        instantiate_against_prelude(&compile("function main(): number { return 0; }"));
    }

    #[test]
    fn main_with_boolean_return_links_with_prelude() {
        instantiate_against_prelude(&compile("function main(): boolean { return true; }"));
    }

    #[test]
    fn string_runtime_imported_from_prelude() {
        let bytes = compile("function main(): void { }");
        // User functions, module start, and the shared field lookup.
        assert_eq!(function_count(&bytes), 3);
        let stable_imports: Vec<_> = imports(&bytes)
            .into_iter()
            .filter(|(_, name)| !is_sub421_temporal_getter_import(name))
            .collect();
        assert!(
            stable_imports.contains(&(
                crate::runtime::prelude::MODULE_NAME.to_string(),
                crate::mangle::prelude("__error_tag").to_string(),
            )),
            "host tag import should use canonical name"
        );
        assert!(
            stable_imports.contains(&(
                crate::runtime::prelude::MODULE_NAME.to_string(),
                crate::mangle::prelude("string_vtable").to_string(),
            )),
            "host-owned string_vtable import should use canonical name"
        );
        assert!(
            !stable_imports
                .iter()
                .any(|(module, _)| module == "submilli:crypto"),
            "unused stdlib imports should be tree-shaken"
        );
        assert!(
            !stable_imports
                .iter()
                .any(|(module, name)| module == "submilli:prelude" && name == "string_vtable"),
            "prelude imports must not use short names"
        );
        assert_eq!(data_count(&bytes), None);
        assert!(data_segments(&bytes).is_empty());
    }

    fn is_sub421_temporal_getter_import(name: &str) -> bool {
        matches!(
            name,
            "Temporal#PlainDate#monthCode"
                | "Temporal#PlainDate#dayOfYear"
                | "Temporal#PlainDate#weekOfYear"
                | "Temporal#PlainDate#yearOfWeek"
                | "Temporal#PlainDate#daysInWeek"
                | "Temporal#PlainDate#daysInMonth"
                | "Temporal#PlainDate#daysInYear"
                | "Temporal#PlainDate#monthsInYear"
                | "Temporal#PlainDate#inLeapYear"
                | "Temporal#PlainTime#millisecond"
                | "Temporal#PlainTime#microsecond"
                | "Temporal#PlainDateTime#dayOfWeek"
                | "Temporal#PlainDateTime#monthCode"
                | "Temporal#PlainDateTime#dayOfYear"
                | "Temporal#PlainDateTime#weekOfYear"
                | "Temporal#PlainDateTime#yearOfWeek"
                | "Temporal#PlainDateTime#daysInWeek"
                | "Temporal#PlainDateTime#daysInMonth"
                | "Temporal#PlainDateTime#daysInYear"
                | "Temporal#PlainDateTime#monthsInYear"
                | "Temporal#PlainDateTime#inLeapYear"
                | "Temporal#PlainDateTime#millisecond"
                | "Temporal#PlainDateTime#microsecond"
                | "Temporal#PlainYearMonth#monthCode"
                | "Temporal#PlainYearMonth#daysInMonth"
                | "Temporal#PlainYearMonth#daysInYear"
                | "Temporal#PlainYearMonth#monthsInYear"
                | "Temporal#PlainYearMonth#inLeapYear"
                | "Temporal#PlainMonthDay#monthCode"
                | "Temporal#ZonedDateTime#monthCode"
                | "Temporal#ZonedDateTime#dayOfYear"
                | "Temporal#ZonedDateTime#weekOfYear"
                | "Temporal#ZonedDateTime#yearOfWeek"
                | "Temporal#ZonedDateTime#daysInWeek"
                | "Temporal#ZonedDateTime#daysInMonth"
                | "Temporal#ZonedDateTime#daysInYear"
                | "Temporal#ZonedDateTime#monthsInYear"
                | "Temporal#ZonedDateTime#inLeapYear"
                | "Temporal#ZonedDateTime#millisecond"
                | "Temporal#ZonedDateTime#microsecond"
                | "Temporal#ZonedDateTime#nanosecond"
        )
    }

    #[test]
    fn main_function_index_follows_imports() {
        let bytes = compile("function main(): void { }");
        assert_eq!(
            main_export_func_idx(&bytes),
            Some(imported_func_count(&bytes) + 1),
        );
    }

    #[test]
    fn main_still_exported_uniquely() {
        let bytes = compile("function main(): void { }");
        assert_eq!(export_names(&bytes), vec!["main".to_string()]);
    }

    #[test]
    fn symbol_table_resolves_prelude_function_indices() {
        let map = build_symbol_table("function main(): void { }");
        // Alphabetical: isFinite, isNaN, string_cmp, string_concat, string_eq, string_length.
        assert_eq!(map.prelude_func_idx("isFinite"), Some(0));
        assert_eq!(map.prelude_func_idx("isNaN"), Some(1));
        assert_eq!(map.prelude_func_idx("string_cmp"), Some(2));
        assert_eq!(map.prelude_func_idx("string_concat"), Some(3));
        assert_eq!(map.prelude_func_idx("string_eq"), Some(4));
        assert_eq!(map.prelude_func_idx("string_length"), Some(5));
        assert_eq!(map.prelude_func_idx("missing"), None);
    }

    #[test]
    fn symbol_table_resolves_prelude_string_type_index() {
        let map = build_symbol_table("function main(): void { }");
        assert_eq!(map.string_type_idx(), Some(3));
        assert_eq!(map.raw_string_type_idx(), Some(0));
    }

    #[test]
    fn module_with_one_string_adds_one_data_segment() {
        let bytes = compile(r#"let x: string = "hello"; function main(): void { }"#);
        instantiate_against_prelude(&bytes);
        assert_eq!(data_count(&bytes), Some(1));
        let segments = data_segments(&bytes);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0], b"h\0e\0l\0l\0o\0".to_vec());
    }

    #[test]
    fn module_with_two_distinct_strings_has_two_segments() {
        let bytes = compile(
            r#"let x: string = "hello"; let y: string = "world"; function main(): void { }"#,
        );
        instantiate_against_prelude(&bytes);
        assert_eq!(data_count(&bytes), Some(2));
        let segments = data_segments(&bytes);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0], b"h\0e\0l\0l\0o\0".to_vec());
        assert_eq!(segments[1], b"w\0o\0r\0l\0d\0".to_vec());
    }

    #[test]
    fn module_with_dedup_strings_has_one_segment() {
        let bytes = compile(
            r#"let x: string = "hello"; let y: string = "hello"; function main(): void { }"#,
        );
        instantiate_against_prelude(&bytes);
        assert_eq!(data_count(&bytes), Some(1));
        assert_eq!(data_segments(&bytes).len(), 1);
    }

    #[test]
    fn module_with_strings_keeps_runtime_imports() {
        let bytes = compile(r#"let x: string = "hi"; function main(): void { }"#);
        // User functions, module start, and the shared field lookup.
        assert_eq!(function_count(&bytes), 3);
        let imp = imports(&bytes);
        assert!(imp.contains(&(
            crate::runtime::prelude::MODULE_NAME.to_string(),
            crate::mangle::prelude("string_vtable").to_string(),
        )));
        assert!(
            !imp.iter()
                .any(|(module, _)| module == crate::runtime::BIGINT_MODULE_NAME),
            "unused bigint imports should be tree-shaken",
        );
    }

    #[test]
    fn cross_module_string_type_canonicalization_proves_out() {
        let bytes = compile(r#"let x: string = "hello"; function main(): void { }"#);
        instantiate_against_prelude(&bytes);
    }

    #[test]
    fn helper_emits_function_body_but_is_not_exported() {
        let bytes = compile("function main(): void { } function helper(): void { }");
        instantiate_against_prelude(&bytes);
        // User functions, module start, and the shared field lookup.
        assert_eq!(function_count(&bytes), 4);
        assert_eq!(export_names(&bytes), vec!["main".to_string()]);
    }

    #[test]
    fn typed_exports_emit_function_and_global_but_skip_types() {
        let bytes = compile(
            r#"
            export interface Public { value: number; }
            export const answer: number = 42;
            export function api(): number { return answer; }
            function main(): void { }
            "#,
        );
        instantiate_against_prelude(&bytes);
        let exports = export_entries(&bytes);
        assert!(
            exports.iter().any(|(name, kind, _)| {
                name == "main#api" && matches!(kind, wasmparser::ExternalKind::Func)
            }),
            "expected exported function entry, got {exports:?}",
        );
        assert!(
            exports.iter().any(|(name, kind, _)| {
                name == "main#answer" && matches!(kind, wasmparser::ExternalKind::Global)
            }),
            "expected exported global entry, got {exports:?}",
        );
        assert!(
            !exports.iter().any(|(name, _, _)| name == "main#Public"),
            "type-only exports must not become Wasm exports: {exports:?}",
        );
    }

    #[test]
    fn package_reexport_aliases_share_one_wrapper_index() {
        let (bytes, package, _) = compile_package_modules(
            "test:lib",
            &[
                (
                    "lib",
                    r#"export { inner as first, inner as second } from "./util";"#,
                ),
                (
                    "util",
                    "/** Inner function. */\nexport function inner(): number { return 1; }",
                ),
            ],
            &[],
        );
        assert_eq!(
            package.values.keys().collect::<Vec<_>>(),
            vec![&"first".to_string(), &"second".to_string()],
        );
        let exports = export_entries(&bytes);
        let first = exports
            .iter()
            .find(|(name, kind, _)| {
                name == "test:lib#first" && matches!(kind, wasmparser::ExternalKind::Func)
            })
            .expect("first function export")
            .2;
        let second = exports
            .iter()
            .find(|(name, kind, _)| {
                name == "test:lib#second" && matches!(kind, wasmparser::ExternalKind::Func)
            })
            .expect("second function export")
            .2;
        assert_eq!(first, second, "aliases for one target share one wrapper");
        assert!(
            main_export_func_idx(&bytes).is_none(),
            "libraries have no main"
        );
    }

    struct RecordingSecurity {
        seen: std::sync::Mutex<Vec<(String, String)>>,
    }

    impl crate::runtime::SecurityCheck for RecordingSecurity {
        fn check(
            &self,
            caller: &str,
            capability: &str,
            _context: &serde_json::Value,
        ) -> crate::runtime::CheckOutcome {
            self.seen
                .lock()
                .expect("recording security mutex")
                .push((caller.to_string(), capability.to_string()));
            if capability == "test.denied" {
                crate::runtime::CheckOutcome::Deny {
                    reason: "blocked by test".to_string(),
                }
            } else {
                crate::runtime::CheckOutcome::Allow
            }
        }
    }

    #[tokio::test]
    async fn a_package_api_attributes_to_the_package_on_both_outcomes() {
        let (lib_bytes, lib_decl, lib_type_info) = compile_package_modules(
            "test:lib",
            &[(
                "lib",
                r#"
                import security from "submilli:security";
                /** Allowed operation. */
                export function allowed(): void {
                    security.check("test.allowed", {});
                }
                /** Denied operation. */
                export function denied(): void {
                    security.check("test.denied", {});
                }
                "#,
            )],
            &[],
        );
        let recording = std::sync::Arc::new(RecordingSecurity {
            seen: std::sync::Mutex::new(Vec::new()),
        });
        let cfg = crate::runtime::RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let mut data =
            crate::runtime::StoreData::with_vfs(crate::runtime::Vfs::tempdir().expect("tempdir"));
        data.install_type_info(lib_type_info);
        data.security_check = recording.clone();
        let mut store = cfg.store(&engine, data).expect("store");
        let mut linker = wasmtime::Linker::<crate::runtime::StoreData>::new(&engine);
        crate::runtime::install_runtime_async(&mut linker, &mut store)
            .await
            .expect("install runtime");
        let lib_module = wasmtime::Module::new(&engine, &lib_bytes).expect("library module");
        let lib_inst = linker
            .instantiate_async(&mut store, &lib_module)
            .await
            .expect("instantiate library");
        linker
            .instance(&mut store, "test:lib", lib_inst)
            .expect("register library instance");
        for name in ["allowed", "denied"] {
            let public_name = crate::mangle::package_symbol("test:lib", name);
            let func = lib_inst
                .get_func(&mut store, public_name.as_str())
                .expect("library public export");
            linker
                .define(&mut store, "test:lib", name, func)
                .expect("plain package import alias");
        }

        let allowed = crate::compile::compile_script(
            r#"import { allowed } from "test:lib"; function main(): void { allowed(); }"#,
            "consumer.subm",
            crate::FileId(0),
            &[&lib_decl],
            &[],
        )
        .expect("allowed consumer compiles");
        store
            .data_mut()
            .install_type_info(allowed.type_info.clone());
        let allowed_module = wasmtime::Module::new(&engine, &allowed.wasm).expect("allowed module");
        let allowed_inst = linker
            .instantiate_async(&mut store, &allowed_module)
            .await
            .expect("instantiate allowed consumer");
        crate::runtime::dispatch_main_async(&mut store, &allowed_inst)
            .await
            .expect("allowed consumer runs");

        assert_eq!(
            recording
                .seen
                .lock()
                .expect("recording security mutex")
                .as_slice(),
            &[("main".to_string(), "test.allowed".to_string())],
        );

        let denied = crate::compile::compile_script(
            r#"import { denied } from "test:lib"; function main(): void { denied(); }"#,
            "consumer.subm",
            crate::FileId(0),
            &[&lib_decl],
            &[],
        )
        .expect("denied consumer compiles");
        store.data_mut().install_type_info(denied.type_info.clone());
        let denied_module = wasmtime::Module::new(&engine, &denied.wasm).expect("denied module");
        let denied_inst = linker
            .instantiate_async(&mut store, &denied_module)
            .await
            .expect("instantiate denied consumer");
        let err = crate::runtime::dispatch_main_async(&mut store, &denied_inst)
            .await
            .expect_err("denied security check throws");
        assert!(
            err.to_string().contains("permission denied"),
            "unexpected error: {err}",
        );
        assert!(
            err.to_string().contains("caller=main"),
            "unexpected error: {err}",
        );
    }

    /// A denial caught mid-flight, rather than one that ends the program. Identity is read
    /// per call from the running frame, so nothing can persist across the catch — this pins
    /// that: `main`'s next call after catching a package's denial is still `main`'s, and is
    /// still refused the secret.
    #[tokio::test]
    async fn a_caught_throw_from_a_package_export_leaves_main_as_the_caller() {
        let (lib_bytes, lib_decl, lib_type_info) = compile_package_modules(
            "test:lib",
            &[(
                "lib",
                r#"
                /** Always throws, so the caller can catch and continue. */
                export function boom(): void {
                    throw new Error("boom");
                }
                "#,
            )],
            &[],
        );
        let cfg = crate::runtime::RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let mut data =
            crate::runtime::StoreData::with_vfs(crate::runtime::Vfs::tempdir().expect("tempdir"));
        data.install_type_info(lib_type_info);
        let mut store = cfg.store_async(&engine, data).expect("store");
        let mut linker = wasmtime::Linker::<crate::runtime::StoreData>::new(&engine);
        crate::runtime::install_runtime_async(&mut linker, &mut store)
            .await
            .expect("install runtime");
        let lib_module = wasmtime::Module::new(&engine, &lib_bytes).expect("library module");
        let lib_inst = linker
            .instantiate_async(&mut store, &lib_module)
            .await
            .expect("instantiate library");
        linker
            .instance(&mut store, "test:lib", lib_inst)
            .expect("register library instance");
        let public_name = crate::mangle::package_symbol("test:lib", "boom");
        let func = lib_inst
            .get_func(&mut store, public_name.as_str())
            .expect("library public export");
        linker
            .define(&mut store, "test:lib", "boom", func)
            .expect("plain package import alias");

        let consumer = crate::compile::compile_script(
            r#"
            import { boom } from "test:lib";
            import { get } from "submilli:secrets";

            function main(): void {
                try {
                    boom();
                } catch (e: Error) {
                    // The export's frame must come off even though it threw.
                }
                const _ = get("TOKEN");
            }
            "#,
            "consumer.subm",
            crate::FileId(0),
            &[&lib_decl],
            &[],
        )
        .expect("consumer compiles");
        store
            .data_mut()
            .install_type_info(consumer.type_info.clone());
        let consumer_module =
            wasmtime::Module::new(&engine, &consumer.wasm).expect("consumer module");
        let consumer_inst = linker
            .instantiate_async(&mut store, &consumer_module)
            .await
            .expect("instantiate consumer");

        let err = crate::runtime::dispatch_main_async(&mut store, &consumer_inst)
            .await
            .expect_err("secrets.get from main is refused");

        assert!(
            err.to_string().contains("caller=main"),
            "a leaked frame would attribute this to test:lib: {err}",
        );
    }

    /// Records every gated call and optionally refuses one named caller, so a
    /// test can pin *who* a call was attributed to rather than only whether it
    /// was allowed.
    struct RecordingPerCaller {
        seen: std::sync::Mutex<Vec<(String, String)>>,
        deny_caller: Option<&'static str>,
    }

    impl crate::runtime::SecurityCheck for RecordingPerCaller {
        fn check(
            &self,
            caller: &str,
            capability: &str,
            _context: &serde_json::Value,
        ) -> crate::runtime::CheckOutcome {
            self.seen
                .lock()
                .expect("recording security mutex")
                .push((caller.to_string(), capability.to_string()));
            if self.deny_caller == Some(caller) {
                crate::runtime::CheckOutcome::Deny {
                    reason: "blocked by test".to_string(),
                }
            } else {
                crate::runtime::CheckOutcome::Allow
            }
        }
    }

    /// A package whose module-level initializer makes a gated call: the probe
    /// runs inside the Wasm start function, during instantiation.
    const PACKAGE_WITH_GATED_INITIALIZER: &str = r#"
        import { exists } from "submilli:fs";
        const _probe: boolean = exists("/init-probe");
        /** No-op export; the initializer is what this package is for. */
        export function noop(): void {}
        "#;

    async fn install_package_with_gated_initializer(
        recording: std::sync::Arc<RecordingPerCaller>,
    ) -> (
        wasmtime::Store<crate::runtime::StoreData>,
        wasmtime::Result<()>,
    ) {
        let (lib_bytes, lib_decl, lib_type_info) =
            compile_package_modules("test:lib", &[("lib", PACKAGE_WITH_GATED_INITIALIZER)], &[]);
        let cfg = crate::runtime::RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let mut data =
            crate::runtime::StoreData::with_vfs(crate::runtime::Vfs::tempdir().expect("tempdir"));
        data.security_check = recording;
        let mut store = cfg.store(&engine, data).expect("store");
        let mut linker = wasmtime::Linker::<crate::runtime::StoreData>::new(&engine);
        crate::runtime::install_runtime_async(&mut linker, &mut store)
            .await
            .expect("install runtime");
        let lib_module = wasmtime::Module::new(&engine, &lib_bytes).expect("library module");
        let outcome = crate::runtime::install_package_modules_async(
            &mut linker,
            &mut store,
            &[crate::runtime::LinkedPackageModule {
                module: &lib_module,
                declaration: &lib_decl,
                type_info: &lib_type_info,
            }],
        )
        .await;
        (store, outcome)
    }

    #[tokio::test]
    async fn package_initializers_are_attributed_to_the_package() {
        let recording = std::sync::Arc::new(RecordingPerCaller {
            seen: std::sync::Mutex::new(Vec::new()),
            deny_caller: None,
        });
        let (_store, outcome) = install_package_with_gated_initializer(recording.clone()).await;
        outcome.expect("package instantiates");

        assert_eq!(
            recording
                .seen
                .lock()
                .expect("recording security mutex")
                .as_slice(),
            &[("test:lib".to_string(), "fs.stat".to_string())],
        );
    }

    /// The documented behaviour break: package init used to run as `main`, so a
    /// package whose initializer needs a capability granted to `main` but not to
    /// the package now fails to instantiate.
    #[tokio::test]
    async fn package_initializer_denied_to_the_package_fails_and_restores_the_stack() {
        let recording = std::sync::Arc::new(RecordingPerCaller {
            seen: std::sync::Mutex::new(Vec::new()),
            deny_caller: Some("test:lib"),
        });
        let (_store, outcome) = install_package_with_gated_initializer(recording.clone()).await;
        let err = outcome.expect_err("denied initializer traps");

        // The initializer runs in the start function, so the denial escapes
        // instantiation as the engine's opaque `ThrownException`. An operator
        // reading only "wasm exception thrown" cannot tell which package or
        // capability was refused, which is the whole diagnosis.
        let message = err.to_string();
        assert!(
            message.contains("package `test:lib` failed to initialize"),
            "the failure must name the package: {message}",
        );
        assert!(
            message.contains("fs.stat") && message.contains("caller=test:lib"),
            "the failure must name the capability and caller: {message}",
        );

        assert_eq!(
            recording
                .seen
                .lock()
                .expect("recording security mutex")
                .as_slice(),
            &[("test:lib".to_string(), "fs.stat".to_string())],
        );
    }

    fn fake_globals_definitions() -> crate::PackageDeclaration {
        use crate::{PackageDeclaration, Span, Type, ValueKind, ValueSymbol};
        let mut defs = PackageDeclaration::with_package("test:globals");
        defs.values.insert(
            "counter".to_string(),
            ValueSymbol {
                name: "counter".to_string(),
                mangled_name: crate::mangle::package_symbol("test:globals", "counter"),
                declaration_span: Span::at(crate::FileId(0)),
                kind: ValueKind::Let {
                    ty: Type::Number,
                    doc: None,
                },
            },
        );
        defs.values.insert(
            "max_iterations".to_string(),
            ValueSymbol {
                name: "max_iterations".to_string(),
                mangled_name: crate::mangle::package_symbol("test:globals", "max_iterations"),
                declaration_span: Span::at(crate::FileId(0)),
                kind: ValueKind::Const {
                    ty: Type::Number,
                    doc: None,
                },
            },
        );
        defs
    }

    fn compile_with_package_imports<'a>(
        source: &str,
        packages: &'a [&'a crate::PackageDeclaration],
    ) -> Vec<u8> {
        let ta = type_check_with_packages(source, packages);
        let (prelude_defs, host_defs, internal_defs) =
            prelude::cached_runtime_package_declarations();
        let mut dependencies: Vec<&crate::PackageDeclaration> = prelude_defs.iter().collect();
        dependencies.extend(host_defs.iter());
        dependencies.extend(internal_defs.iter());
        dependencies.extend_from_slice(packages);
        codegen(source, "script.subm", crate::FileId(0), &ta, &dependencies)
            .expect("code generation")
    }

    fn imports_with_kind(bytes: &[u8]) -> Vec<(String, String, &'static str, bool)> {
        let mut out = Vec::new();
        for payload in Parser::new(0).parse_all(bytes) {
            if let Payload::ImportSection(reader) = payload.expect("payload") {
                for entry in reader {
                    match entry.expect("import") {
                        wasmparser::Imports::Single(_, imp) => {
                            let (kind, mutable) = match imp.ty {
                                wasmparser::TypeRef::Func(_) => ("func", false),
                                wasmparser::TypeRef::Global(g) => ("global", g.mutable),
                                _ => ("other", false),
                            };
                            out.push((imp.module.to_string(), imp.name.to_string(), kind, mutable));
                        }
                        other => panic!("unexpected import form {other:?}"),
                    }
                }
            }
        }
        out
    }

    #[test]
    fn let_and_const_become_global_imports() {
        let defs = fake_globals_definitions();
        let bytes = compile_with_package_imports(
            r#"import { counter, max_iterations } from "test:globals";
function main(): number { return counter + max_iterations; }"#,
            &[&defs],
        );
        let imp = imports_with_kind(&bytes);
        let counter_name = crate::mangle::package_symbol("test:globals", "counter");
        let counter = imp
            .iter()
            .find(|(_, n, _, _)| n == counter_name.as_str())
            .expect("counter import");
        assert_eq!(counter.0, "test:globals");
        assert_eq!(counter.2, "global");
        assert!(counter.3, "let imports as mutable global");

        let max_iterations_name = crate::mangle::package_symbol("test:globals", "max_iterations");
        let max_it = imp
            .iter()
            .find(|(_, n, _, _)| n == max_iterations_name.as_str())
            .expect("max_iterations import");
        assert_eq!(max_it.0, "test:globals");
        assert_eq!(max_it.2, "global");
        // A codegen-compiled package exports every top-level global mutable (so
        // `_start` can initialize it), so a cross-package `const` import must be
        // mutable too — const immutability is typechecker-enforced, not Wasm.
        assert!(
            max_it.3,
            "const imports as mutable global to match the exporter"
        );
    }

    #[test]
    fn ported_methods_route_to_prelude_host_not_wasm_wrapper() {
        let src = "function main(): void { \
                   [1, 2].forEach((x: number) => { }); \
                   const r: string = \"ab\".repeat(2); \
                   assert(r.length === 4); \
                   const s = new Set<number>(); \
                   s.add(1); \
                   s.delete(1); \
                   s.add(2); \
                   assert(s.size === 1 && s.has(2)); \
                   const f: string = (3.14).toFixed(1); \
                   const b: string = true.toString(); \
                   assert(Number.isInteger(4) && Number.parseInt(\"10\", 10) === 10); \
                   assert(Number.EPSILON > 0); \
                   assert(Math.PI > 3 && Math.abs(-2) === 2 && Math.imul(2, 3) === 6 && Math.min(3, 1, 2) === 1 && Math.random() >= 0); \
                   const u = Uint8Array.fromHex(\"01ff\"); \
                   u.fill(7); \
                   assert(u.length === 2 && u.toHex() === \"0707\"); \
                   const enc = new TextEncoder().encode(\"hi\"); \
                   const dec: string = new TextDecoder().decode(enc); \
                   assert(dec === \"hi\"); \
                   const pd = Temporal.PlainDate.from(\"2024-03-09\"); \
                   const pd2 = pd.add({ days: 1 }).subtract({ days: 1 }).with({ day: 10 }); \
                   const pd3 = pd.toPlainYearMonth().toPlainDate({ day: 9 }); \
                   const pd4 = pd.toPlainMonthDay().toPlainDate({ year: 2024 }); \
                   const pdtFromDate = pd.toPlainDateTime(); \
                   const zFromDate = pd.toZonedDateTime(\"UTC\"); \
                   assert(pd.until(pd2).days === 1 && pd2.since(pd).days === 1); \
                   assert(pd.year === 2024 && pd.toString() === \"2024-03-09\" && pd.toJSON() === \"2024-03-09\" && pd2.day === 10 && pd3.day === 9 && pd4.year === 2024 && pdtFromDate.hour === 0 && zFromDate.year === 2024); \
                   const pt = Temporal.PlainTime.from(\"15:30:45.123456789\"); \
                   const pt2 = pt.add({ hours: 1 }).with({ minute: 0 }); \
                   assert(pt.until(pt2).hours === 1 && pt.hour === 15 && pt.nanosecond === 123456789 && pt2.minute === 0); \
                   const pdt = Temporal.PlainDateTime.from(\"2024-03-09T15:30:45\"); \
                   const pdt2 = pdt.subtract({ hours: 1 }).with({ second: 0 }); \
                   const zFromDateTime = pdt.toZonedDateTime(\"UTC\"); \
                   assert(pdt.since(pdt2).hours === 1 && pdt.toPlainDate().day === 9 && pdt.toPlainTime().hour === 15 && pdt.toPlainYearMonth().month === 3 && pdt.toPlainMonthDay().day === 9); \
                   assert(pdt.month === 3 && pdt2.second === 0 && zFromDateTime.month === 3 && Temporal.PlainDateTime.compare(pdt, pdt) === 0); \
                   const ym = Temporal.PlainYearMonth.from(\"2024-03\"); \
                   const ym2 = ym.add({ months: 1 }).with({ month: 5 }); \
                   assert(ym.until(ym2).months === 2 && ym.monthCode === \"M03\" && ym2.month === 5); \
                   const md = Temporal.PlainMonthDay.from(\"03-09\"); \
                   const md2 = md.with({ day: 10 }); \
                   assert(md.day === 9 && md2.day === 10); \
                   const i0 = Temporal.Instant.from(\"2024-03-09T15:30:45.123456789Z\"); \
                   const i1 = Temporal.Instant.fromEpochMilliseconds(i0.epochMilliseconds); \
                   const i2 = Temporal.Instant.fromEpochNanoseconds(i0.epochNanoseconds); \
                   const i3 = i0.add({ hours: 1 }).subtract({ hours: 1 }).round({ smallestUnit: \"second\" }); \
                   const id0 = i0.until(i1); const id1 = i1.since(i0); \
                   const iz = i0.toZonedDateTimeISO(\"UTC\"); \
                   assert(Temporal.Instant.compare(i0, i2) === 0 && i0.equals(i2) && i0.toString().length > 0 && i0.toJSON().length > 0 && i3.epochMilliseconds > 0 && id0.seconds === id1.seconds && iz.year === 2024); \
                   const dur0 = new Temporal.Duration({ hours: 1 }); const dur1 = Temporal.Duration.from(\"PT1H\"); \
                   assert(Temporal.Duration.compare(dur0, dur1) === 0 && Temporal.ZonedDateTime.compare(iz, iz) === 0); \
                   const ni = Temporal.Now.instant(); const ntz = Temporal.Now.timeZoneId(); const nz = Temporal.Now.zonedDateTimeISO(\"UTC\"); \
                   assert(ni.epochMilliseconds > 0 && ntz.length > 0 && nz.timeZoneId === \"UTC\"); \
                   const re: RegExp = new RegExp(\"a\", \"g\"); \
                   assert(re.test(\"a\") && re.global && re.source === \"a\" && re.flags === \"g\" && re.lastIndex >= 0); \
                   const rmatch: RegExpMatch | null = re.exec(\"a\"); \
                   assert(rmatch !== null); \
                   if (rmatch !== null) { const gg = rmatch.groups; const ng = rmatch.namedGroups; \
                     assert(rmatch.match === \"a\" && rmatch.index === 0 && rmatch.input === \"a\" && gg.length >= 0 && ng.size >= 0); } \
                   const sm: RegExpMatch | null = \"a\".match(re); \
                   const marr = \"aa\".matchAll(re); \
                   const rep: string = \"a\".replace(\"a\", \"b\"); \
                   const repall: string = \"aa\".replaceAll(\"a\", \"b\"); \
                   const sp = \"a,b\".split(\",\"); \
                   assert(sm === sm && \"a\".search(re) >= -1 && marr.length >= 0 && rep.length >= 0 && repall.length >= 0 && sp.length === 2); \
                   assert(isNaN(NaN) && !isFinite(Infinity)); \
                   const oo = { a: 1 }; \
                   assert(Object.keys(oo).length === 1 && Object.values(oo).length === 1); \
                   assert(Object.entries(oo).length === 1 && Object.hasOwn(oo, \"a\") && Object.is(1, 1)); }";
        let ta = type_check(src);
        let (prelude_defs, host_defs, internal_defs) =
            prelude::cached_runtime_package_declarations();
        let mut dependencies: Vec<&crate::PackageDeclaration> = prelude_defs.iter().collect();
        dependencies.extend(host_defs.iter());
        dependencies.extend(internal_defs.iter());
        let bytes = codegen(src, "script.subm", crate::FileId(0), &ta, &dependencies)
            .expect("code generation");
        let imp = imports_with_kind(&bytes);

        // Used ported methods route through the Rust prelude-host package under
        // their dispatch key; the dead Wasm prelude wrappers are left unimported.
        // `Set#size` is a property getter, so checking it routes proves the host
        // port owns the whole Set surface — nothing falls back to the Wasm prelude.
        for (iface, method) in [
            ("String", "repeat"),
            ("Array", "forEach"),
            ("SetConstructor", "new"),
            ("Set", "add"),
            ("Set", "has"),
            ("Set", "delete"),
            ("Set", "size"),
            ("Number", "toFixed"),
            ("Boolean", "toString"),
            ("NumberConstructor", "isInteger"),
            ("NumberConstructor", "parseInt"),
            // Static constant `Number.EPSILON` imports its global from prelude-host too.
            ("NumberConstructor", "EPSILON"),
            // Uint8Array: an instance method, a property getter, a static.
            ("Uint8Array", "fill"),
            ("Uint8Array", "length"),
            ("Uint8Array", "toHex"),
            ("Uint8ArrayConstructor", "fromHex"),
            // TextEncoder/TextDecoder: stateless instance methods + their `new` ctors.
            ("TextEncoder", "encode"),
            ("TextDecoder", "decode"),
            ("TextEncoderConstructor", "new"),
            ("TextDecoderConstructor", "new"),
            // RegExp: construction, instance methods, property getters.
            ("RegExpConstructor", "new"),
            ("RegExp", "test"),
            ("RegExp", "exec"),
            ("RegExp", "source"),
            ("RegExp", "flags"),
            ("RegExp", "lastIndex"),
            ("RegExp", "global"),
            // RegExpMatch accessors, including the Array/Map-building ones.
            ("RegExpMatch", "match"),
            ("RegExpMatch", "index"),
            ("RegExpMatch", "input"),
            ("RegExpMatch", "groups"),
            ("RegExpMatch", "namedGroups"),
            // String regex-arm methods (subsumes the SUB-605 string arm).
            ("String", "match"),
            ("String", "search"),
            ("String", "matchAll"),
            ("String", "replace"),
            ("String", "replaceAll"),
            ("String", "split"),
            // ObjectConstructor statics.
            ("ObjectConstructor", "keys"),
            ("ObjectConstructor", "values"),
            ("ObjectConstructor", "entries"),
            ("ObjectConstructor", "hasOwn"),
            ("ObjectConstructor", "is"),
        ] {
            let key = crate::mangle::extend(&crate::mangle::prelude(iface), method);
            assert!(
                imp.iter()
                    .any(|(m, n, _, _)| m == crate::runtime::prelude::MODULE_NAME
                        && n == key.as_str()),
                "{iface}#{method} should import from the prelude host module"
            );
        }

        for name in ["PI", "abs", "imul", "min", "random"] {
            let key = crate::runtime::prelude::math::math_key(name);
            assert!(
                imp.iter()
                    .any(|(m, n, _, _)| m == crate::runtime::prelude::MODULE_NAME
                        && n == key.as_str()),
                "Math#{name} should import from the prelude host module"
            );
        }

        // Temporal methods route to Rust prelude-host functions under their
        // prelude dispatch keys. Used calls must not import the dead Wasm wrappers.
        for (iface, method) in [
            ("InstantConstructor", "from"),
            ("InstantConstructor", "fromEpochMilliseconds"),
            ("InstantConstructor", "fromEpochNanoseconds"),
            ("InstantConstructor", "compare"),
            ("Instant", "epochMilliseconds"),
            ("Instant", "epochNanoseconds"),
            ("Instant", "add"),
            ("Instant", "subtract"),
            ("Instant", "until"),
            ("Instant", "since"),
            ("Instant", "round"),
            ("Instant", "equals"),
            ("Instant", "toString"),
            ("Instant", "toJSON"),
            ("Instant", "toZonedDateTimeISO"),
            ("Now", "instant"),
            ("Now", "timeZoneId"),
            ("Now", "zonedDateTimeISO"),
            ("DurationConstructor", "new"),
            ("DurationConstructor", "from"),
            ("DurationConstructor", "compare"),
            ("Duration", "years"),
            ("Duration", "months"),
            ("Duration", "weeks"),
            ("Duration", "days"),
            ("Duration", "hours"),
            ("Duration", "minutes"),
            ("Duration", "seconds"),
            ("Duration", "milliseconds"),
            ("Duration", "microseconds"),
            ("Duration", "nanoseconds"),
            ("Duration", "sign"),
            ("Duration", "blank"),
            ("Duration", "add"),
            ("Duration", "subtract"),
            ("Duration", "negated"),
            ("Duration", "abs"),
            ("Duration", "with"),
            ("Duration", "round"),
            ("Duration", "total"),
            ("Duration", "toString"),
            ("Duration", "toJSON"),
            ("ZonedDateTimeConstructor", "from"),
            ("ZonedDateTimeConstructor", "compare"),
            ("ZonedDateTime", "timeZoneId"),
            ("ZonedDateTime", "offset"),
            ("ZonedDateTime", "offsetNanoseconds"),
            ("ZonedDateTime", "epochMilliseconds"),
            ("ZonedDateTime", "epochNanoseconds"),
            ("ZonedDateTime", "hoursInDay"),
            ("ZonedDateTime", "year"),
            ("ZonedDateTime", "monthCode"),
            ("ZonedDateTime", "dayOfWeek"),
            ("ZonedDateTime", "inLeapYear"),
            ("ZonedDateTime", "nanosecond"),
            ("ZonedDateTime", "add"),
            ("ZonedDateTime", "subtract"),
            ("ZonedDateTime", "until"),
            ("ZonedDateTime", "since"),
            ("ZonedDateTime", "with"),
            ("ZonedDateTime", "withTimeZone"),
            ("ZonedDateTime", "round"),
            ("ZonedDateTime", "startOfDay"),
            ("ZonedDateTime", "toInstant"),
            ("ZonedDateTime", "toPlainDate"),
            ("ZonedDateTime", "toPlainTime"),
            ("ZonedDateTime", "toPlainDateTime"),
            ("ZonedDateTime", "equals"),
            ("ZonedDateTime", "toString"),
            ("ZonedDateTime", "toJSON"),
            ("PlainDateConstructor", "from"),
            ("PlainDate", "year"),
            ("PlainDate", "toString"),
            ("PlainDate", "toJSON"),
            ("PlainDate", "add"),
            ("PlainDate", "subtract"),
            ("PlainDate", "until"),
            ("PlainDate", "since"),
            ("PlainDate", "with"),
            ("PlainDate", "toPlainYearMonth"),
            ("PlainDate", "toPlainMonthDay"),
            ("PlainDate", "toPlainDateTime"),
            ("PlainDate", "toZonedDateTime"),
            ("PlainTimeConstructor", "from"),
            ("PlainTime", "hour"),
            ("PlainTime", "nanosecond"),
            ("PlainTime", "add"),
            ("PlainTime", "until"),
            ("PlainTime", "with"),
            ("PlainDateTimeConstructor", "from"),
            ("PlainDateTime", "month"),
            ("PlainDateTime", "subtract"),
            ("PlainDateTime", "since"),
            ("PlainDateTime", "with"),
            ("PlainDateTime", "toPlainDate"),
            ("PlainDateTime", "toPlainTime"),
            ("PlainDateTime", "toPlainYearMonth"),
            ("PlainDateTime", "toPlainMonthDay"),
            ("PlainDateTime", "toZonedDateTime"),
            ("PlainDateTimeConstructor", "compare"),
            ("PlainYearMonthConstructor", "from"),
            ("PlainYearMonth", "monthCode"),
            ("PlainYearMonth", "add"),
            ("PlainYearMonth", "until"),
            ("PlainYearMonth", "with"),
            ("PlainYearMonth", "toPlainDate"),
            ("PlainMonthDayConstructor", "from"),
            ("PlainMonthDay", "day"),
            ("PlainMonthDay", "with"),
            ("PlainMonthDay", "toPlainDate"),
        ] {
            let key = crate::mangle::extend(
                &crate::mangle::extend(&crate::mangle::prelude("Temporal"), iface),
                method,
            );
            assert!(
                imp.iter()
                    .any(|(m, n, _, _)| m == crate::runtime::prelude::MODULE_NAME
                        && n == key.as_str()),
                "Temporal#{iface}#{method} should import from the prelude host module"
            );
        }

        // The boxed-primitive vtables are host-owned now, imported from prelude-host.
        for name in ["boxed_number_vtable", "boxed_boolean_vtable"] {
            let key = crate::mangle::prelude(name);
            assert!(
                imp.iter()
                    .any(|(m, n, _, _)| m == crate::runtime::prelude::MODULE_NAME
                        && n == key.as_str()),
                "{name} should import from the prelude host module"
            );
        }

        // `globalThis` numeric symbols also route to prelude-host. `NaN` is
        // lowered as an immediate in this fixture, so no import is emitted for it.
        for name in ["isNaN", "isFinite", "Infinity"] {
            let key = crate::mangle::prelude(name);
            assert!(
                imp.iter()
                    .any(|(m, n, _, _)| m == crate::runtime::prelude::MODULE_NAME
                        && n == key.as_str()),
                "globalThis {name} should import from the prelude host module"
            );
        }
        let unused = crate::mangle::extend(&crate::mangle::prelude("String"), "charAt");
        assert!(
            !imp.iter().any(|(_, n, _, _)| n == unused.as_str()),
            "unused ported methods should be tree-shaken",
        );
    }

    #[test]
    fn global_imports_assigned_indices_starting_from_zero() {
        let defs = fake_globals_definitions();
        let ta = type_check("function main(): void { }");
        let prelude_defs = prelude::prelude_package_declaration();
        let prelude_host_defs = crate::runtime::prelude::package_declaration();
        let dependencies: [&crate::PackageDeclaration; 3] =
            [&prelude_defs, &prelude_host_defs, &defs];
        let _ = codegen(
            "function main(): void { }",
            "script.subm",
            crate::FileId(0),
            &ta,
            &dependencies,
        );

        let mut map = SymbolTable::default();
        let mut next_global_idx: u32 = 0;
        for defs in [&prelude_defs, &defs] {
            for value in defs.values.values() {
                let ty = match &value.kind {
                    crate::ValueKind::Let { ty, .. } | crate::ValueKind::Const { ty, .. } => {
                        Some(ty)
                    }
                    _ => None,
                };
                if let Some(ty) = ty {
                    if matches!(ty, crate::Type::InterfaceRef { .. }) {
                        continue;
                    }
                    map.record_global(value.mangled_name.clone(), next_global_idx);
                    next_global_idx += 1;
                }
            }
        }
        assert_eq!(map.prelude_global_idx("Infinity"), Some(0));
        assert_eq!(map.prelude_global_idx("NaN"), Some(1));
        assert_eq!(map.prelude_global_idx("string_comma"), Some(2));
        assert_eq!(map.prelude_global_idx("string_false"), Some(3));
        assert_eq!(map.prelude_global_idx("string_null"), Some(4));
        assert_eq!(map.prelude_global_idx("string_object_function"), Some(5));
        assert_eq!(map.prelude_global_idx("string_object_object"), Some(6));
        assert_eq!(map.prelude_global_idx("string_true"), Some(7));
        assert_eq!(
            map.global_idx(&crate::mangle::package_symbol("test:globals", "counter")),
            Some(8),
        );
        assert_eq!(
            map.global_idx(&crate::mangle::package_symbol(
                "test:globals",
                "max_iterations"
            )),
            Some(9),
        );
        assert_eq!(
            map.global_idx(&crate::mangle::package_symbol("test:globals", "missing")),
            None,
        );
    }

    #[test]
    fn module_with_global_imports_validates() {
        let defs = fake_globals_definitions();
        let bytes = compile_with_package_imports("function main(): void { }", &[&defs]);
        wasmparser::Validator::new()
            .validate_all(&bytes)
            .expect("module with global imports validates");
    }

    #[test]
    fn no_top_level_globals_still_emits_empty_start() {
        let bytes = compile("function main(): void { }");
        assert_eq!(
            start_function_idx(&bytes),
            Some(imported_func_count(&bytes))
        );
        // User functions, module start, and the shared field lookup.
        assert_eq!(function_count(&bytes), 3);
    }

    #[test]
    fn top_level_string_let_declares_one_mutable_ref_global() {
        let bytes = compile(r#"let x: string = "hi"; function main(): void { }"#);
        let baseline = compile("function main(): void { }");
        assert_eq!(global_count(&bytes), global_count(&baseline) + 1);
        assert!(
            global_decls(&bytes).contains(&(true, "ref")),
            "top-level string let should declare a mutable ref global",
        );
    }

    #[test]
    fn top_level_const_also_declared_mutable_at_wasm_level() {
        let bytes = compile(r#"const x: string = "hi"; function main(): void { }"#);
        let baseline = compile("function main(): void { }");
        assert_eq!(global_count(&bytes), global_count(&baseline) + 1);
        assert!(
            global_decls(&bytes).contains(&(true, "ref")),
            "top-level string const should declare a mutable ref global",
        );
    }

    #[test]
    fn top_level_let_emits_start_section_pointing_at_first_local_func() {
        let bytes = compile(r#"let x: string = "hi"; function main(): void { }"#);
        let first_local_func = imported_func_count(&bytes);
        assert_eq!(start_function_idx(&bytes), Some(first_local_func));
        assert_eq!(main_export_func_idx(&bytes), Some(first_local_func + 1));
    }

    #[test]
    fn top_level_string_let_validates_and_links_with_prelude() {
        let bytes = compile(r#"let x: string = "hello"; function main(): void { }"#);
        instantiate_against_prelude(&bytes);
    }

    #[test]
    fn arithmetic_precedence_returns_seven() {
        assert_eq!(
            run_main_f64("function main(): number { return 1 + 2 * 3; }"),
            7.0
        );
    }

    #[test]
    fn arithmetic_subtraction_and_division() {
        assert_eq!(
            run_main_f64("function main(): number { return 10 - 4 / 2; }"),
            8.0
        );
    }

    #[test]
    fn modulo_truncates_toward_zero() {
        // JS semantics: lhs sign wins.
        assert_eq!(
            run_main_f64("function main(): number { return 10 % 3; }"),
            1.0
        );
        assert_eq!(
            run_main_f64("function main(): number { return -10 % 3; }"),
            -1.0
        );
        assert_eq!(
            run_main_f64("function main(): number { return 10 % -3; }"),
            1.0
        );
    }

    #[test]
    fn division_by_zero_yields_infinity() {
        // f64.div doesn't trap on 1/0 per IEEE 754.
        let r = run_main_f64("function main(): number { return 1 / 0; }");
        assert!(r.is_infinite() && r.is_sign_positive());
    }

    #[test]
    fn zero_div_zero_yields_nan() {
        let r = run_main_f64("function main(): number { return 0 / 0; }");
        assert!(r.is_nan());
    }

    #[test]
    fn unary_neg_negates_grouped_addition() {
        assert_eq!(
            run_main_f64("function main(): number { return -(2 + 3); }"),
            -5.0
        );
    }

    #[test]
    fn unary_pos_is_identity() {
        assert_eq!(run_main_f64("function main(): number { return +5; }"), 5.0);
    }

    #[test]
    fn nested_arithmetic_round_trip() {
        assert_eq!(
            run_main_f64("function main(): number { return (1 + 2) * (4 - 1); }"),
            9.0
        );
    }

    #[test]
    fn top_level_let_referenced_from_main_body() {
        assert_eq!(
            run_main_f64("let x: number = 7; function main(): number { return x + 1; }"),
            8.0
        );
    }

    #[test]
    fn while_counter_loop_sums_three_iterations() {
        let src = "function main(): number {\n  let i = 0;\n  let n = 0;\n  while (i < 3) {\n    n = n + i;\n    i = i + 1;\n  }\n  return n;\n}";
        assert_eq!(run_main_f64(src), 3.0);
    }

    #[test]
    fn while_zero_iterations_skips_body() {
        let src = "function main(): number {\n  let i = 0;\n  while (i < 0) {\n    i = i + 1;\n  }\n  return i;\n}";
        assert_eq!(run_main_f64(src), 0.0);
    }

    #[test]
    fn while_with_nested_if_counts_evens() {
        let src = "function main(): number {\n  let i = 0;\n  let evens = 0;\n  while (i < 5) {\n    if (i % 2 === 0) {\n      evens = evens + 1;\n    }\n    i = i + 1;\n  }\n  return evens;\n}";
        assert_eq!(run_main_f64(src), 3.0);
    }

    #[test]
    fn nested_while_loops_multiply_iterations() {
        let src = "function main(): number {\n  let total = 0;\n  let i = 0;\n  while (i < 3) {\n    let j = 0;\n    while (j < 4) {\n      total = total + 1;\n      j = j + 1;\n    }\n    i = i + 1;\n  }\n  return total;\n}";
        assert_eq!(run_main_f64(src), 12.0);
    }

    #[test]
    fn while_drives_top_level_global() {
        let src = "let g = 0; function main(): number {\n  let i = 0;\n  while (i < 4) {\n    g = g + 1;\n    i = i + 1;\n  }\n  return g;\n}";
        assert_eq!(run_main_f64(src), 4.0);
    }

    #[test]
    fn if_true_takes_then_branch() {
        assert_eq!(
            run_main_f64("function main(): number { if (true) { return 1; } return 2; }"),
            1.0
        );
    }

    #[test]
    fn if_false_skips_then_branch() {
        assert_eq!(
            run_main_f64("function main(): number { if (false) { return 1; } return 2; }"),
            2.0
        );
    }

    #[test]
    fn if_else_picks_branch() {
        assert_eq!(
            run_main_f64("function main(): number { if (false) { return 1; } else { return 2; } }"),
            2.0
        );
        assert_eq!(
            run_main_f64("function main(): number { if (true) { return 1; } else { return 2; } }"),
            1.0
        );
    }

    #[test]
    fn if_combines_with_assignment() {
        assert_eq!(
            run_main_f64(
                "function main(): number { let x: number = 0; if (1 < 2) { x = 1; } else { x = 2; } return x; }"
            ),
            1.0
        );
    }

    #[test]
    fn else_if_chain_terminal_else() {
        let src = "function main(): number { let x: number = 3; if (x === 1) { return 10; } else if (x === 2) { return 20; } else { return 30; } }";
        assert_eq!(run_main_f64(src), 30.0);
    }

    #[test]
    fn else_if_chain_middle_branch() {
        let src = "function main(): number { let x: number = 2; if (x === 1) { return 10; } else if (x === 2) { return 20; } else { return 30; } }";
        assert_eq!(run_main_f64(src), 20.0);
    }

    #[test]
    fn nested_if_inside_if() {
        let src = "function main(): number { let x: number = 3; if (x > 0) { if (x > 2) { return 99; } return 11; } return 0; }";
        assert_eq!(run_main_f64(src), 99.0);
    }

    #[test]
    fn function_local_let_round_trips_via_local() {
        assert_eq!(
            run_main_f64("function main(): number { let x: number = 1; return x; }"),
            1.0
        );
    }

    #[test]
    fn function_local_assignment_overwrites_local() {
        assert_eq!(
            run_main_f64("function main(): number { let x: number = 1; x = 2; return x; }"),
            2.0
        );
    }

    #[test]
    fn function_local_let_inferred_from_initializer() {
        assert_eq!(
            run_main_f64("function main(): number { let x = 5; return x + 1; }"),
            6.0
        );
    }

    #[test]
    fn function_local_const_round_trips() {
        assert_eq!(
            run_main_f64("function main(): number { const c = 5; return c; }"),
            5.0
        );
    }

    #[test]
    fn assignment_to_top_level_let_is_global_set() {
        assert_eq!(
            run_main_f64("let g = 1; function main(): number { g = 2; return g; }"),
            2.0
        );
    }

    #[test]
    fn assignment_uses_self_reference_on_rhs() {
        assert_eq!(
            run_main_f64("function main(): number { let i = 0; i = i + 1; i = i + 1; return i; }"),
            2.0
        );
    }

    #[test]
    fn const_reassignment_is_compile_error() {
        let mut asi = Asi::new(
            "function main(): number { const c = 5; c = 6; return c; }",
            crate::FileId(0),
        );
        let mut tokens: Vec<Token> = Vec::new();
        loop {
            let tok = asi.next_token();
            let is_eof = matches!(tok.kind, TokenKind::Eof);
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        let (ast, _) = parse(
            "function main(): number { const c = 5; c = 6; return c; }",
            tokens,
            crate::FileId(0),
        );
        let diags = infer_with_runtime_packages(
            "function main(): number { const c = 5; c = 6; return c; }",
            &ast,
        );
        assert!(
            diags
                .iter()
                .any(|d| d.message == "cannot assign to const binding `c`"),
            "expected const-reassignment diagnostic, got {diags:?}",
        );
    }

    #[test]
    fn assignment_type_mismatch_is_compile_error() {
        let src = "function main(): number { let x: number = 1; x = 2; return x; }";
        let _ = compile(src);

        let bad = r#"function main(): number { let x: number = 1; x = "hi"; return x; }"#;
        let mut asi = Asi::new(bad, crate::FileId(0));
        let mut tokens: Vec<Token> = Vec::new();
        loop {
            let tok = asi.next_token();
            let is_eof = matches!(tok.kind, TokenKind::Eof);
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        let (ast, _) = parse(bad, tokens, crate::FileId(0));
        let diags = infer_with_runtime_packages(bad, &ast);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("expected `number`")),
            "expected type-mismatch diagnostic, got {diags:?}",
        );
    }

    #[test]
    fn assignment_to_function_is_compile_error() {
        let bad = "function helper(): void { } function main(): void { helper = 1; }";
        let mut asi = Asi::new(bad, crate::FileId(0));
        let mut tokens: Vec<Token> = Vec::new();
        loop {
            let tok = asi.next_token();
            let is_eof = matches!(tok.kind, TokenKind::Eof);
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        let (ast, _) = parse(bad, tokens, crate::FileId(0));
        let diags = infer_with_runtime_packages(bad, &ast);
        assert!(
            diags
                .iter()
                .any(|d| d.message == "cannot assign to function `helper`"),
            "expected function-reassignment diagnostic, got {diags:?}",
        );
    }

    #[test]
    fn boolean_literal_round_trips() {
        assert_eq!(run_main_i32("function main(): boolean { return true; }"), 1);
        assert_eq!(
            run_main_i32("function main(): boolean { return false; }"),
            0
        );
    }

    #[test]
    fn numeric_comparisons() {
        assert_eq!(
            run_main_i32("function main(): boolean { return 1 < 2; }"),
            1
        );
        assert_eq!(
            run_main_i32("function main(): boolean { return 2 < 1; }"),
            0
        );
        assert_eq!(
            run_main_i32("function main(): boolean { return 2 > 1; }"),
            1
        );
        assert_eq!(
            run_main_i32("function main(): boolean { return 2 <= 2; }"),
            1
        );
        assert_eq!(
            run_main_i32("function main(): boolean { return 2 >= 3; }"),
            0
        );
    }

    #[test]
    fn numeric_equality() {
        assert_eq!(
            run_main_i32("function main(): boolean { return 1 === 1; }"),
            1
        );
        assert_eq!(
            run_main_i32("function main(): boolean { return 1 !== 2; }"),
            1
        );
        assert_eq!(
            run_main_i32("function main(): boolean { return 1 == 1; }"),
            1
        );
        assert_eq!(
            run_main_i32("function main(): boolean { return 1 != 1; }"),
            0
        );
    }

    #[test]
    fn boolean_equality() {
        assert_eq!(
            run_main_i32("function main(): boolean { return true === true; }"),
            1
        );
        assert_eq!(
            run_main_i32("function main(): boolean { return true !== false; }"),
            1
        );
    }

    #[test]
    fn string_equality_via_runtime() {
        assert_eq!(
            run_main_i32(r#"function main(): boolean { return ("a" + "b") === "ab"; }"#),
            1
        );
        assert_eq!(
            run_main_i32(r#"function main(): boolean { return "ab" !== "ac"; }"#),
            1
        );
        assert_eq!(
            run_main_i32(r#"function main(): boolean { return "ab" === "ac"; }"#),
            0
        );
    }

    #[test]
    fn unary_not() {
        assert_eq!(
            run_main_i32("function main(): boolean { return !true; }"),
            0
        );
        assert_eq!(
            run_main_i32("function main(): boolean { return !false; }"),
            1
        );
        assert_eq!(
            run_main_i32("function main(): boolean { return !!true; }"),
            1
        );
    }

    #[test]
    fn logical_and_or_results() {
        assert_eq!(
            run_main_i32("function main(): boolean { return true && true; }"),
            1
        );
        assert_eq!(
            run_main_i32("function main(): boolean { return true && false; }"),
            0
        );
        assert_eq!(
            run_main_i32("function main(): boolean { return false || true; }"),
            1
        );
        assert_eq!(
            run_main_i32("function main(): boolean { return false || false; }"),
            0
        );
    }

    #[test]
    fn logical_and_short_circuits_structurally() {
        let bytes = compile("function main(): boolean { return false && true; }");
        instantiate_against_prelude(&bytes);
        let main_idx = main_export_func_idx(&bytes).expect("main exported");
        let body_idx = (main_idx - imported_func_count(&bytes)) as usize;
        let mut bodies_seen = 0;
        let mut saw_if = false;
        for payload in Parser::new(0).parse_all(&bytes) {
            if let Payload::CodeSectionEntry(body) = payload.expect("payload") {
                if bodies_seen == body_idx {
                    let mut reader = body
                        .get_operators_reader()
                        .expect("body operators readable");
                    while !reader.eof() {
                        if matches!(
                            reader.read().expect("operator"),
                            wasmparser::Operator::If { .. }
                        ) {
                            saw_if = true;
                            break;
                        }
                    }
                    break;
                }
                bodies_seen += 1;
            }
        }
        assert!(saw_if, "logical && should compile to a branchy `if` block");
    }

    #[test]
    fn string_concat_compiles_and_links() {
        let bytes = compile(r#"function main(): string { return "a" + "b"; }"#);
        instantiate_against_prelude(&bytes);

        let segments = data_segments(&bytes);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0], b"a\0".to_vec());
        assert_eq!(segments[1], b"b\0".to_vec());

        let concat_idx = imported_func_idx(&bytes, "submilli:prelude#string_concat");
        let calls = main_body_call_targets(&bytes);
        assert_eq!(
            calls,
            vec![concat_idx],
            "main should call string_concat exactly once",
        );
    }

    #[test]
    fn string_concat_with_mixed_length_literals() {
        let bytes = compile(r#"function main(): string { return "Hello, " + "world"; }"#);
        instantiate_against_prelude(&bytes);
        assert_eq!(data_segments(&bytes).len(), 2);
        let concat_idx = imported_func_idx(&bytes, "submilli:prelude#string_concat");
        assert_eq!(main_body_call_targets(&bytes), vec![concat_idx]);
    }

    #[test]
    fn string_concat_chained_emits_two_calls() {
        let bytes = compile(r#"function main(): string { return "a" + "b" + "c"; }"#);
        instantiate_against_prelude(&bytes);
        let concat_idx = imported_func_idx(&bytes, "submilli:prelude#string_concat");
        assert_eq!(main_body_call_targets(&bytes), vec![concat_idx, concat_idx]);
    }

    #[test]
    fn string_concat_uses_top_level_global() {
        let bytes = compile(
            r#"let greeting: string = "Hello, "; function main(): string { return greeting + "world"; }"#,
        );
        instantiate_against_prelude(&bytes);
        let concat_idx = imported_func_idx(&bytes, "submilli:prelude#string_concat");
        assert_eq!(main_body_call_targets(&bytes), vec![concat_idx]);
    }

    #[test]
    fn user_global_idx_resolves_for_consumer_locals() {
        let mut map = SymbolTable::default();
        map.record_global(crate::mangle::package_symbol("main", "x"), 0);
        map.record_global(crate::mangle::package_symbol("main", "y"), 1);
        assert_eq!(
            map.global_idx(&crate::mangle::package_symbol("main", "x")),
            Some(0)
        );
        assert_eq!(
            map.global_idx(&crate::mangle::package_symbol("main", "y")),
            Some(1)
        );
        assert_eq!(
            map.global_idx(&crate::mangle::package_symbol("main", "missing")),
            None
        );
    }

    #[test]
    fn dwarf_sections_emitted_and_validate() {
        let bytes = compile("function main(): void { }");
        assert!(!custom_section(&bytes, ".debug_info").unwrap().is_empty());
        assert!(!custom_section(&bytes, ".debug_abbrev").unwrap().is_empty());
        assert!(!custom_section(&bytes, ".debug_str").unwrap().is_empty());
        instantiate_against_prelude(&bytes);
    }

    #[test]
    fn dwarf_subprogram_carries_main_name() {
        let bytes = compile("function main(): void { }");
        let strs = custom_section(&bytes, ".debug_str").unwrap();
        assert!(
            strs.windows(4).any(|w| w == b"main"),
            "expected `main` in .debug_str, got {strs:?}",
        );
    }

    #[test]
    fn dwarf_skips_compiler_internal_start_function() {
        let bytes = compile("function main(): void { }");
        let strs = custom_section(&bytes, ".debug_str").unwrap();
        assert!(
            !strs.windows(6).any(|w| w == b"_start"),
            "_start should not appear in .debug_str",
        );
    }

    #[test]
    fn dwarf_die_snapshot() {
        let bytes = compile("function main(): void { }");
        let dump = dump_dwarf_dies(&bytes);
        insta::assert_snapshot!(dump);
    }

    #[test]
    fn forward_reference_call_resolves() {
        assert_eq!(
            run_main_f64(
                "function main(): number { return helper(); } \
                 function helper(): number { return 42; }",
            ),
            42.0,
        );
    }

    #[test]
    fn function_with_args_passes_through() {
        assert_eq!(
            run_main_f64(
                "function double(n: number): number { return n * 2; } \
                 function main(): number { return double(21); }",
            ),
            42.0,
        );
    }

    #[test]
    fn function_with_multiple_args_in_order() {
        assert_eq!(
            run_main_f64(
                "function sub(a: number, b: number): number { return a - b; } \
                 function main(): number { return sub(10, 3); }",
            ),
            7.0,
        );
    }

    #[test]
    fn nested_call_evaluates_inner_first() {
        assert_eq!(
            run_main_f64(
                "function inner(n: number): number { return n + 1; } \
                 function outer(n: number): number { return n * 10; } \
                 function main(): number { return outer(inner(5)); }",
            ),
            60.0,
        );
    }

    #[test]
    fn recursion_works_via_self_call() {
        assert_eq!(
            run_main_f64(
                "function fact(n: number): number { \
                    if (n <= 1) { return 1; } else { return n * fact(n - 1); } \
                 } \
                 function main(): number { return fact(5); }",
            ),
            120.0,
        );
    }

    #[test]
    fn mvp_target_greeting_self_check() {
        let src = "\
function greet(name: string): string {\n\
  return \"Hello, \" + name;\n\
}\n\
function main(): boolean {\n\
  return greet(\"world\") === \"Hello, world\";\n\
}\n";
        assert_eq!(run_main_i32(src), 1);
    }

    #[test]
    fn top_level_let_initializer_calls_user_function() {
        let src = "\
function compute(): number { return 7; }\n\
let cached: number = compute();\n\
function main(): number { return cached + 1; }\n";
        assert_eq!(run_main_f64(src), 8.0);
    }

    #[test]
    fn dwarf_subprograms_for_each_user_function() {
        let bytes = compile("function main(): void { } function helper(): void { }");
        let dump = dump_dwarf_dies(&bytes);
        insta::assert_snapshot!(dump);
    }

    #[test]
    fn dwarf_line_section_emitted() {
        let bytes = compile("function main(): void { }");
        assert!(
            !custom_section(&bytes, ".debug_line").unwrap().is_empty(),
            ".debug_line should be emitted",
        );
        instantiate_against_prelude(&bytes);
    }

    #[test]
    fn dwarf_line_program_snapshot() {
        let src = "\
function main(): number {
  let x: number = 1;
  let y: number = 2;
  return x + y;
}
";
        let bytes = compile(src);
        let dump = dump_dwarf_lines(&bytes);
        insta::assert_snapshot!(dump);
    }

    #[test]
    fn dwarf_line_program_covers_helper() {
        let src = "\
function main(): number {
  return helper();
}
function helper(): number {
  return 42;
}
";
        let bytes = compile(src);
        let dump = dump_dwarf_lines(&bytes);
        insta::assert_snapshot!(dump);
    }

    #[test]
    fn assert_true_is_no_op() {
        assert_eq!(
            run_main_f64("function main(): number { assert(true, \"ok\"); return 1; }",),
            1.0,
        );
    }

    #[test]
    fn assert_typechecks_with_string_message() {
        run_main_f64(
            "function main(): number { let m: string = \"ok\"; assert(true, m); return 0; }",
        );
    }

    #[test]
    fn assert_evaluates_msg_for_side_effects() {
        assert_eq!(
            run_main_f64("function main(): number { assert(true, \"a\" + \"b\"); return 1; }",),
            1.0,
        );
    }

    #[test]
    fn user_function_named_assert_is_rejected() {
        let src = "function assert(): void { } function main(): void { }";
        let mut asi = Asi::new(src, crate::FileId(0));
        let mut tokens: Vec<Token> = Vec::new();
        loop {
            let tok = asi.next_token();
            let is_eof = matches!(tok.kind, TokenKind::Eof);
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        let (ast, _) = parse(src, tokens, crate::FileId(0));
        let diags = infer_with_runtime_packages(src, &ast);
        assert!(
            diags.iter().any(|d| d.message
                == "`assert` is a reserved compiler intrinsic and cannot be redeclared"),
            "expected reserved-intrinsic diagnostic, got {diags:?}",
        );
    }

    #[test]
    fn assert_false_throws_with_backtrace() {
        let dump = run_main_expecting_error("function main(): void { assert(false, \"oops\"); }");
        insta::assert_snapshot!(dump);
    }

    #[test]
    fn multi_frame_backtrace() {
        let src = "\
function inner(): void { assert(false, \"x\"); }
function main(): void { inner(); }
";
        let dump = run_main_expecting_error(src);
        insta::assert_snapshot!(dump);
    }

    #[test]
    fn three_frame_backtrace_labels_caller_in_middle() {
        let src = "\
function deepest(): void { assert(false, \"x\"); }
function middle(): void { deepest(); }
function main(): void { middle(); }
";
        let dump = run_main_expecting_error(src);
        insta::assert_snapshot!(dump);
    }

    #[test]
    fn render_backtrace_returns_none_for_non_trap_error() {
        #[derive(Debug)]
        struct Plain;
        impl std::fmt::Display for Plain {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("plain error")
            }
        }
        impl std::error::Error for Plain {}
        let err: wasmtime::Error = wasmtime::Error::new(Plain);
        let (sources, file) = crate::Sources::single("script.subm", "").unwrap();
        assert!(
            crate::render_backtrace(&err, &sources, file, crate::BacktraceMode::Full).is_none()
        );
    }

    fn run_main_with_cfg_expecting_trap(
        cfg: &crate::runtime::RuntimeConfig,
        source: &str,
    ) -> String {
        let bytes = compile(source);
        let (mut store, inst) = link_consumer_with(cfg, &bytes);
        let main = inst
            .get_typed_func::<(), ()>(&mut store, "main")
            .expect("main signature is `() -> void` in expecting-trap fixtures");
        let err = pollster::block_on(main.call_async(&mut store, ()))
            .expect_err("expected main() to trap");
        let (sources, file) = crate::Sources::single("script.subm", source).unwrap();
        crate::render_backtrace(&err, &sources, file, crate::BacktraceMode::Full)
            .expect("backtrace empty — was wasm_backtrace_details enabled?")
    }

    #[test]
    fn raw_trap_renders_an_error_header() {
        let cfg = crate::runtime::RuntimeConfig {
            max_wasm_stack: 64 * 1024,
            ..Default::default()
        };
        let dump = run_main_with_cfg_expecting_trap(
            &cfg,
            "function rec(): void { rec(); } function main(): void { rec(); }",
        );
        assert!(
            dump.starts_with("error: call stack exhausted\n"),
            "expected an `error:` header naming the trap, got:\n{dump}",
        );
    }

    #[test]
    fn fuel_trap_header_does_not_leak_the_engine() {
        // The engine's own text here is `all fuel consumed by WebAssembly`, which
        // names a host mechanism rather than anything the program can act on.
        let cfg = crate::runtime::RuntimeConfig {
            fuel: 1024,
            ..Default::default()
        };
        let dump =
            run_main_with_cfg_expecting_trap(&cfg, "function main(): void { while (true) { } }");
        assert!(
            dump.starts_with("error: fuel exhausted\n"),
            "expected a curated `error:` header, got:\n{dump}",
        );
        assert!(
            !dump.contains("WebAssembly"),
            "engine wording leaked into the header:\n{dump}",
        );
    }

    #[test]
    fn fuel_exhaustion_traps_with_label() {
        let cfg = crate::runtime::RuntimeConfig {
            fuel: 1024,
            ..Default::default()
        };
        let dump =
            run_main_with_cfg_expecting_trap(&cfg, "function main(): void { while (true) { } }");
        assert!(
            dump.contains("[fuel exhausted]"),
            "expected fuel-exhausted label, got:\n{dump}",
        );
    }

    #[test]
    fn stack_overflow_traps_with_label() {
        let cfg = crate::runtime::RuntimeConfig {
            max_wasm_stack: 64 * 1024,
            ..Default::default()
        };
        let dump = run_main_with_cfg_expecting_trap(
            &cfg,
            "function rec(): void { rec(); } \
             function main(): void { rec(); }",
        );
        assert!(
            dump.contains("[stack overflow]"),
            "expected stack-overflow label, got:\n{dump}",
        );
    }

    fn run_main_capturing_console(source: &str) -> String {
        use std::sync::{Arc, Mutex};
        struct Shared(Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for Shared {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().write(buf)
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let buf = Arc::new(Mutex::new(Vec::new()));
        let mut data = crate::runtime::StoreData::with_vfs(
            crate::runtime::Vfs::tempdir().expect("tempdir for codegen test VFS"),
        );
        data.console = Box::new(Shared(Arc::clone(&buf)));
        let cfg = crate::runtime::RuntimeConfig::default();
        let bytes = compile(source);
        let engine = cfg.engine().expect("engine");
        let mut store = cfg.store(&engine, data).expect("store");
        let consumer_module = wasmtime::Module::new(&engine, &bytes).expect("consumer module");
        let mut linker = wasmtime::Linker::<crate::runtime::StoreData>::new(&engine);
        let inst = pollster::block_on(async {
            crate::runtime::install_runtime_async(&mut linker, &mut store)
                .await
                .expect("runtime");
            linker
                .instantiate_async(&mut store, &consumer_module)
                .await
                .expect("consumer instantiates")
        });
        let main = inst
            .get_typed_func::<(), ()>(&mut store, "main")
            .expect("main () -> void");
        pollster::block_on(main.call_async(&mut store, ())).expect("main does not trap");
        let captured = buf.lock().unwrap().clone();
        String::from_utf8(captured).expect("utf8")
    }

    #[test]
    fn console_log_writes_message_with_newline() {
        let out = run_main_capturing_console("function main(): void { console.log(\"hi\"); }");
        assert_eq!(out, "hi\n");
    }

    #[test]
    fn console_log_uses_string_concat() {
        let out = run_main_capturing_console(
            "function main(): void { console.log(\"Hello, \" + \"world\"); }",
        );
        assert_eq!(out, "Hello, world\n");
    }

    #[test]
    fn console_log_joins_multiple_primitive_args() {
        let out =
            run_main_capturing_console("function main(): void { console.log(\"a\", 1, true); }");
        assert_eq!(out, "a 1 true\n");
    }

    #[test]
    fn console_log_coerces_array_and_object_args() {
        let out =
            run_main_capturing_console("function main(): void { console.log([1, 2], { x: 1 }); }");
        assert_eq!(out, "1,2 [object Object]\n");
    }

    #[test]
    fn console_log_accepts_single_non_string_arg() {
        let out = run_main_capturing_console(
            "function main(): void { const items = [1, 2, 3]; console.log(items.length); }",
        );
        assert_eq!(out, "3\n");
    }

    #[test]
    fn console_log_multiple_calls_appends() {
        let out = run_main_capturing_console(
            "function main(): void { console.log(\"a\"); console.log(\"b\"); }",
        );
        assert_eq!(out, "a\nb\n");
    }

    #[test]
    fn console_log_inside_function_call() {
        let out = run_main_capturing_console(
            "function shout(s: string): void { console.log(s); } \
             function main(): void { shout(\"hello\"); }",
        );
        assert_eq!(out, "hello\n");
    }

    #[test]
    fn user_function_named_console_is_rejected() {
        let src = "function console(): void { } function main(): void { }";
        let mut asi = Asi::new(src, crate::FileId(0));
        let mut tokens: Vec<Token> = Vec::new();
        loop {
            let tok = asi.next_token();
            let is_eof = matches!(tok.kind, TokenKind::Eof);
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        let (ast, _) = parse(src, tokens, crate::FileId(0));
        let diags = infer_with_runtime_packages(src, &ast);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("duplicate declaration of `console`")),
            "expected duplicate-declaration diagnostic, got {diags:?}",
        );
    }

    #[test]
    fn object_literal_round_trip() {
        let result =
            run_main_f64("function main(): number { let p = { x: 1, y: 2 }; return p.x + p.y; }");
        assert_eq!(result, 3.0);
    }

    #[test]
    fn object_literal_field_order_independent() {
        let result =
            run_main_f64("function main(): number { let p = { y: 10, x: 5 }; return p.y; }");
        assert_eq!(result, 10.0);
    }

    #[test]
    fn object_with_string_field_round_trip() {
        let bytes = compile(
            r#"function main(): string { let p = { name: "hi", age: 30 }; return p.name; }"#,
        );
        let cfg = crate::runtime::RuntimeConfig::default();
        let result = pollster::block_on(cfg.run(&bytes)).expect("run");
        assert_eq!(result.value.as_deref(), Some("hi"));
    }

    #[test]
    fn object_passed_through_function_signature() {
        let result = run_main_f64(
            "function take(p: { v: number }): number { return p.v; } function main(): number { return take({ v: 7 }); }",
        );
        assert_eq!(result, 7.0);
    }

    #[test]
    fn array_of_numbers_round_trip() {
        let result = run_main_f64("function main(): number { let a = [1, 2, 3]; return a[1]; }");
        assert_eq!(result, 2.0);
    }

    #[test]
    fn array_of_numbers_annotated() {
        let result =
            run_main_f64("function main(): number { let a: number[] = [42]; return a[0]; }");
        assert_eq!(result, 42.0);
    }

    #[test]
    fn array_of_strings_no_boxing() {
        let bytes = compile(r#"function main(): string { let a = ["a", "b", "c"]; return a[2]; }"#);
        let cfg = crate::runtime::RuntimeConfig::default();
        let result = pollster::block_on(cfg.run(&bytes)).expect("run");
        assert_eq!(result.value.as_deref(), Some("c"));
    }

    #[test]
    fn array_of_objects_field_access() {
        let result = run_main_f64(
            "function main(): number { let xs: { v: number }[] = [{ v: 7 }]; return xs[0].v; }",
        );
        assert_eq!(result, 7.0);
    }

    #[test]
    fn array_index_out_of_bounds_throws_catchable() {
        // An OOB index used to map to a raw `array.get` that traps uncatchably;
        // it now throws a catchable `Error`, so user `try/catch` can recover.
        let bytes = compile(
            r#"function main(): string {
                let a = [1, 2, 3];
                try {
                    let _ = a[10];
                    return "no throw";
                } catch (e: Error) {
                    return e.message;
                }
            }"#,
        );
        let cfg = crate::runtime::RuntimeConfig::default();
        let result =
            pollster::block_on(cfg.run(&bytes)).expect("OOB throw is catchable, not a trap");
        assert_eq!(result.value.as_deref(), Some("index out of range"));
    }

    #[test]
    fn array_index_out_of_bounds_uncaught_is_not_a_trap() {
        let bytes = compile("function main(): number { let a = [1, 2, 3]; return a[10]; }");
        let cfg = crate::runtime::RuntimeConfig::default();
        let err = pollster::block_on(cfg.run(&bytes)).expect_err("uncaught OOB throw still aborts");
        assert!(
            err.downcast_ref::<wasmtime::Trap>().is_none(),
            "OOB should surface as a thrown exception, not a Wasm trap: {err:#}",
        );
    }

    // Test helpers flip `boxed: true` manually to exercise boxed codegen paths without needing closure end-to-end.
    use crate::codegen::function_emitter::stmt::emit_statement;
    use crate::{ClosureBody, ExprId, StmtId, TypedExprKind, TypedStmtKind};
    use wasm_encoder::HeapType;
    use wasm_encoder::Ieee64;
    use wasm_encoder::Instruction;
    use wasm_encoder::{
        CodeSection, ConstExpr, ExportKind, ExportSection, Function, FunctionSection,
        GlobalSection, ImportSection, Module, RefType, TypeSection, ValType,
    };

    fn boxed_typed_ast(source: &str) -> TypedAst {
        let mut asi = Asi::new(source, crate::FileId(0));
        let mut tokens: Vec<Token> = Vec::new();
        loop {
            let tok = asi.next_token();
            let is_eof = matches!(tok.kind, TokenKind::Eof);
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        let lex_diags = asi.into_diagnostics();
        assert!(lex_diags.is_empty(), "{lex_diags:?}");
        let (ast, parse_diags) = parse(source, tokens, crate::FileId(0));
        assert!(parse_diags.is_empty(), "{parse_diags:?}");
        let packages = runtime_packages(&[]);
        let (mut ta, infer_diags) = infer(source, "main", &ast, &packages);
        assert!(infer_diags.is_empty(), "{infer_diags:?}");
        let chk = check(&ta);
        assert!(chk.is_empty(), "{chk:?}");
        capture(&mut ta);
        desugar(&mut ta, crate::FileId(0));
        ta
    }

    fn box_named(ta: &mut TypedAst, names: &[&str]) {
        let func_meta: Vec<(usize, Vec<String>, StmtId)> = ta
            .functions
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let names = f.params.iter().map(|p| p.name.name.clone()).collect();
                (i, names, f.body)
            })
            .collect();
        for (fi, param_names, body) in func_meta {
            for (pi, pname) in param_names.iter().enumerate() {
                if names.contains(&pname.as_str()) {
                    ta.functions[fi].params[pi].boxed = true;
                }
            }
            box_walk_stmt(ta, body, names);
        }
        let stmt_ids: Vec<StmtId> = ta.top_level_statements.clone();
        for sid in stmt_ids {
            box_walk_stmt(ta, sid, names);
        }
    }

    fn box_walk_stmt(ta: &mut TypedAst, id: StmtId, names: &[&str]) {
        let kind = ta.stmt(id).kind.clone();
        match kind {
            TypedStmtKind::Block(stmts) => {
                for s in stmts {
                    box_walk_stmt(ta, s, names);
                }
            }
            TypedStmtKind::Let { name, value, .. } => {
                if names.iter().any(|n| *n == name.name)
                    && let TypedStmtKind::Let { boxed, .. } = &mut ta.stmt_mut(id).kind
                {
                    *boxed = true;
                }
                box_walk_expr(ta, value, names);
            }
            TypedStmtKind::Const { value, .. } => box_walk_expr(ta, value, names),
            TypedStmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                box_walk_expr(ta, condition, names);
                box_walk_stmt(ta, then_block, names);
                if let Some(eb) = else_block {
                    box_walk_stmt(ta, eb, names);
                }
            }
            TypedStmtKind::While { condition, body } => {
                box_walk_expr(ta, condition, names);
                box_walk_stmt(ta, body, names);
            }
            TypedStmtKind::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    box_walk_stmt(ta, i, names);
                }
                if let Some(c) = condition {
                    box_walk_expr(ta, c, names);
                }
                if let Some(u) = update {
                    box_walk_stmt(ta, u, names);
                }
                box_walk_stmt(ta, body, names);
            }
            TypedStmtKind::ForOf { iter, body, .. } => {
                box_walk_expr(ta, iter, names);
                box_walk_stmt(ta, body, names);
            }
            TypedStmtKind::DoWhile { body, condition } => {
                box_walk_stmt(ta, body, names);
                box_walk_expr(ta, condition, names);
            }
            TypedStmtKind::Switch {
                discriminant,
                cases,
                default,
                ..
            } => {
                box_walk_expr(ta, discriminant, names);
                for case in cases {
                    box_walk_stmt(ta, case.body, names);
                }
                if let Some(d) = default {
                    box_walk_stmt(ta, d, names);
                }
            }
            TypedStmtKind::Break | TypedStmtKind::Continue | TypedStmtKind::ReboxLocal { .. } => {}
            TypedStmtKind::Return(value) => {
                if let Some(v) = value {
                    box_walk_expr(ta, v, names);
                }
            }
            TypedStmtKind::Expr(e) => box_walk_expr(ta, e, names),
            TypedStmtKind::AssignLocal { ident, value, .. } => {
                if names.iter().any(|n| *n == ident.name)
                    && let TypedStmtKind::AssignLocal { boxed, .. } = &mut ta.stmt_mut(id).kind
                {
                    *boxed = true;
                }
                box_walk_expr(ta, value, names);
            }
            TypedStmtKind::AssignGlobal { value, .. } => {
                box_walk_expr(ta, value, names);
            }
            TypedStmtKind::AssignField {
                receiver, value, ..
            } => {
                box_walk_expr(ta, receiver, names);
                box_walk_expr(ta, value, names);
            }
            TypedStmtKind::AssignIndex {
                receiver,
                index,
                value,
                ..
            } => {
                box_walk_expr(ta, receiver, names);
                box_walk_expr(ta, index, names);
                box_walk_expr(ta, value, names);
            }
            TypedStmtKind::NarrowRegion { source, body, .. } => {
                box_walk_expr(ta, source, names);
                box_walk_stmt(ta, body, names);
            }
            TypedStmtKind::Throw { value } => box_walk_expr(ta, value, names),
            TypedStmtKind::Try {
                body,
                catches,
                finally,
            } => {
                box_walk_stmt(ta, body, names);
                for (i, c) in catches.iter().enumerate() {
                    if names.iter().any(|n| *n == c.binding.name)
                        && let TypedStmtKind::Try { catches: cs, .. } = &mut ta.stmt_mut(id).kind
                    {
                        cs[i].boxed = true;
                    }
                    box_walk_stmt(ta, c.body, names);
                }
                if let Some(f) = finally {
                    box_walk_stmt(ta, f, names);
                }
            }
        }
    }

    fn box_walk_expr(ta: &mut TypedAst, id: ExprId, names: &[&str]) {
        let kind = ta.expr(id).kind.clone();
        match kind {
            TypedExprKind::LocalRef { ident, .. } => {
                if names.iter().any(|n| *n == ident.name)
                    && let TypedExprKind::LocalRef { boxed, .. } = &mut ta.expr_mut(id).kind
                {
                    *boxed = true;
                }
            }
            TypedExprKind::LocalNarrowRef { .. } => {}
            TypedExprKind::Closure { params, body, .. } => {
                for (i, p) in params.iter().enumerate() {
                    if names.iter().any(|n| *n == p.name.name)
                        && let TypedExprKind::Closure { params, .. } = &mut ta.expr_mut(id).kind
                    {
                        params[i].boxed = true;
                    }
                }
                match body {
                    ClosureBody::Expr(e) => box_walk_expr(ta, e, names),
                    ClosureBody::Block(b) => box_walk_stmt(ta, b, names),
                }
            }
            TypedExprKind::Binary { lhs, rhs, .. } => {
                box_walk_expr(ta, lhs, names);
                box_walk_expr(ta, rhs, names);
            }
            TypedExprKind::Unary { operand, .. } => box_walk_expr(ta, operand, names),
            TypedExprKind::TypeofTag { value, .. } | TypedExprKind::InstanceOf { value, .. } => {
                box_walk_expr(ta, value, names);
            }
            TypedExprKind::Call { args, .. }
            | TypedExprKind::McpCall { args, .. }
            | TypedExprKind::SuperCtorCall { args, .. }
            | TypedExprKind::SuperMethodCall { args, .. } => {
                for a in args {
                    box_walk_expr(ta, a, names);
                }
            }
            TypedExprKind::CallClosure { callee, args } => {
                box_walk_expr(ta, callee, names);
                for a in args {
                    box_walk_expr(ta, a, names);
                }
            }
            TypedExprKind::GenericCall { args, .. } => {
                for a in args {
                    box_walk_expr(ta, a.expr, names);
                }
            }
            TypedExprKind::MethodCall { receiver, args, .. } => {
                box_walk_expr(ta, receiver, names);
                for a in args {
                    box_walk_expr(ta, a, names);
                }
            }
            TypedExprKind::GenericMethodCall { receiver, args, .. } => {
                box_walk_expr(ta, receiver, names);
                for a in args {
                    box_walk_expr(ta, a.expr, names);
                }
            }
            TypedExprKind::IntrinsicCall { args, .. } => {
                for a in args {
                    box_walk_expr(ta, a, names);
                }
            }
            TypedExprKind::ObjectLiteral { members, .. } => {
                for member in members {
                    for expression in member.expressions() {
                        box_walk_expr(ta, expression, names);
                    }
                }
            }
            TypedExprKind::ArrayLiteral { elements, .. } => {
                for e in elements {
                    box_walk_expr(ta, e.expr_id(), names);
                }
            }
            TypedExprKind::TupleLiteral { elements, .. } => {
                for e in elements {
                    box_walk_expr(ta, e, names);
                }
            }
            TypedExprKind::FieldAccess { receiver, .. }
            | TypedExprKind::InterfacePropertyAccess { receiver, .. } => {
                box_walk_expr(ta, receiver, names);
            }
            TypedExprKind::IndexAccess { receiver, index } => {
                box_walk_expr(ta, receiver, names);
                box_walk_expr(ta, index, names);
            }
            TypedExprKind::Narrowed { source, inner, .. } => {
                box_walk_expr(ta, source, names);
                box_walk_expr(ta, inner, names);
            }
            TypedExprKind::Ternary { cond, then_, else_ } => {
                box_walk_expr(ta, cond, names);
                box_walk_expr(ta, then_, names);
                box_walk_expr(ta, else_, names);
            }
            TypedExprKind::NullishCoalesce { lhs, rhs } => {
                box_walk_expr(ta, lhs, names);
                box_walk_expr(ta, rhs, names);
            }
            TypedExprKind::OptionalChain { base, parts } => {
                box_walk_expr(ta, base, names);
                let to_walk: Vec<ExprId> = parts
                    .iter()
                    .flat_map(|p| match p {
                        crate::TypedChainPart::Index { idx, .. } => vec![*idx],
                        crate::TypedChainPart::Call { args, .. }
                        | crate::TypedChainPart::MethodCall { args, .. } => args.clone(),
                        crate::TypedChainPart::Field { .. }
                        | crate::TypedChainPart::InterfaceProperty { .. }
                        | crate::TypedChainPart::NonNull { .. } => Vec::new(),
                    })
                    .collect();
                for id in to_walk {
                    box_walk_expr(ta, id, names);
                }
            }
            TypedExprKind::PostfixUnary { target, .. } => match target {
                crate::PostfixTarget::Local { ident, .. } => {
                    if names.iter().any(|n| *n == ident.name)
                        && let TypedExprKind::PostfixUnary {
                            target: crate::PostfixTarget::Local { boxed, .. },
                            ..
                        } = &mut ta.expr_mut(id).kind
                    {
                        *boxed = true;
                    }
                }
                crate::PostfixTarget::Global { .. } => {}
                crate::PostfixTarget::Field { receiver, .. } => {
                    box_walk_expr(ta, receiver, names);
                }
                crate::PostfixTarget::Index {
                    receiver, index, ..
                } => {
                    box_walk_expr(ta, receiver, names);
                    box_walk_expr(ta, index, names);
                }
            },
            TypedExprKind::NonNullAssert { value } | TypedExprKind::Cast { value, .. } => {
                box_walk_expr(ta, value, names);
            }
            TypedExprKind::EffectThen { effect, result } => {
                box_walk_expr(ta, effect, names);
                box_walk_expr(ta, result, names);
            }
            TypedExprKind::Sequence { stmts, result } => {
                for stmt in stmts {
                    box_walk_stmt(ta, stmt, names);
                }
                box_walk_expr(ta, result, names);
            }
            TypedExprKind::Number(_)
            | TypedExprKind::BigInt(_)
            | TypedExprKind::String(_)
            | TypedExprKind::Boolean(_)
            | TypedExprKind::Null
            | TypedExprKind::This
            | TypedExprKind::Regex { .. }
            | TypedExprKind::GlobalRef { .. }
            | TypedExprKind::FunctionRef { .. }
            | TypedExprKind::NumberEnumMember { .. }
            | TypedExprKind::StringEnumMember { .. } => {}
        }
    }

    fn compile_with_boxed(source: &str, names: &[&str]) -> Vec<u8> {
        let mut ta = boxed_typed_ast(source);
        box_named(&mut ta, names);

        let (prelude_defs, host_defs, internal_defs) =
            prelude::cached_runtime_package_declarations();
        let mut dependencies: Vec<&crate::PackageDeclaration> = prelude_defs.iter().collect();
        dependencies.extend(host_defs.iter());
        // `submilli:json` lives in the internal host packages; `main(): string`
        // now routes its JSON encoding through `submilli:json.stringify`.
        dependencies.extend(internal_defs.iter());
        codegen(source, "script.subm", crate::FileId(0), &ta, &dependencies)
            .expect("code generation")
    }

    fn run_main_f64_boxed(source: &str, names: &[&str]) -> f64 {
        run_main_number(&compile_with_boxed(source, names))
    }

    fn run_main_string_boxed(source: &str, names: &[&str]) -> String {
        let bytes = compile_with_boxed(source, names);
        let cfg = crate::runtime::RuntimeConfig::default();
        let result = pollster::block_on(cfg.run(&bytes)).expect("main does not trap");
        result.value.expect("main returns a string")
    }

    #[test]
    fn boxed_let_number_round_trip() {
        let source = "function main(): number { let x: number = 1; x = x + 1; return x; }";
        let result = run_main_f64_boxed(source, &["x"]);
        assert_eq!(result, 2.0);
    }

    #[test]
    fn boxed_let_string_round_trip() {
        let source =
            r#"function main(): string { let s: string = "hello"; s = "world"; return s; }"#;
        let result = run_main_string_boxed(source, &["s"]);
        assert_eq!(result, "world");
    }

    #[test]
    fn boxed_let_user_object_round_trip() {
        let source = "
            function main(): number {
                let p: { v: number } = { v: 42 };
                p = { v: 99 };
                return p.v;
            }
        ";
        let result = run_main_f64_boxed(source, &["p"]);
        assert_eq!(result, 99.0);
    }

    #[test]
    fn boxed_param_round_trip() {
        let source = "
            function helper(p: number): number { return p + 1; }
            function main(): number { return helper(5); }
        ";
        let result = run_main_f64_boxed(source, &["p"]);
        assert_eq!(result, 6.0);
    }

    #[test]
    fn boxed_param_with_assignment() {
        let source = "
            function helper(p: number): number { p = p + 10; return p; }
            function main(): number { return helper(7); }
        ";
        let result = run_main_f64_boxed(source, &["p"]);
        assert_eq!(result, 17.0);
    }

    #[test]
    fn mixed_boxed_and_unboxed_locals_in_one_function() {
        let source = "
            function main(): number {
                let b: number = 10;
                let u: number = 20;
                b = b + 1;
                u = u + 2;
                return b + u;
            }
        ";
        let result = run_main_f64_boxed(source, &["b"]);
        assert_eq!(result, 33.0);
    }

    #[test]
    fn multiple_distinct_box_types_coexist() {
        let source = r#"
            function main(): string {
                let n: number = 1;
                let s: string = "x";
                n = n + 41;
                s = "answer";
                return s;
            }
        "#;
        let result = run_main_string_boxed(source, &["n", "s"]);
        assert_eq!(result, "answer");
        let mut ta = boxed_typed_ast(source);
        box_named(&mut ta, &["n", "s"]);
        let collected = super::box_types::collect(&ta, &mock_symbols_with_intrinsics());
        assert_eq!(collected.len(), 2);
    }

    #[test]
    fn no_boxed_bindings_produces_no_box_types() {
        let ta = boxed_typed_ast("function main(): void { let x: number = 1; }");
        let collected = super::box_types::collect(&ta, &mock_symbols_with_intrinsics());
        assert!(collected.is_empty(), "{collected:?}");
    }

    #[test]
    fn record_box_type_is_idempotent_per_value_type() {
        use crate::Type;
        use wasm_encoder::ValType;
        let mut symbols = SymbolTable::default();
        symbols.record_box_type(ValType::F64, 42);
        assert_eq!(symbols.box_type_idx(&Type::Number), Some(42));
        symbols.record_box_type(ValType::F64, 42);
        assert_eq!(symbols.box_type_idx(&Type::Number), Some(42));
    }

    #[test]
    fn literal_typed_and_base_share_box_wrapper() {
        use crate::Type;
        use crate::types::LiteralF64;
        use wasm_encoder::ValType;
        let mut symbols = SymbolTable::default();
        symbols.record_box_type(ValType::F64, 7);
        assert_eq!(symbols.box_type_idx(&Type::Number), Some(7));
        assert_eq!(
            symbols.box_type_idx(&Type::NumberLiteral(LiteralF64(42.0))),
            Some(7),
        );
    }

    fn mock_symbols_with_intrinsics() -> SymbolTable {
        let mut map = SymbolTable::default();
        map.set_intrinsic_type_indices(super::intrinsics::IntrinsicTypeIndices {
            raw_string: 0,
            vtable: 1,
            object: 2,
            string: 3,
            boxed_number: 4,
            boxed_boolean: 5,
            field_names: 6,
            object_fields: 7,
            object_shape: 8,
            to_string_fn: 9,
            to_json_fn: 10,
            equals_fn: 11,
            hash_fn: 12,
            field_getter: 13,
            field_setter: 14,
            raw_array: 15,
            array: 16,
            raw_uint8_array: 17,
            uint8_array: 18,
            closure: 19,
            class_vtable: 20,
            error_vtable: 21,
            error: 22,
            raw_bigint: 23,
            bigint: 24,
            regex_capture_array: 25,
            regex_match: 26,
            regex: 27,
            regex_match_box: 28,
            temporal_instant: 29,
            temporal_duration: 30,
            temporal_zdt: 31,
            raw_index_array: 32,
            map: 33,
            set: 34,
            url: 35,
            fs_stat: 41,
            fs_peek: 42,
            fs_dir_entry: 43,
            fs_info: 44,
            fs_file_writer: 45,
            http_response: 46,
            http_download_result: 47,
            session_entry: 48,
            session_page: 49,

            temporal_plain_date: 36,
            temporal_plain_time: 37,
            temporal_plain_date_time: 38,
            temporal_plain_year_month: 39,
            temporal_plain_month_day: 40,
        });
        map
    }

    fn compile_with_closures(source: &str) -> Vec<u8> {
        compile(source)
    }

    #[test]
    fn closure_zero_captures_validates() {
        let source = "function main(): void { let f = (x: number) => x * 2; }";
        let bytes = compile_with_closures(source);
        instantiate_against_prelude(&bytes);
    }

    #[test]
    fn closure_const_capture_validates() {
        let source = "
            function main(): void {
                const y: number = 5;
                let f = (x: number) => x + y;
            }
        ";
        let bytes = compile_with_closures(source);
        instantiate_against_prelude(&bytes);
    }

    #[test]
    fn closure_let_capture_validates_with_box() {
        let source = "
            function main(): void {
                let y: number = 5;
                let f = (x: number) => x + y;
            }
        ";
        let bytes = compile_with_closures(source);
        instantiate_against_prelude(&bytes);
    }

    #[test]
    fn closure_param_capture_validates_with_box() {
        let source = "
            function host(p: number): void {
                let f = (x: number) => p + x;
            }
            function main(): void { host(7); }
        ";
        let bytes = compile_with_closures(source);
        instantiate_against_prelude(&bytes);
    }

    #[test]
    fn closure_block_body_with_assignment_validates() {
        let source = "
            function main(): void {
                let counter: number = 0;
                let inc = (): void => { counter = counter + 1; };
            }
        ";
        let bytes = compile_with_closures(source);
        instantiate_against_prelude(&bytes);
    }

    #[test]
    fn two_arrows_same_signature_share_closure_struct() {
        let source = "
            function main(): void {
                let f = (x: number) => x + 1;
                let g = (y: number) => y * 2;
            }
        ";
        let ta = boxed_typed_ast(source);
        let metas = super::analysis::CodegenAnalysis::collect(&ta, &[]).closure_metas;
        assert_eq!(metas.len(), 2);
        assert_eq!(metas[0].signature, metas[1].signature);
        let bytes = compile_with_closures(source);
        instantiate_against_prelude(&bytes);
    }

    #[test]
    fn nested_closure_validates() {
        let source = "
            function main(): void {
                let y: number = 1;
                let outer = (): number => {
                    let inner = (x: number) => x + y;
                    return 0;
                };
            }
        ";
        let bytes = compile_with_closures(source);
        instantiate_against_prelude(&bytes);
    }

    #[test]
    fn function_typed_binding_lowers_to_closure_struct_ref() {
        let source = "function main(): void { let f = (x: number) => x; }";
        let bytes = compile_with_closures(source);
        instantiate_against_prelude(&bytes);
    }

    #[test]
    fn no_closures_no_closure_types_emitted() {
        let ta = boxed_typed_ast("function main(): void { let x: number = 1; }");
        let metas = super::analysis::CodegenAnalysis::collect(&ta, &[]).closure_metas;
        assert!(metas.is_empty(), "{metas:?}");
    }

    #[test]
    fn inline_arrow_call_returns_value() {
        let result = run_main_f64("function main(): number { return ((x: number) => x + 1)(5); }");
        assert_eq!(result, 6.0);
    }

    #[test]
    fn let_stored_arrow_call_returns_value() {
        let result =
            run_main_f64("function main(): number { let f = (x: number) => x + 1; return f(5); }");
        assert_eq!(result, 6.0);
    }

    #[test]
    fn closure_call_with_multiple_args() {
        let result = run_main_f64(
            "function main(): number { let add = (a: number, b: number) => a + b; return add(2, 3); }",
        );
        assert_eq!(result, 5.0);
    }

    #[test]
    fn captured_let_counter_returns_three() {
        let result = run_main_f64(
            "function main(): number {
                let count: number = 0;
                let inc = (): number => { count = count + 1; return count; };
                inc();
                inc();
                return inc();
            }",
        );
        assert_eq!(result, 3.0);
    }

    #[test]
    fn higher_order_function_calls_callback() {
        let result = run_main_f64(
            "function apply(f: (x: number) => number, x: number): number { return f(x); }
             function main(): number { return apply((n: number) => n + 10, 5); }",
        );
        assert_eq!(result, 15.0);
    }

    #[test]
    fn function_as_value_let_bound_call_returns_underlying_result() {
        let result = run_main_f64(
            "function greet(x: number): number { return x + 1; }
             function main(): number { let f = greet; return f(2); }",
        );
        assert_eq!(result, 3.0);
    }

    #[test]
    fn function_as_value_passed_to_hof_invokes_underlying() {
        let result = run_main_f64(
            "function greet(x: number): number { return x + 1; }
             function apply(f: (n: number) => number, n: number): number { return f(n); }
             function main(): number { return apply(greet, 5); }",
        );
        assert_eq!(result, 6.0);
    }

    #[test]
    fn function_as_value_flows_through_multiple_bindings() {
        let result = run_main_f64(
            "function greet(x: number): number { return x + 10; }
             function main(): number { let f = greet; let g = f; return g(2); }",
        );
        assert_eq!(result, 12.0);
    }

    #[test]
    fn direct_call_path_unchanged_when_function_also_used_as_value() {
        let result = run_main_f64(
            "function greet(x: number): number { return x + 1; }
             function apply(f: (n: number) => number, n: number): number { return f(n); }
             function main(): number {
                 let direct = greet(10);
                 let indirect = apply(greet, 20);
                 return direct + indirect;
             }",
        );
        assert_eq!(result, 11.0 + 21.0);
    }

    #[test]
    fn no_function_as_value_no_adapters_emitted() {
        let ta = boxed_typed_ast(
            "function greet(x: number): number { return x + 1; }
             function main(): void { greet(5); }",
        );
        let metas = super::analysis::CodegenAnalysis::collect(&ta, &[]).adapter_metas;
        assert!(metas.is_empty(), "{metas:?}");
    }

    #[test]
    fn direct_callees_in_nested_positions_are_not_collected_as_adapters() {
        let ta = boxed_typed_ast(
            "function greet(x: number): number { return x + 1; }
             function nested(x: number): number { return x; }
             function inner(): number { return 1; }
             function outer(x: number): number { return x; }
             function main(): void {
                 greet(5);
                 nested(greet(5));
                 outer(inner());
             }",
        );
        let metas = super::analysis::CodegenAnalysis::collect(&ta, &[]).adapter_metas;
        assert!(
            metas.is_empty(),
            "direct-only callsites should produce no adapters, got {metas:?}",
        );
    }

    #[test]
    fn function_as_value_collects_one_adapter_per_distinct_function() {
        let ta = boxed_typed_ast(
            "function f1(x: number): number { return x + 1; }
             function f2(x: number): number { return x + 2; }
             function apply(g: (n: number) => number, n: number): number { return g(n); }
             function main(): void {
                 let a = f1;
                 let b = f1;
                 apply(f2, 0);
             }",
        );
        let metas = super::analysis::CodegenAnalysis::collect(&ta, &[]).adapter_metas;
        assert_eq!(metas.len(), 2, "{metas:?}");
        let names: Vec<&str> = metas.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"f1"));
        assert!(names.contains(&"f2"));
    }

    #[test]
    fn higher_order_function_with_capture() {
        let result = run_main_f64(
            "function apply(f: (x: number) => number, x: number): number { return f(x); }
             function main(): number {
                 let bias: number = 100;
                 return apply((n: number) => n + bias, 7);
             }",
        );
        assert_eq!(result, 107.0);
    }

    #[allow(dead_code)]
    fn _silence_unused() {
        let _ = (
            CodeSection::new(),
            ConstExpr::i32_const(0),
            ExportKind::Func,
            ExportSection::new(),
            FunctionSection::new(),
            GlobalSection::new(),
            ImportSection::new(),
            Module::new(),
            RefType {
                nullable: false,
                heap_type: HeapType::Concrete(0),
            },
            TypeSection::new(),
            ValType::F64,
            Function::new(vec![]),
            Ieee64::from(0.0_f64),
            Instruction::Nop,
            emit_statement,
        );
    }
}
