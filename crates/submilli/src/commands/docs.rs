//! `submilli docs <name>` — print a stdlib or installed package's
//! TypeScript-style declarations and description. Runs offline (except for an
//! `@mcp/<server>` package, which is discovered from its server); no Submilli
//! server needed. See [`discovery::Scope`] for what `--blueprint` changes.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::Context;
use interpreter::packages::{self, Resolution};
use submilli_build::PackageStore;

use super::discovery::{self, Scope};

#[derive(clap::Args)]
pub struct Args {
    /// Package name, e.g. `submilli:http` or an installed `@org/name`. A
    /// language built-in (`Temporal`, `Temporal.Instant`) resolves here too.
    name: String,
    /// Show only what programs under this blueprint could import. Without it,
    /// the whole library is shown. An `@mcp/<server>` package is always read
    /// from a blueprint, by default blueprint.yaml.
    #[arg(long)]
    blueprint: Option<PathBuf>,
}

const DEFAULT_BLUEPRINT: &str = "blueprint.yaml";

pub fn execute(args: Args) -> anyhow::Result<ExitCode> {
    if let Some(server) = args.name.strip_prefix("@mcp/") {
        // An `@mcp/<server>` package exists only in a blueprint, so defaulting
        // the file here hides nothing, unlike scoping the library below.
        let path = args.blueprint.unwrap_or_else(|| DEFAULT_BLUEPRINT.into());
        return mcp_docs(server, &path);
    }
    let scope = Scope::load(args.blueprint)?;
    match packages::resolve(&args.name) {
        Resolution::Module(doc) if scope.allows(&doc.name) => {
            println!("{} — {}\n", doc.name, doc.description);
            println!("{}", doc.declarations);
            Ok(ExitCode::SUCCESS)
        }
        Resolution::Module(doc) => {
            eprintln!("{}", scope.hidden_message(&doc.name));
            Ok(ExitCode::FAILURE)
        }
        // A built-in needs no `import`, so serving it here costs the caller
        // nothing — the same redirect `packages.docs` makes over MCP and REST.
        Resolution::Builtin { name, declarations } => {
            println!("{}\n", packages::builtin_no_import_note(&name));
            println!("{declarations}");
            Ok(ExitCode::SUCCESS)
        }
        other => {
            // Installed packages come after the stdlib and the built-ins: a
            // `submilli:*` or built-in name can never be shadowed by a store entry.
            let Ok(artifact) = PackageStore::default().load(&args.name) else {
                discovery::report_miss(&args.name, other, &scope);
                return Ok(ExitCode::FAILURE);
            };
            if !scope.allows(&args.name) {
                eprintln!("{}", scope.hidden_message(&args.name));
                return Ok(ExitCode::FAILURE);
            }
            println!("{}\n", discovery::installed_summary(&artifact));
            println!(
                "{}",
                packages::render_declarations(&artifact.package_declaration)
            );
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn mcp_docs(server: &str, path: &Path) -> anyhow::Result<ExitCode> {
    use interpreter::runtime::{NetworkPolicy, ReqwestHttpClient};
    use submilli_shared::mcp::discovery::{DiscoveryAuth, discover_selected_local};
    use submilli_shared::mcp_token::OAuthTokenManager;

    submilli_shared::mcp::schema_registry::initialize_builtin_packs();

    let yaml =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let blueprint = submilli_blueprint::parse(&yaml)?;
    anyhow::ensure!(
        blueprint.mcp.contains_key(server),
        "MCP server '{server}' is not declared in {}",
        path.display()
    );
    let store = super::local::open_secret_store()?;
    let policy = Arc::new(NetworkPolicy::default());
    let oauth = Arc::new(OAuthTokenManager::new(
        store.clone(),
        Arc::new(ReqwestHttpClient::new(policy.clone())),
        Arc::new(super::mcp::provider_config::load()?),
    ));
    let selected = std::collections::BTreeSet::from([server.to_string()]);
    let catalog = super::local::block_on(discover_selected_local(
        DiscoveryAuth {
            secret_store: Some(&store),
            oauth: Some(&oauth),
            harness_secrets: None,
            network_policy: &policy,
        },
        &blueprint.name,
        &blueprint,
        &selected,
    ))?
    .context("MCP discovery initialization failed")?;
    for warning in catalog.warnings() {
        eprintln!("warning: @mcp/{}: {}", warning.server, warning.message);
    }
    let name = format!("@mcp/{server}");
    let package = catalog
        .package(&name)
        .with_context(|| format!("{name} is declared but unavailable; check discovery warnings"))?;
    println!(
        "{} — {}\n\n{}",
        name,
        package.description(),
        packages::render_declarations(&package.defs)
    );
    Ok(ExitCode::SUCCESS)
}
