//! The engine's own front end: its parser, type checker and Submilli rules,
//! behind [`FrontEnd`].

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use crate::compiler_error::{CompileError, CompilerStage};
use crate::typechecker::infer::{TestModule, TestModules};
use crate::{
    Asi, Ast, CheckedPackage, CheckedScript, Diagnostic, FileId, FrontEnd, ModulePath,
    PackageDeclaration, PackageRequest, PackageTest, ParsedScript, ScriptRequest, Severity,
    StmtKind, Token, TokenKind, lower_patterns,
};

/// The engine's front end.
#[derive(Clone, Copy, Debug, Default)]
pub struct EngineFrontEnd;

impl FrontEnd for EngineFrontEnd {
    /// Runs on the caller's thread: the parser bounds its own recursion and
    /// fits a runtime worker's 2 MiB stack (`tests/parser_depth.rs`).
    fn parse_script(&self, source: String, file: FileId) -> Box<dyn ParsedScript> {
        Box::new(parse(source, file))
    }

    fn check_package(&self, request: &PackageRequest<'_>) -> Result<CheckedPackage, CompileError> {
        crate::compiler_thread::run(|| check_package(request))
    }
}

/// The prelude's, the host's and the internal host modules' declarations,
/// built once.
#[derive(Clone, Copy, Debug)]
pub struct RuntimeDeclarations {
    pub prelude: &'static [&'static PackageDeclaration],
    pub host: &'static [&'static PackageDeclaration],
    /// Declarations codegen links against that no source can import.
    pub internal: &'static [&'static PackageDeclaration],
}

pub fn runtime_declarations() -> RuntimeDeclarations {
    static REFS: OnceLock<[Vec<&'static PackageDeclaration>; 3]> = OnceLock::new();
    let [prelude, host, internal] = REFS.get_or_init(|| {
        let (prelude, host, internal) =
            crate::runtime::prelude::cached_runtime_package_declarations();
        [
            prelude.iter().collect(),
            host.iter().collect(),
            internal.iter().collect(),
        ]
    });
    RuntimeDeclarations {
        prelude,
        host,
        internal,
    }
}

/// A [`ScriptRequest`] against the runtime's declarations and `stdlib`, with
/// nothing else to import.
pub fn script_request<'a>(stdlib: &'a [&'a PackageDeclaration]) -> ScriptRequest<'a> {
    let runtime = runtime_declarations();
    ScriptRequest {
        stdlib,
        ..ScriptRequest::new(runtime.prelude, runtime.host)
    }
}

/// What a script's rules resolve its types against, in the order inference
/// saw them: the runtime's, the stdlib's, what it imports, then the
/// transitive declarations.
pub(crate) fn script_rule_dependencies<'a>(
    request: &ScriptRequest<'a>,
) -> Vec<&'a PackageDeclaration> {
    let mut dependencies: Vec<&PackageDeclaration> = request
        .prelude
        .iter()
        .chain(request.host)
        .chain(request.stdlib)
        .copied()
        .collect();
    dependencies.extend(script_declarations(request));
    dependencies.extend_from_slice(request.transitive);
    dependencies
}

/// The declarations a script imports from besides the runtime's: its
/// dependencies, the MCP packages, and for a test file the package under test.
pub(crate) fn script_declarations<'a>(request: &ScriptRequest<'a>) -> Vec<&'a PackageDeclaration> {
    let mut declarations: Vec<&PackageDeclaration> = request
        .dependencies
        .iter()
        .chain(request.mcp)
        .copied()
        .collect();
    if let Some(test) = request.owner.test()
        && !declarations
            .iter()
            .any(|declaration| declaration.package_name == test.owner.package_name)
    {
        declarations.push(test.owner);
    }
    declarations
}

/// A script the engine parsed.
#[derive(Clone, Debug)]
pub(crate) struct EngineParsedScript {
    pub(crate) file: FileId,
    pub(crate) source: String,
    pub(crate) failure: Option<CompileError>,
    pub(crate) ast: Ast,
    pub(crate) diagnostics: Vec<Diagnostic>,
}

impl EngineParsedScript {
    /// Infer against `request`, without any rule pass. The diagnostics include
    /// errors; the tree is only fit to compile when none is.
    pub(crate) fn infer(
        &self,
        request: &ScriptRequest<'_>,
    ) -> Result<(crate::TypedAst, Vec<Diagnostic>), CompileError> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        let mut diags = self.diagnostics.clone();
        let declarations = script_declarations(request);
        let test = request.owner.test();
        let modules: BTreeMap<ModulePath, TestModule> = test
            .map(|test| {
                test.surface
                    .modules
                    .iter()
                    .map(|(path, exports)| (path.clone(), test_module(test.owner, exports)))
                    .collect()
            })
            .unwrap_or_default();
        if test.is_some() {
            crate::typechecker::infer::check_declaration_types(
                modules.values().map(|module| &module.declaration),
            )?;
        }
        let test_modules = test.map(
            |PackageTest {
                 owner, importer, ..
             }| TestModules {
                importer,
                owner: &owner.package_name,
                modules: &modules,
            },
        );
        let mut infer_refs: Vec<&PackageDeclaration> = request
            .prelude
            .iter()
            .chain(request.host)
            .chain(request.stdlib)
            .copied()
            .collect();
        infer_refs.extend(declarations);
        // `infer_refs` is the script's importable set; `transitive` only
        // resolves types.
        let (ta, infer_diags) = crate::typechecker::infer::infer_with_transitive_for_owner_checked(
            request.owner.package(),
            &self.source,
            crate::mangle::USER_PACKAGE,
            &self.ast,
            &infer_refs,
            request.transitive,
            test_modules,
        )
        .map_err(|mut error| {
            error.diagnostics.splice(0..0, diags.clone());
            error
        })?;
        diags.extend(infer_diags);
        Ok((ta, diags))
    }
}

impl EngineParsedScript {
    /// [`EngineParsedScript::infer`] followed by every rule, TypeScript's and
    /// Submilli's: the whole check, for tools that inspect the typed tree.
    pub(crate) fn infer_with_all_rules(
        &self,
        request: &ScriptRequest<'_>,
    ) -> Result<(crate::TypedAst, Vec<Diagnostic>), CompileError> {
        let (typed_ast, mut diagnostics) = self.infer(request)?;
        diagnostics.extend(
            crate::typechecker::rules::check_script(&typed_ast, &script_rule_dependencies(request))
                .map_err(|error| error.with_prior_diagnostics(&diagnostics))?,
        );
        Ok((typed_ast, diagnostics))
    }
}

#[cfg(test)]
impl EngineParsedScript {
    /// [`ParsedScript::imports`], classified against the core stdlib.
    pub(crate) fn external_imports(&self) -> Result<crate::ScriptImports, CompileError> {
        let stdlib = crate::runtime::stdlib_package_declarations();
        let stdlib_refs: Vec<&PackageDeclaration> = stdlib.iter().collect();
        Ok(crate::classify_imports(&self.imports()?, &stdlib_refs))
    }
}

impl ParsedScript for EngineParsedScript {
    fn source(&self) -> &str {
        &self.source
    }

    fn file(&self) -> FileId {
        self.file
    }

    fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    fn imports(&self) -> Result<Vec<String>, CompileError> {
        let mut specifiers: Vec<String> = Vec::new();
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for stmt_id in &self.ast.top_level {
            let StmtKind::Import { module, .. } = &self
                .ast
                .try_stmt(*stmt_id)
                .map_err(|error| {
                    CompileError::from(error.into_compiler_failure(CompilerStage::Parse))
                        .with_prior_diagnostics(&self.diagnostics)
                })?
                .kind
            else {
                continue;
            };
            if !crate::source::is_relative_specifier(module) && seen.insert(module) {
                specifiers.push(module.clone());
            }
        }
        Ok(specifiers)
    }

    fn check(self: Box<Self>, request: &ScriptRequest<'_>) -> Result<CheckedScript, CompileError> {
        crate::compiler_thread::run(|| {
            let (typed_ast, mut diagnostics) = self.infer(request)?;
            diagnostics.extend(
                crate::typechecker::rules::check_script_typescript(
                    &typed_ast,
                    &script_rule_dependencies(request),
                )
                .map_err(|error| error.with_prior_diagnostics(&diagnostics))?,
            );
            Ok(CheckedScript {
                typed_ast,
                diagnostics,
            })
        })
    }
}

/// Lex, parse and lower a script. A failure is kept in the result, never
/// returned, so the caller still sees the diagnostics before it.
pub(crate) fn parse(source: String, file: FileId) -> EngineParsedScript {
    let mut asi = Asi::new(&source, file);
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

    let (mut ast, parse_diags) = if failure.is_some() {
        (Ast::new(), Vec::new())
    } else {
        match crate::parser::parse_checked(&source, tokens, file) {
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

    EngineParsedScript {
        file,
        source,
        failure,
        ast,
        diagnostics,
    }
}

/// A module's exports as a declaration named after its package, so a test
/// binds them as it binds a package's.
fn test_module(owner: &PackageDeclaration, exports: &crate::InternalModuleExports) -> TestModule {
    let mut declaration = PackageDeclaration::with_package(owner.package_name.clone());
    declaration.values = exports.values.clone();
    declaration.types = exports.types.clone();
    declaration.refresh_shapes();
    TestModule {
        declaration,
        unexported: exports.unexported.clone(),
    }
}

fn check_package(request: &PackageRequest<'_>) -> Result<CheckedPackage, CompileError> {
    if let Some(config) = request.config
        && !request
            .modules
            .iter()
            .any(|module| module.path.as_str() == "config")
    {
        crate::package_config::resolve(&[], config, FileId(0))?;
    }
    // Public declarations may be constructed directly. Validate recursive
    // metadata before cloning them into the inference registries.
    crate::typechecker::infer::check_declaration_types(
        request
            .dependencies
            .iter()
            .chain(request.transitive)
            .copied(),
    )?;
    let mut parsed_modules = Vec::with_capacity(request.modules.len());
    let mut diagnostics = Vec::new();
    for module in request.modules {
        let source = request
            .sources
            .get(module.file)
            .ok_or_else(|| {
                CompileError::from(
                    crate::source::SourceError::UnknownFile { file: module.file }
                        .into_compiler_failure(CompilerStage::Parse),
                )
                .with_prior_diagnostics(&diagnostics)
            })?
            .text();
        let (ast, mut module_diags) = if let Some(config) = request.config
            && module.path.as_str() == "config"
        {
            parse_config_module(source, module.file, config)
        } else {
            parse_package_module(source, module.file)
        }
        .map_err(|mut error| {
            error.diagnostics.splice(0..0, diagnostics.clone());
            error
        })?;
        diagnostics.append(&mut module_diags);
        parsed_modules.push((module.path.clone(), module.file, ast));
    }
    if has_errors(&diagnostics) {
        return Err(diagnostics.into());
    }

    let module_refs: Vec<_> = parsed_modules
        .iter()
        .map(|(path, file, ast)| (path.clone(), *file, ast))
        .collect();
    let mut external_packages = external_packages(request.prelude, request.host, request.stdlib);
    for defs in request.dependencies {
        external_packages.insert(defs.package_name.clone(), (*defs).clone());
    }
    let transitive_packages: BTreeMap<String, PackageDeclaration> = request
        .transitive
        .iter()
        .filter(|defs| !external_packages.contains_key(&defs.package_name))
        .map(|defs| (defs.package_name.clone(), (*defs).clone()))
        .collect();
    let (typed_ast, declaration, mut package_diags, inferred) =
        crate::typechecker::infer::infer_package_typescript(
            request.name,
            request.root_module.clone(),
            module_refs,
            request.sources,
            external_packages,
            transitive_packages,
        )
        .map_err(|mut error| {
            error.diagnostics.splice(0..0, diagnostics.clone());
            error
        })?;
    diagnostics.append(&mut package_diags);
    // Inference that stopped before building the whole tree leaves nothing
    // for the engine's rules or the back end: the errors are the result.
    if inferred == crate::typechecker::infer::Inferred::Stopped {
        return Err(diagnostics.into());
    }
    Ok(CheckedPackage {
        typed_ast,
        declaration,
        diagnostics,
    })
}

/// The packages every package may import without declaring them: the stdlib's,
/// the prelude's and the host's, by name.
pub(crate) fn external_packages(
    prelude: &[&PackageDeclaration],
    host: &[&PackageDeclaration],
    stdlib: &[&PackageDeclaration],
) -> BTreeMap<String, PackageDeclaration> {
    stdlib
        .iter()
        .chain(prelude)
        .chain(host)
        .map(|defs| (defs.package_name.clone(), (*defs).clone()))
        .collect()
}

pub(crate) fn parse_config_module(
    source: &str,
    file: FileId,
    config: &BTreeMap<String, String>,
) -> Result<(Ast, Vec<Diagnostic>), CompileError> {
    let tokens = crate::package_config::resolved_tokens(source, file, config)?;
    let (ast, diagnostics) = crate::parser::parse_checked(source, tokens, file)?;
    Ok((ast, diagnostics))
}

pub(crate) fn parse_package_module(
    source: &str,
    file: FileId,
) -> Result<(Ast, Vec<Diagnostic>), CompileError> {
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

pub(crate) fn has_errors(diags: &[Diagnostic]) -> bool {
    diags.iter().any(|d| d.severity == Severity::Error)
}

pub(crate) fn warnings_only(diags: Vec<Diagnostic>) -> Vec<Diagnostic> {
    diags
        .into_iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect()
}
