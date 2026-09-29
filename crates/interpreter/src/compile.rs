//! End-to-end source-to-Wasm pipeline.
//!
//! Callers run these entry points on a thread with at least
//! [`crate::compiler_limits::COMPILER_STACK_BYTES`] of stack.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use crate::compile_capabilities;
use crate::compiler_error::{CompileError, CompilerFailure, CompilerStage};
use crate::runtime::prelude;
use crate::typed_ast::TypedAst;
use crate::{
    Asi, Ast, DerivedCapability, Diagnostic, FileId, ModulePath, PackageDeclaration, Severity,
    Sources, StmtKind, Token, TokenKind, capture, check, desugar, lower_patterns, runtime,
};

/// Wall-clock spent in each compile phase, filled in as the pipeline runs. A
/// phase that never runs (e.g. `codegen` when the front-end errors) stays zero.
/// Surfaced on [`CompiledScript`] for embedders that report compile-time
/// performance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PhaseTimings {
    pub lex: Duration,
    /// Parse plus pattern-lowering — both run together in the front-end.
    pub parse: Duration,
    /// Type inference plus the post-inference check pass.
    pub typecheck: Duration,
    pub capture: Duration,
    pub desugar: Duration,
    pub codegen: Duration,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScriptImports {
    pub stdlib: BTreeSet<String>,
    pub registry_packages: BTreeSet<String>,
    pub mcp_servers: BTreeSet<String>,
}

#[derive(Clone, Debug)]
pub struct ParsedScript {
    file: FileId,
    source: String,
    failure: Option<CompileError>,
    ast: Ast,
    diagnostics: Vec<Diagnostic>,
    timings: PhaseTimings,
}

impl ParsedScript {
    pub fn external_imports(&self) -> Result<ScriptImports, CompileError> {
        let stdlib_names: BTreeSet<String> = runtime::stdlib_package_declarations()
            .into_iter()
            .map(|defs| defs.package_name)
            .collect();
        let mut imports = ScriptImports::default();
        for stmt_id in &self.ast.top_level {
            let StmtKind::Import { module, .. } = &self
                .ast
                .try_stmt(*stmt_id)
                .map_err(|error| {
                    CompileError::from(
                        error.into_compiler_failure(crate::compiler_error::CompilerStage::Parse),
                    )
                    .with_prior_diagnostics(&self.diagnostics)
                })?
                .kind
            else {
                continue;
            };
            if crate::source::is_relative_specifier(module) {
                continue;
            }
            if let Some(server) = module.strip_prefix("@mcp/") {
                imports.mcp_servers.insert(server.to_string());
            } else if stdlib_names.contains(module) {
                imports.stdlib.insert(module.clone());
            } else {
                imports.registry_packages.insert(module.clone());
            }
        }
        Ok(imports)
    }

    pub fn has_errors(&self) -> bool {
        self.failure.is_some() || has_errors(&self.diagnostics)
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
}

pub fn parse_script(source: &str, file: FileId) -> ParsedScript {
    let mut timings = PhaseTimings::default();

    let lex_start = Instant::now();
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
    let (mut diagnostics, mut failure) = match asi.finish() {
        Ok(diagnostics) => (diagnostics, None),
        Err(error) => (error.clone().into_diagnostics(file), Some(error)),
    };
    timings.lex = lex_start.elapsed();

    let parse_start = Instant::now();
    let (mut ast, parse_diags) = if failure.is_some() {
        (Ast::new(), Vec::new())
    } else {
        match crate::parser::parse_checked(source, tokens, file) {
            Ok(result) => result,
            Err(mut error) => {
                error.diagnostics.splice(0..0, diagnostics.clone());
                let rendered = error.clone().into_diagnostics(file);
                diagnostics.clear();
                failure = Some(error);
                (Ast::new(), rendered)
            }
        }
    };
    diagnostics.extend(parse_diags);
    // must run before type-checking; the typechecker assumes patterns are already lowered
    if failure.is_none() {
        match lower_patterns(ast) {
            Ok(lowered) => ast = lowered,
            Err(fatal) => {
                let error = CompileError {
                    diagnostics: diagnostics.clone(),
                    fatal: Some(fatal),
                };
                diagnostics = error.clone().into_diagnostics(file);
                failure = Some(error);
                ast = Ast::new();
            }
        }
    }
    timings.parse = parse_start.elapsed();

    ParsedScript {
        file,
        source: source.to_owned(),
        failure,
        ast,
        diagnostics,
        timings,
    }
}

/// Run the front-end (infer → check) and return the typed
/// AST alongside the accumulated diagnostics and per-phase timings. Shared by
/// [`compile_script`] and [`typecheck`]; the typed AST is only meaningful when
/// `diags` is error-free. `file` is the [`FileId`] the caller assigned this
/// source in its [`Sources`](crate::Sources) registry; every span is stamped
/// with it so diagnostics resolve against that entry. `packages` join the stdlib
/// in the set the typechecker resolves imports against (e.g. the server's
/// `@mcp/<server>` virtual packages).
fn front_end(
    source: &str,
    parsed: &ParsedScript,
    stdlib_defs: &[PackageDeclaration],
    packages: &[&PackageDeclaration],
) -> Result<(TypedAst, Vec<Diagnostic>, PhaseTimings), CompileError> {
    front_end_with_transitive(source, parsed, stdlib_defs, packages, &[])
}

fn front_end_with_transitive(
    source: &str,
    parsed: &ParsedScript,
    stdlib_defs: &[PackageDeclaration],
    packages: &[&PackageDeclaration],
    transitive: &[&PackageDeclaration],
) -> Result<(TypedAst, Vec<Diagnostic>, PhaseTimings), CompileError> {
    if source != parsed.source {
        return Err(CompilerFailure::Internal {
            stage: CompilerStage::Infer,
            span: None,
            message: "parsed script does not belong to the supplied source".into(),
        }
        .into());
    }
    if let Some(error) = &parsed.failure {
        return Err(error.clone());
    }
    let mut timings = parsed.timings;
    let mut diags = parsed.diagnostics.clone();
    let typecheck_start = Instant::now();
    let (prelude_defs, host_defs, _) = prelude::cached_runtime_package_declarations();
    let mut infer_refs: Vec<&crate::PackageDeclaration> = prelude_defs.iter().collect();
    infer_refs.extend(host_defs.iter());
    infer_refs.extend(stdlib_defs.iter());
    infer_refs.extend_from_slice(packages);
    let (ta, infer_diags) = crate::typechecker::infer::infer_with_transitive_checked(
        source,
        crate::mangle::USER_PACKAGE,
        &parsed.ast,
        &infer_refs,
        transitive,
    )
    .map_err(|mut error| {
        error.diagnostics.splice(0..0, diags.clone());
        error
    })?;
    diags.extend(infer_diags);
    diags.extend(check(&ta).map_err(|error| error.with_prior_diagnostics(&diags))?);
    timings.typecheck = typecheck_start.elapsed();

    Ok((ta, diags, timings))
}

#[derive(Clone, Debug)]
pub struct PackageSourceModule<'a> {
    pub path: ModulePath,
    pub source: &'a str,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompiledPackage {
    pub wasm: Vec<u8>,
    pub type_info: crate::TypeInfoTable,
    pub declaration: PackageDeclaration,
    pub required_capabilities: Vec<DerivedCapability>,
    pub warnings: Vec<Diagnostic>,
}

#[derive(Clone, Debug)]
pub struct CompiledScript {
    pub wasm: Vec<u8>,
    pub type_info: crate::TypeInfoTable,
    pub warnings: Vec<Diagnostic>,
    pub timings: PhaseTimings,
}

/// Typecheck only — runs the front-end and stops before codegen. Returns
/// `Ok(warnings)` when the program is error-free (warnings are non-fatal),
/// `Err(diagnostics)` if any phase errors. Backs `submilli check`.
pub fn typecheck_checked(source: &str, file: FileId) -> Result<Vec<Diagnostic>, CompileError> {
    typecheck_script(source, file).map_err(|error| {
        error.with_limit_span_cut(|span_file| (span_file == file).then_some(source))
    })
}

fn typecheck_script(source: &str, file: FileId) -> Result<Vec<Diagnostic>, CompileError> {
    let parsed = parse_script(source, file);
    let stdlib_defs = runtime::stdlib_package_declarations();
    let (ta, diags, _timings) = front_end(source, &parsed, &stdlib_defs, &[])?;
    if has_errors(&diags) {
        return Err(diags.into());
    }
    // Lowering deepens the typed tree, so check enforces compilation's limit on
    // the lowered tree too.
    let with_front_end_diagnostics = |failure| CompileError {
        diagnostics: diags.clone(),
        fatal: Some(failure),
    };
    let ta = capture(ta).map_err(with_front_end_diagnostics)?;
    desugar(ta, file).map_err(with_front_end_diagnostics)?;
    Ok(warnings_only(diags))
}

/// Typecheck and return the typed AST together with every diagnostic, errors
/// included. For tools that inspect what the typechecker inferred — the
/// TypeScript baseline comparison in `crates/conformance` — rather than compile.
/// The typed AST is only meaningful where the diagnostics are error-free.
pub fn typecheck_to_typed_ast(source: &str, file: FileId) -> (TypedAst, Vec<Diagnostic>) {
    let parsed = parse_script(source, file);
    let stdlib_defs = runtime::stdlib_package_declarations();
    match front_end(source, &parsed, &stdlib_defs, &[]) {
        Ok((ta, diags, _)) => (ta, diags),
        Err(error) => (
            TypedAst::new(),
            error
                .with_limit_span_cut(|span_file| (span_file == file).then_some(source))
                .into_diagnostics(file),
        ),
    }
}

/// Compile a script module. `filename` is the DWARF compile-unit name for
/// backtrace source mapping; `file` is this source's [`FileId`] in the caller's
/// [`Sources`](crate::Sources) registry. `packages` are real package
/// declarations that emit Wasm imports; `mcps` are declaration-only virtual MCP
/// packages.
///
/// Returns [`CompiledScript`] on success. Warnings are non-fatal and should be
/// surfaced without aborting; callers that only need Wasm bytes can read
/// [`CompiledScript::wasm`]. Returns `Err(diagnostics)` if any phase errors.
pub fn compile_script_checked(
    source: &str,
    filename: &str,
    file: FileId,
    packages: &[&PackageDeclaration],
    mcps: &[&PackageDeclaration],
) -> Result<CompiledScript, CompileError> {
    let parsed = parse_script(source, file);
    let stdlib_defs = runtime::stdlib_package_declarations();
    compile_parsed_script_timed_checked(source, filename, &parsed, &stdlib_defs, packages, mcps)
}

/// Compiles a script whose code belongs to `owning_package` rather than to `main`.
///
/// The one caller is a package's test file: it compiles as a script, so its entry point stays
/// mangled as `main`, but its gated calls must be attributed to the package that ships it.
pub fn compile_script_owned_by_checked(
    owning_package: &str,
    source: &str,
    filename: &str,
    file: FileId,
    packages: &[&PackageDeclaration],
    mcps: &[&PackageDeclaration],
) -> Result<CompiledScript, CompileError> {
    let parsed = parse_script(source, file);
    let stdlib_defs = runtime::stdlib_package_declarations();
    let declarations = [packages, mcps].concat();
    compile_parsed_script_owned_by(
        Some(owning_package),
        source,
        filename,
        &parsed,
        &stdlib_defs,
        &declarations,
        &[],
    )
}

pub fn compile_parsed_script_timed_checked(
    source: &str,
    filename: &str,
    parsed: &ParsedScript,
    stdlib_defs: &[PackageDeclaration],
    packages: &[&PackageDeclaration],
    mcps: &[&PackageDeclaration],
) -> Result<CompiledScript, CompileError> {
    compile_parsed_script_with_transitive_checked(
        source,
        filename,
        parsed,
        stdlib_defs,
        packages,
        mcps,
        &[],
    )
}

/// Compile with declarations needed to reconstruct imported types without
/// exposing those transitive packages to source-level imports.
pub fn compile_parsed_script_with_transitive_checked(
    source: &str,
    filename: &str,
    parsed: &ParsedScript,
    stdlib_defs: &[PackageDeclaration],
    packages: &[&PackageDeclaration],
    mcps: &[&PackageDeclaration],
    transitive: &[&PackageDeclaration],
) -> Result<CompiledScript, CompileError> {
    let declarations = [packages, mcps].concat();
    compile_parsed_script_owned_by(
        None,
        source,
        filename,
        parsed,
        stdlib_defs,
        &declarations,
        transitive,
    )
}

fn compile_parsed_script_owned_by(
    owning_package: Option<&str>,
    source: &str,
    filename: &str,
    parsed: &ParsedScript,
    stdlib_defs: &[PackageDeclaration],
    external_declarations: &[&PackageDeclaration],
    transitive: &[&PackageDeclaration],
) -> Result<CompiledScript, CompileError> {
    compile_parsed_script(
        owning_package,
        source,
        filename,
        parsed,
        stdlib_defs,
        external_declarations,
        transitive,
    )
    .map_err(|error| error.with_limit_span_cut(|file| (file == parsed.file).then_some(source)))
}

fn compile_parsed_script(
    owning_package: Option<&str>,
    source: &str,
    filename: &str,
    parsed: &ParsedScript,
    stdlib_defs: &[PackageDeclaration],
    external_declarations: &[&PackageDeclaration],
    transitive: &[&PackageDeclaration],
) -> Result<CompiledScript, CompileError> {
    let (mut ta, diags, mut timings) = front_end_with_transitive(
        source,
        parsed,
        stdlib_defs,
        external_declarations,
        transitive,
    )?;
    if has_errors(&diags) {
        return Err(diags.into());
    }
    let capture_start = Instant::now();
    ta = capture(ta).map_err(|fatal| CompileError {
        diagnostics: diags.clone(),
        fatal: Some(fatal),
    })?;
    timings.capture = capture_start.elapsed();

    let desugar_start = Instant::now();
    ta = desugar(ta, parsed.file).map_err(|fatal| CompileError {
        diagnostics: diags.clone(),
        fatal: Some(fatal),
    })?;
    timings.desugar = desugar_start.elapsed();

    let (prelude_defs, host_defs, internal_defs) = prelude::cached_runtime_package_declarations();
    let mut dependencies: Vec<&crate::PackageDeclaration> = prelude_defs.iter().collect();
    for defs in host_defs {
        dependencies.push(defs);
    }
    for defs in internal_defs {
        dependencies.push(defs);
    }
    for defs in stdlib_defs {
        dependencies.push(defs);
    }
    dependencies.extend_from_slice(external_declarations);
    dependencies.extend_from_slice(transitive);

    let codegen_start = Instant::now();
    let generated = match owning_package {
        Some(pkg) => {
            crate::codegen::codegen_owned_by(pkg, source, filename, parsed.file, &ta, &dependencies)
        }
        None => crate::codegen::codegen_with_type_info(
            source,
            filename,
            parsed.file,
            &ta,
            &dependencies,
        ),
    }
    .map_err(|fatal| CompileError {
        diagnostics: diags.clone(),
        fatal: Some(fatal),
    })?;
    timings.codegen = codegen_start.elapsed();

    Ok(CompiledScript {
        wasm: generated.wasm,
        type_info: generated.type_info,
        warnings: warnings_only(diags),
        timings,
    })
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
    let mut sources = Sources::new();
    compile_package_sources(
        &mut sources,
        package_name,
        root_module,
        modules,
        dependencies,
        transitive,
    )
    .map_err(|error| {
        error.with_limit_span_cut(|file| sources.get(file).map(crate::SourceFile::text))
    })
}

fn compile_package_sources(
    sources: &mut Sources,
    package_name: &str,
    root_module: ModulePath,
    modules: &[PackageSourceModule<'_>],
    dependencies: &[&PackageDeclaration],
    transitive: &[&PackageDeclaration],
) -> Result<CompiledPackage, CompileError> {
    let mut parsed_modules = Vec::with_capacity(modules.len());
    let mut diagnostics = Vec::new();
    for module in modules {
        let file = sources
            .add(module.path.clone(), module.source)
            .map_err(|error| {
                CompileError::from(error.into_compiler_failure(CompilerStage::Parse))
                    .with_prior_diagnostics(&diagnostics)
            })?;
        let (ast, mut module_diags) =
            parse_package_module(module.source, file).map_err(|mut error| {
                error.diagnostics.splice(0..0, diagnostics.clone());
                error
            })?;
        diagnostics.append(&mut module_diags);
        parsed_modules.push((module.path.clone(), file, ast));
    }
    if has_errors(&diagnostics) {
        return Err(diagnostics.into());
    }

    let module_refs: Vec<_> = parsed_modules
        .iter()
        .map(|(path, file, ast)| (path.clone(), *file, ast))
        .collect();
    let stdlib_defs = runtime::stdlib_package_declarations();
    let mut external_packages: BTreeMap<String, PackageDeclaration> = stdlib_defs
        .iter()
        .map(|defs| (defs.package_name.clone(), defs.clone()))
        .collect();
    let (prelude_defs, host_defs, _) = prelude::cached_runtime_package_declarations();
    for defs in prelude_defs {
        external_packages.insert(defs.package_name.clone(), defs.clone());
    }
    for defs in host_defs {
        external_packages.insert(defs.package_name.clone(), defs.clone());
    }
    for defs in dependencies {
        external_packages.insert(defs.package_name.clone(), (*defs).clone());
    }
    let transitive_packages: BTreeMap<String, PackageDeclaration> = transitive
        .iter()
        .filter(|defs| !external_packages.contains_key(&defs.package_name))
        .map(|defs| (defs.package_name.clone(), (*defs).clone()))
        .collect();
    let (mut ta, mut declaration, mut package_diags) =
        crate::typechecker::infer::infer_package_checked(
            package_name,
            root_module.clone(),
            module_refs,
            sources,
            external_packages,
            transitive_packages,
        )
        .map_err(|mut error| {
            error.diagnostics.splice(0..0, diagnostics.clone());
            error
        })?;
    diagnostics.append(&mut package_diags);
    if has_errors(&diagnostics) {
        return Err(diagnostics.into());
    }
    let (required_capabilities, capability_warnings) =
        compile_capabilities::derive_package_requirements(
            package_name,
            &ta,
            &stdlib_defs,
            dependencies,
        )
        .map_err(|error| error.with_prior_diagnostics(&diagnostics))?;
    diagnostics.extend(capability_warnings);
    ta = capture(ta).map_err(|fatal| CompileError {
        diagnostics: diagnostics.clone(),
        fatal: Some(fatal),
    })?;
    let root_file = parsed_modules
        .iter()
        .find(|(path, _, _)| *path == root_module)
        .map(|(_, file, _)| *file)
        .ok_or_else(|| CompilerFailure::Internal {
            stage: CompilerStage::Infer,
            span: None,
            message: "compiled package root module is missing".into(),
        })?;
    ta = desugar(ta, root_file).map_err(|fatal| CompileError {
        diagnostics: diagnostics.clone(),
        fatal: Some(fatal),
    })?;

    let (prelude_defs, host_defs, internal_defs) = prelude::cached_runtime_package_declarations();
    let stdlib_defs = runtime::stdlib_package_declarations();
    let mut codegen_deps: Vec<&PackageDeclaration> = prelude_defs.iter().collect();
    codegen_deps.extend(host_defs.iter());
    codegen_deps.extend(internal_defs.iter());
    codegen_deps.extend(stdlib_defs.iter());
    codegen_deps.extend_from_slice(dependencies);
    codegen_deps.extend_from_slice(transitive);
    let generated =
        crate::codegen::codegen_package_with_type_info(sources, root_file, &ta, &codegen_deps)
            .map_err(|fatal| CompileError {
                diagnostics: diagnostics.clone(),
                fatal: Some(fatal),
            })?;
    declaration.runtime_functions = generated.runtime_functions;
    declaration.runtime_globals = generated.runtime_globals;
    Ok(CompiledPackage {
        wasm: generated.wasm,
        type_info: generated.type_info,
        declaration,
        required_capabilities,
        warnings: warnings_only(diagnostics),
    })
}

fn parse_package_module(
    source: &str,
    file: FileId,
) -> Result<(crate::Ast, Vec<Diagnostic>), CompileError> {
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
    let mut diagnostics = asi.finish()?;
    let (mut ast, parse_diags) =
        crate::parser::parse_checked(source, tokens, file).map_err(|mut error| {
            error.diagnostics.splice(0..0, diagnostics.clone());
            error
        })?;
    diagnostics.extend(parse_diags);
    if !has_errors(&diagnostics) {
        ast = lower_patterns(ast).map_err(|fatal| CompileError {
            diagnostics: diagnostics.clone(),
            fatal: Some(fatal),
        })?;
    }
    Ok((ast, diagnostics))
}

fn has_errors(diags: &[Diagnostic]) -> bool {
    diags.iter().any(|d| d.severity == Severity::Error)
}

fn warnings_only(diags: Vec<Diagnostic>) -> Vec<Diagnostic> {
    diags
        .into_iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect()
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
pub fn compile_parsed_script_timed(
    source: &str,
    filename: &str,
    parsed: &ParsedScript,
    stdlib_defs: &[PackageDeclaration],
    packages: &[&PackageDeclaration],
    mcps: &[&PackageDeclaration],
) -> Result<CompiledScript, Vec<Diagnostic>> {
    compile_parsed_script_timed_checked(source, filename, parsed, stdlib_defs, packages, mcps)
        .map_err(|error| error.into_diagnostics(parsed.file))
}

/// Compatibility adapter returning source and fatal failures as diagnostics.
pub fn compile_parsed_script_with_transitive(
    source: &str,
    filename: &str,
    parsed: &ParsedScript,
    stdlib_defs: &[PackageDeclaration],
    packages: &[&PackageDeclaration],
    mcps: &[&PackageDeclaration],
    transitive: &[&PackageDeclaration],
) -> Result<CompiledScript, Vec<Diagnostic>> {
    compile_parsed_script_with_transitive_checked(
        source,
        filename,
        parsed,
        stdlib_defs,
        packages,
        mcps,
        transitive,
    )
    .map_err(|error| error.into_diagnostics(parsed.file))
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

#[cfg(test)]
mod parsed_script_tests {
    use super::*;

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

        let parsed = parse_script("function main(): number { return 42; }", FileId(0));
        let internal = compile_parsed_script_timed_checked(
            "different source",
            "test.ts",
            &parsed,
            &[],
            &[],
            &[],
        )
        .expect_err("source metadata mismatch");
        assert!(matches!(
            internal.fatal,
            Some(CompilerFailure::Internal {
                stage: CompilerStage::Infer,
                ..
            })
        ));
    }

    #[test]
    fn external_imports_come_from_parsed_imports_only() {
        let parsed = parse_script(
            r#"
                // import fake from "@mcp/commented";
                const s: string = "import x from \"@acme/string\"";
                import { v4 } from "submilli:uuid";
                import linear from "@mcp/linear";
                import { answer } from "@acme/util";
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
            BTreeSet::from(["@acme/util".to_string()])
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
    const ACME: &str = "@acme/util";

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
                generics: Vec::new(),
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
                    index: None,
                    generics: Vec::new(),
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
                    generics: Vec::new(),
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
                generics: Vec::new(),
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

    fn acme_package() -> PackageDeclaration {
        let mut defs = PackageDeclaration::with_package(ACME);
        defs.values.insert(
            "greet".to_string(),
            ValueSymbol {
                name: "greet".to_string(),
                mangled_name: package_symbol(ACME, "greet"),
                declaration_span: Span::at(crate::FileId(0)),
                kind: ValueKind::Function {
                    generics: Vec::new(),
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
        let (_ta, diags, _timings) =
            super::front_end(source, &parsed, &stdlib_defs, packages).expect("front end");
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
        let acme = acme_package();
        let source = r#"
            import { greet } from "@acme/util";
            function main(): void {}
        "#;
        super::compile_script(source, "main.subm", crate::FileId(0), &[&acme], &[])
            .expect("package compiles");
    }

    #[test]
    fn package_not_passed_to_compile_uses_not_found_diagnostic() {
        let acme = acme_package();
        let source = r#"
            import { greet } from "@acme/util";
            function main(): void {}
        "#;
        let _package_not_supplied = acme;
        let errs = compile_errors(source, &[]);
        assert!(
            errs.iter()
                .any(|m| m.contains("package `@acme/util` not found")),
            "expected not-found diagnostic, got: {errs:?}"
        );
    }

    #[test]
    fn unknown_package_still_uses_not_found_diagnostic() {
        let source = r#"
            import { greet } from "@acme/missing";
            function main(): void {}
        "#;
        let errs = compile_errors(source, &[]);
        assert!(
            errs.iter()
                .any(|m| m.contains("package `@acme/missing` not found")),
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
        let (prelude, host, internal) = prelude::cached_runtime_package_declarations();
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
