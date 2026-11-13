//! Embedder-supplied runtime Functions (`@functions` and direct host `invoke`).
//!
//! Each [`FunctionDefinition`] compiles as an isolated module named
//! `@functions/<name>` with `submilli:functions` (`escalate`) in scope, derives
//! its required capabilities, and projects its `export function run(...)` into a
//! combined `@functions` [`PackageDeclaration`] under a camelCase binding
//! (`lead-scan` -> `leadScan`). At runtime, frames inside `@functions/<name>`
//! resolve to the `"main"` principal so functions never amplify caller
//! permissions and never receive package-scoped secrets.
//!
//! # Embedder workflow
//!
//! ```no_run
//! use submilli_engine::docs_view::Unscoped;
//! use submilli_engine::packages::DocAudience;
//! use submilli_engine::runtime::{
//!     StoreData, Vfs, install_package_modules_async, install_runtime_async_for,
//! };
//! use submilli_engine::compile::compile_parsed_script;
//! use submilli_engine::{
//!     EngineFrontEnd, FileId, FrontEnd, FunctionCache, FunctionDefinition, FunctionError,
//!     FunctionSet, OptionalPackage, PackageDeclaration, RuntimeConfig, ScriptRequest,
//!     Stdlib, compile_function_set_for, dispatch_main_async,
//!     install_functions_async, instantiate_program_async, invoke_function_async, script_request,
//! };
//! use wasmtime::{Linker, Module};
//!
//! # async fn example() -> wasmtime::Result<()> {
//! let cfg = RuntimeConfig::default();
//! let engine = cfg.engine()?;
//! let cache = FunctionCache::new();
//! let stdlib = Stdlib::core().with(OptionalPackage::Agents);
//!
//! // 1 & 2. Validate a single function at apply time or assemble a session FunctionSet.
//! let set = FunctionSet::new(vec![FunctionDefinition {
//!     name: "lead-scan".into(),
//!     description: "Scan daily leads.".into(),
//!     source: "export function run(limit: number): number { return limit + 1; }".into(),
//! }]);
//! let compiled_set = compile_function_set_for(stdlib, &engine, &set, &[], &[], Some(&cache))?;
//! let _requires = compiled_set.get("lead-scan").unwrap().requires();
//!
//! // 3. Directly invoke a function from the host with JSON-encoded arguments.
//! let mut store = cfg.store(&engine, StoreData::with_vfs("run-1".into(), Vfs::tempdir()?))?;
//! match invoke_function_async(&mut store, &compiled_set, &[], "lead-scan", &["10"]).await {
//!     Ok(Some(json)) => assert_eq!(json, "11"),
//!     Ok(None) => {}
//!     Err(FunctionError::Escalated { reason: _ }) => {}
//!     Err(other) => return Err(other.into()),
//! }
//!
//! // 4 & 5. Project `@functions` docs/search and link `@functions` into LLM `main.ts` code.
//! let view = compiled_set.view(&Unscoped);
//! let _docs = view.docs_for("@functions", DocAudience::Agent);
//! let _hits = view.search_functions("lead");
//!
//! let main_src =
//!     r#"import { leadScan } from "@functions"; function main(): number { return leadScan(4); }"#;
//! let stdlib = compiled_set.stdlib().package_declarations();
//! let stdlib: Vec<&PackageDeclaration> = stdlib.iter().collect();
//! let parsed = EngineFrontEnd.parse_script(main_src.to_owned(), FileId(0));
//! let main = compile_parsed_script(
//!     parsed,
//!     "main.ts",
//!     &ScriptRequest {
//!         dependencies: &[compiled_set.package_declaration()],
//!         ..script_request(&stdlib)
//!     },
//! )
//! .expect("compile main");
//! let main_module = Module::new(&engine, &main.wasm)?;
//! let mut store = cfg.store(&engine, StoreData::with_vfs("run-2".into(), Vfs::tempdir()?))?;
//! let mut linker = Linker::new(&engine);
//! install_runtime_async_for(&mut linker, &mut store, compiled_set.stdlib()).await?;
//! install_package_modules_async(&mut linker, &mut store, &[]).await?;
//! install_functions_async(&mut linker, &mut store, &compiled_set).await?;
//! store.data_mut().install_type_info(main.type_info);
//! let instance = instantiate_program_async(&linker, &mut store, &main_module).await?;
//! let _out = dispatch_main_async(&mut store, &instance).await?;
//! # Ok(())
//! # }
//! ```

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};
use wasmtime::{Engine, Instance, Linker, Module, Store, Val};

use crate::backtrace::ThrownError;
use crate::docs_view::{DocAvailability, project_declaration, select_documentation};
use crate::packages::{
    DocAudience, ModuleDoc, ModuleSummary, render_declarations_for, with_public_type_names,
};
use crate::runtime::host::Denial;
use crate::runtime::json::JsonUnknownAllocator;
use crate::runtime::json_text::JsonValue;
use crate::runtime::limits::HostBytes;
use crate::runtime::{
    LinkedPackageModule, StoreData, exec, fuel, host, install_package_modules_async,
    install_runtime_async_for, name_failing_initializer,
};
use crate::stdlib::Stdlib;
use crate::stdlib::functions::Escalated;
use crate::{
    CompiledPackage, DerivedCapability, Diagnostic, DocCapability, DocCapabilityBinding,
    DocCapabilityBindingKind, DocComment, FileId, FunctionArgumentCheck, FunctionInvokePlan,
    LineIndex, ModulePath, PackageDeclaration, Param, Severity, Span, TypeInfoTable, ValueKind,
    ValueSymbol,
};

/// Package specifier guest scripts import harness functions from.
pub const FUNCTIONS_PACKAGE: &str = "@functions";

/// Prefix of each compiled function's internal Wasm module name
/// (`@functions/<name>`).
pub const FUNCTIONS_MODULE_PREFIX: &str = "@functions/";

/// Standard-library specifier available exclusively inside function sources.
pub const FUNCTIONS_STDLIB_MODULE: &str = crate::stdlib::functions::MODULE_NAME;

/// Maximum byte length of a function's kebab-case name.
pub const MAX_FUNCTION_NAME_LEN: usize = 64;

const FUNCTIONS_MODULE_DESCRIPTION: &str =
    "Harness-supplied functions compiled for this run, called under caller permissions.";

/// One embedder-supplied function source before compilation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionDefinition {
    /// Lowercase kebab-case name (`lead-scan`), `1..=64` ASCII bytes.
    pub name: String,
    /// Human- and agent-facing summary rendered in `@functions` discovery.
    pub description: String,
    /// TypeScript / Submilli module source exporting `function run(...)`.
    pub source: String,
}

/// Ordered set of functions supplied by an embedder for a workspace or session.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FunctionSet {
    pub functions: Vec<FunctionDefinition>,
}

impl FunctionSet {
    pub fn new(functions: Vec<FunctionDefinition>) -> Self {
        Self { functions }
    }

    pub fn is_empty(&self) -> bool {
        self.functions.is_empty()
    }
}

/// Why a function's kebab-case `name` is invalid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FunctionNameError {
    Empty,
    TooLong { len: usize, max: usize },
    InvalidFormat,
    ReservedExportName { export_name: String },
}

impl std::fmt::Display for FunctionNameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "function name must not be empty"),
            Self::TooLong { len, max } => {
                write!(f, "function name is {len} bytes (maximum is {max})")
            }
            Self::InvalidFormat => write!(
                f,
                "function name must be lowercase kebab-case (`[a-z][a-z0-9]*(-[a-z0-9]+)*`)"
            ),
            Self::ReservedExportName { export_name } => write!(
                f,
                "function export name `{export_name}` is a reserved identifier and cannot be imported as a binding"
            ),
        }
    }
}

impl std::error::Error for FunctionNameError {}

/// Validate a function's kebab-case name and return its camelCase `@functions`
/// export name (`"lead-scan"` -> `"leadScan"`).
pub fn validate_function_name(name: &str) -> Result<String, FunctionNameError> {
    if name.is_empty() {
        return Err(FunctionNameError::Empty);
    }
    if name.len() > MAX_FUNCTION_NAME_LEN {
        return Err(FunctionNameError::TooLong {
            len: name.len(),
            max: MAX_FUNCTION_NAME_LEN,
        });
    }
    let bytes = name.as_bytes();
    if !bytes[0].is_ascii_lowercase() {
        return Err(FunctionNameError::InvalidFormat);
    }
    let mut prev_hyphen = false;
    for &b in &bytes[1..] {
        if b == b'-' {
            if prev_hyphen {
                return Err(FunctionNameError::InvalidFormat);
            }
            prev_hyphen = true;
        } else if b.is_ascii_lowercase() || b.is_ascii_digit() {
            prev_hyphen = false;
        } else {
            return Err(FunctionNameError::InvalidFormat);
        }
    }
    if prev_hyphen {
        return Err(FunctionNameError::InvalidFormat);
    }

    let export_name = kebab_to_camel(name);
    if is_reserved_binding_name(&export_name) {
        return Err(FunctionNameError::ReservedExportName { export_name });
    }
    Ok(export_name)
}

/// Compile-time failure when building a [`FunctionSet`].
#[derive(Clone, Debug)]
pub enum FunctionCompileFailure {
    InvalidName {
        function: String,
        error: FunctionNameError,
    },
    DuplicateName {
        function: String,
    },
    ExportNameCollision {
        first: String,
        second: String,
        export_name: String,
    },
    ReservedDependency {
        package: String,
    },
    InvalidDependency {
        message: String,
    },
    MissingRunExport {
        function: String,
        line: usize,
        message: String,
    },
    Compile {
        function: String,
        line: usize,
        diagnostics: Vec<Diagnostic>,
        rendered: String,
    },
    ModuleBuild {
        function: String,
        message: String,
    },
}

impl FunctionCompileFailure {
    /// The offending function's kebab-case name, when the failure is attributable
    /// to one function in the set.
    pub fn function_name(&self) -> Option<&str> {
        match self {
            Self::InvalidName { function, .. }
            | Self::DuplicateName { function }
            | Self::MissingRunExport { function, .. }
            | Self::Compile { function, .. }
            | Self::ModuleBuild { function, .. } => Some(function.as_str()),
            Self::ExportNameCollision { second, .. } => Some(second.as_str()),
            Self::ReservedDependency { .. } | Self::InvalidDependency { .. } => None,
        }
    }

    /// 1-based source line number for source/contract errors on a function.
    pub fn line(&self) -> Option<usize> {
        match self {
            Self::MissingRunExport { line, .. } | Self::Compile { line, .. } => Some(*line),
            Self::InvalidName { .. }
            | Self::DuplicateName { .. }
            | Self::ExportNameCollision { .. }
            | Self::ReservedDependency { .. }
            | Self::InvalidDependency { .. }
            | Self::ModuleBuild { .. } => None,
        }
    }
}

impl std::fmt::Display for FunctionCompileFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidName { function, error } => {
                write!(f, "invalid function name `{function}`: {error}")
            }
            Self::DuplicateName { function } => {
                write!(f, "duplicate function name `{function}` in function set")
            }
            Self::ExportNameCollision {
                first,
                second,
                export_name,
            } => write!(
                f,
                "functions `{first}` and `{second}` both map to `@functions` export `{export_name}`"
            ),
            Self::ReservedDependency { package } => write!(
                f,
                "package `{package}` uses a reserved runtime principal name and cannot be supplied as a function dependency"
            ),
            Self::InvalidDependency { message } => {
                write!(f, "invalid function dependency declaration: {message}")
            }
            Self::MissingRunExport {
                function,
                line,
                message,
            } => write!(f, "function `{function}` (line {line}): {message}"),
            Self::Compile {
                function,
                line,
                rendered,
                ..
            } => write!(f, "function `{function}` (line {line}):\n{rendered}"),
            Self::ModuleBuild { function, message } => {
                write!(
                    f,
                    "function `{function}` failed Wasm module validation: {message}"
                )
            }
        }
    }
}

impl std::error::Error for FunctionCompileFailure {}

/// Content-addressed cache of compiled function artifacts.
///
/// Keyed by compiler version, [`Stdlib`] selection, function name, source hash,
/// and dependency declaration hashes so unchanged functions skip front-end
/// inference, capability derivation, and Wasm codegen across turns.
#[derive(Clone, Default)]
pub struct FunctionCache {
    // Poisoning means a thread panicked while updating an in-memory cache map;
    // AGENTS.md permits panicking on poisoned std::sync::Mutex access.
    inner: Arc<Mutex<FunctionCacheInner>>,
}

#[derive(Default)]
struct FunctionCacheInner {
    entries: HashMap<[u8; 32], Arc<CachedFunctionArtifact>>,
    hits: u64,
    misses: u64,
}

struct CachedFunctionArtifact {
    compiled: CompiledPackage,
    invoke: RuntimeInvokePlan,
}

impl FunctionCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.lock().entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Number of per-function cache hits observed by this cache.
    pub fn hits(&self) -> u64 {
        self.lock().hits
    }

    /// Number of per-function cache misses (fresh compilations) observed.
    pub fn misses(&self) -> u64 {
        self.lock().misses
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, FunctionCacheInner> {
        // Poisoning means a panic may have left `entries` and hit/miss counters
        // partly updated; AGENTS.md permits propagating poisoned Mutex panics.
        self.inner.lock().expect("function cache lock poisoned")
    }
}

/// Metadata and compiled artifact for one function in a [`CompiledFunctionSet`].
#[derive(Clone)]
pub struct CompiledFunction {
    name: String,
    export_name: String,
    description: String,
    module_name: String,
    module: Module,
    wasm: Vec<u8>,
    type_info: TypeInfoTable,
    requires: Vec<DerivedCapability>,
    warnings: Vec<Diagnostic>,
    invoke: RuntimeInvokePlan,
}

#[derive(Clone)]
enum RuntimeInvokePlan {
    Generic,
    Callable {
        min_args: usize,
        fixed: Vec<RuntimeParamPlan>,
        rest: Option<RuntimeParamPlan>,
    },
}

#[derive(Clone)]
struct RuntimeParamPlan {
    name: String,
    index: usize,
    cast: RuntimeParamCast,
}

#[derive(Clone)]
enum RuntimeParamCast {
    Checked,
    Uncastable { ty_display: String, reason: String },
}

impl CompiledFunction {
    /// Original kebab-case function name (`"lead-scan"`).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Exported camelCase identifier in `@functions` (`"leadScan"`).
    pub fn export_name(&self) -> &str {
        &self.export_name
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    /// Internal Wasm module name (`"@functions/lead-scan"`).
    pub fn module_name(&self) -> &str {
        &self.module_name
    }

    pub fn wasm(&self) -> &[u8] {
        &self.wasm
    }

    pub fn module(&self) -> &Module {
        &self.module
    }

    /// Capabilities statically derived from this function's call graph.
    pub fn requires(&self) -> &[DerivedCapability] {
        &self.requires
    }

    pub fn warnings(&self) -> &[Diagnostic] {
        &self.warnings
    }
}

impl std::fmt::Debug for CompiledFunction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompiledFunction")
            .field("name", &self.name)
            .field("export_name", &self.export_name)
            .field("module_name", &self.module_name)
            .field("requires", &self.requires)
            .finish()
    }
}

/// Compiled `@functions` set ready for guest linking, capability-filtered
/// discovery, and direct host invocation.
#[derive(Clone)]
pub struct CompiledFunctionSet {
    stdlib: Stdlib,
    functions: Vec<CompiledFunction>,
    by_name: BTreeMap<String, usize>,
    by_export: BTreeMap<String, usize>,
    package_declaration: PackageDeclaration,
}

impl std::fmt::Debug for CompiledFunctionSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompiledFunctionSet")
            .field("stdlib", &self.stdlib)
            .field("functions", &self.functions)
            .finish()
    }
}

impl CompiledFunctionSet {
    pub fn stdlib(&self) -> Stdlib {
        self.stdlib
    }

    pub fn is_empty(&self) -> bool {
        self.functions.is_empty()
    }

    pub fn len(&self) -> usize {
        self.functions.len()
    }

    pub fn functions(&self) -> &[CompiledFunction] {
        &self.functions
    }

    /// Look up a compiled function by its kebab-case `name` (`"lead-scan"`) or
    /// its camelCase `@functions` export name (`"leadScan"`).
    pub fn get(&self, name: &str) -> Option<&CompiledFunction> {
        self.by_name
            .get(name)
            .or_else(|| self.by_export.get(name))
            .map(|&idx| &self.functions[idx])
    }

    /// Combined `@functions` [`PackageDeclaration`] to pass in `packages` when
    /// compiling a guest program that imports `@functions`.
    pub fn package_declaration(&self) -> &PackageDeclaration {
        &self.package_declaration
    }

    /// Narrow `@functions` to the functions whose derived capabilities are
    /// available to a reader under `availability`.
    pub fn view(&self, availability: &dyn DocAvailability) -> FunctionsView<'_> {
        let mut declaration =
            project_declaration(&self.package_declaration, availability).declaration;
        // `project_declaration` keeps standalone types that no function referenced
        // (`exported_alone`). Prune any standalone type owned by a function that
        // capability projection hid so denied functions leave no trace in `types`.
        let visible_values = &declaration.values;
        declaration
            .types
            .retain(|_, sym| self.is_type_owner_visible(visible_values, &sym.mangled_name));
        FunctionsView {
            set: self,
            declaration,
        }
    }

    fn is_type_owner_visible(
        &self,
        visible_values: &BTreeMap<String, ValueSymbol>,
        mangled: &crate::MangledName,
    ) -> bool {
        let Some(owner_module) = function_owner_module(mangled.as_str()) else {
            return true;
        };
        let Some(fn_name) = owner_module.strip_prefix(FUNCTIONS_MODULE_PREFIX) else {
            return true;
        };
        let Some(func) = self.get(fn_name) else {
            return false;
        };
        visible_values.contains_key(&func.export_name)
    }
}

/// Summary of a single visible function in a [`FunctionsView`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionSummary {
    pub name: String,
    pub export_name: String,
    pub description: String,
}

/// Capability-projected view of a [`CompiledFunctionSet`] for `docs` and
/// `search`.
///
/// Unlike stdlib packages (where ungated helpers like `fs.cwd()` hide when every
/// gated operation in the module is denied), each function in `@functions` is an
/// independent unit: `@functions` stays visible whenever at least one function
/// in the set remains callable by the reader.
pub struct FunctionsView<'a> {
    set: &'a CompiledFunctionSet,
    declaration: PackageDeclaration,
}

impl<'a> FunctionsView<'a> {
    /// Whether at least one function in `@functions` is available to the reader.
    pub fn is_usable(&self) -> bool {
        !self.declaration.values.is_empty()
    }

    /// Narrowed `@functions` declaration containing only callable functions and
    /// the exported types they reach.
    pub fn declaration(&self) -> &PackageDeclaration {
        &self.declaration
    }

    /// Render documentation for `"@functions"` or a single function
    /// (`"@functions/lead-scan"` or `"@functions/leadScan"`). Returns `None`
    /// when unknown or hidden by capability availability.
    pub fn docs(&self, name: &str) -> Option<ModuleDoc> {
        self.docs_for(name, DocAudience::Author)
    }

    /// [`Self::docs`] rendered for `audience` (`Author` vs `Agent`).
    pub fn docs_for(&self, name: &str, audience: DocAudience) -> Option<ModuleDoc> {
        if name == FUNCTIONS_PACKAGE {
            if !self.is_usable() {
                return None;
            }
            return Some(ModuleDoc {
                name: FUNCTIONS_PACKAGE.to_string(),
                description: FUNCTIONS_MODULE_DESCRIPTION.to_string(),
                declarations: render_declarations_for(&self.declaration, audience),
            });
        }
        let target = name.strip_prefix(FUNCTIONS_MODULE_PREFIX)?;
        let func = self.set.get(target)?;
        if !self.declaration.values.contains_key(&func.export_name) {
            return None;
        }
        let single = single_function_declaration(&self.declaration, func)?;
        Some(ModuleDoc {
            name: format!("{FUNCTIONS_MODULE_PREFIX}{}", func.name),
            description: func.description.clone(),
            declarations: render_declarations_for(&single, audience),
        })
    }

    /// Module-level search matching [`StdlibView::search`](crate::packages::StdlibView::search):
    /// returns `@functions` when at least one function is usable and `query`
    /// matches `@functions`, its summary, or any visible function's kebab name,
    /// camelCase export name, description, or reachable type.
    pub fn search(&self, query: &str) -> Vec<ModuleSummary> {
        if !self.is_usable() {
            return Vec::new();
        }
        let q = query.trim().to_lowercase();
        if q.is_empty()
            || FUNCTIONS_PACKAGE.contains(&q)
            || FUNCTIONS_MODULE_DESCRIPTION.to_lowercase().contains(&q)
            || !self.search_functions(query).is_empty()
            || self
                .declaration
                .types
                .keys()
                .any(|ty| ty.to_lowercase().contains(&q))
        {
            vec![ModuleSummary {
                name: FUNCTIONS_PACKAGE.to_string(),
                description: FUNCTIONS_MODULE_DESCRIPTION.to_string(),
            }]
        } else {
            Vec::new()
        }
    }

    /// Per-function search over the functions visible in this view.
    pub fn search_functions(&self, query: &str) -> Vec<FunctionSummary> {
        let q = query.trim().to_lowercase();
        self.set
            .functions
            .iter()
            .filter(|func| self.declaration.values.contains_key(&func.export_name))
            .filter(|func| {
                q.is_empty()
                    || func.name.to_lowercase().contains(&q)
                    || func.export_name.to_lowercase().contains(&q)
                    || func.description.to_lowercase().contains(&q)
            })
            .map(|func| FunctionSummary {
                name: func.name.clone(),
                export_name: func.export_name.clone(),
                description: func.description.clone(),
            })
            .collect()
    }
}

/// Compile a [`FunctionSet`] against [`Stdlib::core`] and optional linked
/// package declarations.
pub fn compile_function_set(
    engine: &Engine,
    set: &FunctionSet,
    dependencies: &[&PackageDeclaration],
    transitive: &[&PackageDeclaration],
    cache: Option<&FunctionCache>,
) -> Result<CompiledFunctionSet, FunctionCompileFailure> {
    compile_function_set_for(Stdlib::core(), engine, set, dependencies, transitive, cache)
}

/// [`compile_function_set`] for an embedder that enables optional stdlib
/// packages (`submilli:agents`, `submilli:skills`) in `stdlib`.
///
/// The functions compile one after another on one compiler thread, so the
/// caller needn't size its stack.
pub fn compile_function_set_for(
    stdlib: Stdlib,
    engine: &Engine,
    set: &FunctionSet,
    dependencies: &[&PackageDeclaration],
    transitive: &[&PackageDeclaration],
    cache: Option<&FunctionCache>,
) -> Result<CompiledFunctionSet, FunctionCompileFailure> {
    for defs in dependencies.iter().chain(transitive.iter()) {
        if defs.package_name == crate::mangle::USER_PACKAGE
            || is_functions_module_name(&defs.package_name)
        {
            return Err(FunctionCompileFailure::ReservedDependency {
                package: defs.package_name.clone(),
            });
        }
    }

    // Validate caller-constructed `PackageDeclaration`s against recursion/size
    // bounds before serializing them for cache digests or handing them to
    // compiler passes (even when `set` is empty or hits `FunctionCache`).
    crate::typechecker::infer::check_declaration_types(
        dependencies.iter().chain(transitive.iter()).copied(),
    )
    .map_err(|failure| FunctionCompileFailure::InvalidDependency {
        message: failure.to_string(),
    })?;

    let mut seen_names = BTreeSet::new();
    let mut seen_exports: BTreeMap<String, String> = BTreeMap::new();
    let mut validated_names = Vec::with_capacity(set.functions.len());
    for def in &set.functions {
        let export_name = validate_function_name(&def.name).map_err(|error| {
            FunctionCompileFailure::InvalidName {
                function: def.name.clone(),
                error,
            }
        })?;
        if !seen_names.insert(def.name.clone()) {
            return Err(FunctionCompileFailure::DuplicateName {
                function: def.name.clone(),
            });
        }
        if let Some(first) = seen_exports.insert(export_name.clone(), def.name.clone()) {
            return Err(FunctionCompileFailure::ExportNameCollision {
                first,
                second: def.name.clone(),
                export_name,
            });
        }
        validated_names.push(export_name);
    }

    let deps_digest = dependency_declarations_digest(stdlib, dependencies, transitive)?;
    // One compiler thread for the whole set: each function's compile runs in
    // place on it.
    let (compiled_functions, fn_declarations) = crate::compiler_thread::on_compiler_stack(|| {
        let mut compiled_functions = Vec::with_capacity(set.functions.len());
        let mut fn_declarations = Vec::with_capacity(set.functions.len());
        for (def, export_name) in set.functions.iter().zip(validated_names) {
            let (compiled_fn, fn_decl) = compile_single_function(
                stdlib,
                engine,
                def,
                export_name,
                dependencies,
                transitive,
                &deps_digest,
                cache,
            )?;
            compiled_functions.push(compiled_fn);
            fn_declarations.push(fn_decl);
        }
        Ok((compiled_functions, fn_declarations))
    })
    .map_err(|error| {
        compile_error_to_function_failure(
            FUNCTIONS_PACKAGE,
            "",
            crate::compiler_error::CompilerFailure::Unavailable {
                message: error.to_string(),
            }
            .into(),
        )
    })??;

    let package_declaration =
        build_functions_package_declaration(&compiled_functions, &fn_declarations);
    let mut by_name = BTreeMap::new();
    let mut by_export = BTreeMap::new();
    for (idx, func) in compiled_functions.iter().enumerate() {
        by_name.insert(func.name.clone(), idx);
        by_export.insert(func.export_name.clone(), idx);
    }

    Ok(CompiledFunctionSet {
        stdlib,
        functions: compiled_functions,
        by_name,
        by_export,
        package_declaration,
    })
}

/// Instantiate every function in `set` into `store` and register both its
/// `@functions/<name>` module instance and its exports under `@functions` on
/// `linker` so a guest `main` module importing `@functions` can instantiate
/// directly against it with zero JSON marshalling.
pub async fn install_functions_async(
    linker: &mut Linker<StoreData>,
    store: &mut Store<StoreData>,
    set: &CompiledFunctionSet,
) -> wasmtime::Result<()> {
    crate::stdlib::functions::install(linker)?;
    for func in &set.functions {
        store.data_mut().install_type_info(func.type_info.clone());
        let outcome = linker.instantiate_async(&mut *store, &func.module).await;
        let instance = name_failing_initializer(store, "function", &func.name, outcome)?;
        linker.instance(&mut *store, &func.module_name, instance)?;
        for export in func.module.exports() {
            // Forward both package-qualified (`@functions/<name>#...`) and
            // module-qualified (`mod:@functions/<name>#...`) exports onto
            // `@functions` so unexported base/derived classes with narrowing
            // checks link cleanly without colliding across functions.
            if function_owner_module(export.name()) == Some(func.module_name.as_str())
                && let Some(ext) = instance.get_export(&mut *store, export.name())
            {
                linker.define(&mut *store, FUNCTIONS_PACKAGE, export.name(), ext)?;
            }
        }
    }
    Ok(())
}

/// Outcome failure when the host directly invokes a function by name via
/// [`invoke_function_async`].
#[derive(Debug)]
pub enum FunctionError {
    /// No function of that name exists in the set.
    Unavailable(String),
    /// Wrong argument count, malformed JSON argument, uncastable parameter type,
    /// generic `run<T>`, or runtime parameter shape mismatch — checked before
    /// `run` executes.
    InvalidArguments(String),
    /// The function called `escalate(reason)` from `submilli:functions`.
    Escalated { reason: String },
    /// A gated capability check was denied (attributed to `"main"` when called
    /// directly by the function, or `@org/pkg` inside a linked package).
    Denied(Denial),
    /// Fuel, timeout deadline, or store memory ran out.
    BudgetExhausted(wasmtime::Error),
    /// An initializer or `run` threw an uncaught exception or trapped.
    Failed(wasmtime::Error),
}

impl std::fmt::Display for FunctionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(message) => write!(f, "function unavailable: {message}"),
            Self::InvalidArguments(message) => write!(f, "invalid function arguments: {message}"),
            Self::Escalated { reason } => write!(f, "function escalated: {reason}"),
            Self::Denied(denial) => write!(
                f,
                "function denied `{}` for `{}`",
                denial.capability, denial.caller
            ),
            Self::BudgetExhausted(error) => write!(f, "function budget exhausted: {error}"),
            Self::Failed(error) => write!(f, "function failed: {error}"),
        }
    }
}

impl std::error::Error for FunctionError {}

/// Directly invoke a named function (`"lead-scan"` or `"leadScan"`) from the
/// host in a fresh `store`, supplying positional JSON-encoded arguments and
/// returning the JSON-encoded result (`Ok(None)` when `run` returns `void` or
/// `undefined`).
///
/// Validates function existence, arity, JSON syntax, and parameter runtime types
/// before `run` executes. Preserves `store.data().effects` so durable replay
/// works identically to `main` execution.
pub async fn invoke_function_async<S: AsRef<str>>(
    store: &mut Store<StoreData>,
    set: &CompiledFunctionSet,
    packages: &[LinkedPackageModule<'_>],
    name: &str,
    args_json: &[S],
) -> Result<Option<String>, FunctionError> {
    let func = set.get(name).ok_or_else(|| {
        FunctionError::Unavailable(format!("function `{name}` is not in the function set"))
    })?;

    let RuntimeInvokePlan::Callable {
        min_args,
        fixed,
        rest,
    } = &func.invoke
    else {
        return Err(FunctionError::InvalidArguments(format!(
            "function `{}` has type parameters on `run` and cannot be invoked directly with JSON arguments",
            func.name
        )));
    };

    let supplied = args_json.len();
    if supplied < *min_args {
        return Err(FunctionError::InvalidArguments(format!(
            "function `{}` requires at least {min_args} argument{}, got {supplied}",
            func.name,
            if *min_args == 1 { "" } else { "s" }
        )));
    }
    if rest.is_none() && supplied > fixed.len() {
        let max_args = fixed.len();
        return Err(FunctionError::InvalidArguments(format!(
            "function `{}` accepts at most {max_args} argument{}, got {supplied}",
            func.name,
            if max_args == 1 { "" } else { "s" }
        )));
    }

    // Reject uncastable parameter slots populated by this call before charging,
    // parsing, or instantiating any Wasm module.
    for param in fixed.iter().take(supplied) {
        reject_uncastable_param(&func.name, param)?;
    }
    if supplied > fixed.len()
        && let Some(rest_param) = rest
    {
        reject_uncastable_param(&func.name, rest_param)?;
    }

    // Account transient host memory and CPU fuel for UTF-16 conversion and JSON
    // parsing against `store` before allocating `Vec<u16>` or `JsonValue` trees.
    let (parsed_args, input_bytes_guard) = parse_metered_json_args(store, &func.name, args_json)?;

    let mut linker = Linker::new(store.engine());
    install_runtime_async_for(&mut linker, store, set.stdlib())
        .await
        .map_err(classify_function_error)?;
    crate::stdlib::functions::install(&mut linker).map_err(classify_function_error)?;
    install_package_modules_async(&mut linker, store, packages)
        .await
        .map_err(classify_function_error)?;

    store.data_mut().install_type_info(func.type_info.clone());
    let init_outcome = linker.instantiate_async(&mut *store, &func.module).await;
    let instance = name_failing_initializer(store, "function", &func.name, init_outcome)
        .map_err(classify_function_error)?;

    let allocator = JsonUnknownAllocator::new(store).map_err(classify_function_error)?;
    let undefined_val = store
        .data()
        .undefined_value
        .ok_or_else(|| {
            FunctionError::Failed(wasmtime::Error::msg(
                "the undefined global is not installed",
            ))
        })?
        .get(&mut *store);

    let total_slots = fixed.len() + usize::from(rest.is_some());
    let mut checked_vals = Vec::with_capacity(total_slots);
    let mut parsed_iter = parsed_args.into_iter();
    for param in fixed {
        let Some(parsed_val) = parsed_iter.next() else {
            // Omitted optional/defaulted parameter: pass `$undefined` directly
            // so `run`'s prologue evaluates any default expression.
            checked_vals.push(undefined_val);
            continue;
        };
        let raw_val = allocator
            .allocate(store, &parsed_val)
            .map_err(classify_function_error)?;
        let checked = check_single_argument(store, &instance, func, param, raw_val).await?;
        checked_vals.push(checked);
    }
    if let Some(rest_param) = rest {
        let rest_items: Vec<JsonValue> = parsed_iter.collect();
        if matches!(rest_param.cast, RuntimeParamCast::Uncastable { .. }) {
            // `supplied <= fixed.len()` (verified above): allocate an empty array.
            let empty = allocator
                .allocate(store, &JsonValue::Array(Vec::new()))
                .map_err(classify_function_error)?;
            checked_vals.push(empty);
        } else {
            let raw_rest = allocator
                .allocate(store, &JsonValue::Array(rest_items))
                .map_err(classify_function_error)?;
            let checked =
                check_single_argument(store, &instance, func, rest_param, raw_rest).await?;
            checked_vals.push(checked);
        }
    }
    // Parsed host trees have been moved into WasmGC heap values; release the
    // transient host-byte reservation before running guest code.
    drop(input_bytes_guard);

    let invoke_export_name = crate::mangle::function_invoke(&func.module_name);
    let invoke_fn = instance
        .get_func(&mut *store, invoke_export_name.as_str())
        .ok_or_else(|| {
            FunctionError::Failed(wasmtime::Error::msg(format!(
                "function `{}` is missing invoke export `{invoke_export_name}`",
                func.name
            )))
        })?;

    let mut out = [Val::null_any_ref()];
    let call_outcome = invoke_fn
        .call_async(&mut *store, &checked_vals, &mut out)
        .await;
    call_outcome.map_err(|error| classify_function_error(exec::uncaught_error(store, error)))?;

    let slot = host::abi_arg(&out, 0).map_err(classify_function_error)?;
    if matches!(slot, Val::AnyRef(None)) {
        return Ok(None);
    }
    // `run` has completed (along with any host side effects it performed); settle
    // string marshalling rather than refusing on tight remaining fuel.
    let encoded = fuel::settle_result(store, |store| {
        exec::read_output_string(store, &out, "function invoke")
    })
    .map_err(classify_function_error)?;
    Ok(Some(encoded))
}

pub(crate) fn is_functions_module_name(name: &str) -> bool {
    name == FUNCTIONS_PACKAGE || name.starts_with(FUNCTIONS_MODULE_PREFIX)
}

pub(crate) fn principal_for_module_name(name: &str) -> &str {
    if is_functions_module_name(name) {
        crate::mangle::USER_PACKAGE
    } else {
        name
    }
}

fn function_owner_module(mangled: &str) -> Option<&str> {
    let trimmed = mangled.strip_prefix("mod:").unwrap_or(mangled);
    if !trimmed.starts_with(FUNCTIONS_MODULE_PREFIX) {
        return None;
    }
    let (module_name, _) = trimmed.split_once('#')?;
    Some(module_name)
}

#[allow(clippy::too_many_arguments)]
fn compile_single_function(
    stdlib: Stdlib,
    engine: &Engine,
    def: &FunctionDefinition,
    export_name: String,
    dependencies: &[&PackageDeclaration],
    transitive: &[&PackageDeclaration],
    deps_digest: &[u8; 32],
    cache: Option<&FunctionCache>,
) -> Result<(CompiledFunction, PackageDeclaration), FunctionCompileFailure> {
    let module_name = format!("{FUNCTIONS_MODULE_PREFIX}{}", def.name);
    let cache_key = function_cache_key(&def.name, &def.source, deps_digest);

    let artifact = if let Some(cache) = cache
        && let Some(hit) = lookup_cached_artifact(cache, &cache_key)
    {
        hit
    } else {
        let root_module = ModulePath::from(def.name.as_str());
        let (compiled, invoke_plan) = crate::compile::compile_function_module(
            stdlib,
            &module_name,
            root_module,
            &def.source,
            dependencies,
            transitive,
        )
        .map_err(|error| compile_error_to_function_failure(&def.name, &def.source, error))?;

        let invoke = build_runtime_invoke_plan(
            &def.name,
            &def.source,
            &compiled.declaration,
            invoke_plan.as_ref(),
        )?;

        let built = Arc::new(CachedFunctionArtifact { compiled, invoke });
        if let Some(cache) = cache {
            store_cached_artifact(cache, cache_key, Arc::clone(&built));
        }
        built
    };

    let module = Module::new(engine, &artifact.compiled.wasm).map_err(|error| {
        FunctionCompileFailure::ModuleBuild {
            function: def.name.clone(),
            message: error.to_string(),
        }
    })?;

    let effective_description =
        effective_function_description(&def.description, &artifact.compiled.declaration);
    let warnings = filter_run_doc_warning(&artifact.compiled.warnings, &effective_description);

    Ok((
        CompiledFunction {
            name: def.name.clone(),
            export_name,
            description: effective_description,
            module_name,
            module,
            wasm: artifact.compiled.wasm.clone(),
            type_info: artifact.compiled.type_info.clone(),
            requires: artifact.compiled.required_capabilities.clone(),
            warnings,
            invoke: artifact.invoke.clone(),
        },
        artifact.compiled.declaration.clone(),
    ))
}

fn build_runtime_invoke_plan(
    function_name: &str,
    source: &str,
    declaration: &PackageDeclaration,
    invoke_plan: Option<&FunctionInvokePlan>,
) -> Result<RuntimeInvokePlan, FunctionCompileFailure> {
    let Some(run_symbol) = declaration.values.get("run") else {
        return Err(FunctionCompileFailure::MissingRunExport {
            function: function_name.to_string(),
            line: 1,
            message: "missing required `export function run(...)`".to_string(),
        });
    };
    let ValueKind::Function {
        generics, params, ..
    } = &run_symbol.kind
    else {
        return Err(FunctionCompileFailure::MissingRunExport {
            function: function_name.to_string(),
            line: line_for_span(source, run_symbol.declaration_span),
            message:
                "`run` must be exported as `export function run(...)`, not a variable or constant"
                    .to_string(),
        });
    };
    if !generics.is_empty() {
        return Ok(RuntimeInvokePlan::Generic);
    }
    let Some(plan) = invoke_plan else {
        return Err(FunctionCompileFailure::MissingRunExport {
            function: function_name.to_string(),
            line: line_for_span(source, run_symbol.declaration_span),
            message:
                "`run` must be exported as `export function run(...)`, not a variable or constant"
                    .to_string(),
        });
    };
    if params.len() != plan.arguments.len() {
        return Err(FunctionCompileFailure::ModuleBuild {
            function: function_name.to_string(),
            message: "exported `run` parameter count did not match host-invocation plan"
                .to_string(),
        });
    }

    let mut fixed = Vec::new();
    let mut rest = None;
    for (index, (param, check)) in params.iter().zip(&plan.arguments).enumerate() {
        let entry = build_param_plan(index, param, check);
        if param.rest {
            rest = Some(entry);
        } else {
            fixed.push(entry);
        }
    }
    let min_args = params
        .iter()
        .take(fixed.len())
        .rposition(|param| !param.is_omittable())
        .map_or(0, |idx| idx + 1);

    Ok(RuntimeInvokePlan::Callable {
        min_args,
        fixed,
        rest,
    })
}

fn build_param_plan(
    index: usize,
    param: &Param,
    check: &FunctionArgumentCheck,
) -> RuntimeParamPlan {
    let cast = match check {
        FunctionArgumentCheck::Checked(_) => RuntimeParamCast::Checked,
        FunctionArgumentCheck::Uncastable { reason } => RuntimeParamCast::Uncastable {
            ty_display: param.ty.to_string(),
            reason: reason.clone(),
        },
    };
    RuntimeParamPlan {
        name: param.name.clone(),
        index,
        cast,
    }
}

fn effective_function_description(description: &str, declaration: &PackageDeclaration) -> String {
    let trimmed = description.trim();
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }
    let Some(ValueKind::Function { doc: Some(doc), .. }) =
        declaration.values.get("run").map(|sym| &sym.kind)
    else {
        return String::new();
    };
    doc.summary.trim().to_string()
}

fn filter_run_doc_warning(warnings: &[Diagnostic], effective_description: &str) -> Vec<Diagnostic> {
    if effective_description.is_empty() {
        return warnings.to_vec();
    }
    warnings
        .iter()
        .filter(|diag| {
            !(diag.severity == Severity::Warning
                && diag.message == "exported symbol `run` has no doc comment")
        })
        .cloned()
        .collect()
}

fn build_functions_package_declaration(
    functions: &[CompiledFunction],
    declarations: &[PackageDeclaration],
) -> PackageDeclaration {
    let mut combined = PackageDeclaration::with_package(FUNCTIONS_PACKAGE);

    // Count exported type short names across all functions so collisions get
    // deterministically disambiguated (`LeadScanParams` vs `EnrichContactParams`)
    // while unique names keep their authored spelling.
    let mut type_name_counts: BTreeMap<String, usize> = BTreeMap::new();
    for decl in declarations {
        for type_name in decl.types.keys() {
            let count = type_name_counts.entry(type_name.clone()).or_insert(0);
            *count = count.saturating_add(1);
        }
    }

    // Populate all function value exports first so type-name disambiguation can
    // also avoid colliding with any function's camelCase `export_name` (preventing
    // value-namespace shadowing on exported classes/enums and cross-namespace
    // collisions in `.d.ts` rendering and `select_documentation`).
    for (func, decl) in functions.iter().zip(declarations) {
        if let Some(run_sym) = decl.values.get("run") {
            let mut exported = run_sym.clone();
            exported.name = func.export_name.clone();
            if let ValueKind::Function { doc, .. } = &mut exported.kind {
                *doc = Some(synthesize_function_doc(
                    doc.as_ref(),
                    &func.description,
                    &func.requires,
                ));
            }
            combined.values.insert(func.export_name.clone(), exported);
        }
    }

    for (func, decl) in functions.iter().zip(declarations) {
        for (short_name, type_sym) in &decl.types {
            let collides = type_name_counts.get(short_name).copied().unwrap_or(0) > 1
                || combined.values.contains_key(short_name);
            let preferred = if collides {
                format!(
                    "{}{}",
                    kebab_to_pascal(&func.name),
                    capitalize_ascii_first(short_name)
                )
            } else {
                short_name.clone()
            };
            let public_name = unique_type_key(&combined.types, &combined.values, preferred);
            let mut cloned = type_sym.clone();
            cloned.name = public_name.clone();
            combined.types.insert(public_name, cloned);
        }

        for (mangled, runtime_fn) in &decl.runtime_functions {
            combined
                .runtime_functions
                .insert(mangled.clone(), runtime_fn.clone());
        }
        for (mangled, ty) in &decl.runtime_globals {
            combined.runtime_globals.insert(mangled.clone(), ty.clone());
        }
        for mangled in &decl.runtime_generics {
            combined.runtime_generics.insert(mangled.clone());
        }
        for public_name in &decl.closure_caches {
            combined.closure_caches.insert(public_name.clone());
        }
        for (mangled, type_sym) in &decl.runtime_types {
            combined
                .runtime_types
                .insert(mangled.clone(), type_sym.clone());
        }
    }

    // Rewrite `Type` display names (`Params` -> `LeadScanParams`) across all
    // signatures and nested type definitions so rendered `.d.ts` declarations and
    // compiler diagnostics match `combined.types`.
    let mut combined = with_public_type_names(&combined).into_owned();
    combined.refresh_shapes();
    combined
}

fn synthesize_function_doc(
    authored: Option<&DocComment>,
    description: &str,
    requires: &[DerivedCapability],
) -> DocComment {
    let mut doc = authored.cloned().unwrap_or_else(empty_doc_comment);
    let trimmed = description.trim();
    if !trimmed.is_empty() {
        doc.summary = trimmed.to_string();
    }
    doc.capabilities = requires.iter().map(derived_to_doc_capability).collect();
    doc
}

fn empty_doc_comment() -> DocComment {
    DocComment {
        span: Span::at(FileId(0)),
        summary: String::new(),
        params: Vec::new(),
        returns: None,
        capabilities: Vec::new(),
        throws: Vec::new(),
        deprecated: None,
        examples: Vec::new(),
        unknown_tags: Vec::new(),
    }
}

fn derived_to_doc_capability(derived: &DerivedCapability) -> DocCapability {
    let span = Span::at(FileId(0));
    let bindings = derived
        .known_bindings
        .iter()
        .map(|(field, operand)| DocCapabilityBinding {
            field: field.clone(),
            field_span: span,
            kind: DocCapabilityBindingKind::Literal {
                value: crate::capability_derivation::parse_filter_literal(operand),
                span,
            },
        })
        .collect();
    DocCapability {
        tag_span: span,
        capability: derived.capability.clone(),
        capability_span: span,
        bindings,
        description: String::new(),
        diagnostics: Vec::new(),
    }
}

/// Build a single-function slice of an already capability-projected
/// `@functions` declaration, keeping `func.export_name`, the transitive closure
/// of types reachable from its signature, and any standalone exported types
/// authored in `func` itself — without mutating `@capability` metadata.
fn single_function_declaration(
    projected: &PackageDeclaration,
    func: &CompiledFunction,
) -> Option<PackageDeclaration> {
    if !projected.values.contains_key(&func.export_name) {
        return None;
    }
    // Pre-scope to `func.export_name` and exclude sibling `@functions/<other>`
    // types before `select_documentation` so untagged name lookups cannot pull in
    // sibling functions or sibling types.
    let mut scoped = projected.clone();
    scoped.values.retain(|name, _| name == &func.export_name);
    scoped.types.retain(
        |_, sym| match function_owner_module(sym.mangled_name.as_str()) {
            Some(owner) => owner == func.module_name,
            None => true,
        },
    );
    let mut requested = vec![func.export_name.clone()];
    for (type_name, sym) in &scoped.types {
        if function_owner_module(sym.mangled_name.as_str()) == Some(func.module_name.as_str()) {
            requested.push(type_name.clone());
        }
    }
    select_documentation(&scoped, &requested).ok()
}

fn reject_uncastable_param(
    function_name: &str,
    param: &RuntimeParamPlan,
) -> Result<(), FunctionError> {
    if let RuntimeParamCast::Uncastable { ty_display, reason } = &param.cast {
        return Err(FunctionError::InvalidArguments(format!(
            "parameter `{}` (`{ty_display}`) of function `{function_name}` cannot be supplied as JSON: {reason}",
            param.name
        )));
    }
    Ok(())
}

fn parse_metered_json_args<S: AsRef<str>>(
    store: &mut Store<StoreData>,
    function_name: &str,
    args_json: &[S],
) -> Result<(Vec<JsonValue>, Vec<HostBytes>), FunctionError> {
    let mut parsed_args = Vec::with_capacity(args_json.len());
    let mut guards = Vec::with_capacity(args_json.len());
    for (idx, raw) in args_json.iter().enumerate() {
        let text = raw.as_ref();
        let utf8_len = text.len();
        // `utf16_len <= utf8_len` holds for every UTF-8 slice; reserve `HostBytes`
        // and charge `SCAN` from O(1) `utf8_len` before iterating `text`.
        let byte_estimate = (utf8_len as u64).saturating_mul(8);
        let guard = HostBytes::new(&store.data().tenant_limits, byte_estimate)
            .map_err(|err| FunctionError::BudgetExhausted(wasmtime::Error::new(err)))?;
        fuel::charge(&mut *store, fuel::SCAN, utf8_len as u64).map_err(classify_function_error)?;

        let mut units = Vec::new();
        units.try_reserve_exact(utf8_len).map_err(|err| {
            FunctionError::Failed(host::fatal_host_error(format!(
                "failed to allocate UTF-16 argument buffer: {err}"
            )))
        })?;
        units.extend(text.encode_utf16());

        fuel::charge(&mut *store, fuel::PARSE, units.len() as u64)
            .map_err(classify_function_error)?;

        let value = crate::runtime::json_text::parse(&units).map_err(|error| {
            FunctionError::InvalidArguments(format!(
                "argument {} of function `{function_name}` is not valid JSON: {}",
                idx + 1,
                error.message()
            ))
        })?;
        guards.push(guard);
        parsed_args.push(value);
    }
    Ok((parsed_args, guards))
}

async fn check_single_argument(
    store: &mut Store<StoreData>,
    instance: &Instance,
    func: &CompiledFunction,
    param: &RuntimeParamPlan,
    raw_value: Val,
) -> Result<Val, FunctionError> {
    reject_uncastable_param(&func.name, param)?;
    let check_export_name = crate::mangle::function_argument(&func.module_name, param.index);
    let check_fn = instance
        .get_func(&mut *store, check_export_name.as_str())
        .ok_or_else(|| {
            FunctionError::Failed(wasmtime::Error::msg(format!(
                "function `{}` is missing argument check export `{check_export_name}`",
                func.name
            )))
        })?;
    let mut checked = [Val::null_any_ref()];
    let outcome = check_fn
        .call_async(&mut *store, &[raw_value], &mut checked)
        .await;
    outcome.map_err(
        |error| match classify_function_error(exec::uncaught_error(store, error)) {
            FunctionError::Failed(err) if err.is::<ThrownError>() => {
                FunctionError::InvalidArguments(format!(
                    "invalid argument `{}` (position {}) for function `{}`: {err}",
                    param.name,
                    param.index + 1,
                    func.name
                ))
            }
            other => other,
        },
    )?;
    let [value] = checked;
    Ok(value)
}

fn classify_function_error(error: wasmtime::Error) -> FunctionError {
    if let Some(escalated) = error.downcast_ref::<Escalated>() {
        return FunctionError::Escalated {
            reason: escalated.reason.clone(),
        };
    }
    if matches!(
        error.downcast_ref::<wasmtime::Trap>(),
        Some(wasmtime::Trap::OutOfFuel | wasmtime::Trap::Interrupt)
    ) || crate::runtime::is_memory_exhausted(&error)
    {
        return FunctionError::BudgetExhausted(error);
    }
    if let Some(denial) = error
        .downcast_ref::<ThrownError>()
        .and_then(|thrown| thrown.denial.clone())
    {
        return FunctionError::Denied(denial);
    }
    FunctionError::Failed(error)
}

fn compile_error_to_function_failure(
    function_name: &str,
    source: &str,
    error: crate::compiler_error::CompileError,
) -> FunctionCompileFailure {
    let file = FileId(0);
    let diagnostics = error.into_diagnostics(file);
    let first_error = diagnostics
        .iter()
        .find(|d| d.severity == Severity::Error)
        .or_else(|| diagnostics.first());
    let line = first_error.map_or(1, |diag| line_for_span(source, diag.span));
    let mut sources = crate::Sources::new();
    let _ = sources.add(ModulePath::from(function_name), source);
    let rendered = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|diag| crate::diagnostics::render(diag, &sources))
        .collect::<Vec<_>>()
        .join("\n");
    FunctionCompileFailure::Compile {
        function: function_name.to_string(),
        line,
        diagnostics,
        rendered,
    }
}

fn line_for_span(source: &str, span: Span) -> usize {
    if span.file != FileId(0) {
        return 1;
    }
    LineIndex::new(source)
        .ok()
        .and_then(|idx| idx.line_col(span.start).ok())
        .map_or(1, |(line, _)| line as usize)
}

fn dependency_declarations_digest(
    stdlib: Stdlib,
    dependencies: &[&PackageDeclaration],
    transitive: &[&PackageDeclaration],
) -> Result<[u8; 32], FunctionCompileFailure> {
    let mut hasher = Sha256::new();
    hasher.update(b"submilli-functions-v1\0");
    hasher.update(env!("CARGO_PKG_VERSION").as_bytes());
    hasher.update(b"\0");
    for opt in stdlib.optional_packages() {
        hasher.update(opt.module_name().as_bytes());
        hasher.update(b"\0");
    }
    hasher.update(b"--deps--\0");
    for defs in dependencies {
        hash_declaration(&mut hasher, defs)?;
    }
    hasher.update(b"--transitive--\0");
    for defs in transitive {
        hash_declaration(&mut hasher, defs)?;
    }
    Ok(hasher.finalize().into())
}

fn hash_declaration(
    hasher: &mut Sha256,
    defs: &PackageDeclaration,
) -> Result<(), FunctionCompileFailure> {
    hasher.update(defs.package_name.as_bytes());
    hasher.update(b"\0");
    let bytes =
        serde_json::to_vec(defs).map_err(|error| FunctionCompileFailure::InvalidDependency {
            message: format!(
                "failed to serialize dependency declaration `{}`: {error}",
                defs.package_name
            ),
        })?;
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(&bytes);
    Ok(())
}

fn function_cache_key(name: &str, source: &str, deps_digest: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(deps_digest);
    hasher.update((name.len() as u64).to_le_bytes());
    hasher.update(name.as_bytes());
    hasher.update((source.len() as u64).to_le_bytes());
    hasher.update(source.as_bytes());
    hasher.finalize().into()
}

fn lookup_cached_artifact(
    cache: &FunctionCache,
    key: &[u8; 32],
) -> Option<Arc<CachedFunctionArtifact>> {
    let mut guard = cache.lock();
    if let Some(found) = guard.entries.get(key).cloned() {
        guard.hits = guard.hits.saturating_add(1);
        Some(found)
    } else {
        guard.misses = guard.misses.saturating_add(1);
        None
    }
}

fn store_cached_artifact(
    cache: &FunctionCache,
    key: [u8; 32],
    artifact: Arc<CachedFunctionArtifact>,
) {
    let mut guard = cache.lock();
    guard.entries.insert(key, artifact);
}

fn kebab_to_camel(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut upper_next = false;
    for ch in name.chars() {
        if ch == '-' {
            upper_next = true;
        } else if upper_next {
            out.push(ch.to_ascii_uppercase());
            upper_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

fn kebab_to_pascal(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut upper_next = true;
    for ch in name.chars() {
        if ch == '-' {
            upper_next = true;
        } else if upper_next {
            out.push(ch.to_ascii_uppercase());
            upper_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

fn capitalize_ascii_first(name: &str) -> String {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let mut out = String::with_capacity(name.len());
    out.push(first.to_ascii_uppercase());
    out.extend(chars);
    out
}

fn unique_type_key<T, V>(
    existing_types: &BTreeMap<String, T>,
    existing_values: &BTreeMap<String, V>,
    base: String,
) -> String {
    if !existing_types.contains_key(&base) && !existing_values.contains_key(&base) {
        return base;
    }
    let mut suffix: usize = 2;
    loop {
        let candidate = format!("{base}_{suffix}");
        if !existing_types.contains_key(&candidate) && !existing_values.contains_key(&candidate) {
            return candidate;
        }
        suffix = suffix.saturating_add(1);
    }
}

fn is_reserved_binding_name(name: &str) -> bool {
    matches!(
        name,
        "arguments"
            | "await"
            | "break"
            | "case"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "debugger"
            | "default"
            | "delete"
            | "do"
            | "else"
            | "enum"
            | "eval"
            | "export"
            | "extends"
            | "false"
            | "finally"
            | "for"
            | "function"
            | "if"
            | "implements"
            | "import"
            | "in"
            | "Infinity"
            | "instanceof"
            | "interface"
            | "let"
            | "NaN"
            | "new"
            | "null"
            | "package"
            | "private"
            | "protected"
            | "public"
            | "return"
            | "static"
            | "super"
            | "switch"
            | "this"
            | "throw"
            | "true"
            | "try"
            | "typeof"
            | "undefined"
            | "var"
            | "void"
            | "while"
            | "with"
            | "yield"
    )
}

#[cfg(test)]
mod tests;
