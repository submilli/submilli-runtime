//! The [`FrontEnd`] trait: what turns source into a [`TypedAst`] for the
//! engine's back end.
//!
//! A front end parses and checks. The engine's back end (capture, desugar and
//! codegen) takes what it returns. The engine ships its own front end, and
//! ts-rust is another. Callers hold a `&dyn FrontEnd` and don't depend on
//! which one runs.
//!
//! Every implementation promises:
//!
//! * Its output is deterministic: the same request gives the same tree,
//!   declaration and diagnostics.
//! * It runs on a thread with the stack it needs, so callers don't size
//!   stacks.
//! * Its diagnostics are complete for the checks it owns. A result with an
//!   error diagnostic is an `Err`, and the warnings of an `Ok` are all it
//!   found.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    CompileError, Diagnostic, FileId, InternalModuleExports, ModulePath, PackageDeclaration,
    Sources, TypedAst,
};

/// A front end: parses scripts and checks scripts and packages.
///
/// The trait is object safe, so the server and the CLI can hold a
/// `&dyn FrontEnd`.
pub trait FrontEnd: Send + Sync {
    /// Name and version: what built a tree, for metadata and cache keys.
    fn id(&self) -> &str;

    /// Parse a script. Parsing never fails outright: a syntax error, or a
    /// limit the parse hit, is in [`ParsedScript::diagnostics`] and fails
    /// [`ParsedScript::check`].
    fn parse_script(&self, source: String, file: FileId) -> Box<dyn ParsedScript>;

    /// Parse and check a package's modules against its dependencies.
    fn check_package(&self, request: &PackageRequest<'_>) -> Result<CheckedPackage, CompileError>;
}

/// A parsed script, before any package is loaded.
pub trait ParsedScript: Send {
    /// The text [`FrontEnd::parse_script`] was given.
    fn source(&self) -> &str;

    /// The id [`FrontEnd::parse_script`] was given.
    fn file(&self) -> FileId;

    /// Syntax errors and warnings, before any package is loaded.
    fn diagnostics(&self) -> &[Diagnostic];

    /// Whether the parse reported an error, so [`ParsedScript::check`] fails.
    fn has_errors(&self) -> bool {
        self.diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.severity == crate::Severity::Error)
    }

    /// What the script imports, so the caller can load those packages.
    /// `stdlib` names the standard-library packages the embedder offers.
    fn imports(&self, stdlib: &[&PackageDeclaration]) -> Result<ScriptImports, CompileError>;

    /// Check against the loaded packages. Consumes the parse.
    fn check(self: Box<Self>, request: &ScriptRequest<'_>) -> Result<CheckedScript, CompileError>;
}

/// What a script imports from outside its own source.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScriptImports {
    pub stdlib: BTreeSet<String>,
    pub registry_packages: BTreeSet<String>,
    pub mcp_servers: BTreeSet<String>,
    /// The script imports `@tools`, the session's application tools.
    pub tools: bool,
    /// The script imports `@functions`, the harness-supplied function set.
    pub functions: bool,
}

/// The declarations a script is checked against.
#[derive(Clone, Copy, Debug, Default)]
pub struct ScriptRequest<'a> {
    /// Whose code the script is.
    pub owner: ScriptOwner<'a>,
    /// The prelude's declarations.
    pub prelude: &'a [&'a PackageDeclaration],
    /// The host's declarations.
    pub host: &'a [&'a PackageDeclaration],
    /// The standard-library packages the embedder offers.
    pub stdlib: &'a [&'a PackageDeclaration],
    /// The packages the script may import.
    pub dependencies: &'a [&'a PackageDeclaration],
    /// What the dependencies depend on in turn. Their types may appear in a
    /// dependency's surface, but the script can't import them.
    pub transitive: &'a [&'a PackageDeclaration],
    /// The `@mcp/<server>` virtual packages, which declare but emit no Wasm.
    pub mcp: &'a [&'a PackageDeclaration],
}

/// Whose code a script is. Its entry point stays `main` either way; the owner
/// decides whose gated calls it makes.
#[derive(Clone, Copy, Debug, Default)]
pub enum ScriptOwner<'a> {
    /// The program itself.
    #[default]
    Main,
    /// A package's: its code, its gated calls.
    Package(&'a str),
    /// A package's test file under `submilli build test`.
    Test(PackageTest<'a>),
}

impl<'a> ScriptOwner<'a> {
    /// The package the script belongs to, when it is not `main`.
    pub fn package(&self) -> Option<&'a str> {
        match self {
            Self::Main => None,
            Self::Package(name) => Some(name),
            Self::Test(test) => Some(test.owner.package_name.as_str()),
        }
    }

    /// The package test, when the script is one.
    pub fn test(&self) -> Option<PackageTest<'a>> {
        match self {
            Self::Test(test) => Some(*test),
            Self::Main | Self::Package(_) => None,
        }
    }
}

/// A package's test file: it may import the package's non-entry modules by
/// relative path (`../src/paths`).
#[derive(Clone, Copy, Debug)]
pub struct PackageTest<'a> {
    /// The package under test, which owns the script.
    pub owner: &'a PackageDeclaration,
    /// The non-entry modules the test may import.
    pub surface: &'a PackageTestSurface,
    /// The test file's path in the package directory, without its extension
    /// (`tests/paths.test`).
    pub importer: &'a ModulePath,
}

/// A checked script.
#[derive(Clone, Debug)]
pub struct CheckedScript {
    pub typed_ast: TypedAst,
    pub warnings: Vec<Diagnostic>,
}

/// One module of a package.
#[derive(Clone, Copy, Debug)]
pub struct PackageModule<'a> {
    pub path: &'a ModulePath,
    /// The module's id in [`PackageRequest::sources`], which holds its text.
    /// Every span in the module carries it.
    pub file: FileId,
}

/// A package to check.
#[derive(Clone, Copy)]
pub struct PackageRequest<'a> {
    pub name: &'a str,
    /// The module whose exports are the package's.
    pub root_module: &'a ModulePath,
    pub modules: &'a [PackageModule<'a>],
    /// The caller's registry of the modules' text. The back end resolves the
    /// checked tree's spans against the same registry.
    pub sources: &'a Sources,
    /// The prelude's declarations.
    pub prelude: &'a [&'a PackageDeclaration],
    /// The host's declarations.
    pub host: &'a [&'a PackageDeclaration],
    /// The standard-library packages the embedder offers.
    pub stdlib: &'a [&'a PackageDeclaration],
    /// The packages the package declares, the only ones it may import.
    pub dependencies: &'a [&'a PackageDeclaration],
    /// What the dependencies depend on in turn. A dependency's surface can
    /// name a class from one of them, so checking needs the declaration
    /// though the package can't import from it.
    pub transitive: &'a [&'a PackageDeclaration],
    /// Overrides for the root `config` module's values. `None` reads a
    /// module named `config` as an ordinary module.
    pub config: Option<&'a BTreeMap<String, String>>,
    pub surface: PackageSurface,
}

/// A checked package.
#[derive(Clone, Debug)]
pub struct CheckedPackage {
    pub typed_ast: TypedAst,
    /// The package's public surface. Codegen fills in its runtime functions,
    /// runtime globals and closure caches later; here they are empty.
    pub declaration: PackageDeclaration,
    pub warnings: Vec<Diagnostic>,
}

/// What a compiled package's Wasm exports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackageSurface {
    /// The entry module's exports: what programs and other packages link against.
    Public,
    /// Also every function, global and class static that a non-entry module
    /// exports, for the package's own tests under `submilli build test`. Never
    /// publish such a build: Wasm compiled elsewhere could import an internal
    /// helper and skip the checks the public functions run. A public build
    /// still exports every class's constructor and methods, internal classes'
    /// included, so keep secrets and checks out of internal classes. Internal
    /// functions escape in this build, so their Wasm signatures can be wider
    /// than the public build's.
    WithTestModules,
}

/// The exports of a package's non-entry modules, by path under `src/`, which a
/// [`PackageTest`] binds a test's relative imports to. The declaration,
/// capabilities, docs and authority map never include them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PackageTestSurface {
    pub modules: BTreeMap<ModulePath, InternalModuleExports>,
}
