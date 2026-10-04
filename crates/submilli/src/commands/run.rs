//! `submilli run` — console output goes to stderr; JSON-encoded `main` return goes to stdout.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use interpreter::runtime::limits::ExecutionUsage;

use anyhow::{Context, anyhow};
use interpreter::diagnostics;
use interpreter::runtime::{
    DEFAULT_MAX_EXECUTION_TOKENS, ExecutionTokenBudget, HttpClient, LinkedPackageModule, LlmLimits,
    McpTransport, NetworkPolicy, ReqwestHttpClient, RuntimeConfig, SharedTokenBudget, StoreData,
    Vfs, install_package_modules_async, install_runtime_async, install_tenant_limits,
};
use interpreter::{
    BacktraceMode, Sources, dispatch_main_async, failure_message, instantiate_program_async,
    render_backtrace,
};
use submilli_blueprint::{Blueprint, VarBindings, resolve_variables};
use submilli_build::{Artifact, PackageStore};
use submilli_shared::llm::provider::DEFAULT_MAX_CONCURRENCY;
use submilli_shared::llm::{BlueprintLlmProvider, HttpModelDispatch, ModelDispatch};
use submilli_shared::mcp::StreamableHttpTransport;
use submilli_shared::mcp::discovery::{DiscoveryAuth, McpCatalog, discover_all_local};
use submilli_shared::mcp_token::OAuthTokenManager;
use submilli_shared::secret_store::SecretStore;
use submilli_shared::{BlueprintAuthProxy, BlueprintSecretProvider, PolicyCheck};
use wasmtime::{Linker, Module};

#[derive(clap::Args)]
pub struct Args {
    /// Path to the `.ts` or `.subm` script to run.
    script: PathBuf,

    /// Fuel budget. Defaults to the runtime's
    /// `RuntimeConfig::default()` value.
    #[arg(long)]
    fuel: Option<u64>,

    /// Print fuel, peak accounted memory, and timings to stderr after execution.
    #[arg(long)]
    report: bool,

    /// Maximum wasm stack in bytes.
    #[arg(long = "max-stack")]
    max_stack: Option<usize>,

    /// Wall-clock deadline in milliseconds. Omit for no deadline.
    #[arg(long)]
    timeout: Option<u64>,

    /// Directory to expose as the script's VFS root. The future
    /// `submilli:fs.*` host functions will resolve paths
    /// against it. Omit to allocate a fresh tempdir under the OS
    /// temp root for the duration of the run.
    #[arg(long)]
    vfs: Option<PathBuf>,

    /// Apply a blueprint's policy (capability gating, deny-by-default) and
    /// `auth_proxy:` secret injection to this local run. Without it, the run is
    /// unrestricted (allow-all). `store:` secrets resolve from the local
    /// secret store, and authenticated
    /// `@mcp/<server>` servers are called in-process — no running server needed.
    #[arg(long)]
    blueprint: Option<PathBuf>,

    /// Bind a blueprint variable for this run, `NAME=VALUE` (repeatable), the
    /// way an application binds it when it opens a session. Requires
    /// `--blueprint`; the blueprint must declare the variable, and its
    /// `required` variables must all be bound.
    #[arg(long = "var", value_name = "NAME=VALUE", requires = "blueprint")]
    vars: Vec<String>,

    /// Tokens this run's `submilli:llm` calls may spend in total. A run that
    /// asks for more raises a catchable `QuotaExceededError` rather than being billed.
    ///
    /// Finite by default, deliberately: unlike `submilli:session`, whose state
    /// is memory-only, a blueprint-configured provider spends real money against
    /// the operator's credential, and a CLI run has no server-wide ceiling
    /// behind it. [default: 1000000]
    /// Env: `$SUBMILLI_MAX_EXECUTION_LLM_TOKENS`.
    #[arg(long, value_name = "TOKENS")]
    max_llm_tokens: Option<u64>,

    /// Prompts one `llm.batch` dispatches at once. [default: 4]
    /// Env: `$SUBMILLI_MAX_LLM_CONCURRENCY`.
    #[arg(long, value_name = "PROMPTS")]
    max_llm_concurrency: Option<usize>,
}

/// The CLI's rung of the same ladder the server walks: a flag, then an explicit
/// `SUBMILLI_*` variable, then the built-in default.
///
/// There is no config-file tier because `submilli run` has no config file — the
/// blueprint is the only file it reads.
///
/// `Ok(None)` means "nothing set at any rung, use the default". A set-but-bad
/// variable is `Err`, never `Ok(None)`: falling through to the default would
/// silently spend more than the operator's typo'd ceiling asked for.
///
/// Reading goes through a `lookup` rather than `std::env::var` directly, the way
/// [`submilli_server`'s own resolver does][1], so the precedence rules are
/// testable without mutating the process environment out from under tests
/// running in parallel threads.
///
/// [1]: https://docs.rs/submilli-server
fn env_ladder<T: std::str::FromStr + Copy>(
    flag: Option<T>,
    name: &str,
    expected: &str,
    lookup: &impl Fn(&str) -> Option<String>,
) -> anyhow::Result<Option<T>> {
    if let Some(value) = flag {
        return Ok(Some(value));
    }
    match lookup(name).map(|v| v.trim().to_owned()) {
        Some(raw) if !raw.is_empty() => raw
            .parse()
            .map(Some)
            .map_err(|_| anyhow!("${name}: expected {expected}, got `{raw}`")),
        _ => Ok(None),
    }
}

/// This run's token ceiling and fan-out bound, resolved off the same ladder and
/// with the same defaults the server uses.
fn llm_settings(args: &Args) -> anyhow::Result<(LlmLimits, usize)> {
    llm_settings_from(args, &|name| std::env::var(name).ok())
}

fn llm_settings_from(
    args: &Args,
    lookup: &impl Fn(&str) -> Option<String>,
) -> anyhow::Result<(LlmLimits, usize)> {
    let per_execution_tokens = env_ladder(
        args.max_llm_tokens,
        "SUBMILLI_MAX_EXECUTION_LLM_TOKENS",
        "a whole number of tokens",
        lookup,
    )?
    .unwrap_or(DEFAULT_MAX_EXECUTION_TOKENS);
    if per_execution_tokens == 0 {
        anyhow::bail!("max llm tokens must be at least 1, got 0");
    }
    let max_concurrency = env_ladder(
        args.max_llm_concurrency,
        "SUBMILLI_MAX_LLM_CONCURRENCY",
        "a whole number of prompts",
        lookup,
    )?
    .unwrap_or(DEFAULT_MAX_CONCURRENCY);
    if max_concurrency == 0 {
        anyhow::bail!("max llm concurrency must be at least 1, got 0");
    }
    Ok((
        LlmLimits {
            per_execution_tokens,
            ..LlmLimits::default()
        },
        max_concurrency,
    ))
}

impl Args {
    /// Static invocation shape for metrics — which optional flags were supplied,
    /// never their (dynamic) values.
    pub(crate) fn metric_flags(&self) -> Vec<(&'static str, bool)> {
        vec![
            ("has_fuel", self.fuel.is_some()),
            ("report", self.report),
            ("has_max_stack", self.max_stack.is_some()),
            ("has_timeout", self.timeout.is_some()),
            ("has_vfs", self.vfs.is_some()),
            ("has_blueprint", self.blueprint.is_some()),
            ("has_vars", !self.vars.is_empty()),
            ("has_max_llm_tokens", self.max_llm_tokens.is_some()),
            (
                "has_max_llm_concurrency",
                self.max_llm_concurrency.is_some(),
            ),
        ]
    }
}

pub fn execute(args: Args) -> anyhow::Result<ExitCode> {
    // `None` means "build the real HTTP dispatch below, once the blueprint and
    // the secret store it needs are in hand". A test passes `Some(fake)` to
    // drive `llm.call` without a socket.
    execute_with_dispatch(args, None)
}

/// `execute`, with the outbound model dispatch optionally overridden.
///
/// Split out so a test can drive a real `llm.call` — and the budget refusal that
/// guards it — without a live provider, the way the server's own tests do. A
/// `None` here is not "no provider": it selects the real one.
pub(crate) fn execute_with_dispatch(
    args: Args,
    llm_dispatch: Option<Arc<dyn ModelDispatch>>,
) -> anyhow::Result<ExitCode> {
    // A host call that re-enters Wasm nests frames on the native stack, so the
    // program runs on a thread sized for the Wasm stack it is given. The same
    // thread compiles it, which needs the interpreter's compiler stack.
    let stack_size = runtime_config(&args)
        .native_stack_size()
        .max(interpreter::compiler_limits::COMPILER_STACK_BYTES);
    std::thread::Builder::new()
        .name("submilli-run".into())
        .stack_size(stack_size)
        .spawn(move || execute_on_this_thread(args, llm_dispatch))
        .context("starting the thread that runs the program")?
        .join()
        .map_err(|_| anyhow::anyhow!("the thread running the program panicked"))?
}

fn execute_on_this_thread(
    args: Args,
    llm_dispatch: Option<Arc<dyn ModelDispatch>>,
) -> anyhow::Result<ExitCode> {
    let started = Instant::now();
    let (llm_limits, llm_concurrency) = llm_settings(&args)?;
    let source = fs::read_to_string(&args.script)
        .with_context(|| format!("reading {}", args.script.display()))?;
    let filename = args.script.to_string_lossy().into_owned();
    // The script's `file` id stamps compile diagnostics; package sources are
    // added after package resolution so runtime package frames can render source.
    let (mut sources, file) = Sources::single(filename.clone(), source.clone())?;

    let blueprint = match &args.blueprint {
        Some(path) => {
            let yaml =
                fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
            match submilli_blueprint::parse(&yaml) {
                Ok(bp) => {
                    if let Some(message) = named_volume_refusal(&bp) {
                        eprintln!("error: {}: {message}", path.display());
                        return Ok(ExitCode::from(1));
                    }
                    Some(Arc::new(bp))
                }
                Err(err) => {
                    eprintln!("error: {}: {err}", path.display());
                    return Ok(ExitCode::from(1));
                }
            }
        }
        None => None,
    };
    let variables = match blueprint.as_ref() {
        Some(bp) => match bind_variables(bp, &args.vars) {
            Ok(bindings) => Arc::new(bindings),
            Err(err) => {
                eprintln!("error: {err}");
                return Ok(ExitCode::from(1));
            }
        },
        None => Arc::new(VarBindings::new()),
    };
    let package_artifacts = match blueprint.as_ref() {
        Some(bp) => match load_blueprint_packages(bp) {
            Ok(artifacts) => artifacts,
            Err(err) => {
                eprintln!("error: {err}");
                return Ok(ExitCode::from(1));
            }
        },
        None => Vec::new(),
    };
    register_package_sources(&mut sources, &package_artifacts)?;

    // The closure may hold packages the blueprint doesn't list; those are
    // linked but stay out of the script's importable surface.
    let package_refs: Vec<_> = package_artifacts
        .iter()
        .filter(|artifact| {
            blueprint
                .as_ref()
                .is_some_and(|bp| bp.packages.contains(&artifact.metadata.package_name))
        })
        .map(|artifact| &artifact.package_declaration)
        .collect();

    let transitive_refs: Vec<_> = package_artifacts
        .iter()
        .filter(|artifact| {
            !blueprint
                .as_ref()
                .is_some_and(|bp| bp.packages.contains(&artifact.metadata.package_name))
        })
        .map(|artifact| &artifact.package_declaration)
        .collect();

    // Host fns (`http`/`fs`/MCP) are async, so the run drives on a private
    // current-thread runtime. MCP discovery is async too and must precede
    // compilation (the `@mcp/<server>` decls type-check the script), so the
    // runtime is built here and reused for the later dispatch.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("building tokio runtime")?;

    // With a blueprint, resolve `store:` secrets from the local store and call
    // authenticated `@mcp/<server>` servers in-process — server-free parity.
    let mut secret_store: Option<Arc<dyn SecretStore>> = None;
    let mut mcp_transport: Option<Arc<dyn McpTransport>> = None;
    // A local run is unrestricted; the server is where deny-private applies.
    let network_policy = Arc::new(NetworkPolicy::default());
    let mcp_catalog = match blueprint.as_ref() {
        Some(bp) => {
            let store = crate::commands::local::open_secret_store()?;
            let http = Arc::new(ReqwestHttpClient::new(Arc::clone(&network_policy)))
                as Arc<dyn HttpClient>;
            let oauth = Arc::new(OAuthTokenManager::new(
                store.clone(),
                http,
                Arc::new(Vec::new()),
            ));
            let catalog = rt.block_on(discover_all_local(
                DiscoveryAuth {
                    secret_store: Some(&store),
                    oauth: Some(&oauth),
                    harness_secrets: None,
                    network_policy: &network_policy,
                },
                &bp.name,
                bp,
            ));
            mcp_transport = Some(Arc::new(StreamableHttpTransport::new(
                bp.name.clone(),
                bp.clone(),
                Some(oauth),
                Some(store.clone()),
                Arc::clone(&network_policy),
            )));
            secret_store = Some(store);
            catalog
        }
        None => McpCatalog::empty(),
    };
    for warning in mcp_catalog.warnings() {
        eprintln!("warning: @mcp/{}: {}", warning.server, warning.message);
    }
    let mcp_defs = mcp_catalog.defs_refs();
    let git_enabled = blueprint.as_ref().is_some_and(|bp| bp.git.is_some());
    let parsed = interpreter::parse_script(&source, file);
    let mut stdlib = interpreter::runtime::stdlib_package_declarations();
    stdlib.retain(|decl| git_enabled || decl.package_name != "submilli:git");
    let compiled = interpreter::compile::compile_parsed_script_with_transitive(
        &source,
        &filename,
        &parsed,
        &stdlib,
        &package_refs,
        &mcp_defs,
        &transitive_refs,
    );
    let compiled = match compiled {
        Ok(compiled) => {
            for d in &compiled.warnings {
                eprint!("{}", diagnostics::render(d, &sources));
            }
            compiled
        }
        Err(diags) => {
            for d in &diags {
                eprint!("{}", diagnostics::render(d, &sources));
            }
            return Ok(ExitCode::from(1));
        }
    };

    let cfg = runtime_config(&args);

    let engine = cfg.engine()?;
    let package_modules = compile_package_modules(&engine, &package_artifacts)?;
    let mut vfs = match args.vfs {
        Some(path) => Vfs::external(path).context("opening --vfs directory")?,
        None => Vfs::tempdir().context("allocating temporary VFS directory")?,
    };
    if let Some(blueprint) = &blueprint {
        let config = blueprint.vfs.resolve(&variables)?;
        if !matches!(config, submilli_blueprint::VfsConfig::None) {
            vfs = vfs
                .with_cwd(config.cwd())
                .context("preparing blueprint cwd")?;
        }
    }
    let size_limit = blueprint.as_ref().and_then(|bp| bp.vfs.size_limit());
    if let Some(limit) = size_limit {
        let measured = vfs.measure_usage();
        if let Err(err) = &measured {
            eprintln!(
                "warning: the VFS couldn't be measured against the blueprint's size_limit, so it is treated as full: {err}"
            );
        }
        vfs = vfs.with_measured_limit(limit, measured.ok());
    }
    // Use with_vfs_and_cap directly rather than RuntimeConfig::run to keep console output streaming, not buffered.
    let mut data = StoreData::with_vfs_and_cap(vfs, cfg.max_store_bytes);
    data.vfs_info.size_limit = size_limit;
    data.install_type_info(compiled.type_info.clone());
    if let Some(bp) = &blueprint {
        data.git = submilli_shared::resolve_git(bp, &variables)?;
        data.security_check = Arc::new(PolicyCheck::with_variables(bp.clone(), variables));
        data.auth_proxy = Arc::new(BlueprintAuthProxy::new(bp.clone(), secret_store.clone()));
        data.secret_provider = Arc::new(BlueprintSecretProvider::new(
            bp.clone(),
            secret_store.clone(),
        ));
        if let Some(transport) = mcp_transport.clone() {
            data.mcp_transport = Some(transport);
        }
        // The CLI is wired where `submilli:session` deliberately is not: session
        // state is memory-only and a CLI run has nothing to carry it across,
        // whereas a blueprint-configured provider is what makes `submilli run`
        // useful for testing a program before it reaches a server — and the
        // credentials already resolve from the blueprint being loaded here.
        //
        // The budget is allocated whether or not a dispatch is installed, so the
        // ceiling is enforced on the path that spends, and the aggregate is
        // per-run: a CLI invocation is one execution, so its own ceiling is the
        // only one there is to share.
        data.llm_budget = Some(Arc::new(ExecutionTokenBudget::new(
            llm_limits,
            SharedTokenBudget::new(llm_limits.per_execution_tokens),
        )));
        // An injected dispatch wins (a test's fake); otherwise the real HTTP one,
        // holding this blueprint and the same secret store the MCP transport
        // resolves through. A blueprint that declares no `llm:` block still gets
        // a provider — and `llm.call` against it fails on the undeclared model,
        // naming the block to add, rather than on a missing provider.
        let dispatch = llm_dispatch.clone().unwrap_or_else(|| {
            Arc::new(HttpModelDispatch::new(
                bp.clone(),
                secret_store.clone(),
                Arc::clone(&network_policy),
            ))
        });
        data.llm_provider = Some(Arc::new(
            BlueprintLlmProvider::new(bp.clone(), dispatch).with_max_concurrency(llm_concurrency),
        ));
    }
    let mut store = cfg.store(&engine, data)?;
    install_tenant_limits(&mut store);
    let module = Module::new(&engine, &compiled.wasm)?;
    let mut linker = Linker::<StoreData>::new(&engine);

    let compile_elapsed = started.elapsed();
    let run_started = Instant::now();
    let dispatch = rt.block_on(async {
        let linked_packages: Vec<_> = package_modules
            .iter()
            .zip(package_artifacts.iter())
            .map(|(package, artifact)| LinkedPackageModule {
                module: &package.module,
                declaration: &artifact.package_declaration,
                type_info: &artifact.type_info,
            })
            .collect();
        install_runtime_async(&mut linker, &mut store).await?;
        // Installing a package runs its top-level statements, so the deadline
        // covers them as it covers the program's.
        let _watchdog = cfg.arm_timeout(&engine);
        install_package_modules_async(&mut linker, &mut store, &linked_packages).await?;
        let instance = instantiate_program_async(&linker, &mut store, &module).await?;
        dispatch_main_async(&mut store, &instance).await
    });
    let cleanup = rt.block_on(store.data_mut().blocking_work.finish());
    let (dispatch, secondary_cleanup) = settle_worker_cleanup(dispatch, cleanup);
    let run_elapsed = run_started.elapsed();
    let exit = match dispatch {
        Ok(Some(json)) => {
            println!("{json}");
            Ok(ExitCode::SUCCESS)
        }
        Ok(None) => Ok(ExitCode::SUCCESS),
        Err(err) => {
            if let Some(bt) = render_backtrace(&err, &sources, file, BacktraceMode::Full) {
                eprint!("{bt}");
            } else {
                eprintln!("error: {}", failure_message(&err));
            }
            Ok(ExitCode::from(1))
        }
    };
    if let Some(error) = secondary_cleanup {
        eprintln!("worker cleanup failure: {error}");
    }
    if args.report {
        let usage = ExecutionUsage::capture(&store, cfg.fuel)?;
        eprintln!(
            "fuel: {} (wasm {}, host {})   memory peak: {:.1} MB   wall: {} ms (compile {} ms, run {} ms)",
            grouped_fuel(usage.fuel),
            grouped_fuel(usage.wasm_fuel),
            grouped_fuel(usage.host_fuel),
            usage.memory_peak as f64 / 1_000_000.0,
            (compile_elapsed + run_elapsed).as_millis(),
            compile_elapsed.as_millis(),
            run_elapsed.as_millis(),
        );
    }
    exit
}

fn settle_worker_cleanup<T>(
    dispatch: wasmtime::Result<T>,
    cleanup: Result<(), interpreter::runtime::blocking::BlockingWorkDrainError>,
) -> (
    wasmtime::Result<T>,
    Option<interpreter::runtime::blocking::BlockingWorkDrainError>,
) {
    match (dispatch, cleanup) {
        (Ok(_), Err(error)) => (
            Err(
                interpreter::runtime::host::fatal_host_error("worker cleanup failed")
                    .context(error),
            ),
            None,
        ),
        (dispatch, cleanup) => (dispatch, cleanup.err()),
    }
}

fn grouped_fuel(fuel: u64) -> String {
    let digits = fuel.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

struct LocalPackageModule {
    module: Module,
}

/// The engine limits the flags ask for; also what the run thread's stack is sized from.
fn runtime_config(args: &Args) -> RuntimeConfig {
    let mut cfg = RuntimeConfig::default();
    if let Some(fuel) = args.fuel {
        cfg.fuel = fuel;
    }
    if let Some(bytes) = args.max_stack {
        cfg.max_wasm_stack = bytes;
    }
    cfg.timeout = args.timeout.map(Duration::from_millis);
    cfg
}

/// Parse `--var NAME=VALUE` pairs and resolve them against the blueprint's
/// declarations, so a run is refused for the same reasons a session would be:
/// an undeclared name, or a required variable left unbound.
fn bind_variables(blueprint: &Blueprint, raw: &[String]) -> anyhow::Result<VarBindings> {
    let mut supplied = BTreeMap::new();
    for pair in raw {
        let (name, value) = pair
            .split_once('=')
            .with_context(|| format!("--var '{pair}' must be in `NAME=VALUE` form"))?;
        if name.is_empty() {
            anyhow::bail!("--var '{pair}' has an empty name");
        }
        supplied.insert(name.to_owned(), value.to_owned());
    }
    resolve_variables(&blueprint.variables, &supplied)
        .map_err(|err| anyhow!("invalid variables: {err}"))
}

/// Why a blueprint cannot run locally: named volumes are declared in a server's
/// config, and a local run has no server to resolve them through.
fn named_volume_refusal(blueprint: &Blueprint) -> Option<String> {
    let reference = blueprint.vfs.named_references().into_iter().next()?;
    let place = match reference.mount {
        None => "as its vfs root".to_string(),
        Some(path) => format!("at `{path}`"),
    };
    Some(format!(
        "blueprint '{}' uses named volume '{}' {place}; named volumes are declared in a server \
         config, so run it on `submilli-server`, or drop the volume for local runs (use \
         `--vfs <dir>` to give the program a directory)",
        blueprint.name, reference.volume
    ))
}

fn load_blueprint_packages(blueprint: &Blueprint) -> anyhow::Result<Vec<Artifact>> {
    let store = PackageStore::default();
    store
        .load_closure(blueprint.packages.iter().map(String::as_str))
        .map_err(anyhow::Error::from)
}

fn compile_package_modules(
    engine: &wasmtime::Engine,
    artifacts: &[Artifact],
) -> anyhow::Result<Vec<LocalPackageModule>> {
    artifacts
        .iter()
        .map(|artifact| {
            let module = Module::new(engine, &artifact.wasm).map_err(|err| {
                anyhow!(
                    "compiling package {}: {err}",
                    artifact.package_declaration.package_name
                )
            })?;
            Ok(LocalPackageModule { module })
        })
        .collect()
}

/// Registers every package module so its frames render with their source.
fn register_package_sources(
    sources: &mut Sources,
    artifacts: &[Artifact],
) -> Result<(), interpreter::source::SourceError> {
    for artifact in artifacts {
        let name = &artifact.package_declaration.package_name;
        for source in &artifact.sources {
            sources.add_package_module(name, source.path.as_str(), &source.text)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use submilli_shared::llm::{
        ModelDispatch, ModelRequest, ProviderFailure, ProviderResponse, ProviderUsage, StopReason,
    };

    use super::*;

    /// A dispatch that always answers, counting how many times it was reached.
    /// The seam exists so the CLI's budget wiring is testable without a live
    /// provider — the same seam `submilli-shared`'s own tests drive.
    struct AlwaysOk(Arc<AtomicUsize>);

    impl ModelDispatch for AlwaysOk {
        fn dispatch<'a>(
            &'a self,
            _request: ModelRequest<'a>,
        ) -> Pin<Box<dyn Future<Output = Result<ProviderResponse, ProviderFailure>> + Send + 'a>>
        {
            self.0.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                Ok(ProviderResponse {
                    text: Some("answer".to_string()),
                    stop_reason: StopReason::Stop,
                    usage: ProviderUsage::reported(1.0, 1.0),
                })
            })
        }
    }

    const BLUEPRINT: &str = r#"name: llm-cli
permissions:
  main:
    - capability: llm.call
      action: allow
llm:
  providers:
    fake:
      type: anthropic
  models:
    test-model:
      provider: fake
"#;

    const CATCH: &str = r#"import llm from "submilli:llm";
function main(): string {
    try {
        const t = llm.call("test-model", "hi").text;
        return "OK:" + (t === null ? "NULL" : (t as string));
    } catch (e: Error) {
        return e.message;
    }
}"#;

    struct Fixture {
        _dir: tempfile::TempDir,
        args: Args,
    }

    fn fixture(name: &str, max_llm_tokens: Option<u64>) -> Fixture {
        let dir = tempfile::tempdir().expect("tempdir");
        let script = dir.path().join(format!("{name}.subm"));
        std::fs::write(&script, CATCH).expect("write script");
        let blueprint = dir.path().join("blueprint.yaml");
        std::fs::write(&blueprint, BLUEPRINT).expect("write blueprint");
        Fixture {
            _dir: dir,
            args: Args {
                script,
                fuel: None,
                report: false,
                max_stack: None,
                timeout: None,
                vfs: None,
                blueprint: Some(blueprint),
                vars: Vec::new(),
                max_llm_tokens,
                max_llm_concurrency: None,
            },
        }
    }

    #[test]
    fn a_blueprint_with_named_volumes_is_refused_locally() {
        for (yaml, expected) in [
            (
                "name: x\nvfs:\n  mode: named\n  volume: notes\n",
                "named volume 'notes' as its vfs root",
            ),
            (
                "name: x\nvfs:\n  mounts:\n    /memory: {mode: named, volume: memory}\n",
                "named volume 'memory' at `/memory`",
            ),
        ] {
            let blueprint = submilli_blueprint::parse(yaml).unwrap();
            let message = named_volume_refusal(&blueprint).expect(yaml);
            assert!(message.contains(expected), "{message}");
            assert!(message.contains("submilli-server"), "{message}");
        }
        let plain = submilli_blueprint::parse("name: x\nvfs: per_session\n").unwrap();
        assert_eq!(named_volume_refusal(&plain), None);
    }

    /// `submilli run` with a blueprint-configured provider executes a call.
    ///
    /// This is the whole reason the CLI is wired where `submilli:session`
    /// deliberately is not: a program can be exercised against its real provider
    /// before it reaches a server.
    #[test]
    fn a_cli_run_with_a_configured_provider_executes_a_call() {
        let calls = Arc::new(AtomicUsize::new(0));
        let f = fixture("llm_ok", None);
        let code = execute_with_dispatch(f.args, Some(Arc::new(AlwaysOk(Arc::clone(&calls)))))
            .expect("the run should not fail");

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "the configured provider must have been dispatched"
        );
    }

    /// **The CLI is not an unbounded-spend escape hatch.** A run whose
    /// reservation exceeds its per-execution ceiling is refused *before*
    /// dispatch, which is what makes it a ceiling rather than an after-the-fact
    /// accounting of spend that already happened.
    ///
    /// A dispatch is installed deliberately: without one the missing-provider
    /// error would mask the budget refusal (the provider is resolved before the
    /// reservation is taken), and the test would pass for the wrong reason.
    #[test]
    fn a_cli_run_over_its_per_execution_ceiling_is_refused_before_dispatch() {
        let calls = Arc::new(AtomicUsize::new(0));
        // One call reserves the default output cap plus its input estimate, so
        // a ceiling of 10 cannot cover it.
        let f = fixture("llm_over", Some(10));
        let code = execute_with_dispatch(f.args, Some(Arc::new(AlwaysOk(Arc::clone(&calls)))))
            .expect("a caught QuotaExceededError still exits cleanly");

        assert_eq!(code, ExitCode::SUCCESS, "the guest caught the refusal");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "a call over the ceiling must never reach the provider"
        );
    }

    /// The ceiling resolves off the flag, then the environment, then the
    /// default — the same ladder the server walks, minus the config-file rung
    /// the CLI has no file for.
    ///
    /// Asserted on the resolver rather than through a run, so the tiers are
    /// distinguishable by value instead of by whether a call happened to be
    /// refused.
    #[test]
    fn the_cli_ceiling_walks_the_flag_then_env_then_default() {
        const TOKENS: &str = "SUBMILLI_MAX_EXECUTION_LLM_TOKENS";

        // Distinct numbers per rung, so precedence is proven rather than
        // coincidental: 400 (flag) and 200 (env) cannot be confused, and a
        // resolver that picked the larger or the smaller would fail one row.
        for (flag, env, expected, tier) in [
            (Some(400), Some("200"), 400, "the flag"),
            (None, Some("200"), 200, "the env var"),
            (None, None, DEFAULT_MAX_EXECUTION_TOKENS, "unset"),
        ] {
            let f = fixture("ladder", flag);
            let (limits, _) =
                llm_settings_from(&f.args, &env_from(&[(TOKENS, env)])).expect("resolves");
            assert_eq!(
                limits.per_execution_tokens, expected,
                "{tier} should have won"
            );
        }

        // That the default is *finite* — the whole point of wiring the CLI's
        // budget — is asserted behaviourally by
        // `a_cli_run_over_its_per_execution_ceiling_is_refused_before_dispatch`,
        // which shows a run being refused rather than dispatched. A comparison
        // against a constant here would be true at compile time and prove
        // nothing.
    }

    /// A set-but-unparseable ceiling fails the run naming the variable, rather
    /// than falling through to the default.
    #[test]
    fn a_malformed_ceiling_fails_naming_the_variable() {
        const TOKENS: &str = "SUBMILLI_MAX_EXECUTION_LLM_TOKENS";
        let f = fixture("ladder_bad", None);
        let err = llm_settings_from(&f.args, &env_from(&[(TOKENS, Some("lots"))]))
            .expect_err("a malformed ceiling must fail");
        assert!(
            err.to_string().contains(TOKENS),
            "the error must name the variable: {err}"
        );
    }

    fn declaring(yaml: &str) -> Blueprint {
        submilli_blueprint::parse(yaml).expect("blueprint parses")
    }

    #[test]
    fn var_flags_bind_declared_variables_and_fill_defaults() {
        let bp = declaring(
            "name: t\nvariables:\n  tenant: { required: true }\n  region: { default: eu }\n",
        );
        let bound = bind_variables(&bp, &["tenant=acme".into()]).expect("binds");
        assert_eq!(bound["tenant"], "acme");
        assert_eq!(bound["region"], "eu");
    }

    #[test]
    fn var_flags_are_checked_like_a_session() {
        let bp = declaring("name: t\nvariables:\n  tenant: { required: true }\n");
        let missing = bind_variables(&bp, &[]).expect_err("required variable unbound");
        assert!(missing.to_string().contains("tenant"), "{missing}");
        let unknown = bind_variables(&bp, &["tenant=a".into(), "nope=b".into()])
            .expect_err("undeclared variable");
        assert!(unknown.to_string().contains("nope"), "{unknown}");
        let malformed = bind_variables(&bp, &["tenant".into()]).expect_err("no '='");
        assert!(malformed.to_string().contains("NAME=VALUE"), "{malformed}");
    }

    /// A lookup over a fixed set of variables, so precedence is asserted without
    /// mutating the process environment other tests are reading.
    fn env_from(pairs: &[(&str, Option<&str>)]) -> impl Fn(&str) -> Option<String> {
        let owned: Vec<(String, Option<String>)> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.map(str::to_owned)))
            .collect();
        move |name| {
            owned
                .iter()
                .find(|(key, _)| key == name)
                .and_then(|(_, value)| value.clone())
        }
    }
}

#[cfg(test)]
mod worker_cleanup_tests {
    use super::*;

    async fn injected_drain_failure() -> interpreter::runtime::blocking::BlockingWorkDrainError {
        let mut workers = interpreter::runtime::blocking::BlockingWork::default();
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, gate) = std::sync::mpsc::channel();
        let mut waiter = Box::pin(workers.spawn(move || {
            started.send(()).unwrap();
            gate.recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            panic!("injected abandoned worker failure");
        }));
        tokio::select! {
            _ = &mut waiter => panic!("worker ended early"),
            result = ready => result.unwrap(),
        }
        drop(waiter);
        release.send(()).unwrap();
        workers.finish().await.unwrap_err()
    }

    #[tokio::test]
    async fn cleanup_failure_turns_success_into_fatal_failure() {
        let cleanup = injected_drain_failure().await;
        let (result, secondary) = settle_worker_cleanup(Ok(42), Err(cleanup));
        let error = result.unwrap_err();
        assert!(error.is::<interpreter::runtime::host::FatalHostError>());
        assert!(error.is::<interpreter::runtime::blocking::BlockingWorkDrainError>());
        assert!(error.to_string().contains("blocking worker panicked"));
        assert!(secondary.is_none());
    }

    #[tokio::test]
    async fn cleanup_failure_preserves_original_execution_error() {
        for message in ["execution failed", "execution cancelled"] {
            let cleanup = injected_drain_failure().await;
            let (result, secondary) =
                settle_worker_cleanup::<()>(Err(wasmtime::Error::msg(message)), Err(cleanup));
            assert_eq!(result.unwrap_err().to_string(), message);
            assert_eq!(
                secondary.unwrap().errors(),
                &[interpreter::runtime::blocking::BlockingWorkError::WorkerPanicked]
            );
        }
    }
}
