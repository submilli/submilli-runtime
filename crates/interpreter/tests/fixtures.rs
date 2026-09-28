//! `// expect-error-count: N` checks the total compiler diagnostic count, including
//! warnings. Declare it once per fixture, including multi-file fixtures.
//! Fixture harness: pass if no trap; `// expect-error: <substring>` and
//! `// expect-warning: <substring>` header directives check expected diagnostics.
//! `// deny-capability: <substring>` installs a security policy denying every
//! matching capability with reason "denied by fixture policy", so fixtures can
//! exercise the catchable `PermissionDeniedError` path.
//! `// deny-llm-model: <model>` installs a *context*-keyed policy instead,
//! denying `llm.call` only for that model — the shape a blueprint `model`
//! filter has. `deny-capability` cannot express it: it matches on the
//! capability name and ignores context, and `call`, `batch`, and `models()`
//! share the single `llm.call` capability, so denying by name would refuse
//! `models()` outright rather than filtering its candidates.
//!
//! A fixture whose name contains `llm_` also gets a canned [`FixtureLlm`]
//! provider, and the budget-oriented ones get ceilings small enough to reach.
//! `llm_no_provider` deliberately gets none, which is the unconfigured runtime.
//!
//! The default run is a smoke subset. Set `SUBMILLI_FULL_TEST=1` for
//! nightly/CI coverage, or `SUBMILLI_FIXTURE_FILTER=<substring>` for targeted
//! local runs.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use interpreter::runtime::{
    ExecutionTokenBudget, FailureReason, InMemorySessionKv, LinkedPackageModule, LlmCallError,
    LlmFailure, LlmLimits, LlmModel, LlmOutcome, LlmProvider, SharedTokenBudget, StoreData, Vfs,
    install_package_modules_async, install_runtime_host_functions, install_runtime_store_bound,
    install_tenant_limits,
};
use interpreter::{
    Asi, BacktraceMode, CompiledPackage, Diagnostic, ModulePath, PackageDeclaration,
    PackageSourceModule, RunResult, RuntimeConfig, Sources, Token, TokenKind,
    compile_package_with_transitive, compile_script, diagnostics, infer_package, lower_patterns,
    parse, render_backtrace,
};
use wasmtime::{Engine, Linker, Module};

const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

#[test]
fn fixtures() {
    let mut paths = Vec::new();
    collect(Path::new(FIXTURE_DIR), &mut paths);
    paths.sort();
    let discovered = paths.len();
    select_fixture_suite(&mut paths);
    assert!(
        !paths.is_empty(),
        "no fixtures discovered under {FIXTURE_DIR}"
    );
    eprintln!(
        "fixtures: running {} of {} discovered fixture(s); set SUBMILLI_FULL_TEST=1 to run all",
        paths.len(),
        discovered,
    );

    let runtime = Arc::new(PreparedRuntime::new().expect("prepare runtime"));
    let mut outcomes = run_fixtures_parallel(paths.clone(), runtime);
    outcomes.sort_by(|a, b| a.path.cmp(&b.path));

    let mut failures: Vec<String> = Vec::new();
    for outcome in outcomes {
        if let Err(msg) = outcome.result {
            failures.push(format!("--- {} ---\n{msg}", rel(&outcome.path)));
        }
    }

    if !failures.is_empty() {
        panic!(
            "\n{} fixture failure(s) of {}:\n\n{}",
            failures.len(),
            paths.len(),
            failures.join("\n\n"),
        );
    }
}

struct FixtureResult {
    path: PathBuf,
    result: Result<(), String>,
}

#[derive(Clone)]
struct PreparedRuntime {
    config: RuntimeConfig,
    engine: Engine,
    base_linker: Linker<StoreData>,
}

impl PreparedRuntime {
    fn new() -> wasmtime::Result<Self> {
        let config = RuntimeConfig::default();
        let engine = config.engine_async()?;
        let mut base_linker = Linker::<StoreData>::new(&engine);
        install_runtime_host_functions(&mut base_linker)?;
        Ok(Self {
            config,
            engine,
            base_linker,
        })
    }

    async fn run(
        &self,
        compiled: &interpreter::compile::CompiledScript,
        deny_capabilities: &[String],
        deny_llm_models: &[String],
        fixture: &str,
    ) -> wasmtime::Result<RunResult> {
        struct Sink(Arc<Mutex<Vec<u8>>>);
        impl Write for Sink {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().write(buf)
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let buf = Arc::new(Mutex::new(Vec::new()));
        let mut data = StoreData::with_vfs_and_cap(Vfs::tempdir()?, self.config.max_store_bytes);
        data.console = Box::new(Sink(Arc::clone(&buf)));
        data.session_kv = Some(Arc::new(InMemorySessionKv::default()));
        install_llm_fixture_support(&mut data, fixture);
        data.install_type_info(compiled.type_info.clone());
        if !deny_capabilities.is_empty() {
            data.security_check = Arc::new(FixtureDeny(deny_capabilities.to_vec()));
        }
        if !deny_llm_models.is_empty() {
            data.security_check = Arc::new(FixtureDenyLlmModel(deny_llm_models.to_vec()));
        }
        let mut store = self.config.store_async(&self.engine, data)?;
        install_tenant_limits(&mut store);
        let module = Module::new(&self.engine, &compiled.wasm)?;
        let mut linker = self.base_linker.clone();
        install_runtime_store_bound(&mut linker, &mut store)?;
        let inst = linker.instantiate_async(&mut store, &module).await?;
        let _watchdog = self.config.arm_timeout(&self.engine);
        let value = interpreter::dispatch_main_async(&mut store, &inst).await?;
        let captured = buf.lock().unwrap().clone();
        let console = String::from_utf8(captured)
            .map_err(|e| wasmtime::Error::msg(format!("console output not utf-8: {e}")))?;
        Ok(RunResult { value, console })
    }

    /// Link `deps` (in dependency order) under their package names, then
    /// instantiate the root script and run its `main`. Mirrors `run` but with a
    /// graph of separately compiled dependency packages linked in first
    /// (SUB-488 cross-package fixtures). The root is a script — packages are
    /// libraries and may not declare `main`.
    async fn run_packages(
        &self,
        deps: &[&CompiledPackage],
        root: &interpreter::compile::CompiledScript,
        deny_capabilities: &[String],
        fixture: &str,
    ) -> wasmtime::Result<RunResult> {
        let mut data = StoreData::with_vfs_and_cap(Vfs::tempdir()?, self.config.max_store_bytes);
        data.session_kv = Some(Arc::new(InMemorySessionKv::default()));
        install_llm_fixture_support(&mut data, fixture);
        if !deny_capabilities.is_empty() {
            data.security_check = Arc::new(FixtureDeny(deny_capabilities.to_vec()));
        }
        let mut store = self.config.store_async(&self.engine, data)?;
        install_tenant_limits(&mut store);
        let mut linker = self.base_linker.clone();
        install_runtime_store_bound(&mut linker, &mut store)?;

        let dep_modules: Vec<Module> = deps
            .iter()
            .map(|d| Module::new(&self.engine, &d.wasm))
            .collect::<wasmtime::Result<_>>()?;
        let linked: Vec<LinkedPackageModule<'_>> = deps
            .iter()
            .zip(&dep_modules)
            .map(|(d, module)| LinkedPackageModule {
                module,
                declaration: &d.declaration,
                type_info: &d.type_info,
            })
            .collect();
        install_package_modules_async(&mut linker, &mut store, &linked).await?;

        store.data_mut().install_type_info(root.type_info.clone());
        let root_module = Module::new(&self.engine, &root.wasm)?;
        let inst = linker.instantiate_async(&mut store, &root_module).await?;
        let _watchdog = self.config.arm_timeout(&self.engine);
        let value = interpreter::dispatch_main_async(&mut store, &inst).await?;
        Ok(RunResult {
            value,
            console: String::new(),
        })
    }
}

fn run_fixtures_parallel(paths: Vec<PathBuf>, runtime: Arc<PreparedRuntime>) -> Vec<FixtureResult> {
    let worker_count = worker_count(paths.len());
    let mut chunks = vec![Vec::new(); worker_count];
    for (i, path) in paths.into_iter().enumerate() {
        chunks[i % worker_count].push(path);
    }

    let mut handles = Vec::new();
    for chunk in chunks {
        let runtime = Arc::clone(&runtime);
        handles.push(std::thread::spawn(move || {
            let tokio = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            chunk
                .into_iter()
                .map(|path| {
                    let result = run_one(&path, &runtime, &tokio);
                    FixtureResult { path, result }
                })
                .collect::<Vec<_>>()
        }));
    }

    handles
        .into_iter()
        .flat_map(|handle| handle.join().expect("fixture worker panicked"))
        .collect()
}

fn worker_count(case_count: usize) -> usize {
    let default = std::thread::available_parallelism().map_or(1, usize::from);
    let requested = match std::env::var("SUBMILLI_TEST_PARALLELISM") {
        Ok(value) if is_false(&value) => 1,
        Ok(value) if is_true(&value) => default,
        Ok(value) => value
            .parse::<usize>()
            .ok()
            .filter(|value| *value > 0)
            .unwrap_or(default),
        Err(_) => default,
    };
    requested.min(case_count.max(1))
}

fn is_true(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn is_false(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "0" | "false" | "no" | "off"
    )
}

fn select_fixture_suite(paths: &mut Vec<PathBuf>) {
    if let Ok(filter) = std::env::var("SUBMILLI_FIXTURE_FILTER") {
        paths.retain(|path| fixture_rel(path).contains(&filter));
        return;
    }

    if run_full_fixture_suite() {
        return;
    }

    let mut seen_dirs = std::collections::BTreeSet::new();
    paths.retain(|path| {
        let rel = fixture_rel(path);
        if is_top_level_fixture(&rel) {
            return true;
        }
        let Some((dir, _)) = rel.split_once('/') else {
            return false;
        };
        seen_dirs.insert(dir.to_string())
    });
}

fn run_full_fixture_suite() -> bool {
    std::env::var("SUBMILLI_FULL_TEST").is_ok_and(|value| !is_false(&value))
}

fn is_top_level_fixture(rel: &str) -> bool {
    if rel.contains('/') {
        return false;
    }
    if is_large_top_level_matrix(rel) {
        TOP_LEVEL_MATRIX_SMOKE.contains(&rel)
    } else {
        true
    }
}

fn is_large_top_level_matrix(rel: &str) -> bool {
    let Some((prefix, _)) = rel.split_once('_') else {
        return false;
    };
    matches!(
        prefix,
        "array"
            | "bigint"
            | "conformance"
            | "regex"
            | "string"
            | "temporal"
            | "tuple"
            | "uint8array"
    )
}

fn fixture_rel(path: &Path) -> String {
    path.strip_prefix(FIXTURE_DIR)
        .unwrap_or(path)
        .display()
        .to_string()
}

const TOP_LEVEL_MATRIX_SMOKE: &[&str] = &[
    "array_index_oob_throws.subm",
    "array_map.subm",
    "array_sort.subm",
    "array_splice.subm",
    "bigint_arithmetic.subm",
    "bigint_conversions.subm",
    "regex_literal_basic.subm",
    "regex_test_lastindex_global.subm",
    "string_match_and_search.subm",
    "string_normalize.subm",
    "string_replace.subm",
    "string_slice.subm",
    "temporal_duration_arithmetic.subm",
    "temporal_errors_are_catchable.subm",
    "temporal_now_smoke.subm",
    "temporal_plain_arithmetic.subm",
    "temporal_zdt_from_iso.subm",
    "tuple_basic.subm",
    "tuple_methods.subm",
    "uint8array_from_hex_errors.subm",
    "uint8array_map.subm",
    "uint8array_new.subm",
    "uint8array_set.subm",
    "uint8array_to_json.subm",
];

fn run_one(
    path: &Path,
    runtime: &PreparedRuntime,
    tokio: &tokio::runtime::Runtime,
) -> Result<(), String> {
    if path.is_dir() {
        if path.join("packages.manifest").is_file() {
            return run_packages_fixture(path, runtime, tokio);
        }
        return run_multi(path);
    }
    let src = fs::read_to_string(path).map_err(|e| format!("read: {e}"))?;
    let filename = rel(path);
    let expectations = parse_expectations(&src)?;

    let compiled = compile_script(&src, &filename, interpreter::FileId(0), &[], &[]);
    let diags = match &compiled {
        Ok(compiled) => &compiled.warnings,
        Err(diags) => diags,
    };
    assert_diagnostic_count(expectations.diagnostic_count, diags)?;
    match (compiled, expectations.errors.as_slice()) {
        (Ok(_), n) if !n.is_empty() => Err(format!(
            "expected compile error(s) {n:?}, but compilation succeeded",
        )),

        (Err(diags), n) if !n.is_empty() || expectations.diagnostic_count.is_some() => {
            assert_diagnostics(&diags, n, &filename, &src)?;
            assert_diagnostics(&diags, &expectations.warnings, &filename, &src)?;
            Ok(())
        }

        (Err(diags), _) => Err(format!(
            "compile failed:\n{}",
            render_diags(&diags, &filename, &src),
        )),

        (Ok(compiled), _) => {
            assert_diagnostics(&compiled.warnings, &expectations.warnings, &filename, &src)?;
            let outcome = tokio.block_on(runtime.run(
                &compiled,
                &expectations.deny_capabilities,
                &expectations.deny_llm_models,
                &filename,
            ));
            match outcome {
                Ok(_) => Ok(()),
                Err(err) => {
                    let (sources, file) = Sources::single(filename.as_str(), src).unwrap();
                    // The rendered backtrace already carries the `error: …` header.
                    Err(
                        match render_backtrace(&err, &sources, file, BacktraceMode::Full) {
                            Some(bt) => format!("trapped:\n{bt}"),
                            None => format!("trapped: {err}"),
                        },
                    )
                }
            }
        }
    }
}

fn run_multi(path: &Path) -> Result<(), String> {
    let mut files = Vec::new();
    collect_source_files(path, &mut files);
    files.sort();

    let mut sources = Sources::new();
    let mut owned_asts = Vec::new();
    let mut all_diags = Vec::new();
    let mut error_needles = Vec::new();
    let mut diagnostic_count = None;
    let mut warning_needles = Vec::new();
    let mut seen = std::collections::BTreeMap::new();

    for file_path in files {
        let src = fs::read_to_string(&file_path).map_err(|e| format!("read: {e}"))?;
        let expectations = parse_expectations(&src)?;
        merge_diagnostic_count(&mut diagnostic_count, expectations.diagnostic_count)?;
        error_needles.extend(expectations.errors);
        warning_needles.extend(expectations.warnings);
        let module = module_path_for_fixture(path, &file_path)?;
        if let Some(first) = seen.insert(module.as_str().to_string(), file_path.clone()) {
            let message = format!(
                "delete or rename one source file for module `{}`; both {} and {} exist",
                module,
                rel(&first),
                rel(&file_path)
            );
            if error_needles.iter().any(|needle| message.contains(needle)) {
                return Ok(());
            }
            return Err(message);
        }
        let file_id = sources.add(module.clone(), src.clone()).unwrap();
        let (mut ast, mut diags) = parse_source(&src, file_id);
        ast = lower_patterns(ast).unwrap();
        all_diags.append(&mut diags);
        owned_asts.push((module, file_id, ast));
    }

    let stdlib_defs = interpreter::runtime::stdlib_package_declarations();
    let mut external_packages: std::collections::BTreeMap<String, PackageDeclaration> = stdlib_defs
        .into_iter()
        .map(|defs| (defs.package_name.clone(), defs))
        .collect();
    let (prelude_defs, host_defs, _) =
        interpreter::runtime::prelude::cached_runtime_package_declarations();
    for defs in prelude_defs {
        external_packages.insert(defs.package_name.clone(), defs.clone());
    }
    for defs in host_defs {
        external_packages.insert(defs.package_name.clone(), defs.clone());
    }
    let module_refs: Vec<_> = owned_asts
        .iter()
        .map(|(module, file, ast)| (module.clone(), *file, ast))
        .collect();
    let (_ta, _public, mut infer_diags) = infer_package(
        "main",
        ModulePath::from("lib"),
        module_refs,
        &sources,
        external_packages,
        std::collections::BTreeMap::new(),
    );
    all_diags.append(&mut infer_diags);

    assert_diagnostic_count(diagnostic_count, &all_diags)?;
    match (
        all_diags
            .iter()
            .any(|d| d.severity == interpreter::Severity::Error),
        error_needles.as_slice(),
    ) {
        (false, n) if !n.is_empty() => Err(format!(
            "expected compile error(s) {n:?}, but package inference succeeded",
        )),
        (true, n) if !n.is_empty() || diagnostic_count.is_some() => {
            assert_multi_diagnostics(&all_diags, n, &sources)?;
            assert_multi_diagnostics(&all_diags, &warning_needles, &sources)?;
            Ok(())
        }
        (true, _) => Err(format!(
            "package inference failed:\n{}",
            render_multi_diags(&all_diags, &sources),
        )),
        (false, _) => {
            assert_multi_diagnostics(&all_diags, &warning_needles, &sources)?;
            Ok(())
        }
    }
}

/// One entry from a `packages.manifest`. Line format (whitespace-separated,
/// `#`/blank lines ignored): `<package-name> <subdir> <dep,dep|-> [root]`.
/// The `root` entry is compiled as a *script* (it carries `main`); every other
/// entry is a library package. The script depends on every package by name.
struct ManifestPackage {
    name: String,
    dir: String,
    deps: Vec<String>,
    is_root: bool,
}

fn parse_packages_manifest(path: &Path) -> Result<Vec<ManifestPackage>, String> {
    let text = fs::read_to_string(path.join("packages.manifest"))
        .map_err(|e| format!("read manifest: {e}"))?;
    let mut pkgs = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 2 {
            return Err(format!(
                "manifest line needs `<name> <dir> [deps|-] [root]`: {line}"
            ));
        }
        let deps = match cols.get(2).copied() {
            None | Some("-") => Vec::new(),
            Some(list) => list.split(',').map(str::to_string).collect(),
        };
        pkgs.push(ManifestPackage {
            name: cols[0].to_string(),
            dir: cols[1].to_string(),
            deps,
            is_root: cols.get(3) == Some(&"root"),
        });
    }
    Ok(pkgs)
}

/// The closure of `pkg`'s dependencies minus the ones it declares — what the
/// driver passes as `transitive`. The root entry is skipped: it is compiled as a
/// script, not a package, so it has no declaration to hand out.
fn transitive_deps(pkg: &ManifestPackage, manifest: &[ManifestPackage], root: &str) -> Vec<String> {
    let mut queue: Vec<String> = pkg
        .deps
        .iter()
        .flat_map(|name| deps_of(name, manifest))
        .collect();
    let mut seen: Vec<String> = Vec::new();
    while let Some(name) = queue.pop() {
        if name == root || pkg.deps.contains(&name) || seen.contains(&name) {
            continue;
        }
        queue.extend(deps_of(&name, manifest));
        seen.push(name);
    }
    seen
}

fn deps_of(name: &str, manifest: &[ManifestPackage]) -> Vec<String> {
    manifest
        .iter()
        .find(|p| p.name == name)
        .map(|p| p.deps.clone())
        .unwrap_or_default()
}

/// Dependency-first order (deps precede dependents). The manifest graph is tiny,
/// so a repeated-passes sort is plenty.
fn topo_packages(pkgs: &[ManifestPackage]) -> Result<Vec<String>, String> {
    let mut order: Vec<String> = Vec::new();
    while order.len() < pkgs.len() {
        let mut progressed = false;
        for pkg in pkgs {
            if order.contains(&pkg.name) {
                continue;
            }
            if pkg.deps.iter().all(|d| order.contains(d)) {
                order.push(pkg.name.clone());
                progressed = true;
            }
        }
        if !progressed {
            return Err("manifest has a dependency cycle or unknown dep".to_string());
        }
    }
    Ok(order)
}

/// Compile each package in the manifest (dependency order, separate Wasm
/// modules), then link them and run the root package's `main`. Pass = no trap.
/// A package that expects a compile error opts in via `// expect-error:`.
fn run_packages_fixture(
    path: &Path,
    runtime: &PreparedRuntime,
    tokio: &tokio::runtime::Runtime,
) -> Result<(), String> {
    let manifest = parse_packages_manifest(path)?;
    let order = topo_packages(&manifest)?;
    let root_entry = manifest
        .iter()
        .find(|p| p.is_root)
        .ok_or_else(|| "manifest has no `root` entry".to_string())?;

    // Read every entry's sources up front (owned, so `PackageSourceModule` can
    // borrow them through compilation).
    let mut sources_by_pkg: std::collections::BTreeMap<String, Vec<(ModulePath, String)>> =
        std::collections::BTreeMap::new();
    let mut error_needles: Vec<String> = Vec::new();
    let mut diagnostic_count = None;
    let mut deny_capabilities: Vec<String> = Vec::new();
    for pkg in &manifest {
        let dir = path.join(&pkg.dir);
        let mut files = Vec::new();
        collect_source_files(&dir, &mut files);
        files.sort();
        let mut modules = Vec::new();
        for file in files {
            let src = fs::read_to_string(&file).map_err(|e| format!("read: {e}"))?;
            let expectations = parse_expectations(&src)?;
            merge_diagnostic_count(&mut diagnostic_count, expectations.diagnostic_count)?;
            error_needles.extend(expectations.errors);
            deny_capabilities.extend(expectations.deny_capabilities);
            let module = module_path_for_fixture(&dir, &file)?;
            modules.push((module, src));
        }
        sources_by_pkg.insert(pkg.name.clone(), modules);
    }

    // Compile the library packages (dependency-first); the root is compiled as a
    // script afterward against every package declaration.
    let mut compile_diagnostics = Vec::new();
    let mut compiled: std::collections::BTreeMap<String, CompiledPackage> =
        std::collections::BTreeMap::new();
    for name in order.iter().filter(|n| **n != root_entry.name) {
        let pkg = manifest
            .iter()
            .find(|p| &p.name == name)
            .expect("name in manifest");
        let modules = &sources_by_pkg[name];
        let psm: Vec<PackageSourceModule<'_>> = modules
            .iter()
            .map(|(module, source)| PackageSourceModule {
                path: module.clone(),
                source: source.as_str(),
            })
            .collect();
        let dep_decls: Vec<&PackageDeclaration> =
            pkg.deps.iter().map(|d| &compiled[d].declaration).collect();
        // Mirrors the build driver: a package compiles against its declared
        // dependencies plus — resolvable but not importable — the rest of the
        // closure, which its dependencies' public surfaces can name.
        let transitive_decls: Vec<&PackageDeclaration> =
            transitive_deps(pkg, &manifest, &root_entry.name)
                .iter()
                .map(|d| &compiled[d].declaration)
                .collect();
        match compile_package_with_transitive(
            name,
            ModulePath::from("lib"),
            &psm,
            &dep_decls,
            &transitive_decls,
        ) {
            Ok(cp) => {
                compile_diagnostics.extend(cp.warnings.iter().cloned());
                compiled.insert(name.clone(), cp);
            }
            Err(diags) => {
                compile_diagnostics.extend(diags);
                return finish_with_expected_errors(
                    &error_needles,
                    diagnostic_count,
                    &compile_diagnostics,
                    name,
                );
            }
        }
    }

    // The root script depends on every library package by name.
    let root_modules = &sources_by_pkg[&root_entry.name];
    let (root_path, root_src) = root_modules
        .first()
        .filter(|_| root_modules.len() == 1)
        .ok_or_else(|| "root entry must contain exactly one script file".to_string())?;
    let all_decls: Vec<&PackageDeclaration> = order
        .iter()
        .filter(|n| **n != root_entry.name)
        .map(|n| &compiled[n].declaration)
        .collect();
    let root_script = match compile_script(
        root_src,
        root_path.as_str(),
        interpreter::FileId(0),
        &all_decls,
        &[],
    ) {
        Ok(script) => script,
        Err(diags) => {
            compile_diagnostics.extend(diags);
            return finish_with_expected_errors(
                &error_needles,
                diagnostic_count,
                &compile_diagnostics,
                &root_entry.name,
            );
        }
    };

    compile_diagnostics.extend(root_script.warnings.iter().cloned());
    assert_diagnostic_count(diagnostic_count, &compile_diagnostics)?;
    if !error_needles.is_empty() {
        return Err(format!(
            "expected compile error(s) {error_needles:?}, but everything compiled",
        ));
    }

    let deps: Vec<&CompiledPackage> = order
        .iter()
        .filter(|n| **n != root_entry.name)
        .map(|n| &compiled[n])
        .collect();
    let fixture = rel(path);
    match tokio.block_on(runtime.run_packages(&deps, &root_script, &deny_capabilities, &fixture)) {
        Ok(_) => Ok(()),
        Err(err) => Err(format!("trapped: {err}")),
    }
}

/// Shared compile-error handling for a manifest entry: if the fixture declared
/// `// expect-error:` needles and they all match, the failure is expected (pass);
/// otherwise report the unexpected compile failure.
fn finish_with_expected_errors(
    needles: &[String],
    diagnostic_count: Option<usize>,
    diags: &[Diagnostic],
    entry: &str,
) -> Result<(), String> {
    assert_diagnostic_count(diagnostic_count, diags)?;
    if (!needles.is_empty() || diagnostic_count.is_some())
        && needles
            .iter()
            .all(|needle| diags.iter().any(|d| d.message.contains(needle)))
    {
        return Ok(());
    }
    Err(format!(
        "`{entry}` failed to compile:\n{}",
        diags
            .iter()
            .map(|d| d.message.clone())
            .collect::<Vec<_>>()
            .join("\n"),
    ))
}

/// A count applies to the whole fixture; declare it once, including in multi-file fixtures.
fn merge_diagnostic_count(target: &mut Option<usize>, count: Option<usize>) -> Result<(), String> {
    if let Some(count) = count {
        if target.is_some() {
            return Err("declare expect-error-count only once per fixture".into());
        }
        *target = Some(count);
    }
    Ok(())
}

fn assert_diagnostic_count(expected: Option<usize>, diags: &[Diagnostic]) -> Result<(), String> {
    if let Some(expected) = expected
        && diags.len() != expected
    {
        return Err(format!(
            "expected {expected} diagnostic(s), got {}: {:?}",
            diags.len(),
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        ));
    }
    Ok(())
}

fn parse_source(src: &str, file: interpreter::FileId) -> (interpreter::Ast, Vec<Diagnostic>) {
    let mut asi = Asi::new(src, file);
    let mut tokens: Vec<Token> = Vec::new();
    loop {
        let tok = asi.next_token();
        let is_eof = matches!(tok.kind, TokenKind::Eof);
        tokens.push(tok);
        if is_eof {
            break;
        }
    }
    let mut diags = asi.into_diagnostics();
    let (ast, parse_diags) = parse(src, tokens, file);
    diags.extend(parse_diags);
    (ast, diags)
}

#[derive(Default)]
struct Expectations {
    errors: Vec<String>,
    diagnostic_count: Option<usize>,
    warnings: Vec<String>,
    deny_capabilities: Vec<String>,
    deny_llm_models: Vec<String>,
}

fn parse_expectations(src: &str) -> Result<Expectations, String> {
    let mut expectations = Expectations::default();
    for line in src.lines().take_while(|l| {
        let t = l.trim_start();
        t.is_empty() || t.starts_with("//")
    }) {
        if let Some((_, s)) = line.split_once("// expect-error-count:") {
            let count = s
                .trim()
                .parse::<usize>()
                .map_err(|_| format!("invalid expect-error-count: {s:?}"))?;
            merge_diagnostic_count(&mut expectations.diagnostic_count, Some(count))?;
        }
        if let Some((_, s)) = line.split_once("// expect-error:") {
            let needle = s.trim();
            if !needle.is_empty() {
                expectations.errors.push(needle.to_string());
            }
        }
        if let Some((_, s)) = line.split_once("// expect-warning:") {
            let needle = s.trim();
            if !needle.is_empty() {
                expectations.warnings.push(needle.to_string());
            }
        }
        if let Some((_, s)) = line.split_once("// deny-capability:") {
            let needle = s.trim();
            if !needle.is_empty() {
                expectations.deny_capabilities.push(needle.to_string());
            }
        }
        if let Some((_, s)) = line.split_once("// deny-llm-model:") {
            let needle = s.trim();
            if !needle.is_empty() {
                expectations.deny_llm_models.push(needle.to_string());
            }
        }
    }
    Ok(expectations)
}

/// The canned model provider the `llm_*` fixtures dispatch against.
///
/// U7's real provider is unreachable from here by construction —
/// `submilli-shared` depends on `interpreter` and not the reverse — so the
/// fixtures implement the U1 trait directly. This is a *behavioral* fake, not a
/// stub: it reproduces the parts of the contract the fixtures assert against,
/// including the ones a lazier fake would paper over. It rejects a model it does
/// not serve (so the gate-ordering fixtures are not vacuous), it returns exactly
/// one outcome per prompt in input order, and it mixes successes with failures
/// inside one batch so R3 has something real to survive.
///
/// Behavior is keyed off the fixture's own filename rather than a directive,
/// because each scenario wants a *different provider*, not a different policy —
/// and a fixture that had to describe its provider in a header comment would
/// state the setup twice.
struct FixtureLlm {
    /// Which scenario this instance plays, from the fixture's file stem.
    scenario: String,
}

impl FixtureLlm {
    fn new(fixture: &str) -> Self {
        Self {
            scenario: fixture.to_string(),
        }
    }

    /// The models this provider serves for the running fixture.
    ///
    /// `llm_models_empty` serves none, which is distinct from "no provider":
    /// the provider answers, with an empty catalog. `llm_models_no_description`
    /// declares a model with neither a description nor a context window, so the
    /// `null`-means-unknown rule has a real subject.
    fn catalog(&self) -> Vec<LlmModel> {
        if self.scenario.contains("models_empty") {
            return Vec::new();
        }
        if self.scenario.contains("models_no_description") {
            return vec![
                LlmModel {
                    name: "claude-haiku-4-5".to_string(),
                    description: Some("Cheap and fast.".to_string()),
                    context_window: Some(200_000),
                },
                // Declared by name only — the operator asserted nothing else.
                LlmModel::new("bare-model"),
            ];
        }
        vec![
            LlmModel {
                name: "claude-haiku-4-5".to_string(),
                description: Some("Cheap and fast.".to_string()),
                context_window: Some(200_000),
            },
            LlmModel {
                name: "claude-sonnet-5".to_string(),
                description: Some("Strong reasoning.".to_string()),
                context_window: Some(1_000_000),
            },
            // The candidate a `model`-filtered policy hides in
            // `llm_models_filtered`. Serving it here is what makes that
            // fixture's filtering assertion mean something.
            LlmModel {
                name: "internal-secret-model".to_string(),
                description: Some("Operator-only.".to_string()),
                context_window: Some(8_000),
            },
        ]
    }

    /// The outcome for prompt `index`.
    ///
    /// The typed fixtures answer in JSON so the structural check has something
    /// to verify; `llm_typed_mismatch` and `llm_typed_provider_ignored_schema`
    /// answer with the *wrong* shape on purpose, which is the only way the
    /// "schema is advisory, the check is not" claim gets tested.
    fn outcome(&self, index: usize, prompt: &str) -> LlmOutcome {
        let scenario = self.scenario.as_str();

        if scenario.contains("typed_mismatch") {
            // Conforms to no requested type: `level` is a number where the
            // interface says string, so the checked cast must throw.
            return LlmOutcome::success(r#"{"level":7,"rationale":"malformed on purpose"}"#);
        }
        if scenario.contains("typed_provider_ignored_schema") {
            // A provider that ignored the schema entirely and answered in
            // prose. The schema is advisory; the check is not.
            return LlmOutcome::success("I think this ticket is probably critical, honestly.");
        }
        if scenario.contains("typed_get") {
            return LlmOutcome::success(
                r#"{"level":"critical","rationale":"payment path down",
                    "detail":{"service":"billing","restarts":3},"owner":null}"#,
            );
        }
        if scenario.contains("batch_partial_failure") {
            // R3: element 1 fails mid-batch and still carries partial text,
            // while 0 and 2 succeed. A whole-batch `Err` would discard them.
            return match index {
                1 => LlmOutcome::failed(
                    LlmFailure::new(FailureReason::Truncated, "output token cap reached")
                        .with_finish_reason("length"),
                    Some("partial answer that survi"),
                ),
                _ => LlmOutcome::success(format!("answer {index}")),
            };
        }
        if scenario.contains("error_taxonomy") {
            // One element per category, so the fixture can assert the
            // kebab-case spelling and the text/retryable rules per reason.
            return match index {
                0 => LlmOutcome::failed(
                    LlmFailure::new(FailureReason::Truncated, "output token cap reached")
                        .with_finish_reason("length"),
                    Some("cut off mid-sent"),
                ),
                1 => LlmOutcome::failed(
                    LlmFailure::new(FailureReason::ContentFiltered, "blocked by safety filter"),
                    Some("redacted portion "),
                ),
                2 => LlmOutcome::failed(
                    LlmFailure::new(FailureReason::InvalidOutput, "output did not parse"),
                    None::<String>,
                ),
                3 => LlmOutcome::failed(
                    LlmFailure::new(FailureReason::RateLimited, "rate limited").with_status(429),
                    None::<String>,
                ),
                4 => LlmOutcome::failed(
                    LlmFailure::new(FailureReason::RequestRejected, "request rejected")
                        .with_status(400),
                    None::<String>,
                ),
                5 => LlmOutcome::failed(
                    LlmFailure::new(FailureReason::ProviderUnavailable, "provider unavailable")
                        .with_status(503),
                    None::<String>,
                ),
                6 => LlmOutcome::failed(
                    LlmFailure::new(FailureReason::Transport, "connection reset"),
                    None::<String>,
                ),
                7 => LlmOutcome::failed(
                    LlmFailure::new(FailureReason::Cancelled, "cancelled"),
                    None::<String>,
                ),
                _ => LlmOutcome::failed(
                    LlmFailure::new(FailureReason::Incomplete, "unclassified stop")
                        .with_finish_reason("provider-specific-stop"),
                    None::<String>,
                ),
            };
        }
        if scenario.contains("budget") {
            // Report usage so the budget fixtures reconcile against real
            // numbers rather than an indeterminate hold.
            return LlmOutcome::success(format!("answer {index}")).with_usage(Some(10), Some(10));
        }

        // The default: echo the prompt's length rather than the prompt, so a
        // fixture can prove ordering without the fake becoming a prompt oracle.
        LlmOutcome::success(format!("answer {index} for {} bytes", prompt.len()))
    }
}

impl LlmProvider for FixtureLlm {
    fn call<'a>(
        &'a self,
        model: &'a str,
        prompts: &'a [String],
        _schema_json: Option<&'a str>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<LlmOutcome>, LlmCallError>> + Send + 'a>,
    > {
        // Declaration is authoritative (KTD9): a provider rejects a model it
        // does not serve, and names the ones it does. Reproducing that here is
        // what keeps `llm_denied`'s ordering claim honest — a fake that
        // accepted every model name would pass that fixture vacuously.
        let catalog = self.catalog();
        if !catalog.iter().any(|m| m.name == model) {
            let model = model.to_string();
            let available = catalog.into_iter().map(|m| m.name).collect();
            return Box::pin(async move { Err(LlmCallError::UnknownModel { model, available }) });
        }
        // Exactly one outcome per prompt, in input order.
        let outcomes = prompts
            .iter()
            .enumerate()
            .map(|(i, prompt)| self.outcome(i, prompt))
            .collect();
        Box::pin(async move { Ok(outcomes) })
    }

    fn models<'a>(
        &'a self,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<LlmModel>, LlmCallError>> + Send + 'a>,
    > {
        let models = self.catalog();
        Box::pin(async move { Ok(models) })
    }
}

/// Wire the provider and budget a given fixture wants.
///
/// `llm_no_provider` is the one fixture that gets *no* provider: the field stays
/// `None`, which is the unconfigured runtime R12 describes. Everything else gets
/// the fake; the budget fixtures additionally get a ceiling small enough to
/// actually hit, since a default-sized budget would make their assertions
/// unreachable.
fn install_llm_fixture_support(data: &mut StoreData, fixture: &str) {
    if !fixture.contains("llm_") {
        return;
    }
    if fixture.contains("no_provider") {
        return;
    }

    data.llm_provider = Some(Arc::new(FixtureLlm::new(fixture)));

    // Each reserved call costs its prompt estimate plus the output cap, so
    // these ceilings are sized to admit the first call(s) and refuse the next.
    let limits = if fixture.contains("prompt_bounds") {
        LlmLimits {
            max_prompt_count: 4,
            max_prompt_bytes: 64,
            ..LlmLimits::default()
        }
    } else if fixture.contains("budget") {
        LlmLimits {
            per_execution_tokens: 200,
            default_output_cap: 50,
            ..LlmLimits::default()
        }
    } else {
        return;
    };

    // `llm_budget_aggregate` shares a server-wide ceiling tighter than its own,
    // so the refusal names the *server* budget and not this execution's.
    let aggregate_cap = if fixture.contains("budget_aggregate") {
        120
    } else {
        u64::MAX
    };
    data.llm_budget = Some(Arc::new(ExecutionTokenBudget::new(
        limits,
        SharedTokenBudget::new(aggregate_cap),
    )));
}

/// A `model`-filtered policy: denies `llm.call` for the named models and allows
/// everything else, keyed off the check *context* rather than the capability
/// name.
///
/// `// deny-capability:` cannot express this. It matches on the capability name
/// and ignores context, and `call`, `batch`, and `models()` all share the single
/// `llm.call` capability — so denying by name would refuse `models()` itself
/// before it ever reached the per-candidate filter, and the filtering this
/// exists to test would never run. A blueprint `model` filter is precisely a
/// context-keyed rule, so the fixture policy has to be one too.
///
/// This is a *policy* denial, which is the half that filters candidates. An
/// invariant denial means the check could not be made at all and must propagate
/// rather than silently shorten the list — `crates/submilli-server/tests/` is
/// where filter-conditional denial gets its integration coverage.
struct FixtureDenyLlmModel(Vec<String>);

impl interpreter::runtime::SecurityCheck for FixtureDenyLlmModel {
    fn check(
        &self,
        _caller: &str,
        capability: &str,
        context: &serde_json::Value,
    ) -> interpreter::runtime::CheckOutcome {
        let model = context.get("model").and_then(serde_json::Value::as_str);
        if capability == "llm.call" && model.is_some_and(|m| self.0.iter().any(|d| d == m)) {
            return interpreter::runtime::CheckOutcome::Deny {
                reason: "denied by fixture model filter".to_string(),
            };
        }
        interpreter::runtime::CheckOutcome::Allow
    }
}

/// Denies every capability containing one of the `// deny-capability:` needles
/// with a fixed reason the fixture can assert against.
struct FixtureDeny(Vec<String>);

impl interpreter::runtime::SecurityCheck for FixtureDeny {
    fn check(
        &self,
        _caller: &str,
        capability: &str,
        _context: &serde_json::Value,
    ) -> interpreter::runtime::CheckOutcome {
        if self.0.iter().any(|needle| capability.contains(needle)) {
            interpreter::runtime::CheckOutcome::Deny {
                reason: "denied by fixture policy".to_string(),
            }
        } else {
            interpreter::runtime::CheckOutcome::Allow
        }
    }
}

fn assert_diagnostics(
    diags: &[Diagnostic],
    needles: &[String],
    filename: &str,
    src: &str,
) -> Result<(), String> {
    for needle in needles {
        // also match help/notes so fixtures can assert against help: text
        let matched = diags.iter().any(|d| {
            d.message.contains(needle)
                || d.help.iter().any(|h| h.contains(needle))
                || d.notes.iter().any(|(_, text)| text.contains(needle))
        });
        if !matched {
            return Err(format!(
                "expected diagnostic substring {needle:?} not found.\nactual:\n{}",
                render_diags(diags, filename, src),
            ));
        }
    }
    Ok(())
}

fn assert_multi_diagnostics(
    diags: &[Diagnostic],
    needles: &[String],
    sources: &Sources,
) -> Result<(), String> {
    for needle in needles {
        let matched = diags.iter().any(|d| {
            d.message.contains(needle)
                || d.help.iter().any(|h| h.contains(needle))
                || d.notes.iter().any(|(_, text)| text.contains(needle))
        });
        if !matched {
            return Err(format!(
                "expected diagnostic substring {needle:?} not found.\nactual:\n{}",
                render_multi_diags(diags, sources),
            ));
        }
    }
    Ok(())
}

fn render_diags(diags: &[Diagnostic], filename: &str, src: &str) -> String {
    let (sources, _) = Sources::single(filename, src).unwrap();
    diags
        .iter()
        .map(|d| diagnostics::render(d, &sources))
        .collect::<Vec<_>>()
        .join("\n")
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir({}): {e}", dir.display()));
    // A multi-package fixture (its own subdirs are packages) or a single
    // multi-module package (`lib.*` at this level) is one fixture, not a tree of
    // standalone files — don't descend.
    if dir.join("packages.manifest").exists()
        || dir.join("lib.ts").exists()
        || dir.join("lib.subm").exists()
    {
        out.push(dir.to_path_buf());
        return;
    }
    for entry in entries {
        let p = entry.expect("dir entry").path();
        if p.is_dir() {
            collect(&p, out);
        } else if is_source_file(&p) {
            out.push(p);
        }
    }
}

fn collect_source_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir({}): {e}", dir.display()));
    for entry in entries {
        let p = entry.expect("dir entry").path();
        if p.is_dir() {
            collect_source_files(&p, out);
        } else if is_source_file(&p) {
            out.push(p);
        }
    }
}

fn is_source_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|s| s.to_str()),
        Some("ts" | "subm")
    )
}

fn module_path_for_fixture(root: &Path, file: &Path) -> Result<ModulePath, String> {
    let rel = file
        .strip_prefix(root)
        .map_err(|e| format!("strip fixture root: {e}"))?;
    let without_ext = rel.with_extension("");
    Ok(ModulePath::from(
        without_ext
            .to_str()
            .ok_or_else(|| format!("non-utf8 path: {}", file.display()))?
            .replace('\\', "/"),
    ))
}

fn render_multi_diags(diags: &[Diagnostic], sources: &Sources) -> String {
    diags
        .iter()
        .map(|d| diagnostics::render(d, sources))
        .collect::<Vec<_>>()
        .join("\n")
}

fn rel(p: &Path) -> String {
    p.strip_prefix(env!("CARGO_MANIFEST_DIR"))
        .unwrap_or(p)
        .display()
        .to_string()
}

#[test]
fn diagnostic_count_directive_rejects_missing_and_duplicate_reports() {
    let expectations = parse_expectations(
        "// expect-error-count: 2\n// expect-error: mismatch\n// expect-error: mismatch\n",
    )
    .unwrap();
    let diagnostic = Diagnostic {
        severity: interpreter::Severity::Error,
        span: interpreter::Span::at(interpreter::FileId(0)),
        message: "mismatch".into(),
        help: vec![],
        notes: vec![],
    };
    assert!(
        assert_diagnostic_count(
            expectations.diagnostic_count,
            std::slice::from_ref(&diagnostic)
        )
        .is_err()
    );
    assert!(
        assert_diagnostic_count(
            expectations.diagnostic_count,
            &[diagnostic.clone(), diagnostic.clone()]
        )
        .is_ok()
    );
    assert!(assert_diagnostic_count(Some(1), &[diagnostic.clone(), diagnostic]).is_err());
    assert!(assert_diagnostic_count(Some(0), &[]).is_ok());
}

#[test]
fn diagnostic_count_directive_rejects_malformed_and_ambiguous_counts() {
    for source in [
        "// expect-error-count: nope",
        "// expect-error-count: -1",
        "// expect-error-count:",
        "// expect-error-count: 1\n// expect-error-count: 1",
    ] {
        assert!(parse_expectations(source).is_err(), "{source}");
    }
    let mut count = Some(1);
    assert!(merge_diagnostic_count(&mut count, Some(1)).is_err());
}
