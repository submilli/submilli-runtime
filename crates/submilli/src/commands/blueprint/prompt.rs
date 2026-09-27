//! `submilli blueprint prompt [--blueprint <path>]` — print the LLM-facing
//! `execute` tool description (the MCP "system prompt"), with `{vfs_mode}`,
//! `{http_access}`, and `{builtins}` resolved exactly as a running server
//! resolves them. A debugging aid: it shows the prompt an MCP client would
//! receive for a given blueprint. Runs offline; no server needed.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use submilli_blueprint::Blueprint;

const DEFAULT_FILE: &str = "blueprint.yaml";

#[derive(clap::Args)]
pub struct Args {
    /// Blueprint whose policy resolves `{vfs_mode}` / `{http_access}`. Defaults
    /// to `./blueprint.yaml` if present, otherwise an empty (deny-all) policy.
    #[arg(long)]
    blueprint: Option<PathBuf>,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    let blueprint = resolve_blueprint(args.blueprint.as_deref())?;
    println!(
        "{}",
        submilli_shared::prompt::execute_tool_description(
            &blueprint,
            submilli_shared::prompt::PromptSurface::Mcp
        )
    );
    Ok(ExitCode::SUCCESS)
}

/// The blueprint whose policy resolves the prompt's placeholders. An explicit
/// `--blueprint` must exist and parse; otherwise we read `./blueprint.yaml` if
/// it's there, and fall back to the server's default posture (ephemeral vfs,
/// deny-all) when no blueprint is in play — noting the fallback on stderr so
/// the output isn't silently generic.
fn resolve_blueprint(blueprint: Option<&Path>) -> Result<Blueprint> {
    let path = if let Some(p) = blueprint {
        p.to_path_buf()
    } else {
        let default = PathBuf::from(DEFAULT_FILE);
        if !default.exists() {
            eprintln!(
                "note: no blueprint given and ./{DEFAULT_FILE} not found — \
                 showing the prompt for the default policy (ephemeral vfs, no HTTP)."
            );
            return Ok(Blueprint::default());
        }
        default
    };
    let yaml = std::fs::read_to_string(&path)
        .with_context(|| format!("reading blueprint {}", path.display()))?;
    submilli_blueprint::parse(&yaml)
        .with_context(|| format!("parsing blueprint {}", path.display()))
}
