//! `submilli build` — package-project tooling around `submilli.toml`:
//! `init` / `new` scaffold the manifest and package folders, `check` compiles
//! the packages in dependency order, `authority-map` emits their authority
//! graphs, and `publish-local` compiles and installs the artifacts into the
//! local package store.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Context;
use interpreter::{Severity, Sources, Span, diagnostics};
use submilli_build::{
    BuildDiagnostic, BuildSeverity, BuiltPackage, DependencyKind, DriverError, InstallPreparation,
    Lockfile, PackageName, PackageStore, ProjectManifest, ResolveError, ScaffoldError, add_package,
    build_packages, find_manifest_upwards, init_project, install_packages, is_valid_package_name,
    parse_manifest, refresh_dependency_types, refresh_editor_files, resolve_github_closure,
    write_capabilities_file,
};

use crate::commands::github::retry;

mod security_review;

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
    Test(TestArgs),
    /// Compile packages and print their full authority call graph as JSON.
    AuthorityMap(CompileArgs),
    /// Review package authorization with Codex, Claude Code, or Copilot CLI.
    SecurityReview(security_review::Args),
}

#[derive(clap::Args)]
struct CompileArgs {
    /// Compile only this package and its sibling dependencies.
    #[arg(short = 'p', long = "package")]
    package: Option<String>,

    /// Fail on code warnings; also enabled by SUBMILLI_DENY_WARNINGS=1.
    #[arg(long)]
    deny_warnings: bool,
}

#[derive(clap::Args)]
#[command(
    after_help = "Tests receive no credentials by default. Credential precedence (highest first): --env-var > --env-file > --all-env, regardless of argument order."
)]
struct TestArgs {
    #[command(flatten)]
    compile: CompileArgs,

    /// Supply all Unicode process environment variables as test credentials.
    #[arg(long)]
    all_env: bool,

    /// Supply a process variable (repeatable or comma-separated); fail if unset or non-Unicode.
    #[arg(long, value_name = "NAME", value_delimiter = ',')]
    env_var: Vec<String>,

    /// Read credentials from a file; relative paths use the current directory. Fail if unreadable.
    #[arg(long, value_name = "PATH")]
    env_file: Option<PathBuf>,

    /// Skip network.test.{ts,subm} and network_*.test.{ts,subm} anywhere under tests/.
    #[arg(long)]
    skip_network: bool,
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
            BuildCmd::AuthorityMap(_) => "build.authority_map",
            BuildCmd::SecurityReview(_) => "build.security_review",
        }
    }

    pub(crate) fn metric_flags(&self) -> Vec<(&'static str, bool)> {
        match &self.cmd {
            BuildCmd::Check(compile)
            | BuildCmd::PublishLocal(compile)
            | BuildCmd::AuthorityMap(compile) => {
                vec![("has_package", compile.package.is_some())]
            }
            BuildCmd::Test(test) => vec![("has_package", test.compile.package.is_some())],
            BuildCmd::SecurityReview(review) => vec![("has_package", review.package.is_some())],
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
        BuildCmd::AuthorityMap(compile) => execute_authority_map(compile),
        BuildCmd::SecurityReview(review) => security_review::execute(review),
    }
}

#[derive(serde::Serialize)]
struct AuthorityMapOutput<'a> {
    schema_version: u32,
    packages: Vec<AuthorityPackageOutput<'a>>,
}

#[derive(serde::Serialize)]
struct AuthorityPackageOutput<'a> {
    name: &'a str,
    #[serde(flatten)]
    map: &'a interpreter::AuthorityMap,
}

fn execute_authority_map(args: CompileArgs) -> anyhow::Result<ExitCode> {
    let selected = args.package.clone();
    let (_, built) = match compile_project(args)? {
        Ok(compiled) => compiled,
        Err(code) => return Ok(code),
    };
    report_package_warnings(&built);
    let mut packages = built
        .iter()
        .filter(|package| {
            selected
                .as_deref()
                .is_none_or(|name| package.name.as_str() == name)
        })
        .map(|package| AuthorityPackageOutput {
            name: package.name.as_str(),
            map: &package.authority_map,
        })
        .collect::<Vec<_>>();
    packages.sort_by(|left, right| left.name.cmp(right.name));
    let output = AuthorityMapOutput {
        schema_version: 2,
        packages,
    };
    let mut stdout = io::stdout().lock();
    serde_json::to_writer_pretty(&mut stdout, &output).context("serializing authority map")?;
    writeln!(stdout).context("writing authority map")?;
    Ok(ExitCode::SUCCESS)
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
    eprintln!("created {}", scaffolded.docs_readme.display());
    eprintln!("created {}", scaffolded.readme.display());
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
    eprintln!("created {}", scaffolded.docs_readme.display());
    eprintln!("created {}", scaffolded.readme.display());
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

fn publish_dependencies(
    preparation: InstallPreparation,
    built: &[BuiltPackage],
    deny_warnings: bool,
) -> Result<(), ExitCode> {
    check_package_warnings(&preparation, built, deny_warnings)?;
    publish_prepared_dependencies(preparation)
}

fn check_package_warnings(
    preparation: &InstallPreparation,
    built: &[BuiltPackage],
    deny_warnings: bool,
) -> Result<(), ExitCode> {
    for warning in &preparation.warnings {
        eprint!("{warning}");
    }
    let count = preparation.warnings.len()
        + built
            .iter()
            .map(|package| package.warnings.len())
            .sum::<usize>();
    if deny_warnings && count > 0 {
        report_package_warnings(built);
        eprintln!("error: {}", submilli_build::warning_denial_message(count));
        return Err(ExitCode::from(1));
    }
    Ok(())
}

fn publish_prepared_dependencies(preparation: InstallPreparation) -> Result<(), ExitCode> {
    preparation.publish(false).map_err(|error| {
        crate::commands::install::render_install_error(&error);
        ExitCode::from(1)
    })
}

// External dependencies resolve from the same local store publish-local
// installs into, so `check` needs the store too.
type CompiledProject = (PackageStore, Vec<BuiltPackage>);

fn compile_project(args: CompileArgs) -> anyhow::Result<Result<CompiledProject, ExitCode>> {
    let deny_warnings = args.deny_warnings || submilli_build::deny_warnings_from_env();
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
    let preparation = match resolve_github_dependencies(&manifest, &manifest_dir, &store) {
        Ok(preparation) => preparation,
        Err(code) => return Ok(Err(code)),
    };
    if let Err(code) =
        refresh_dependency_editor_types(&manifest, &manifest_dir, preparation.store())
    {
        return Ok(Err(code));
    }
    let only = args.package.map(PackageName::new);
    match build_packages(&manifest, &manifest_dir, preparation.store(), only.as_ref()) {
        Ok(built) => {
            if let Err(code) = publish_dependencies(preparation, &built, deny_warnings) {
                return Ok(Err(code));
            }
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
) -> Result<InstallPreparation, ExitCode> {
    let existing_lock = match Lockfile::read(manifest_dir) {
        Ok(lock) => lock,
        Err(err) => {
            eprintln!("error: {err}");
            return Err(ExitCode::from(1));
        }
    };

    let resolved = retry::with_authentication_retry(|auth| {
        resolve_github_closure(store, manifest, &auth.fetcher(), existing_lock.as_ref())
            .inspect_err(render_resolve_error)
    });
    let Ok(closure) = resolved else {
        return Err(ExitCode::from(1));
    };
    let mut preparation = InstallPreparation::new(store).map_err(|error| {
        crate::commands::install::render_install_error(&error);
        ExitCode::from(1)
    })?;
    if let Err(err) = preparation.prepare_plan(&closure.plan, true) {
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
    Ok(preparation)
}

/// Give the editor declarations for the dependencies that come from the store.
///
/// Each dependency's closure loads on its own, so one the store can't load
/// costs only its own declarations; the build reports it, naming the package
/// that needs it. A store package named like a package of this project gets no
/// declarations, since the project's own source is what the editor should
/// open, but the dependencies may still borrow its types.
fn refresh_dependency_editor_types(
    manifest: &ProjectManifest,
    manifest_dir: &Path,
    store: &PackageStore,
) -> Result<(), ExitCode> {
    let project_names: BTreeSet<&str> = manifest
        .packages
        .iter()
        .map(|package| package.name.as_str())
        .collect();
    let external_names: BTreeSet<&str> = manifest
        .packages
        .iter()
        .flat_map(|package| &package.dependencies)
        .filter(|dependency| dependency.kind != DependencyKind::Sibling)
        .map(|dependency| dependency.name.as_str())
        .collect();
    let mut artifacts = BTreeMap::new();
    for name in external_names {
        let Ok(closure) = store.load_closure([name]) else {
            continue;
        };
        for artifact in closure {
            artifacts
                .entry(artifact.metadata.package_name.clone())
                .or_insert(artifact);
        }
    }
    let (project_packages, dependencies): (Vec<_>, Vec<_>) = artifacts
        .values()
        .map(|artifact| &artifact.package_declaration)
        .partition(|declaration| project_names.contains(declaration.package_name.as_str()));
    if let Err(err) = refresh_dependency_types(manifest_dir, &dependencies, &project_packages) {
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
        } => render_manifest_diagnostics(manifest_path, manifest_text.as_str(), diagnostics),
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

pub(super) fn render_manifest_diagnostics(
    manifest_path: &Path,
    manifest_text: impl AsRef<str>,
    diags: &[BuildDiagnostic],
) {
    use interpreter::rendering::{RenderError, RenderLimits, failure_text};
    let primary = diags
        .first()
        .map_or("invalid manifest", |diag| diag.message.as_str());
    let (sources, file) = match Sources::single(manifest_path.display().to_string(), manifest_text)
    {
        Ok(source) => source,
        Err(error) => {
            eprintln!("{}", failure_text(primary, &error.into()));
            return;
        }
    };
    let views = diags.iter().map(|diag| {
        let span = match diag.span {
            Some(span) => {
                let start = u32::try_from(span.start)
                    .map_err(|_| RenderError::InvalidMetadata("invalid manifest span"))?;
                let end = u32::try_from(span.end)
                    .map_err(|_| RenderError::InvalidMetadata("invalid manifest span"))?;
                Span::new(file, start, end)?
            }
            None => Span::at(file),
        };
        Ok(diagnostics::DiagnosticView {
            severity: match diag.severity {
                BuildSeverity::Error => Severity::Error,
                BuildSeverity::Warning => Severity::Warning,
            },
            span,
            message: &diag.message,
            help: &diag.help,
            notes: &[],
        })
    });
    match diagnostics::render_views(views, &sources, RenderLimits::collection()) {
        Ok(rendered) => eprint!("{}", rendered.text),
        Err(error) => eprintln!("{}", failure_text(primary, &error)),
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
        BacktraceMode, PackageDeclaration, Sources, compile_script_owned_by, dispatch_main_async,
        failure_message, instantiate_program_async,
    };
    use submilli_build::{
        Artifact, ArtifactSource, BuiltPackage, DriverError, PackageName, PackageStore,
        build_packages, find_manifest_upwards, parse_manifest,
    };
    use wasmtime::{Engine, Linker, Module};

    use super::{TestArgs, render_manifest_diagnostics, resolve_github_dependencies};

    pub(super) fn execute_test(args: TestArgs) -> anyhow::Result<ExitCode> {
        let secret_provider = std::sync::Arc::new(EnvSecretProvider::load(&args)?);
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
        let deny_warnings = args.compile.deny_warnings || submilli_build::deny_warnings_from_env();
        let preparation = match resolve_github_dependencies(&manifest, &manifest_dir, &store) {
            Ok(preparation) => preparation,
            Err(code) => return Ok(code),
        };
        let only = args.compile.package.map(PackageName::new);
        let built =
            match build_packages(&manifest, &manifest_dir, preparation.store(), only.as_ref()) {
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
        if let Err(code) = super::check_package_warnings(&preparation, &built, deny_warnings) {
            return Ok(code);
        }
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
        let externals = match preparation
            .store()
            .load_closure(external_names.iter().map(String::as_str))
        {
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
        let package_sources = package_sources(&externals, &built);

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
            package_sources: &package_sources,
            manifest_dir: &manifest_dir,
            secret_provider: &secret_provider,
        };

        let mut warning_count = 0usize;
        let mut passed = 0usize;
        let mut failed = 0usize;
        let mut files = 0usize;
        let mut skipped = 0usize;
        for pkg in &targets {
            let Some(pkg_path) = path_by_name.get(pkg.name.as_str()) else {
                continue;
            };
            let tests_dir = manifest_dir.join(pkg_path).join("tests");
            let test_files = discover_test_files(&tests_dir)
                .with_context(|| format!("scanning {}", tests_dir.display()))?;
            for test_file in test_files {
                if args.skip_network && is_network_test_file(&test_file) {
                    let path = test_file.strip_prefix(&manifest_dir).unwrap_or(&test_file);
                    println!("skip {} (--skip-network)", path.display());
                    skipped += 1;
                    continue;
                }
                files += 1;
                let (p, f) = run_test_file(
                    &ctx,
                    pkg.name.as_str(),
                    &test_file,
                    deny_warnings,
                    &mut warning_count,
                )?;
                passed += p;
                failed += f;
            }
            let (p, f) = check_doc_examples(
                pkg,
                pkg_path,
                &declarations,
                deny_warnings,
                &mut warning_count,
            );
            if p + f > 0 {
                files += 1;
            }
            passed += p;
            failed += f;
        }

        if files == 0 && skipped == 0 {
            eprintln!("no test files found (looked for tests/**/*.test.{{ts,subm}})");
            if let Err(code) = super::publish_prepared_dependencies(preparation) {
                return Ok(code);
            }
            return Ok(ExitCode::SUCCESS);
        }
        println!("\n{passed} passed, {failed} failed across {files} files");
        if skipped > 0 {
            println!("{skipped} network test files skipped (--skip-network)");
        }
        if deny_warnings && warning_count > 0 {
            eprintln!(
                "error: {}",
                submilli_build::warning_denial_message(warning_count)
            );
        }
        if failed > 0 {
            return Ok(ExitCode::from(1));
        }
        if let Err(code) = super::publish_prepared_dependencies(preparation) {
            return Ok(code);
        }
        Ok(ExitCode::SUCCESS)
    }

    /// Compile-check the `docs/readme.md` fenced `ts` examples of one package.
    /// Returns (passed, failed) counts, zero/zero when the docs have no examples.
    fn check_doc_examples(
        pkg: &BuiltPackage,
        pkg_path: &Path,
        declarations: &[&PackageDeclaration],
        deny_warnings: bool,
        warning_count: &mut usize,
    ) -> (usize, usize) {
        let display_path = format!("{}/docs/readme.md", pkg_path.display());
        let mut passed = 0usize;
        let mut failed = 0usize;
        for (index, example) in submilli_build::extract_doc_examples(&pkg.documentation)
            .iter()
            .enumerate()
        {
            let label = format!("{display_path} :: example {} (compile)", index + 1);
            match submilli_build::compile_doc_example_warnings(example, &display_path, declarations)
            {
                Ok(warnings) => {
                    for warning in &warnings {
                        eprint!("{warning}");
                    }
                    *warning_count += warnings.len();
                    if deny_warnings && !warnings.is_empty() {
                        println!("FAIL {label} (warnings denied)");
                        failed += 1;
                    } else {
                        println!("ok   {label}");
                        passed += 1;
                    }
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

    /// Every linked package's name and modules, so a test's package frames
    /// render with their source.
    fn package_sources<'a>(
        externals: &'a [Artifact],
        built: &'a [BuiltPackage],
    ) -> Vec<(&'a str, &'a [ArtifactSource])> {
        let externals = externals.iter().map(|artifact| {
            (
                artifact.package_declaration.package_name.as_str(),
                artifact.sources.as_slice(),
            )
        });
        let built = built
            .iter()
            .map(|pkg| (pkg.name.as_str(), pkg.sources.as_slice()));
        externals.chain(built).collect()
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

    /// An immutable snapshot of only the credentials explicitly selected for this run.
    struct EnvSecretProvider {
        vars: std::collections::HashMap<String, String>,
    }

    impl EnvSecretProvider {
        fn load(args: &TestArgs) -> anyhow::Result<Self> {
            let mut vars = std::collections::HashMap::new();
            if args.all_env {
                vars.extend(std::env::vars_os().filter_map(|(name, value)| {
                    Some((name.into_string().ok()?, value.into_string().ok()?))
                }));
            }
            if let Some(path) = &args.env_file {
                let text = std::fs::read_to_string(path)
                    .with_context(|| format!("reading credential file {}", path.display()))?;
                vars.extend(text.lines().filter_map(parse_dotenv_line));
            }
            for name in &args.env_var {
                let value = match std::env::var(name) {
                    Ok(value) => value,
                    Err(std::env::VarError::NotPresent) => {
                        anyhow::bail!("--env-var {name}: process variable is not set");
                    }
                    Err(std::env::VarError::NotUnicode(_)) => {
                        anyhow::bail!("--env-var {name}: process variable is not valid Unicode");
                    }
                };
                vars.insert(name.clone(), value);
            }
            Ok(Self { vars })
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
        package_sources: &'a [(&'a str, &'a [ArtifactSource])],
        manifest_dir: &'a Path,
        secret_provider: &'a std::sync::Arc<EnvSecretProvider>,
    }

    fn run_test_file(
        ctx: &TestContext<'_>,
        package_name: &str,
        test_file: &Path,
        deny_warnings: bool,
        warning_count: &mut usize,
    ) -> anyhow::Result<(usize, usize)> {
        let TestContext {
            cfg,
            engine,
            rt,
            declarations,
            linked,
            package_sources,
            manifest_dir,
            secret_provider,
        } = *ctx;
        let source = std::fs::read_to_string(test_file)
            .with_context(|| format!("reading {}", test_file.display()))?;
        let filename = test_file
            .strip_prefix(manifest_dir)
            .unwrap_or(test_file)
            .to_string_lossy()
            .into_owned();
        let (mut sources, file) = Sources::single(filename.clone(), source.clone())?;

        let (bytes, type_info) = match compile_script_owned_by(
            package_name,
            &source,
            &filename,
            file,
            declarations,
            &[],
        ) {
            Ok(compiled) => {
                crate::commands::check::render(&compiled.warnings, &sources)?;
                *warning_count += compiled.warnings.len();
                if deny_warnings && !compiled.warnings.is_empty() {
                    println!("FAIL {filename} (warnings denied)");
                    return Ok((0, 1));
                }
                (compiled.wasm, compiled.type_info)
            }
            Err(diags) => {
                crate::commands::check::render(&diags, &sources)?;
                println!("FAIL {filename}  (compile error)");
                return Ok((0, 1));
            }
        };

        // After compiling, so the file's diagnostics resolve against it alone.
        for &(package, modules) in package_sources {
            for module in modules {
                sources.add_package_module(package, module.path.as_str(), &module.text)?;
            }
        }

        let module = Module::new(engine, &bytes)
            .map_err(|err| anyhow::anyhow!("compiling {filename}: {err}"))?;

        let outcome = rt.block_on(async {
            let mut data = StoreData::with_vfs_and_cap(Vfs::tempdir()?, cfg.max_store_bytes);
            data.secret_provider = secret_provider.clone();
            let mut store = cfg.store_async(engine, data)?;
            install_tenant_limits(&mut store);
            let mut linker = Linker::<StoreData>::new(engine);
            install_runtime_async(&mut linker, &mut store).await?;
            interpreter::stdlib::test::install(&mut linker)?;
            let _watchdog = cfg.arm_timeout(engine)?;
            // The packages' top-level statements, then the file's, run here; their
            // failure is the file's first segment failing, as one in `main` would be.
            let result = async {
                install_package_modules_async(&mut linker, &mut store, linked).await?;
                // The test script compiles as its own module; install its TypeInfo so
                // `JSON.stringify` of a typed object resolves against the test package.
                store.data_mut().install_type_info(type_info);
                let instance = instantiate_program_async(&linker, &mut store, &module).await?;
                dispatch_main_async(&mut store, &instance).await
            }
            .await;
            let labels = store.data().test_labels.borrow().clone();
            Ok::<_, anyhow::Error>((result, labels))
        });

        let (result, labels) = match outcome {
            Ok(pair) => pair,
            Err(err) => {
                // A setup failure is a property of the file, not a reason to
                // abort the whole run.
                println!("FAIL {filename}  ({err})");
                return Ok((0, 1));
            }
        };

        Ok(report_outcome(&filename, result, &labels, &sources, file))
    }

    /// Map a finished run to (passed, failed) segment counts and print a line
    /// per segment. Segments before the open one always passed (execution
    /// reached the next `label`); the open segment fails iff the run ended in
    /// an error, in top-level statements or in `main`.
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
                match interpreter::backtrace::render_checked(
                    &err,
                    sources,
                    file,
                    BacktraceMode::Full,
                ) {
                    Ok(Some(rendered)) => eprint!("{}", rendered.text),
                    Ok(None) => eprintln!("error: {}", failure_message(&err)),
                    Err(failure) => eprintln!(
                        "{}",
                        interpreter::rendering::failure_text(&failure_message(&err), &failure)
                    ),
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

    fn is_network_test_file(path: &Path) -> bool {
        path.file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| {
                name.strip_suffix(".test.ts")
                    .or_else(|| name.strip_suffix(".test.subm"))
            })
            .is_some_and(|stem| stem == "network" || stem.starts_with("network_"))
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
