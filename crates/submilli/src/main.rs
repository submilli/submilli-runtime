use std::process::ExitCode;

use clap::{Parser, Subcommand};

mod commands;
mod telemetry;

#[derive(Parser)]
#[command(
    name = "submilli",
    version,
    about = "Submilli: agent-native TypeScript-subset interpreter."
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run a Submilli script in-process.
    Run(commands::run::Args),
    /// Typecheck a Submilli script without running it.
    Check(commands::check::Args),
    /// Scaffold, check, and publish a package project (submilli.toml).
    Build(commands::build::Args),
    /// Install a package from GitHub into the local store, pinned to a commit.
    Install(commands::install::Args),
    /// Print a stdlib package's declarations and description.
    Docs(commands::docs::Args),
    /// Search available stdlib packages by name, description, or symbol.
    Search(commands::search::Args),
    /// Print declarations for language built-ins, or list the catalog.
    Builtins(commands::builtins::Args),
    /// Install or update the Submilli coding-assistant skill.
    #[command(subcommand)]
    Skill(commands::skill::SkillCmd),
    /// Replace this executable with the latest published release.
    Upgrade(commands::upgrade::Args),
    /// Apply blueprint YAML documents to a running submilli-server.
    Apply(commands::apply::Args),
    /// Author a blueprint file locally (scaffold, edit).
    #[command(subcommand)]
    Blueprint(commands::blueprint::BlueprintCmd),
    /// Manage secrets in the local secret store (no running server).
    #[command(subcommand)]
    Secret(commands::secret::SecretCmd),
    /// Authenticate outbound OAuth MCP servers locally (no running server).
    #[command(subcommand)]
    Mcp(commands::mcp::McpCmd),
    /// Interact with a running submilli-server.
    #[command(subcommand)]
    Server(commands::server::ServerCmd),
}

fn main() -> anyhow::Result<ExitCode> {
    let _guard = telemetry::init();

    let cli = Cli::parse();
    if !matches!(cli.cmd, Cmd::Skill(_)) {
        commands::skill::warn_if_outdated();
    }
    record_invocation(&cli.cmd);
    // Compilation recurses with program nesting and needs the interpreter's
    // documented stack; the main thread's size is platform-dependent.
    std::thread::Builder::new()
        .name("submilli-command".into())
        .stack_size(interpreter::compiler_limits::COMPILER_STACK_BYTES)
        .spawn(move || execute(cli.cmd))
        .map_err(|error| anyhow::anyhow!("cannot start the command thread: {error}"))?
        .join()
        .map_err(|_| anyhow::anyhow!("the command thread panicked"))?
}

fn execute(cmd: Cmd) -> anyhow::Result<ExitCode> {
    match cmd {
        Cmd::Run(args) => commands::run::execute(args),
        Cmd::Check(args) => commands::check::execute(args),
        Cmd::Build(args) => commands::build::execute(args),
        Cmd::Install(args) => commands::install::execute(args),
        Cmd::Docs(args) => commands::docs::execute(args),
        Cmd::Search(args) => commands::search::execute(args),
        Cmd::Builtins(args) => commands::builtins::execute(args),
        Cmd::Skill(cmd) => commands::skill::execute(cmd),
        Cmd::Upgrade(args) => commands::upgrade::execute(args),
        Cmd::Apply(args) => commands::apply::execute(args),
        Cmd::Blueprint(cmd) => commands::blueprint::execute(cmd),
        Cmd::Secret(cmd) => commands::secret::execute(cmd),
        Cmd::Mcp(cmd) => commands::mcp::execute(cmd),
        Cmd::Server(cmd) => commands::server::execute(cmd),
    }
}

/// One counter per CLI invocation, tagged with the (static) command path and the
/// shape of its flags — never dynamic values like a blueprint name or file path.
fn record_invocation(cmd: &Cmd) {
    let (command, flags) = invocation_attrs(cmd);
    let mut counter =
        sentry::metrics::counter("submilli.cli.invocation", 1).attribute("command", command);
    for (key, value) in flags {
        counter = counter.attribute(key, value);
    }
    counter.capture();
}

fn invocation_attrs(cmd: &Cmd) -> (&'static str, Vec<(&'static str, bool)>) {
    match cmd {
        Cmd::Run(a) => ("run", a.metric_flags()),
        Cmd::Check(_) => ("check", Vec::new()),
        Cmd::Build(a) => (a.label(), a.metric_flags()),
        Cmd::Install(a) => ("install", a.metric_flags()),
        Cmd::Docs(_) => ("docs", Vec::new()),
        Cmd::Search(a) => ("search", a.metric_flags()),
        Cmd::Builtins(a) => ("builtins", a.metric_flags()),
        Cmd::Skill(_) => ("skill", Vec::new()),
        Cmd::Upgrade(_) => ("upgrade", Vec::new()),
        Cmd::Apply(_) => ("apply", Vec::new()),
        Cmd::Blueprint(sub) => (blueprint_label(sub), Vec::new()),
        Cmd::Secret(sub) => (secret_label(sub), Vec::new()),
        Cmd::Mcp(sub) => (mcp_label(sub), Vec::new()),
        Cmd::Server(sub) => (server_label(sub), Vec::new()),
    }
}

fn secret_label(cmd: &commands::secret::SecretCmd) -> &'static str {
    use commands::secret::SecretCmd;
    match cmd {
        SecretCmd::Put(_) => "secret.put",
        SecretCmd::Delete(_) => "secret.delete",
        SecretCmd::List(_) => "secret.list",
    }
}

fn mcp_label(cmd: &commands::mcp::McpCmd) -> &'static str {
    use commands::mcp::McpCmd;
    match cmd {
        McpCmd::Authenticate(_) => "mcp.authenticate",
        McpCmd::Deauthenticate(_) => "mcp.deauthenticate",
        McpCmd::AuthStatus(_) => "mcp.auth_status",
        McpCmd::Provider(_) => "mcp.provider",
    }
}

fn blueprint_label(cmd: &commands::blueprint::BlueprintCmd) -> &'static str {
    use commands::blueprint::BlueprintCmd;
    match cmd {
        BlueprintCmd::Init(_) => "blueprint.init",
        BlueprintCmd::Lint(_) => "blueprint.lint",
        BlueprintCmd::AddMcp(_) => "blueprint.add_mcp",
        BlueprintCmd::AddPackage(_) => "blueprint.add_package",
        BlueprintCmd::Variable(_) => "blueprint.variable",
        BlueprintCmd::Secret(_) => "blueprint.secret",
        BlueprintCmd::AuthProxy(_) => "blueprint.auth_proxy",
        BlueprintCmd::Capability(_) => "blueprint.capability",
        BlueprintCmd::Git(_) => "blueprint.git",
        BlueprintCmd::Prompt(_) => "blueprint.prompt",
    }
}

fn server_label(cmd: &commands::server::ServerCmd) -> &'static str {
    use commands::server::ServerCmd;
    match cmd {
        ServerCmd::RunCode(_) => "server.run_code",
        ServerCmd::Packages(_) => "server.packages",
        ServerCmd::Status(_) => "server.status",
        ServerCmd::Stop(_) => "server.stop",
        ServerCmd::Blueprint(_) => "server.blueprint",
        ServerCmd::Secret(_) => "server.secret",
        ServerCmd::Session(_) => "server.session",
        ServerCmd::Mcp(_) => "server.mcp",
        ServerCmd::Docs(_) => "server.docs",
    }
}
