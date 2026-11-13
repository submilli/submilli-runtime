//! The source-to-Wasm pipeline: a [`FrontEnd`] followed by the engine's back
//! end (capture, desugar and codegen).
//!
//! Every entry point runs on its own compiler-sized thread
//! ([`crate::compiler_thread`]), so callers don't size stacks.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use crate::compile_capabilities;
use crate::compiler_error::{CompileError, CompilerFailure, CompilerStage};
use crate::front_end::{
    EngineFrontEnd, has_errors, runtime_declarations, script_declarations, script_request,
    script_rule_dependencies, warnings_only,
};
pub use crate::imports::ScriptImports;
use crate::stdlib::Stdlib;
use crate::typed_ast::TypedAst;
use crate::{
    Ast, CheckedPackage, CheckedScript, Diagnostic, FileId, FrontEnd, ModulePath,
    PackageDeclaration, PackageModule, PackageRequest, ParsedScript, ScriptOwner, ScriptRequest,
    Sources, capture, desugar, lower_patterns,
};
pub use submilli_front_end::front_end::{PackageSurface, PackageTestSurface};

/// A package module as callers hand it to the engine, before it has a
/// [`FileId`] in a [`Sources`].
#[derive(Clone, Debug)]
pub struct PackageSourceModule<'a> {
    pub path: ModulePath,
    pub source: &'a str,
}

/// Wall-clock spent in each compile phase, filled in as the pipeline runs. A
/// phase that never runs (e.g. `codegen` when the front end errors) stays zero.
/// Surfaced on [`CompiledScript`] for embedders that report compile-time
/// performance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PhaseTimings {
    /// Lexing, parsing and pattern lowering: [`FrontEnd::parse_script`]. Zero
    /// unless the caller that parsed records it.
    pub parse: Duration,
    /// [`ParsedScript::check`] (inference and the front end's rules) plus
    /// Submilli's rules.
    pub typecheck: Duration,
    pub capture: Duration,
    pub desugar: Duration,
    pub codegen: Duration,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompiledPackage {
    pub wasm: Vec<u8>,
    pub type_info: crate::TypeInfoTable,
    pub declaration: PackageDeclaration,
    pub required_capabilities: Vec<crate::DerivedCapability>,
    pub authority_map: crate::AuthorityMap,
    pub warnings: Vec<Diagnostic>,
    /// The non-entry modules the package's tests may import. `Some` only for
    /// [`PackageSurface::WithTestModules`].
    pub test_surface: Option<PackageTestSurface>,
}

#[derive(Clone, Debug)]
pub struct CompiledScript {
    pub wasm: Vec<u8>,
    pub type_info: crate::TypeInfoTable,
    pub warnings: Vec<Diagnostic>,
    pub timings: PhaseTimings,
}

/// Check `parsed` and compile it to Wasm. `filename` is the DWARF
/// compile-unit name for backtrace source mapping.
///
/// Warnings are non-fatal and come back on [`CompiledScript::warnings`]. Any
/// error is an `Err`.
pub fn compile_parsed_script(
    parsed: Box<dyn ParsedScript>,
    filename: &str,
    request: &ScriptRequest<'_>,
) -> Result<CompiledScript, CompileError> {
    let source = parsed.source().to_owned();
    let file = parsed.file();
    crate::compiler_thread::run(|| {
        let mut timings = PhaseTimings::default();
        let (ta, warnings) = check_and_lower(parsed, request, &mut timings)?;
        let codegen_start = Instant::now();
        let generated = codegen_script(&ta, &source, filename, file, request).map_err(|fatal| {
            CompileError {
                diagnostics: warnings.clone(),
                fatal: Some(fatal),
            }
        })?;
        timings.codegen = codegen_start.elapsed();
        Ok(CompiledScript {
            wasm: generated.wasm,
            type_info: generated.type_info,
            warnings,
            timings,
        })
    })
    .map_err(|error| cut_script_limit_span(error, &source, file))
}

/// Check `parsed` without generating code: what `submilli check` runs.
/// Returns its warnings. The lowering passes still run, because they deepen
/// the tree, so a check enforces compilation's limits.
pub fn check_parsed_script(
    parsed: Box<dyn ParsedScript>,
    request: &ScriptRequest<'_>,
) -> Result<Vec<Diagnostic>, CompileError> {
    let source = parsed.source().to_owned();
    let file = parsed.file();
    crate::compiler_thread::run(|| {
        let (_, warnings) = check_and_lower(parsed, request, &mut PhaseTimings::default())?;
        Ok(warnings)
    })
    .map_err(|error| cut_script_limit_span(error, &source, file))
}

/// Check `parsed`, then capture and desugar the tree, timing each phase.
/// Returns the lowered tree and the warnings.
fn check_and_lower(
    parsed: Box<dyn ParsedScript>,
    request: &ScriptRequest<'_>,
    timings: &mut PhaseTimings,
) -> Result<(TypedAst, Vec<Diagnostic>), CompileError> {
    let file = parsed.file();
    let typecheck_start = Instant::now();
    let CheckedScript {
        typed_ast,
        mut diagnostics,
    } = parsed.check(request)?;
    // Submilli's rules are the engine's, whichever front end built the tree.
    diagnostics.extend(
        crate::typechecker::rules::check_script_submilli(
            &typed_ast,
            &script_rule_dependencies(request),
        )
        .map_err(|error| error.with_prior_diagnostics(&diagnostics))?,
    );
    timings.typecheck = typecheck_start.elapsed();
    if has_errors(&diagnostics) {
        return Err(diagnostics.into());
    }
    let warnings = diagnostics;
    let with_warnings = |fatal| CompileError {
        diagnostics: warnings.clone(),
        fatal: Some(fatal),
    };

    let capture_start = Instant::now();
    let ta = capture(typed_ast).map_err(with_warnings)?;
    timings.capture = capture_start.elapsed();

    let desugar_start = Instant::now();
    let ta = desugar(ta, file).map_err(with_warnings)?;
    timings.desugar = desugar_start.elapsed();
    Ok((ta, warnings))
}

/// Generate a script's Wasm. A test file links against the package build
/// with its internal exports.
fn codegen_script(
    ta: &TypedAst,
    source: &str,
    filename: &str,
    file: FileId,
    request: &ScriptRequest<'_>,
) -> Result<crate::codegen::GeneratedModule, CompilerFailure> {
    let codegen_owner = request
        .owner
        .test()
        .map(|test| test_codegen_owner(test.owner, test.surface));
    let runtime = runtime_declarations();
    let mut dependencies: Vec<&PackageDeclaration> = request
        .prelude
        .iter()
        .chain(request.host)
        .chain(runtime.internal)
        .chain(request.stdlib)
        .copied()
        .collect();
    dependencies.extend(script_declarations(request).into_iter().map(|declaration| {
        match &codegen_owner {
            Some(owner) if owner.package_name == declaration.package_name => owner,
            _ => declaration,
        }
    }));
    dependencies.extend_from_slice(request.transitive);
    match request.owner.package() {
        Some(owner) => {
            crate::codegen::codegen_owned_by(owner, source, filename, file, ta, &dependencies)
        }
        None => crate::codegen::codegen_with_type_info(source, filename, file, ta, &dependencies),
    }
}

fn cut_script_limit_span(error: CompileError, source: &str, file: FileId) -> CompileError {
    error.with_limit_span_cut(|span_file| (span_file == file).then_some(source))
}

/// `owner` plus the internal values and classes in `surface`, keyed by their
/// mangled names, which no source identifier can spell. A class joins `types`
/// so its statics are imported as a public class's are; its layout stays in
/// `runtime_types`. It replaces the owner in codegen only, which must import
/// what the typechecker bound to the package's modules; the typechecker keeps
/// the public declaration, so `from "<owner>"` still reaches only the entry
/// module.
fn test_codegen_owner(
    owner: &PackageDeclaration,
    surface: &PackageTestSurface,
) -> PackageDeclaration {
    let mut declaration = owner.clone();
    let public_values: BTreeSet<&crate::MangledName> = owner
        .values
        .values()
        .map(|value| &value.mangled_name)
        .collect();
    let public_types: BTreeSet<&crate::MangledName> = owner
        .types
        .values()
        .map(|symbol| &symbol.mangled_name)
        .collect();
    for exports in surface.modules.values() {
        for value in exports.values.values() {
            if !public_values.contains(&value.mangled_name) {
                declaration
                    .values
                    .entry(value.mangled_name.to_string())
                    .or_insert_with(|| value.clone());
            }
        }
        for symbol in exports.types.values() {
            if matches!(symbol.kind, crate::TypeKind::Class { .. })
                && !public_types.contains(&symbol.mangled_name)
            {
                declaration
                    .types
                    .entry(symbol.mangled_name.to_string())
                    .or_insert_with(|| symbol.clone());
            }
        }
    }
    declaration.refresh_shapes();
    declaration
}

/// Register `modules` in a new [`Sources`], in order, for a [`PackageRequest`].
pub fn package_sources(modules: &[PackageSourceModule<'_>]) -> Result<Sources, CompileError> {
    let mut sources = Sources::new();
    for module in modules {
        sources
            .add(module.path.clone(), module.source)
            .map_err(|error| {
                CompileError::from(error.into_compiler_failure(CompilerStage::Parse))
            })?;
    }
    Ok(sources)
}

/// Every file of `sources` as a package module, in order.
pub fn package_modules(sources: &Sources) -> Vec<PackageModule<'_>> {
    sources
        .iter()
        .map(|(file, source)| PackageModule {
            path: &source.path,
            file,
        })
        .collect()
}

/// Check and compile the package `request` describes with `front_end`.
pub fn compile_package_request(
    front_end: &dyn FrontEnd,
    request: &PackageRequest<'_>,
) -> Result<CompiledPackage, CompileError> {
    crate::compiler_thread::run(|| compile_checked_package(front_end, request))
        .map(|(compiled, _)| compiled)
        .map_err(|error| cut_package_limit_span(error, request.sources))
}

fn cut_package_limit_span(error: CompileError, sources: &Sources) -> CompileError {
    error.with_limit_span_cut(|file| sources.get(file).map(crate::SourceFile::text))
}

/// Compile a single-module `@functions/<name>` source with `submilli:functions`
/// available and return both its compiled package artifact and its host-invoke
/// plan.
pub(crate) fn compile_function_module(
    stdlib: Stdlib,
    package_name: &str,
    root_module: ModulePath,
    source: &str,
    dependencies: &[&PackageDeclaration],
    transitive: &[&PackageDeclaration],
) -> Result<(CompiledPackage, Option<crate::FunctionInvokePlan>), CompileError> {
    let mut stdlib_defs = stdlib.package_declarations();
    stdlib_defs.push(crate::stdlib::functions::package_declaration());
    stdlib_defs.sort_by(|a, b| a.package_name.cmp(&b.package_name));
    let stdlib_refs: Vec<&PackageDeclaration> = stdlib_defs.iter().collect();
    let sources = package_sources(&[PackageSourceModule {
        path: root_module.clone(),
        source,
    }])?;
    let modules = package_modules(&sources);
    let runtime = runtime_declarations();
    let request = PackageRequest {
        name: package_name,
        root_module: &root_module,
        modules: &modules,
        sources: &sources,
        prelude: runtime.prelude,
        host: runtime.host,
        stdlib: &stdlib_refs,
        dependencies,
        transitive,
        config: None,
        surface: PackageSurface::Public,
    };
    crate::compiler_thread::run(|| compile_checked_package(&EngineFrontEnd, &request))
        .map_err(|error| cut_package_limit_span(error, &sources))
}

fn compile_checked_package(
    front_end: &dyn FrontEnd,
    request: &PackageRequest<'_>,
) -> Result<(CompiledPackage, Option<crate::FunctionInvokePlan>), CompileError> {
    let CheckedPackage {
        typed_ast: mut ta,
        mut declaration,
        mut diagnostics,
    } = front_end.check_package(request)?;
    // Submilli's rules are the engine's, whichever front end built the tree.
    diagnostics.extend(
        crate::typechecker::rules::check_package_submilli(
            &ta,
            &declaration,
            &crate::front_end::PackageDeclarations::of(request).rule_dependencies(),
        )
        .map_err(|error| error.with_prior_diagnostics(&diagnostics))?,
    );
    if has_errors(&diagnostics) {
        return Err(diagnostics.into());
    }
    let internal_modules = std::mem::take(&mut ta.internal_modules);
    let test_surface = match request.surface {
        PackageSurface::Public => None,
        PackageSurface::WithTestModules => Some(own_test_surface(&ta, internal_modules)),
    };
    let (required_capabilities, capability_warnings) =
        compile_capabilities::derive_package_requirements(
            &declaration,
            &ta,
            request.stdlib,
            request.dependencies,
            request.transitive,
        )
        .map_err(|error| error.with_prior_diagnostics(&diagnostics))?;
    diagnostics.extend(capability_warnings);
    let with_diagnostics = |fatal| CompileError {
        diagnostics: diagnostics.clone(),
        fatal: Some(fatal),
    };
    let authority_map = crate::authority::analyse(
        &declaration,
        &ta,
        request.sources,
        request
            .stdlib
            .iter()
            .chain(request.prelude)
            .chain(request.host)
            .chain(request.dependencies)
            .chain(request.transitive)
            .copied(),
    )
    .map_err(with_diagnostics)?;
    ta = capture(ta).map_err(with_diagnostics)?;
    let root_file = request
        .modules
        .iter()
        .find(|module| module.path == request.root_module)
        .map(|module| module.file)
        .ok_or_else(|| CompilerFailure::Internal {
            stage: CompilerStage::Infer,
            span: None,
            message: "compiled package root module is missing".into(),
        })?;
    ta = desugar(ta, root_file).map_err(with_diagnostics)?;
    if let Some(surface) = &test_surface {
        ta.test_exports = test_exports(surface, &ta.exports);
    }

    let runtime = runtime_declarations();
    let codegen_deps: Vec<&PackageDeclaration> = request
        .prelude
        .iter()
        .chain(request.host)
        .chain(runtime.internal)
        .chain(request.stdlib)
        .chain(request.dependencies)
        .chain(request.transitive)
        .copied()
        .collect();
    let generated = crate::codegen::codegen_package_with_type_info(
        request.sources,
        root_file,
        &ta,
        &codegen_deps,
    )
    .map_err(with_diagnostics)?;
    declaration.runtime_functions = generated.runtime_functions;
    declaration.runtime_globals = generated.runtime_globals;
    declaration.closure_caches = generated.closure_caches;
    Ok((
        CompiledPackage {
            wasm: generated.wasm,
            type_info: generated.type_info,
            declaration,
            required_capabilities,
            authority_map,
            warnings: warnings_only(diagnostics),
            test_surface,
        },
        ta.function_invoke,
    ))
}

/// The modules' exports minus values and classes another package declares,
/// which a module can re-export but this package's Wasm cannot export.
fn own_test_surface(
    ta: &TypedAst,
    modules: BTreeMap<ModulePath, crate::InternalModuleExports>,
) -> PackageTestSurface {
    let own_values: BTreeSet<&crate::MangledName> = ta
        .functions
        .iter()
        .map(|function| &function.mangled_name)
        .chain(ta.globals.iter().map(|global| &global.mangled_name))
        .collect();
    let own_classes: BTreeSet<&crate::MangledName> = ta
        .types
        .iter()
        .filter_map(|declaration| match declaration {
            crate::TypedTypeDecl::Class(class) => Some(&class.mangled_name),
            _ => None,
        })
        .collect();
    let modules = modules
        .into_iter()
        .map(|(path, mut exports)| {
            exports
                .values
                .retain(|_, value| own_values.contains(&value.mangled_name));
            exports.types.retain(|_, symbol| {
                !matches!(symbol.kind, crate::TypeKind::Class { .. })
                    || own_classes.contains(&symbol.mangled_name)
            });
            (path, exports)
        })
        .collect();
    PackageTestSurface { modules }
}

/// The Wasm exports that let a test call into `surface`: each value under its
/// mangled name, and the public statics of each class, each once and none
/// already among the package's `public` exports. Two modules can export one
/// declaration, and a re-exported one is public under the same mangled name.
fn test_exports(
    surface: &PackageTestSurface,
    public: &[crate::ExportEntry],
) -> Vec<crate::ExportEntry> {
    let mut seen: BTreeSet<crate::MangledName> = public
        .iter()
        .map(|entry| entry.public_name.clone())
        .collect();
    let mut exports = Vec::new();
    for module in surface.modules.values() {
        for value in module.values.values() {
            exports.push(crate::ExportEntry {
                public_name: value.mangled_name.clone(),
                target: value.mangled_name.clone(),
                kind: match value.kind {
                    crate::ValueKind::Function { .. } => crate::ExportKind::Function,
                    crate::ValueKind::Let { .. } | crate::ValueKind::Const { .. } => {
                        crate::ExportKind::Global
                    }
                },
                span: value.declaration_span,
            });
        }
        for symbol in module.types.values() {
            exports.extend(crate::typechecker::infer::static_export_entries(
                &symbol.kind,
                &symbol.mangled_name,
                symbol.declaration_span,
            ));
        }
    }
    exports.retain(|entry| seen.insert(entry.public_name.clone()));
    exports
}

// ---------------------------------------------------------------------------
// Conveniences: the engine front end with the core stdlib and the runtime's
// declarations. They back tests and examples; embedders build a request.

/// Check a script with the core stdlib and nothing to import. Returns its
/// warnings.
pub fn typecheck_checked(source: &str, file: FileId) -> Result<Vec<Diagnostic>, CompileError> {
    let stdlib = crate::runtime::stdlib_package_declarations();
    let stdlib_refs: Vec<&PackageDeclaration> = stdlib.iter().collect();
    let parsed = EngineFrontEnd.parse_script(source.to_owned(), file);
    check_parsed_script(parsed, &script_request(&stdlib_refs))
}

/// Typecheck and return the typed AST together with every diagnostic, errors
/// included. For tools that inspect what the typechecker inferred — the
/// TypeScript baseline comparison in `crates/conformance` — rather than compile.
/// The typed AST is only meaningful where the diagnostics are error-free.
pub fn typecheck_to_typed_ast(source: &str, file: FileId) -> (TypedAst, Vec<Diagnostic>) {
    let stdlib = crate::runtime::stdlib_package_declarations();
    let stdlib_refs: Vec<&PackageDeclaration> = stdlib.iter().collect();
    let request = script_request(&stdlib_refs);
    let result = crate::compiler_thread::run(|| {
        crate::front_end::parse(source.to_owned(), file).infer_with_all_rules(&request)
    });
    match result {
        Ok(result) => result,
        Err(error) => (
            TypedAst::new(),
            error
                .with_limit_span_cut(|span_file| (span_file == file).then_some(source))
                .into_diagnostics(file),
        ),
    }
}

/// [`typecheck_to_typed_ast`] for the modules of one package, which import each
/// other by relative path. Every module is checked, imported or not, and inference
/// runs past syntax errors, as for a script. The typed AST holds every module's
/// declarations; each span carries the [`FileId`] of its module, which is its index
/// in `modules`.
pub fn typecheck_package_to_typed_ast(
    package_name: &str,
    root_module: ModulePath,
    modules: &[PackageSourceModule<'_>],
) -> (TypedAst, Vec<Diagnostic>) {
    let mut sources = Sources::new();
    let result = crate::compiler_thread::run(|| {
        let mut diagnostics = Vec::new();
        let Some(parsed_modules) =
            parse_modules_past_errors(&mut sources, modules, &mut diagnostics)
        else {
            return Ok((TypedAst::new(), diagnostics));
        };
        let module_refs = parsed_modules
            .iter()
            .map(|(path, file, ast)| (path.clone(), *file, ast))
            .collect();
        let stdlib = crate::runtime::stdlib_package_declarations();
        let stdlib_refs: Vec<&PackageDeclaration> = stdlib.iter().collect();
        let runtime = runtime_declarations();
        match crate::typechecker::infer::infer_package_checked(
            package_name,
            root_module,
            module_refs,
            &sources,
            crate::front_end::external_packages(runtime.prelude, runtime.host, &stdlib_refs),
            BTreeMap::new(),
        ) {
            Ok((ta, _, package_diagnostics)) => {
                diagnostics.extend(package_diagnostics);
                Ok((ta, diagnostics))
            }
            Err(error) => {
                let error = cut_package_limit_span(error, &sources);
                diagnostics.extend(error.into_diagnostics(FileId(0)));
                Ok((TypedAst::new(), diagnostics))
            }
        }
    });
    result.unwrap_or_else(|error| (TypedAst::new(), error.into_diagnostics(FileId(0))))
}

/// Lower a parsed module that has syntax errors: `parse_package_module` leaves
/// such a module unlowered, but inference past syntax errors assumes lowering.
fn lower_recovered_module(
    (ast, diagnostics): (Ast, Vec<Diagnostic>),
) -> Result<(Ast, Vec<Diagnostic>), CompileError> {
    if ast.lowered_local_types.is_some() {
        return Ok((ast, diagnostics));
    }
    match lower_patterns(ast) {
        Ok(ast) => Ok((ast, diagnostics)),
        Err(fatal) => Err(CompileError {
            diagnostics,
            fatal: Some(fatal),
        }),
    }
}

/// Registers, parses and lowers each module, adding its diagnostics, errors
/// included, to `diagnostics`. `None` when one fails fatally, so nothing can be
/// checked.
fn parse_modules_past_errors(
    sources: &mut Sources,
    modules: &[PackageSourceModule<'_>],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Vec<(ModulePath, FileId, Ast)>> {
    let mut parsed_modules = Vec::with_capacity(modules.len());
    for module in modules {
        let file = match sources.add(module.path.clone(), module.source) {
            Ok(file) => file,
            Err(error) => {
                let error = CompileError::from(error.into_compiler_failure(CompilerStage::Parse));
                diagnostics.extend(error.into_diagnostics(FileId(0)));
                return None;
            }
        };
        // A config module has no blocks to lower.
        let (ast, mut module_diagnostics) = match if module.path.as_str() == "config" {
            crate::front_end::parse_config_module(module.source, file, &BTreeMap::new())
        } else {
            crate::front_end::parse_package_module(module.source, file)
                .and_then(lower_recovered_module)
        } {
            Ok(parsed) => parsed,
            Err(error) => {
                diagnostics.extend(error.into_diagnostics(file));
                return None;
            }
        };
        diagnostics.append(&mut module_diagnostics);
        parsed_modules.push((module.path.clone(), file, ast));
    }
    Some(parsed_modules)
}

/// Compile a script module with the core stdlib. `filename` is the DWARF
/// compile-unit name for backtrace source mapping; `file` is this source's
/// [`FileId`] in the caller's [`Sources`](crate::Sources) registry. `packages`
/// are real package declarations that emit Wasm imports; `mcps` are
/// declaration-only virtual MCP packages.
pub fn compile_script_checked(
    source: &str,
    filename: &str,
    file: FileId,
    packages: &[&PackageDeclaration],
    mcps: &[&PackageDeclaration],
) -> Result<CompiledScript, CompileError> {
    compile_script_for(ScriptOwner::Main, source, filename, file, packages, mcps)
}

/// Compiles a script whose code belongs to `owning_package` rather than to `main`.
pub fn compile_script_owned_by_checked(
    owning_package: &str,
    source: &str,
    filename: &str,
    file: FileId,
    packages: &[&PackageDeclaration],
    mcps: &[&PackageDeclaration],
) -> Result<CompiledScript, CompileError> {
    compile_script_for(
        ScriptOwner::Package(owning_package),
        source,
        filename,
        file,
        packages,
        mcps,
    )
}

fn compile_script_for(
    owner: ScriptOwner<'_>,
    source: &str,
    filename: &str,
    file: FileId,
    packages: &[&PackageDeclaration],
    mcps: &[&PackageDeclaration],
) -> Result<CompiledScript, CompileError> {
    let stdlib = crate::runtime::stdlib_package_declarations();
    let stdlib_refs: Vec<&PackageDeclaration> = stdlib.iter().collect();
    let parse_start = Instant::now();
    let parsed = EngineFrontEnd.parse_script(source.to_owned(), file);
    let parse = parse_start.elapsed();
    let request = ScriptRequest {
        owner,
        dependencies: packages,
        mcp: mcps,
        ..script_request(&stdlib_refs)
    };
    let mut compiled = compile_parsed_script(parsed, filename, &request)?;
    compiled.timings.parse = parse;
    Ok(compiled)
}

pub fn compile_package_checked(
    package_name: &str,
    root_module: ModulePath,
    modules: &[PackageSourceModule<'_>],
    dependencies: &[&PackageDeclaration],
) -> Result<CompiledPackage, CompileError> {
    compile_package_with_transitive_checked(package_name, root_module, modules, dependencies, &[])
}

/// [`compile_package`] with the rest of the dependency closure supplied
/// separately.
///
/// `dependencies` is what the package declares, and only those are importable.
/// `transitive` is everything they in turn depend on: a direct dependency's
/// public surface can name a class from one of them — as a parent, a return
/// type, a static's type — and resolving that needs the declaration even though
/// this package must not `import` from it. Without it the ancestor walk
/// truncates silently and codegen reconstructs a layout that doesn't
/// canonicalize against the producer's, which surfaces at link time as
/// `imported global type mismatch`.
pub fn compile_package_with_transitive_checked(
    package_name: &str,
    root_module: ModulePath,
    modules: &[PackageSourceModule<'_>],
    dependencies: &[&PackageDeclaration],
    transitive: &[&PackageDeclaration],
) -> Result<CompiledPackage, CompileError> {
    let stdlib = Stdlib::core().package_declarations();
    let stdlib_refs: Vec<&PackageDeclaration> = stdlib.iter().collect();
    let sources = package_sources(modules)?;
    let package_modules = package_modules(&sources);
    let runtime = runtime_declarations();
    let no_config = BTreeMap::new();
    compile_package_request(
        &EngineFrontEnd,
        &PackageRequest {
            name: package_name,
            root_module: &root_module,
            modules: &package_modules,
            sources: &sources,
            prelude: runtime.prelude,
            host: runtime.host,
            stdlib: &stdlib_refs,
            dependencies,
            transitive,
            config: Some(&no_config),
            surface: PackageSurface::Public,
        },
    )
}

/// Compatibility adapter returning source and fatal failures as diagnostics.
pub fn typecheck(source: &str, file: FileId) -> Result<Vec<Diagnostic>, Vec<Diagnostic>> {
    typecheck_checked(source, file).map_err(|error| error.into_diagnostics(file))
}

/// Compatibility adapter returning source and fatal failures as diagnostics.
pub fn compile_script(
    source: &str,
    filename: &str,
    file: FileId,
    packages: &[&PackageDeclaration],
    mcps: &[&PackageDeclaration],
) -> Result<CompiledScript, Vec<Diagnostic>> {
    compile_script_checked(source, filename, file, packages, mcps)
        .map_err(|error| error.into_diagnostics(file))
}

/// Compatibility adapter returning source and fatal failures as diagnostics.
pub fn compile_script_owned_by(
    owning_package: &str,
    source: &str,
    filename: &str,
    file: FileId,
    packages: &[&PackageDeclaration],
    mcps: &[&PackageDeclaration],
) -> Result<CompiledScript, Vec<Diagnostic>> {
    compile_script_owned_by_checked(owning_package, source, filename, file, packages, mcps)
        .map_err(|error| error.into_diagnostics(file))
}

/// Compatibility adapter returning source and fatal failures as diagnostics.
pub fn compile_package(
    package_name: &str,
    root_module: ModulePath,
    modules: &[PackageSourceModule<'_>],
    dependencies: &[&PackageDeclaration],
) -> Result<CompiledPackage, Vec<Diagnostic>> {
    compile_package_checked(package_name, root_module, modules, dependencies)
        .map_err(|error| error.into_diagnostics(FileId(0)))
}

/// Compatibility adapter returning source and fatal failures as diagnostics.
pub fn compile_package_with_transitive(
    package_name: &str,
    root_module: ModulePath,
    modules: &[PackageSourceModule<'_>],
    dependencies: &[&PackageDeclaration],
    transitive: &[&PackageDeclaration],
) -> Result<CompiledPackage, Vec<Diagnostic>> {
    compile_package_with_transitive_checked(
        package_name,
        root_module,
        modules,
        dependencies,
        transitive,
    )
    .map_err(|error| error.into_diagnostics(FileId(0)))
}

/// The engine's parse of `source`, for tests that look inside it.
#[cfg(test)]
pub(crate) fn parse_script(source: &str, file: FileId) -> crate::front_end::EngineParsedScript {
    crate::front_end::parse(source.to_owned(), file)
}

/// Infer and check `parsed` against the runtime, `stdlib_defs` and
/// `packages`, keeping every diagnostic, errors included.
#[cfg(test)]
fn front_end(
    parsed: &crate::front_end::EngineParsedScript,
    stdlib_defs: &[PackageDeclaration],
    packages: &[&PackageDeclaration],
) -> Result<(TypedAst, Vec<Diagnostic>), CompileError> {
    let stdlib_refs: Vec<&PackageDeclaration> = stdlib_defs.iter().collect();
    let request = ScriptRequest {
        dependencies: packages,
        ..script_request(&stdlib_refs)
    };
    parsed.infer_with_all_rules(&request)
}

#[cfg(test)]
mod parsed_script_tests {
    use super::*;

    #[test]
    fn a_package_build_reports_every_module_syntax_error() {
        crate::compiler_thread::on_test_stack(|| {
            // A module with syntax errors isn't lowered for a build: a recovered
            // parse can leave nodes lowering would trip on, and the build
            // stops before inference anyway.
            let modules = [
                PackageSourceModule {
                    path: ModulePath::from("lib"),
                    source: "export function f(): void {\n  \
                             const x = g<number>(({ a }: { a: number }) => a;\n}\n",
                },
                PackageSourceModule {
                    path: ModulePath::from("other"),
                    source: "export const y = ;\n",
                },
            ];
            let error = compile_package_checked("test", ModulePath::from("lib"), &modules, &[])
                .expect_err("syntax errors");
            assert!(error.fatal.is_none(), "{error:?}");
            let files: std::collections::BTreeSet<u32> =
                error.diagnostics.iter().map(|d| d.span.file.0).collect();
            assert_eq!(files.len(), 2, "{:?}", error.diagnostics);
        });
    }

    #[test]
    fn package_inference_past_syntax_errors_sees_lowered_modules() {
        crate::compiler_thread::on_test_stack(|| {
            // Inference runs past `lib`'s syntax error, so `lib` is lowered
            // anyway: its local interface is lifted and its destructured
            // parameter lowered, and the other module's type error is still
            // reported.
            let modules = [
                PackageSourceModule {
                    path: ModulePath::from("lib"),
                    source: "export function f([n]: number[]): number {\n  \
                             interface Row { n: number }\n  const r: Row = { n };\n  \
                             return r.n;\n}\nconst broken = ;\n",
                },
                PackageSourceModule {
                    path: ModulePath::from("other"),
                    source: "const text: string = 1;\n",
                },
            ];
            let (_, diagnostics) =
                typecheck_package_to_typed_ast("test", ModulePath::from("lib"), &modules);
            let messages: Vec<&str> = diagnostics.iter().map(|d| d.message.as_str()).collect();
            assert!(
                messages.iter().any(|m| m.contains("expected expression")),
                "{messages:?}"
            );
            assert!(
                messages.iter().any(|m| m.contains("expected `string`")),
                "{messages:?}"
            );
            // An unlowered module fails inference as an internal compiler
            // failure.
            assert!(
                !messages
                    .iter()
                    .any(|m| m.contains("internal compiler failure")),
                "{messages:?}"
            );
        });
    }

    #[test]
    fn typed_compile_errors_distinguish_source_limits_and_internal_failures() {
        let source_error = compile_script_checked(
            "function main(): number { return false; }",
            "test.ts",
            FileId(0),
            &[],
            &[],
        )
        .expect_err("source type mismatch");
        assert!(source_error.fatal.is_none());
        assert!(!source_error.diagnostics.is_empty());

        let nested = format!(
            "function main(): number {{ return {}1{}; }}",
            "(".repeat(2048),
            ")".repeat(2048)
        );
        let limit = compile_script_checked(&nested, "test.ts", FileId(0), &[], &[])
            .expect_err("parser limit");
        assert!(matches!(
            limit.fatal,
            Some(CompilerFailure::Limit {
                stage: CompilerStage::Parse,
                ..
            })
        ));
        let rendered = limit.into_diagnostics(FileId(0));
        assert_eq!(
            rendered
                .iter()
                .filter(|d| d.message.contains("parser recursion limit exceeded"))
                .count(),
            1
        );
    }

    #[test]
    fn external_imports_come_from_parsed_imports_only() {
        let parsed = parse_script(
            r#"
                // import fake from "@mcp/commented";
                const s: string = "import x from \"@pied-piper/string\"";
                import { v4 } from "submilli:uuid";
                import linear from "@mcp/linear";
                import { answer } from "@pied-piper/util";
                function main(): string { return v4(); }
            "#,
            FileId(0),
        );

        let imports = parsed.external_imports().unwrap();

        assert_eq!(
            imports.stdlib,
            BTreeSet::from(["submilli:uuid".to_string()])
        );
        assert_eq!(imports.mcp_servers, BTreeSet::from(["linear".to_string()]));
        assert_eq!(
            imports.registry_packages,
            BTreeSet::from(["@pied-piper/util".to_string()])
        );
    }

    #[test]
    fn relative_imports_are_not_external_imports() {
        let parsed = parse_script(
            r#"
                import { local } from "./util";
                function main(): number { return 1; }
            "#,
            FileId(0),
        );

        assert_eq!(parsed.external_imports().unwrap(), ScriptImports::default());
    }
}

#[cfg(test)]
mod importless_library_tests {
    //! SUB-386: a value whose type comes from a library must support member
    //! access and use without the consumer separately importing the type name.
    //! The http tests cover this against the real stdlib; these exercise the
    //! mechanism for an arbitrary package declaration — proving the FQN registry is
    //! populated from `packages_by_name`, not just the prelude — and for the
    //! other by-name return shapes (an interface *and* an enum).

    use std::collections::BTreeMap;

    use crate::mangle::package_symbol;
    use crate::package_declaration::{Dispatch, MethodSig, PropertySig, TypeKind, ValueKind};
    use crate::{ObjectField, Package, PackageDeclaration, Span, Type, TypeSymbol, ValueSymbol};

    const SYNTH: &str = "submilli:synth";
    const PIED_PIPER: &str = "@pied-piper/util";

    fn synth_package() -> PackageDeclaration {
        let mut defs = PackageDeclaration::with_package(SYNTH);

        let mut properties = BTreeMap::new();
        properties.insert(
            "label".to_string(),
            PropertySig {
                ty: Type::String,
                readonly: true,
                intrinsic: false,
                optional: false,
                doc: None,
            },
        );
        let mut methods = BTreeMap::new();
        methods.insert(
            "size".to_string(),
            MethodSig {
                overloads: Vec::new(),
                optional: false,
                generics: Default::default(),
                params: Vec::new(),
                ret: Type::Number,
                predicate: None,
                doc: None,
            },
        );
        defs.types.insert(
            "Widget".to_string(),
            TypeSymbol {
                name: "Widget".to_string(),
                mangled_name: package_symbol(SYNTH, "Widget"),
                declaration_span: Span::at(crate::FileId(0)),
                kind: TypeKind::Interface {
                    extends: Vec::new(),
                    more_indexes: Vec::new(),
                    index: None,
                    generics: Default::default(),
                    methods,
                    properties,
                    dispatch: Dispatch::Direct,
                    doc: None,
                },
            },
        );

        defs.types.insert(
            "Color".to_string(),
            TypeSymbol {
                name: "Color".to_string(),
                mangled_name: package_symbol(SYNTH, "Color"),
                declaration_span: Span::at(crate::FileId(0)),
                kind: TypeKind::NumberEnum {
                    variants: vec![("Red".to_string(), 0.0), ("Green".to_string(), 1.0)],
                    doc: None,
                },
            },
        );

        // A recursive alias `type Tree = { value: number; children: Tree[] }`,
        // built the way a library shim would: an inline body whose recursive
        // position is a [`Type::AliasRef`] back-edge carrying its own package.
        let tree_ref = || Type::AliasRef {
            mangled: crate::mangle::package_symbol(SYNTH, "Tree"),
            package: Package(SYNTH.to_string()),
            name: "Tree".to_string(),
            args: Vec::new(),
        };
        let tree_body = || {
            let mut fields = BTreeMap::new();
            fields.insert("value".to_string(), ObjectField::required(Type::Number));
            fields.insert(
                "children".to_string(),
                ObjectField::required(Type::Array(Box::new(tree_ref()))),
            );
            Type::Object {
                more_indexes: Vec::new(),
                index: None,
                fields,
            }
        };
        defs.types.insert(
            "Tree".to_string(),
            TypeSymbol {
                name: "Tree".to_string(),
                mangled_name: package_symbol(SYNTH, "Tree"),
                declaration_span: Span::at(crate::FileId(0)),
                kind: TypeKind::Alias {
                    generics: Default::default(),
                    ty: tree_body(),
                    doc: None,
                },
            },
        );

        let synth_ref = |name: &str, ty: Type| ValueSymbol {
            name: name.to_string(),
            mangled_name: package_symbol(SYNTH, name),
            declaration_span: Span::at(crate::FileId(0)),
            kind: ValueKind::Function {
                generics: Default::default(),
                params: Vec::new(),
                ret: ty,
                type_predicate: None,
                doc: None,
            },
        };
        defs.values.insert(
            "makeWidget".to_string(),
            synth_ref(
                "makeWidget",
                Type::InterfaceRef {
                    mangled: crate::mangle::package_symbol(SYNTH, "Widget"),
                    package: Package(SYNTH.to_string()),
                    name: "Widget".to_string(),
                    args: Vec::new(),
                },
            ),
        );
        defs.values.insert(
            "pickColor".to_string(),
            synth_ref(
                "pickColor",
                Type::NumberEnum {
                    mangled: crate::mangle::package_symbol(SYNTH, "Color"),
                    package: Package(SYNTH.to_string()),
                    name: "Color".to_string(),
                    member: None,
                },
            ),
        );
        defs.values.insert(
            "makeTree".to_string(),
            synth_ref(
                "makeTree",
                Type::Alias {
                    mangled: crate::mangle::package_symbol(SYNTH, "Tree"),
                    package: Package(SYNTH.to_string()),
                    name: "Tree".to_string(),
                    args: Vec::new(),
                    ty: Box::new(tree_body()),
                },
            ),
        );
        defs
    }

    fn pied_piper_package() -> PackageDeclaration {
        let mut defs = PackageDeclaration::with_package(PIED_PIPER);
        defs.values.insert(
            "greet".to_string(),
            ValueSymbol {
                name: "greet".to_string(),
                mangled_name: package_symbol(PIED_PIPER, "greet"),
                declaration_span: Span::at(crate::FileId(0)),
                kind: ValueKind::Function {
                    generics: Default::default(),
                    params: Vec::new(),
                    ret: Type::String,
                    type_predicate: None,
                    doc: None,
                },
            },
        );
        defs
    }

    fn errors(source: &str, packages: &[&PackageDeclaration]) -> Vec<String> {
        let parsed = super::parse_script(source, crate::FileId(0));
        let stdlib_defs = crate::runtime::stdlib_package_declarations();
        let (_ta, diags) = super::front_end(&parsed, &stdlib_defs, packages).expect("front end");
        diags
            .into_iter()
            .filter(|d| d.severity == crate::Severity::Error)
            .map(|d| d.message)
            .collect()
    }

    fn compile_errors(source: &str, packages: &[&PackageDeclaration]) -> Vec<String> {
        super::compile_script(source, "main.subm", crate::FileId(0), packages, &[])
            .expect_err("expected compile error")
            .into_iter()
            .filter(|d| d.severity == crate::Severity::Error)
            .map(|d| d.message)
            .collect()
    }

    #[test]
    fn package_declarations_passed_to_compile_are_importable() {
        let pied_piper = pied_piper_package();
        let source = r#"
            import { greet } from "@pied-piper/util";
            function main(): void {}
        "#;
        super::compile_script(source, "main.subm", crate::FileId(0), &[&pied_piper], &[])
            .expect("package compiles");
    }

    #[test]
    fn package_not_passed_to_compile_uses_not_found_diagnostic() {
        let pied_piper = pied_piper_package();
        let source = r#"
            import { greet } from "@pied-piper/util";
            function main(): void {}
        "#;
        let _package_not_supplied = pied_piper;
        let errs = compile_errors(source, &[]);
        assert!(
            errs.iter()
                .any(|m| m.contains("package `@pied-piper/util` not found")),
            "expected not-found diagnostic, got: {errs:?}"
        );
    }

    #[test]
    fn unknown_package_still_uses_not_found_diagnostic() {
        let source = r#"
            import { greet } from "@pied-piper/missing";
            function main(): void {}
        "#;
        let errs = compile_errors(source, &[]);
        assert!(
            errs.iter()
                .any(|m| m.contains("package `@pied-piper/missing` not found")),
            "expected not-found diagnostic, got: {errs:?}"
        );
    }

    #[test]
    fn stdlib_imports_do_not_require_blueprint_packages() {
        let source = r#"
            import { v4 } from "submilli:uuid";
            function main(): void {}
        "#;
        super::compile_script(source, "main.subm", crate::FileId(0), &[], &[])
            .expect("stdlib package compiles without package declarations");
    }

    #[test]
    fn importless_interface_member_access() {
        let synth = synth_package();
        // Imports only the function — never names `Widget`.
        let source = r#"
            import { makeWidget } from "submilli:synth";
            function main(): void {
                const w = makeWidget();
                assert(w.label === "x", "property read");
                const n: number = w.size();
                assert(n >= 0, "method call");
            }
        "#;
        assert_eq!(errors(source, &[&synth]), Vec::<String>::new());
    }

    #[test]
    fn importless_enum_value_use() {
        let synth = synth_package();
        // Imports only the function — never names `Color`.
        let source = r#"
            import { pickColor } from "submilli:synth";
            function main(): void {
                const a = pickColor();
                const b = pickColor();
                assert(a === b, "enum values compare");
            }
        "#;
        assert_eq!(errors(source, &[&synth]), Vec::<String>::new());
    }

    #[test]
    fn importless_recursive_alias_deep_member_access() {
        // `makeTree(): Tree` where `Tree = { value: number; children: Tree[] }`.
        // Reaching `children[0]` resolves a `Type::AliasRef` recursion back-edge
        // by its package — the alias name `Tree` is never imported. This guards
        // the registry routing in `rehydrate_alias_refs` / `expand_alias_ref`.
        let synth = synth_package();
        let source = r#"
            import { makeTree } from "submilli:synth";
            function main(): void {
                const t = makeTree();
                const root: number = t.value;
                assert(root >= 0, "root value");
                const child: number = t.children[0].value;
                assert(child >= 0, "deep recursive-alias value");
                const grand: number = t.children[0].children[0].value;
                assert(grand >= 0, "two levels deep");
            }
        "#;
        assert_eq!(errors(source, &[&synth]), Vec::<String>::new());
    }

    #[test]
    fn importless_recursive_alias_resolves_body_shape() {
        // Reading a *nonexistent* field on a deep recursion back-edge must still
        // produce a precise field-not-found diagnostic — proving the body shape
        // resolved through the registry rather than collapsing to `Error` (which
        // would silently swallow the access).
        let synth = synth_package();
        let source = r#"
            import { makeTree } from "submilli:synth";
            function main(): void {
                const t = makeTree();
                const _x = t.children[0].nope;
            }
        "#;
        let errs = errors(source, &[&synth]);
        assert!(
            errs.iter().any(|m| m.contains("nope")),
            "expected a field-not-found error naming `nope`; got: {errs:?}"
        );
    }

    #[test]
    fn unimported_type_name_still_unresolved_in_source() {
        // The fix must NOT make the library type name resolvable as a *source*
        // annotation without an import — only structural access is import-free.
        let synth = synth_package();
        let source = r#"
            import { makeWidget } from "submilli:synth";
            function main(): void {
                const w: Widget = makeWidget();
            }
        "#;
        let errs = errors(source, &[&synth]);
        assert!(
            errs.iter().any(|m| m.contains("unknown type `Widget`")),
            "expected unknown-type error for unimported source annotation; got: {errs:?}"
        );
    }
}

#[cfg(test)]
mod untyped_arena_failures {
    use super::*;
    use crate::arena::{ArenaKind, with_node_limit};
    use crate::compiler_error::{CompilerFailure, CompilerStage};

    #[test]
    fn parser_arena_limit_is_fatal_and_keeps_prior_diagnostics() {
        // The node limit is this thread's, so the compiler must run on it.
        crate::compiler_thread::on_test_stack(
            parser_arena_limit_is_fatal_and_keeps_prior_diagnostics_inner,
        );
    }

    fn parser_arena_limit_is_fatal_and_keeps_prior_diagnostics_inner() {
        let source = "let = 0; function main(): number { return 1; }";
        for arena in [ArenaKind::Expressions, ArenaKind::Statements] {
            let error = with_node_limit(arena, 0, || {
                compile_script_checked(source, "limit.ts", FileId(0), &[], &[])
            })
            .unwrap_err();
            assert!(matches!(
                error.fatal,
                Some(CompilerFailure::Limit {
                    stage: CompilerStage::Parse,
                    span: None,
                    ..
                })
            ));
            assert!(
                !error.diagnostics.is_empty(),
                "earlier syntax diagnostics must survive"
            );
        }
        assert!(
            !compile_script_checked(
                "function main(): number { return 1; }",
                "healthy.ts",
                FileId(0),
                &[],
                &[]
            )
            .unwrap()
            .wasm
            .is_empty()
        );
    }

    #[test]
    fn lowering_arena_limit_stops_script_before_inference() {
        // The node limit is this thread's, so the compiler must run on it.
        crate::compiler_thread::on_test_stack(
            lowering_arena_limit_stops_script_before_inference_inner,
        );
    }

    fn lowering_arena_limit_stops_script_before_inference_inner() {
        let source = "function main(): number { const [x] = [1]; return x; }";
        let mut lexer = crate::Asi::new(source, FileId(0));
        let mut tokens = Vec::new();
        loop {
            let token = lexer.next_token();
            let done = matches!(token.kind, crate::TokenKind::Eof);
            tokens.push(token);
            if done {
                break;
            }
        }
        let (ast, _) = crate::parser::parse_checked(source, tokens, FileId(0)).unwrap();
        for (arena, count) in [
            (ArenaKind::Expressions, ast.exprs_len()),
            (ArenaKind::Statements, ast.stmts_len()),
        ] {
            let error = with_node_limit(arena, u32::try_from(count).unwrap(), || {
                compile_script_checked(source, "limit.ts", FileId(0), &[], &[])
            })
            .unwrap_err();
            assert!(matches!(
                error.fatal,
                Some(CompilerFailure::Limit {
                    stage: CompilerStage::Infer,
                    span: None,
                    ..
                })
            ));
        }
        assert!(
            !compile_script_checked(source, "healthy.ts", FileId(0), &[], &[])
                .unwrap()
                .wasm
                .is_empty()
        );
    }

    #[test]
    fn package_arena_limit_returns_no_artifact() {
        // The node limit is this thread's, so the compiler must run on it.
        crate::compiler_thread::on_test_stack(package_arena_limit_returns_no_artifact_inner);
    }

    fn package_arena_limit_returns_no_artifact_inner() {
        let modules = [PackageSourceModule {
            path: ModulePath::from("lib"),
            source: "export function value(): number { return 1; }",
        }];
        let error = with_node_limit(ArenaKind::Expressions, 0, || {
            compile_package_checked("test", ModulePath::from("lib"), &modules, &[])
        })
        .unwrap_err();
        assert!(matches!(
            error.fatal,
            Some(CompilerFailure::Limit {
                stage: CompilerStage::Parse,
                ..
            })
        ));
        compile_package_checked("test", ModulePath::from("lib"), &modules, &[]).unwrap();
    }

    #[test]
    fn corrupt_parsed_imports_return_an_internal_failure() {
        let mut parsed = parse_script("function main(): number { return 1; }", FileId(0));
        parsed.ast.top_level.push(crate::StmtId(u32::MAX));
        let error = parsed.external_imports().unwrap_err();
        assert!(matches!(
            error.fatal,
            Some(CompilerFailure::Internal { span: None, .. })
        ));
    }
}

#[cfg(test)]
mod typed_arena_failures {
    use super::*;
    use crate::arena::{ArenaKind, with_node_limit};

    fn assert_limit(error: CompileError) {
        assert!(
            matches!(error.fatal, Some(CompilerFailure::Limit { span: None, .. })),
            "{error:?}"
        );
    }

    #[test]
    fn typed_allocation_failure_stops_script_and_package_compilation() {
        // The node limit is this thread's, so the compiler must run on it.
        crate::compiler_thread::on_test_stack(
            typed_allocation_failure_stops_script_and_package_compilation_inner,
        );
    }

    fn typed_allocation_failure_stops_script_and_package_compilation_inner() {
        let source = "function main(): number { return 1; }";
        let modules = [PackageSourceModule {
            path: ModulePath::from("lib"),
            source: "export function value(): number { return 1; }",
        }];
        for arena in [ArenaKind::TypedExpressions, ArenaKind::TypedStatements] {
            assert_limit(
                with_node_limit(arena, 0, || {
                    compile_script_checked(source, "limit.ts", FileId(0), &[], &[])
                })
                .unwrap_err(),
            );
            assert_limit(
                with_node_limit(arena, 0, || {
                    compile_package_checked("test", ModulePath::from("lib"), &modules, &[])
                })
                .unwrap_err(),
            );
        }
        assert!(
            !compile_script_checked(source, "healthy.ts", FileId(0), &[], &[])
                .unwrap()
                .wasm
                .is_empty()
        );
        assert!(
            !compile_package_checked("test", ModulePath::from("lib"), &modules, &[])
                .unwrap()
                .wasm
                .is_empty()
        );
    }

    #[test]
    fn typed_allocation_failure_keeps_prior_inference_diagnostics() {
        // The node limit is this thread's, so the compiler must run on it.
        crate::compiler_thread::on_test_stack(
            typed_allocation_failure_keeps_prior_inference_diagnostics_inner,
        );
    }

    fn typed_allocation_failure_keeps_prior_inference_diagnostics_inner() {
        let source = "function main(): number { let value: number = \"wrong\"; return 2; }";
        let (ast, diagnostics) = typecheck_to_typed_ast(source, FileId(0));
        assert!(!diagnostics.is_empty());
        for (arena, count) in [
            (ArenaKind::TypedExpressions, ast.exprs_len()),
            (ArenaKind::TypedStatements, ast.stmts_len()),
        ] {
            let error = with_node_limit(arena, u32::try_from(count - 1).unwrap(), || {
                compile_script_checked(source, "limit.ts", FileId(0), &[], &[])
            })
            .unwrap_err();
            assert!(!error.diagnostics.is_empty(), "{error:?}");
            assert_limit(error);
        }
    }

    #[test]
    fn codegen_allocation_failure_returns_no_wasm() {
        let source = "function main(): number { let value: number | null = 1; if (value !== null) { return value; } return 0; }";
        let (ast, diagnostics) = typecheck_to_typed_ast(source, FileId(0));
        assert!(diagnostics.is_empty());
        let ast = desugar(capture(ast).unwrap(), FileId(0)).unwrap();
        let (prelude, host, internal) =
            crate::runtime::prelude::cached_runtime_package_declarations();
        let dependencies: Vec<_> = prelude.iter().chain(host).chain(internal).collect();
        let failure = with_node_limit(
            ArenaKind::TypedExpressions,
            u32::try_from(ast.exprs_len()).unwrap(),
            || crate::codegen::codegen(source, "limit.ts", FileId(0), &ast, &dependencies),
        )
        .unwrap_err();
        assert!(
            matches!(
                failure,
                CompilerFailure::Limit {
                    stage: CompilerStage::Codegen,
                    span: None,
                    ..
                }
            ),
            "{failure}"
        );
        assert!(
            !crate::codegen::codegen(source, "healthy.ts", FileId(0), &ast, &dependencies)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn desugaring_capacity_failure_does_not_return_a_transformed_tree() {
        let source = "function main(): void { do {} while (false); }";
        let (ast, diagnostics) = typecheck_to_typed_ast(source, FileId(0));
        assert!(diagnostics.is_empty());
        for (arena, count) in [
            (ArenaKind::TypedExpressions, ast.exprs_len()),
            (ArenaKind::TypedStatements, ast.stmts_len()),
        ] {
            let failure = with_node_limit(arena, u32::try_from(count).unwrap(), || {
                desugar(ast.clone(), FileId(0))
            })
            .unwrap_err();
            assert_limit(failure.into());
        }
        desugar(ast, FileId(0)).unwrap();
    }
}

#[cfg(test)]
mod alias_reference_value_tests {
    //! SUB-2273: a package value typed by a bare reference to one of its
    //! aliases (`Type::AliasRef`, no inline body) is kept as a reference of no
    //! shape. A member call, an operator or a typed slot that reads it directly
    //! needs the alias body's own representation, so the module must convert
    //! it there, as it does when the value is first bound to a local.

    use crate::mangle::package_symbol;
    use crate::package_declaration::{TypeKind, ValueKind};
    use crate::{Package, PackageDeclaration, Span, Type, TypeSymbol, ValueSymbol};

    const KIT: &str = "@acme/kit";

    /// Each function of `@acme/kit` and the alias its result is typed by.
    const RESULTS: [(&str, &str); 6] = [
        ("mkBig", "Big"),
        ("mkId", "Id"),
        ("mkNum", "Num"),
        ("mkFlag", "Flag"),
        ("mkStrs", "Strs"),
        ("mkNumAlias", "NumAlias"),
    ];

    fn reference(name: &str) -> Type {
        Type::AliasRef {
            mangled: package_symbol(KIT, name),
            package: Package(KIT.to_string()),
            name: name.to_string(),
            args: Vec::new(),
        }
    }

    fn kit() -> PackageDeclaration {
        let mut defs = PackageDeclaration::with_package(KIT);
        for (name, ty) in [
            ("Id", Type::String),
            ("Num", Type::Number),
            ("Flag", Type::Boolean),
            ("Big", Type::BigInt),
            ("Strs", Type::Array(Box::new(Type::String))),
            ("NumAlias", reference("Num")),
        ] {
            defs.types.insert(
                name.to_string(),
                TypeSymbol {
                    name: name.to_string(),
                    mangled_name: package_symbol(KIT, name),
                    declaration_span: Span::at(crate::FileId(0)),
                    kind: TypeKind::Alias {
                        generics: Default::default(),
                        ty,
                        doc: None,
                    },
                },
            );
        }
        let returning = |alias: &str| ValueKind::Function {
            generics: Default::default(),
            params: Vec::new(),
            ret: reference(alias),
            type_predicate: None,
            doc: None,
        };
        for (name, alias) in RESULTS {
            let kind = returning(alias);
            defs.values.insert(
                name.to_string(),
                ValueSymbol {
                    name: name.to_string(),
                    mangled_name: package_symbol(KIT, name),
                    declaration_span: Span::at(crate::FileId(0)),
                    kind,
                },
            );
        }
        defs
    }

    /// Uses TypeScript accepts of a value typed by an alias reference. The
    /// front end reads a package constant only where it is called, so each use
    /// reads a function's result.
    const USES: &[&str] = &[
        "const u = kit.mkId().toUpperCase();",
        "const n = kit.mkId().length;",
        "const j = kit.mkStrs().join(\",\");",
        "const s = kit.mkNum().toFixed(1);",
        "const y = kit.mkNum() + 1;",
        "const y = kit.mkNum() * 2;",
        "const y = -kit.mkNum();",
        "const y = +kit.mkId();",
        "const t = kit.mkNum() < 3;",
        "const t = kit.mkNum() < kit.mkNumAlias();",
        "const t = kit.mkId() < \"x\";",
        "const c = kit.mkId() + \"!\";",
        "const y = kit.mkNumAlias() + 1;",
        "let x: number = 7;\nx %= kit.mkNum();",
        "const b = kit.mkNum() === 1;",
        "const f = kit.mkFlag();",
        "const f: boolean = kit.mkFlag();",
        "let b = true;\nb = kit.mkFlag();",
        "function h(b: boolean): boolean { return b; }\nconst r = h(kit.mkFlag());",
        "function g(): boolean { return kit.mkFlag(); }",
        "const n = !kit.mkFlag();",
        "if (kit.mkFlag()) { console.log(\"y\"); }",
        "const f = kit.mkFlag() ? 1 : 0;",
        "const b = kit.mkNum() !== 2;",
        "const s = `${kit.mkNum()}`;",
        "switch (kit.mkNum()) { case 1: break; }",
        "const e = kit.mkStrs()[0];",
        "const l = kit.mkStrs().length;",
        "let t = 0;\nfor (const s of kit.mkStrs()) { t += s.length; }",
        "const c = [...kit.mkStrs()];",
        "const n = kit.mkId()?.length;",
        "const y = kit.mkBig() + 1n;",
        "const y = -kit.mkBig();",
        "const t = kit.mkBig() < 2n;",
    ];

    fn source(statements: &str) -> String {
        format!(
            "import * as kit from \"{KIT}\";\n{statements}\nfunction main(): number {{ return 0; }}\n"
        )
    }

    /// The front end types each call as the alias it names, written out; the
    /// result still returns in the reference's slot.
    #[test]
    fn uses_of_a_result_typed_by_an_alias_reference_load() {
        let kit = kit();
        let engine = crate::runtime::RuntimeConfig::default().engine().unwrap();
        let mut failures = Vec::new();
        for statements in USES {
            match super::compile_script(
                &source(statements),
                "main.ts",
                crate::FileId(0),
                &[&kit],
                &[],
            ) {
                Ok(compiled) => {
                    if let Err(error) = wasmtime::Module::new(&engine, &compiled.wasm) {
                        failures.push(format!("{statements}: does not load: {error:#}"));
                    }
                }
                Err(diagnostics) => failures.push(format!(
                    "{statements}: {:?}",
                    diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
                )),
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// A front end may instead type the read itself as the declaration's alias
    /// reference, so the tree and the slot agree; each operator and member call
    /// then reads the reference. Retyping each call as its declared result
    /// gives that tree.
    #[test]
    fn uses_of_a_value_typed_as_the_alias_reference_load() {
        let kit = kit();
        let engine = crate::runtime::RuntimeConfig::default().engine().unwrap();
        let stdlib_defs = crate::runtime::stdlib_package_declarations();
        let (prelude_defs, host_defs, internal_defs) =
            crate::runtime::prelude::cached_runtime_package_declarations();
        let mut dependencies: Vec<&PackageDeclaration> = prelude_defs.iter().collect();
        dependencies.extend(host_defs);
        dependencies.extend(internal_defs);
        dependencies.extend(stdlib_defs.iter());
        dependencies.push(&kit);
        let mut failures = Vec::new();
        for statements in USES {
            let source = source(statements);
            let parsed = super::parse_script(&source, crate::FileId(0));
            let (mut ta, diagnostics) =
                super::front_end(&parsed, &stdlib_defs, &[&kit]).expect("front end");
            assert!(
                !super::has_errors(&diagnostics),
                "{statements}: {diagnostics:?}"
            );
            for index in 0..ta.exprs_len() {
                let expr = ta.try_expr_mut(crate::ExprId(index as u32)).unwrap();
                if let crate::TypedExprKind::Call { mangled, .. } = &expr.kind
                    && let Some((_, alias)) = RESULTS
                        .iter()
                        .find(|(name, _)| *mangled == package_symbol(KIT, name))
                {
                    expr.ty = reference(alias);
                }
            }
            let ta = super::desugar(super::capture(ta).unwrap(), crate::FileId(0)).unwrap();
            let loaded = crate::codegen::codegen_with_type_info(
                &source,
                "main.ts",
                crate::FileId(0),
                &ta,
                &dependencies,
            )
            .map_err(|failure| format!("{failure:?}"))
            .and_then(|generated| {
                wasmtime::Module::new(&engine, &generated.wasm)
                    .map(drop)
                    .map_err(|error| format!("does not load: {error:#}"))
            });
            if let Err(failure) = loaded {
                failures.push(format!("{statements}: {failure}"));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}
