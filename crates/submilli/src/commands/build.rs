//! `submilli build` — package-project tooling around `submilli.toml`:
//! `init` / `new` scaffold the manifest and package folders, `check` compiles
//! the packages in dependency order, and `publish-local` compiles and installs
//! the artifacts into the local package store.

use std::fs;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Context;
use interpreter::{Severity, Sources, Span, diagnostics};
use submilli_build::{
    BuildDiagnostic, BuildSeverity, BuiltPackage, DriverError, Lockfile, PackageName, PackageStore,
    ProjectManifest, ResolveError, ScaffoldError, add_package, build_packages,
    find_manifest_upwards, init_project, install_packages, install_plan, is_valid_package_name,
    parse_manifest, refresh_editor_files, resolve_github_closure, write_capabilities_file,
};
use submilli_shared::github::GithubRepoFetcher;

#[derive(clap::Args)]
pub struct Args {
    #[command(subcommand)]
    cmd: BuildCmd,
}

#[derive(clap::Subcommand)]
enum BuildCmd {
    /// Create a submilli.toml with a first package and scaffold its folders.
    Init(InitArgs),
    /// Add a new package to submilli.toml and scaffold its folders.
    New(NewArgs),
    /// Compile the project's packages in dependency order without installing.
    Check(CompileArgs),
    /// Compile the project's packages and install them into the local store.
    PublishLocal(CompileArgs),
    /// Compile and run the project's `tests/**/*.test.{ts,subm}` files.
    Test(CompileArgs),
}

#[derive(clap::Args)]
struct CompileArgs {
    /// Compile only this package and its sibling dependencies.
    #[arg(short = 'p', long = "package")]
    package: Option<String>,
}

#[derive(clap::Args)]
struct InitArgs {
    /// First package name (`@scope/name`). Prompted on stdin when omitted.
    name: Option<String>,

    /// First package path relative to submilli.toml. Defaults to `.`.
    path: Option<PathBuf>,
}

#[derive(clap::Args)]
struct NewArgs {
    /// Package name (`@scope/name`).
    name: String,

    /// Package path relative to submilli.toml.
    path: PathBuf,
}

impl Args {
    pub(crate) fn label(&self) -> &'static str {
        match &self.cmd {
            BuildCmd::Init(_) => "build.init",
            BuildCmd::New(_) => "build.new",
            BuildCmd::Check(_) => "build.check",
            BuildCmd::PublishLocal(_) => "build.publish_local",
            BuildCmd::Test(_) => "build.test",
        }
    }

    pub(crate) fn metric_flags(&self) -> Vec<(&'static str, bool)> {
        match &self.cmd {
            BuildCmd::Check(compile)
            | BuildCmd::PublishLocal(compile)
            | BuildCmd::Test(compile) => {
                vec![("has_package", compile.package.is_some())]
            }
            BuildCmd::Init(_) | BuildCmd::New(_) => Vec::new(),
        }
    }
}

pub fn execute(args: Args) -> anyhow::Result<ExitCode> {
    match args.cmd {
        BuildCmd::Init(init) => execute_init(init),
        BuildCmd::New(new) => execute_new(new),
        BuildCmd::Check(compile) => execute_check(compile),
        BuildCmd::PublishLocal(compile) => execute_publish_local(compile),
        BuildCmd::Test(compile) => test_runner::execute_test(compile),
    }
}

fn execute_init(args: InitArgs) -> anyhow::Result<ExitCode> {
    let name = match args.name {
        Some(name) => name,
        None => prompt_package_name()?,
    };
    let dir = std::env::current_dir().context("resolving current directory")?;
    let package_path = args.path.unwrap_or_else(|| PathBuf::from("."));
    let scaffolded = match init_project(&dir, &name, &package_path) {
        Ok(scaffolded) => scaffolded,
        Err(err) => return Ok(report_scaffold_error(err)),
    };
    eprintln!("created {}", scaffolded.manifest_path.display());
    eprintln!("created {}", scaffolded.entrypoint.display());
    eprintln!("created {}", scaffolded.test_file.display());
    eprintln!(
        "add packages with `submilli build new <@scope/name> <path>`; compile and install with `submilli build publish-local`; run tests with `submilli build test`"
    );
    if let Some(hint) = super::skill::adoption_hint(&dir) {
        eprintln!("{hint}");
    }
    Ok(ExitCode::SUCCESS)
}

fn execute_new(args: NewArgs) -> anyhow::Result<ExitCode> {
    let dir = std::env::current_dir().context("resolving current directory")?;
    let manifest_path = dir.join("submilli.toml");
    let scaffolded = match add_package(&manifest_path, &args.name, &args.path) {
        Ok(scaffolded) => scaffolded,
        Err(err) => return Ok(report_scaffold_error(err)),
    };
    eprintln!(
        "added {} to {}",
        args.name,
        scaffolded.manifest_path.display()
    );
    eprintln!("created {}", scaffolded.entrypoint.display());
    eprintln!("created {}", scaffolded.test_file.display());
    Ok(ExitCode::SUCCESS)
}

fn prompt_package_name() -> anyhow::Result<String> {
    let stdin = io::stdin();
    let interactive = stdin.is_terminal();
    loop {
        eprint!("package name (@scope/name): ");
        let mut line = String::new();
        let bytes = stdin
            .read_line(&mut line)
            .context("reading package name from stdin")?;
        if bytes == 0 {
            anyhow::bail!(
                "no package name provided; pass one as an argument: `submilli build init <@scope/name>`"
            );
        }
        let name = line.trim();
        if is_valid_package_name(name) {
            return Ok(name.to_string());
        }
        eprintln!("invalid package name `{name}`; expected `@scope/name`");
        if !interactive {
            anyhow::bail!(
                "invalid package name on stdin; pass one as an argument: `submilli build init <@scope/name>`"
            );
        }
    }
}

fn report_scaffold_error(err: ScaffoldError) -> ExitCode {
    if let ScaffoldError::Parse {
        path,
        text,
        diagnostics,
    } = err
    {
        render_manifest_diagnostics(&path, text, &diagnostics);
    } else {
        eprintln!("error: {err}");
    }
    ExitCode::from(1)
}

fn execute_check(args: CompileArgs) -> anyhow::Result<ExitCode> {
    let (_, built) = match compile_project(args)? {
        Ok(compiled) => compiled,
        Err(code) => return Ok(code),
    };
    report_package_warnings(&built);
    for package in &built {
        eprintln!(
            "checked {} v{}",
            package.name.as_str(),
            package.version.as_str()
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn execute_publish_local(args: CompileArgs) -> anyhow::Result<ExitCode> {
    let (store, built) = match compile_project(args)? {
        Ok(compiled) => compiled,
        Err(code) => return Ok(code),
    };
    report_package_warnings(&built);
    let dirs = match install_packages(&store, &built) {
        Ok(dirs) => dirs,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(ExitCode::from(1));
        }
    };

    for (package, dir) in built.iter().zip(&dirs) {
        eprintln!(
            "installed {} v{} -> {}",
            package.name.as_str(),
            package.version.as_str(),
            dir.display()
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn report_package_warnings(packages: &[BuiltPackage]) {
    for package in packages {
        for warning in &package.warnings {
            eprint!("{warning}");
        }
    }
}

// External dependencies resolve from the same local store publish-local
// installs into, so `check` needs the store too.
type CompiledProject = (PackageStore, Vec<BuiltPackage>);

fn compile_project(args: CompileArgs) -> anyhow::Result<Result<CompiledProject, ExitCode>> {
    let cwd = std::env::current_dir().context("resolving current directory")?;
    let Some(manifest_path) = find_manifest_upwards(&cwd) else {
        eprintln!(
            "error: no submilli.toml found in {} or any parent directory; run `submilli build init` to create one",
            cwd.display()
        );
        return Ok(Err(ExitCode::from(1)));
    };
    let manifest_text = fs::read_to_string(&manifest_path)
        .with_context(|| format!("reading {}", manifest_path.display()))?;
    let manifest_dir = manifest_path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();

    let manifest = match parse_manifest(&manifest_text, &manifest_dir) {
        Ok(manifest) => manifest,
        Err(diags) => {
            render_manifest_diagnostics(&manifest_path, manifest_text, &diags);
            return Ok(Err(ExitCode::from(1)));
        }
    };
    if let Err(err) = refresh_editor_files(&manifest_dir, &manifest) {
        eprintln!("error: {err}");
        return Ok(Err(ExitCode::from(1)));
    }

    let store = PackageStore::default();
    if let Err(code) = resolve_github_dependencies(&manifest, &manifest_dir, &store) {
        return Ok(Err(code));
    }
    let only = args.package.map(PackageName::new);
    match build_packages(&manifest, &manifest_dir, &store, only.as_ref()) {
        Ok(built) => {
            write_local_capabilities(&manifest, &manifest_dir, &built);
            Ok(Ok((store, built)))
        }
        Err(DriverError::Compile { rendered, .. }) => {
            eprint!("{rendered}");
            Ok(Err(ExitCode::from(1)))
        }
        Err(err) => {
            eprintln!("error: {err}");
            Ok(Err(ExitCode::from(1)))
        }
    }
}

/// Fetch and install the project's GitHub-dependency closure into the store,
/// then write `submilli.lock` beside `submilli.toml`. A lock that already pins
/// every declared dep (with the store satisfied) short-circuits the fetch.
/// Projects with no GitHub deps do nothing (and drop a stale lockfile).
fn resolve_github_dependencies(
    manifest: &ProjectManifest,
    manifest_dir: &Path,
    store: &PackageStore,
) -> Result<(), ExitCode> {
    let existing_lock = match Lockfile::read(manifest_dir) {
        Ok(lock) => lock,
        Err(err) => {
            eprintln!("error: {err}");
            return Err(ExitCode::from(1));
        }
    };

    let closure =
        match resolve_github_closure(store, manifest, &GithubRepoFetcher, existing_lock.as_ref()) {
            Ok(closure) => closure,
            Err(err) => {
                render_resolve_error(&err);
                return Err(ExitCode::from(1));
            }
        };
    if let Err(err) = install_plan(store, &closure.plan, true) {
        crate::commands::install::render_install_error(&err);
        return Err(ExitCode::from(1));
    }

    let write_result = if closure.locked.is_empty() {
        Lockfile::remove(manifest_dir)
    } else {
        Lockfile::new(closure.locked).write(manifest_dir)
    };
    if let Err(err) = write_result {
        eprintln!("error: {err}");
        return Err(ExitCode::from(1));
    }
    Ok(())
}

fn render_resolve_error(err: &ResolveError) {
    match err {
        ResolveError::Manifest {
            manifest_path,
            manifest_text,
            diagnostics,
            ..
        } => render_manifest_diagnostics(manifest_path, manifest_text.clone(), diagnostics),
        other => eprintln!("error: {other}"),
    }
}

/// Write each built package's derived `capabilities.yaml` next to its source,
/// so `build check`/`build test` surface the capabilities it requires (and
/// provides) without a `publish-local`. Best-effort: a write failure warns but
/// doesn't fail the build.
fn write_local_capabilities(
    manifest: &ProjectManifest,
    manifest_dir: &Path,
    built: &[BuiltPackage],
) {
    for pkg in built {
        let Some(entry) = manifest.packages.iter().find(|p| p.name == pkg.name) else {
            continue;
        };
        let dir = manifest_dir.join(entry.path.as_path());
        if let Err(err) = write_capabilities_file(&dir, &pkg.capabilities) {
            eprintln!(
                "warning: could not write capabilities.yaml for {}: {err}",
                pkg.name.as_str()
            );
        }
    }
}

fn render_manifest_diagnostics(
    manifest_path: &Path,
    manifest_text: String,
    diags: &[BuildDiagnostic],
) {
    let (sources, file) = Sources::single(manifest_path.display().to_string(), manifest_text);
    for diag in diags {
        let span = match diag.span {
            Some(span) => Span::new(file, span.start as u32, span.end as u32),
            None => Span::at(file),
        };
        let rendered = diagnostics::render(
            &interpreter::Diagnostic {
                severity: match diag.severity {
                    BuildSeverity::Error => Severity::Error,
                    BuildSeverity::Warning => Severity::Warning,
                },
                span,
                message: diag.message.clone(),
                help: diag.help.clone(),
                notes: Vec::new(),
            },
            &sources,
        );
        eprint!("{rendered}");
    }
}

/// `submilli build test` — discover `tests/**/*.test.{ts,subm}` under each scoped
/// package, compile each against its package surface + dependency closure (with
/// `submilli:test` available), run its `main()` in a fresh instance, and report
/// per-segment pass/fail. Capability checks run live against the default
/// allow-all policy (no blueprint in v1; configuring a test blueprint is a
/// follow-up). Each `label()` opens a reported segment; reaching the next label
/// (or a clean `main()` return) passes the prior one, and an uncaught
/// error/trap fails the open one. A file with no `label()` calls is one
/// anonymous test that passes iff `main()` returns cleanly.
mod test_runner {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::Path;
    use std::process::ExitCode;

    use anyhow::Context;
    use interpreter::runtime::{
        LinkedPackageModule, RuntimeConfig, StoreData, Vfs, install_package_modules_async,
        install_runtime_async, install_tenant_limits,
    };
    use interpreter::{
        BacktraceMode, PackageDeclaration, Sources, compile_script_owned_by, diagnostics,
        dispatch_main_async, render_backtrace,
    };
    use submilli_build::{
        Artifact, BuiltPackage, DriverError, PackageName, PackageStore, build_packages,
        find_manifest_upwards, parse_manifest,
    };
    use wasmtime::{Engine, Linker, Module};

    use super::{CompileArgs, render_manifest_diagnostics, resolve_github_dependencies};

    pub(super) fn execute_test(args: CompileArgs) -> anyhow::Result<ExitCode> {
        let cwd = std::env::current_dir().context("resolving current directory")?;
        let Some(manifest_path) = find_manifest_upwards(&cwd) else {
            eprintln!(
                "error: no submilli.toml found in {} or any parent directory; run `submilli build init` to create one",
                cwd.display()
            );
            return Ok(ExitCode::from(1));
        };
        let manifest_text = std::fs::read_to_string(&manifest_path)
            .with_context(|| format!("reading {}", manifest_path.display()))?;
        let manifest_dir = manifest_path
            .parent()
            .filter(|dir| !dir.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let manifest = match parse_manifest(&manifest_text, &manifest_dir) {
            Ok(manifest) => manifest,
            Err(diags) => {
                render_manifest_diagnostics(&manifest_path, manifest_text, &diags);
                return Ok(ExitCode::from(1));
            }
        };

        let store = PackageStore::default();
        if let Err(code) = resolve_github_dependencies(&manifest, &manifest_dir, &store) {
            return Ok(code);
        }
        let only = args.package.map(PackageName::new);
        let built = match build_packages(&manifest, &manifest_dir, &store, only.as_ref()) {
            Ok(built) => built,
            Err(DriverError::Compile { rendered, .. }) => {
                eprint!("{rendered}");
                return Ok(ExitCode::from(1));
            }
            Err(err) => {
                eprintln!("error: {err}");
                return Ok(ExitCode::from(1));
            }
        };
        super::report_package_warnings(&built);
        super::write_local_capabilities(&manifest, &manifest_dir, &built);

        // External (registry) dependencies aren't in `built`; load their wasm
        // from the store so the full closure can instantiate. Externals never
        // depend on freshly-built siblings, so linking them first is a valid
        // dependency order.
        let built_names: BTreeSet<&str> = built.iter().map(|b| b.name.as_str()).collect();
        let mut external_names: BTreeSet<String> = BTreeSet::new();
        for pkg in &built {
            for dep in &pkg.dependencies {
                if !built_names.contains(dep.name.as_str()) {
                    external_names.insert(dep.name.clone());
                }
            }
        }
        let externals = match store.load_closure(external_names.iter().map(String::as_str)) {
            Ok(artifacts) => artifacts,
            Err(err) => {
                eprintln!("error: loading dependencies: {err}");
                return Ok(ExitCode::from(1));
            }
        };

        let cfg = RuntimeConfig::default();
        let engine = cfg.engine()?;
        let package_modules = match compile_modules(&engine, &externals, &built) {
            Ok(modules) => modules,
            Err(err) => {
                eprintln!("error: {err}");
                return Ok(ExitCode::from(1));
            }
        };

        let test_decl = interpreter::stdlib::test::package_declaration();
        let declarations = declaration_refs(&externals, &built, &test_decl);
        let linked = linked_modules(&externals, &built, &package_modules);

        // Tests run for the scoped package(s): `-p` narrows to one, otherwise
        // every package in the manifest. The full `built` closure is linked
        // regardless so imports resolve.
        let path_by_name: BTreeMap<&str, &Path> = manifest
            .packages
            .iter()
            .map(|p| (p.name.as_str(), p.path.as_path()))
            .collect();
        let targets: Vec<&BuiltPackage> = match &only {
            Some(name) => built.iter().filter(|b| b.name == *name).collect(),
            None => built.iter().collect(),
        };

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("building tokio runtime")?;

        let ctx = TestContext {
            cfg: &cfg,
            engine: &engine,
            rt: &rt,
            declarations: &declarations,
            linked: &linked,
            manifest_dir: &manifest_dir,
        };

        let mut passed = 0usize;
        let mut failed = 0usize;
        let mut files = 0usize;
        for pkg in &targets {
            let Some(pkg_path) = path_by_name.get(pkg.name.as_str()) else {
                continue;
            };
            let tests_dir = manifest_dir.join(pkg_path).join("tests");
            let test_files = discover_test_files(&tests_dir)
                .with_context(|| format!("scanning {}", tests_dir.display()))?;
            for test_file in test_files {
                files += 1;
                let (p, f) = run_test_file(&ctx, pkg.name.as_str(), &test_file)?;
                passed += p;
                failed += f;
            }
            let (p, f) = check_doc_examples(pkg, pkg_path, &declarations);
            if p + f > 0 {
                files += 1;
            }
            passed += p;
            failed += f;
        }

        if files == 0 {
            eprintln!("no test files found (looked for tests/**/*.test.{{ts,subm}})");
            return Ok(ExitCode::SUCCESS);
        }
        println!("\n{passed} passed, {failed} failed across {files} files");
        if failed == 0 {
            Ok(ExitCode::SUCCESS)
        } else {
            Ok(ExitCode::from(1))
        }
    }

    /// Compile-check the `docs/readme.md` fenced `ts` examples of one package.
    /// Returns (passed, failed) counts, zero/zero when the docs have no examples.
    fn check_doc_examples(
        pkg: &BuiltPackage,
        pkg_path: &Path,
        declarations: &[&PackageDeclaration],
    ) -> (usize, usize) {
        let display_path = format!("{}/docs/readme.md", pkg_path.display());
        let mut passed = 0usize;
        let mut failed = 0usize;
        for (index, example) in submilli_build::extract_doc_examples(&pkg.documentation)
            .iter()
            .enumerate()
        {
            let label = format!("{display_path} :: example {} (compile)", index + 1);
            match submilli_build::compile_check_doc_example(example, &display_path, declarations) {
                Ok(()) => {
                    println!("ok   {label}");
                    passed += 1;
                }
                Err(rendered) => {
                    eprint!("{rendered}");
                    println!("FAIL {label}");
                    failed += 1;
                }
            }
        }
        (passed, failed)
    }

    fn compile_modules(
        engine: &Engine,
        externals: &[Artifact],
        built: &[BuiltPackage],
    ) -> anyhow::Result<Vec<Module>> {
        let mut modules = Vec::with_capacity(externals.len() + built.len());
        for artifact in externals {
            let name = artifact.metadata.package_name.clone();
            modules.push(
                Module::new(engine, &artifact.wasm)
                    .map_err(|err| anyhow::anyhow!("compiling dependency {name}: {err}"))?,
            );
        }
        for pkg in built {
            let name = pkg.name.as_str();
            modules.push(
                Module::new(engine, &pkg.wasm)
                    .map_err(|err| anyhow::anyhow!("compiling package {name}: {err}"))?,
            );
        }
        Ok(modules)
    }

    fn declaration_refs<'a>(
        externals: &'a [Artifact],
        built: &'a [BuiltPackage],
        test_decl: &'a PackageDeclaration,
    ) -> Vec<&'a PackageDeclaration> {
        let mut refs: Vec<&PackageDeclaration> = Vec::new();
        refs.extend(externals.iter().map(|a| &a.package_declaration));
        refs.extend(built.iter().map(|b| &b.declaration));
        refs.push(test_decl);
        refs
    }

    fn linked_modules<'a>(
        externals: &'a [Artifact],
        built: &'a [BuiltPackage],
        modules: &'a [Module],
    ) -> Vec<LinkedPackageModule<'a>> {
        let mut linked = Vec::with_capacity(modules.len());
        let (external_modules, built_modules) = modules.split_at(externals.len());
        for (artifact, module) in externals.iter().zip(external_modules) {
            linked.push(LinkedPackageModule {
                module,
                declaration: &artifact.package_declaration,
                type_info: &artifact.type_info,
            });
        }
        for (pkg, module) in built.iter().zip(built_modules) {
            linked.push(LinkedPackageModule {
                module,
                declaration: &pkg.declaration,
                type_info: &pkg.type_info,
            });
        }
        linked
    }

    // Test-time secret bridge: `secrets.get("NAME")` resolves `NAME` from the
    // environment, with a `.env` file in the manifest dir loaded first (real
    // environment variables win). So `JINA_API_KEY=… submilli build test` — or a
    // `.env` holding it — makes a token available to tests; an unset name reads
    // as a missing secret (`None`). Loading `.env` is test-only.
    struct EnvSecretProvider {
        vars: std::collections::HashMap<String, String>,
    }

    impl EnvSecretProvider {
        fn load(manifest_dir: &Path) -> Self {
            let mut vars = std::collections::HashMap::new();
            if let Ok(text) = std::fs::read_to_string(manifest_dir.join(".env")) {
                for line in text.lines() {
                    if let Some((k, v)) = parse_dotenv_line(line) {
                        vars.insert(k, v);
                    }
                }
            }
            vars.extend(std::env::vars());
            Self { vars }
        }
    }

    impl interpreter::runtime::SecretProvider for EnvSecretProvider {
        fn get<'a>(
            &'a self,
            name: &'a str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Option<String>, String>> + Send + 'a>,
        > {
            let value = self.vars.get(name).cloned();
            Box::pin(async move { Ok(value) })
        }
    }

    /// Parse one `.env` line: `KEY=VALUE`, tolerating a leading `export`,
    /// surrounding quotes, blank lines, and `#` comments. Returns `None` for
    /// lines that don't bind a name.
    fn parse_dotenv_line(line: &str) -> Option<(String, String)> {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
        let (key, value) = line.split_once('=')?;
        let key = key.trim();
        if key.is_empty() {
            return None;
        }
        let value = value.trim();
        let unquoted = value
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
            .unwrap_or(value);
        Some((key.to_string(), unquoted.to_string()))
    }

    /// The pieces every test file in a run links against, unchanged across
    /// files: the compiled package closure and the environment around it.
    struct TestContext<'a> {
        cfg: &'a RuntimeConfig,
        engine: &'a Engine,
        rt: &'a tokio::runtime::Runtime,
        declarations: &'a [&'a PackageDeclaration],
        linked: &'a [LinkedPackageModule<'a>],
        manifest_dir: &'a Path,
    }

    fn run_test_file(
        ctx: &TestContext<'_>,
        package_name: &str,
        test_file: &Path,
    ) -> anyhow::Result<(usize, usize)> {
        let TestContext {
            cfg,
            engine,
            rt,
            declarations,
            linked,
            manifest_dir,
        } = *ctx;
        let source = std::fs::read_to_string(test_file)
            .with_context(|| format!("reading {}", test_file.display()))?;
        let filename = test_file
            .strip_prefix(manifest_dir)
            .unwrap_or(test_file)
            .to_string_lossy()
            .into_owned();
        let (sources, file) = Sources::single(filename.clone(), source.clone());

        let (bytes, type_info) = match compile_script_owned_by(
            package_name,
            &source,
            &filename,
            file,
            declarations,
            &[],
        ) {
            Ok(compiled) => {
                for w in &compiled.warnings {
                    eprint!("{}", diagnostics::render(w, &sources));
                }
                (compiled.wasm, compiled.type_info)
            }
            Err(diags) => {
                for d in &diags {
                    eprint!("{}", diagnostics::render(d, &sources));
                }
                println!("FAIL {filename}  (compile error)");
                return Ok((0, 1));
            }
        };

        let module = Module::new(engine, &bytes)
            .map_err(|err| anyhow::anyhow!("compiling {filename}: {err}"))?;

        let outcome = rt.block_on(async {
            let mut data = StoreData::with_vfs_and_cap(Vfs::tempdir()?, cfg.max_store_bytes);
            data.secret_provider = std::sync::Arc::new(EnvSecretProvider::load(manifest_dir));
            let mut store = cfg.store_async(engine, data)?;
            install_tenant_limits(&mut store);
            let mut linker = Linker::<StoreData>::new(engine);
            install_runtime_async(&mut linker, &mut store).await?;
            interpreter::stdlib::test::install(&mut linker)?;
            install_package_modules_async(&mut linker, &mut store, linked).await?;
            // The test script compiles as its own module; install its TypeInfo so
            // `JSON.stringify` of a typed object resolves against the test package.
            store.data_mut().install_type_info(type_info);
            let instance = linker.instantiate_async(&mut store, &module).await?;
            let _watchdog = cfg.arm_timeout(engine);
            let result = dispatch_main_async(&mut store, &instance).await;
            let labels = store.data().test_labels.borrow().clone();
            Ok::<_, anyhow::Error>((result, labels))
        });

        let (result, labels) = match outcome {
            Ok(pair) => pair,
            Err(err) => {
                // Linking/instantiation failure is a property of the file, not
                // a reason to abort the whole run.
                println!("FAIL {filename}  ({err})");
                return Ok((0, 1));
            }
        };

        Ok(report_outcome(&filename, result, &labels, &sources, file))
    }

    /// Map a finished run to (passed, failed) segment counts and print a line
    /// per segment. Segments before the open one always passed (execution
    /// reached the next `label`); the open segment fails iff `main` threw.
    fn report_outcome(
        filename: &str,
        result: wasmtime::Result<Option<String>>,
        labels: &[String],
        sources: &Sources,
        file: interpreter::FileId,
    ) -> (usize, usize) {
        let segment = |label: Option<&str>| match label {
            Some(l) => format!("{filename} :: {l}"),
            None => filename.to_string(),
        };
        match result {
            Ok(_) if labels.is_empty() => {
                println!("ok   {}", segment(None));
                (1, 0)
            }
            Ok(_) => {
                for l in labels {
                    println!("ok   {}", segment(Some(l)));
                }
                (labels.len(), 0)
            }
            Err(err) => {
                let passed = labels.len().saturating_sub(1);
                for l in &labels[..passed] {
                    println!("ok   {}", segment(Some(l)));
                }
                println!("FAIL {}", segment(labels.last().map(String::as_str)));
                if let Some(bt) = render_backtrace(&err, sources, file, BacktraceMode::Full) {
                    eprint!("{bt}");
                } else {
                    eprintln!("error: {err}");
                }
                (passed, 1)
            }
        }
    }

    /// Recursively collect `*.test.ts` / `*.test.subm` files under `dir`,
    /// sorted for deterministic order. A missing `tests/` directory yields no
    /// files. Both extensions are accepted (forgiveness principle): `.ts` is
    /// the canonical scaffolded extension, `.subm` stays valid.
    fn discover_test_files(dir: &Path) -> std::io::Result<Vec<std::path::PathBuf>> {
        let mut out = Vec::new();
        collect_test_files(dir, &mut out)?;
        out.sort();
        Ok(out)
    }

    fn is_test_file(name: &str) -> bool {
        name.ends_with(".test.ts") || name.ends_with(".test.subm")
    }

    fn collect_test_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) -> std::io::Result<()> {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err),
        };
        for entry in entries {
            let path = entry?.path();
            if path.is_dir() {
                collect_test_files(&path, out)?;
            } else if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(is_test_file)
            {
                out.push(path);
            }
        }
        Ok(())
    }
}
