//! `submilli run` — console output goes to stderr; JSON-encoded `main` return goes to stdout.

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, anyhow};
use interpreter::diagnostics;
use interpreter::runtime::{
    HttpClient, LinkedPackageModule, McpTransport, NetworkPolicy, ReqwestHttpClient, RuntimeConfig,
    StoreData, Vfs, install_package_modules_async, install_runtime_async, install_tenant_limits,
};
use interpreter::{BacktraceMode, Sources, compile_script, dispatch_main_async, render_backtrace};
use submilli_blueprint::Blueprint;
use submilli_build::{Artifact, PackageStore};
use submilli_shared::mcp::StreamableHttpTransport;
use submilli_shared::mcp::discovery::{DiscoveryAuth, McpCatalog, discover_all};
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
    /// unrestricted (allow-all). `env` / `file` / `store` secret sources all
    /// resolve (`store:` from the local secret store), and authenticated
    /// `@mcp/<server>` servers are called in-process — no running server needed.
    #[arg(long)]
    blueprint: Option<PathBuf>,
}

impl Args {
    /// Static invocation shape for metrics — which optional flags were supplied,
    /// never their (dynamic) values.
    pub(crate) fn metric_flags(&self) -> Vec<(&'static str, bool)> {
        vec![
            ("has_fuel", self.fuel.is_some()),
            ("has_max_stack", self.max_stack.is_some()),
            ("has_timeout", self.timeout.is_some()),
            ("has_vfs", self.vfs.is_some()),
            ("has_blueprint", self.blueprint.is_some()),
        ]
    }
}

pub fn execute(args: Args) -> anyhow::Result<ExitCode> {
    let source = fs::read_to_string(&args.script)
        .with_context(|| format!("reading {}", args.script.display()))?;
    let filename = args.script.to_string_lossy().into_owned();
    // The script's `file` id stamps compile diagnostics; package sources are
    // added after package resolution so runtime package frames can render source.
    let (mut sources, file) = Sources::single(filename.clone(), source.clone());

    let blueprint = match &args.blueprint {
        Some(path) => {
            let yaml =
                fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
            match submilli_blueprint::parse(&yaml) {
                Ok(bp) => Some(Arc::new(bp)),
                Err(err) => {
                    eprintln!("error: {}: {err}", path.display());
                    return Ok(ExitCode::from(1));
                }
            }
        }
        None => None,
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
    register_package_sources(&mut sources, &package_artifacts);

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
    let mcp_catalog = match blueprint.as_ref() {
        Some(bp) => {
            let store = crate::commands::local::open_secret_store()?;
            let http = Arc::new(ReqwestHttpClient::new(Arc::new(NetworkPolicy::default())))
                as Arc<dyn HttpClient>;
            let oauth = Arc::new(OAuthTokenManager::new(
                store.clone(),
                http,
                Arc::new(Vec::new()),
            ));
            let catalog = rt.block_on(discover_all(
                DiscoveryAuth {
                    secret_store: Some(&store),
                    oauth: Some(&oauth),
                    harness_secrets: None,
                },
                &bp.name,
                bp,
            ));
            mcp_transport = Some(Arc::new(StreamableHttpTransport::new(
                bp.name.clone(),
                bp.clone(),
                Some(oauth),
                Some(store.clone()),
            )));
            secret_store = Some(store);
            catalog
        }
        None => McpCatalog::empty(),
    };
    let mcp_defs = mcp_catalog.defs_refs();
    let compiled = compile_script(&source, &filename, file, &package_refs, &mcp_defs);
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

    let mut cfg = RuntimeConfig::default();
    if let Some(f) = args.fuel {
        cfg.fuel = f;
    }
    if let Some(s) = args.max_stack {
        cfg.max_wasm_stack = s;
    }
    cfg.timeout = args.timeout.map(Duration::from_millis);

    let engine = cfg.engine()?;
    let package_modules = compile_package_modules(&engine, &package_artifacts)?;
    let vfs = match args.vfs {
        Some(path) => Vfs::external(path).context("opening --vfs directory")?,
        None => Vfs::tempdir().context("allocating temporary VFS directory")?,
    };
    // Use with_vfs_and_cap directly rather than RuntimeConfig::run to keep console output streaming, not buffered.
    let mut data = StoreData::with_vfs_and_cap(vfs, cfg.max_store_bytes);
    data.install_type_info(compiled.type_info.clone());
    if let Some(bp) = &blueprint {
        data.security_check = Arc::new(PolicyCheck::new(bp.clone()));
        data.auth_proxy = Arc::new(BlueprintAuthProxy::new(bp.clone(), secret_store.clone()));
        data.secret_provider = Arc::new(BlueprintSecretProvider::new(
            bp.clone(),
            secret_store.clone(),
        ));
        if let Some(transport) = mcp_transport.clone() {
            data.mcp_transport = Some(transport);
        }
    }
    let mut store = cfg.store(&engine, data)?;
    install_tenant_limits(&mut store);
    let module = Module::new(&engine, &compiled.wasm)?;
    let mut linker = Linker::<StoreData>::new(&engine);

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
        install_package_modules_async(&mut linker, &mut store, &linked_packages).await?;
        let instance = linker.instantiate_async(&mut store, &module).await?;
        let _watchdog = cfg.arm_timeout(&engine);
        dispatch_main_async(&mut store, &instance).await
    });
    match dispatch {
        Ok(Some(json)) => {
            println!("{json}");
            Ok(ExitCode::SUCCESS)
        }
        Ok(None) => Ok(ExitCode::SUCCESS),
        Err(err) => {
            if let Some(bt) = render_backtrace(&err, &sources, file, BacktraceMode::Full) {
                eprint!("{bt}");
            } else {
                eprintln!("error: {err}");
            }
            Ok(ExitCode::from(1))
        }
    }
}

struct LocalPackageModule {
    module: Module,
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

fn register_package_sources(sources: &mut Sources, artifacts: &[Artifact]) {
    for artifact in artifacts {
        for source in &artifact.sources {
            sources.add(source.path.clone(), source.text.clone());
        }
    }
}
