//! `submilli playground`: a project-local playground that serves the project's
//! blueprint on loopback and reports every run.
//!
//! `start` (the default) runs it in the background and prints a ready record with
//! a single-use login link; a second `start` attaches to the running one. `status`,
//! `open`, and `stop` reach it through the control listener after the nonce
//! challenge. Unix only for now: detaching, owner-only files, and pid checks sit
//! behind `cfg(unix)`, and elsewhere the command says it is not supported yet.

// Elsewhere only the arguments and the not-supported message are used; the rest
// stays compiled so the command's interface is the same on every platform.
#![cfg_attr(not(unix), allow(dead_code))]

use std::net::IpAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args as ClapArgs, Subcommand};
use ipnet::IpNet;

#[cfg(unix)]
mod client;
#[cfg(unix)]
mod control_auth;
#[cfg(unix)]
mod fsx;
#[cfg(unix)]
mod host;
#[cfg(unix)]
mod labels;
#[cfg(unix)]
mod log;
#[cfg(unix)]
mod packages;
#[cfg(unix)]
mod project;
#[cfg(unix)]
mod scaffold;
#[cfg(unix)]
mod state;
#[cfg(unix)]
mod store;
#[cfg(unix)]
mod watch;

// Exit codes. 0 is success, including a `stop` that found nothing running; 1 is
// any other failure, including a `stop` that timed out.
/// `start` found a playground starting, stopping, or not answering, and left it alone.
const EXIT_BUSY: u8 = 1;
/// No project to serve, or no single blueprint in it.
const EXIT_NO_PROJECT: u8 = 2;
/// A package the blueprint needs could not be built or found.
const EXIT_PACKAGE_RESOLUTION: u8 = 5;
/// `status`, `open`, or `stop` found no playground it can reach: none running, or
/// one starting, stopping, or failing the nonce challenge.
const EXIT_UNREACHABLE: u8 = 6;
// A start interrupted while it waits stops what it launched and exits as the shell
// reports the signal: 129 for SIGHUP, 130 for SIGINT, 143 for SIGTERM. A signal
// the caller ignores (`nohup`) stays ignored.

#[derive(ClapArgs)]
#[command(args_conflicts_with_subcommands = true)]
pub struct Args {
    #[command(subcommand)]
    cmd: Option<PlaygroundCmd>,
    #[command(flatten)]
    start: StartArgs,
}

#[derive(Subcommand)]
pub enum PlaygroundCmd {
    /// Start the playground in the background, or attach to the running one,
    /// and print its ready record with a fresh login link (the default).
    Start(StartArgs),
    /// Report whether the playground is running, where, and with what.
    Status(OutputArgs),
    /// Print a fresh single-use login link for the running playground.
    Open(OutputArgs),
    /// Stop the running playground: drain its runs and end browser sessions.
    Stop(OutputArgs),
    /// Create a `submilli/` folder with a starter billing package, a blueprint that
    /// pins charges to the signed-in customer, and an example program. Writes
    /// nothing outside `submilli/`.
    Init(OutputArgs),
}

#[derive(ClapArgs, Clone, Default)]
pub struct StartArgs {
    /// Print the ready record as JSON.
    #[arg(long)]
    json: bool,
    /// Serve in this terminal until `submilli playground stop` or a signal,
    /// instead of in the background.
    #[arg(long)]
    foreground: bool,
    /// The blueprint to serve. Default: the single file under
    /// `submilli/blueprints/`, or the project's root `blueprint.yaml`.
    #[arg(long, value_name = "PATH")]
    blueprint: Option<PathBuf>,
    #[command(flatten)]
    egress: EgressArgs,
    /// How long a background start waits for the playground to be ready.
    #[arg(long, value_name = "MILLISECONDS", hide = true)]
    ready_timeout_ms: Option<u64>,
    /// Serve as the detached child of a `start`, which waits for its ready file.
    #[arg(long, hide = true)]
    child: bool,
}

/// The outbound grants programs get, on top of the deployed server's default of
/// denying loopback and private ranges.
#[derive(ClapArgs, Clone, Default)]
pub struct EgressArgs {
    /// Let programs reach IPv4 and IPv6 loopback, for an app's local services.
    /// Env: `$SUBMILLI_ALLOW_LOCALHOST` (`1`/`true`/`yes`/`on`), additive.
    #[arg(long)]
    allow_localhost: bool,
    /// Let programs reach RFC1918, CGNAT, and IPv6 ULA private ranges.
    /// Env: `$SUBMILLI_ALLOW_PRIVATE` (`1`/`true`/`yes`/`on`), additive.
    #[arg(long)]
    allow_private: bool,
    /// Let programs reach this address or CIDR range (repeatable).
    /// Env: `$SUBMILLI_ALLOW_IP` (comma-separated), additive.
    #[arg(long = "allow-ip", value_name = "IP|CIDR", value_parser = parse_ip_or_cidr)]
    allow_ip: Vec<IpNet>,
}

#[derive(ClapArgs, Clone, Copy)]
pub struct OutputArgs {
    /// Print JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum Output {
    Text,
    Json,
}

impl Output {
    fn from_json(json: bool) -> Self {
        if json { Self::Json } else { Self::Text }
    }
}

impl Args {
    /// Whether this invocation serves the playground in this process, where no
    /// CLI telemetry may run: the server's runs would otherwise reach it.
    pub fn serves(&self) -> bool {
        let start = match &self.cmd {
            None => &self.start,
            Some(PlaygroundCmd::Start(start)) => start,
            Some(_) => return false,
        };
        start.child || start.foreground
    }

    pub fn label(&self) -> &'static str {
        match &self.cmd {
            None | Some(PlaygroundCmd::Start(_)) => "playground.start",
            Some(PlaygroundCmd::Status(_)) => "playground.status",
            Some(PlaygroundCmd::Open(_)) => "playground.open",
            Some(PlaygroundCmd::Stop(_)) => "playground.stop",
            Some(PlaygroundCmd::Init(_)) => "playground.init",
        }
    }
}

#[cfg(not(unix))]
pub fn execute(_args: Args) -> anyhow::Result<ExitCode> {
    eprintln!("submilli playground is not supported on this platform yet; use macOS or Linux");
    Ok(ExitCode::from(1))
}

#[cfg(unix)]
pub fn execute(args: Args) -> anyhow::Result<ExitCode> {
    match args.cmd {
        None => unix::start(args.start),
        Some(PlaygroundCmd::Start(start)) => unix::start(start),
        Some(PlaygroundCmd::Status(output)) => unix::status(Output::from_json(output.json)),
        Some(PlaygroundCmd::Open(output)) => unix::open(Output::from_json(output.json)),
        Some(PlaygroundCmd::Stop(output)) => unix::stop(Output::from_json(output.json)),
        Some(PlaygroundCmd::Init(output)) => unix::init(Output::from_json(output.json)),
    }
}

/// The resolved egress grants: the flags plus their `SUBMILLI_ALLOW_*` variables.
#[derive(Clone, Default)]
pub(crate) struct Egress {
    allow_localhost: bool,
    allow_private: bool,
    allow_ip: Vec<IpNet>,
}

impl Egress {
    fn resolve(args: &EgressArgs) -> anyhow::Result<Self> {
        let mut allow_ip = args.allow_ip.clone();
        if let Ok(value) = std::env::var("SUBMILLI_ALLOW_IP") {
            for entry in value.split(',').map(str::trim).filter(|e| !e.is_empty()) {
                allow_ip.push(
                    parse_ip_or_cidr(entry)
                        .map_err(|message| anyhow::anyhow!("$SUBMILLI_ALLOW_IP: {message}"))?,
                );
            }
        }
        allow_ip.dedup();
        Ok(Self {
            allow_localhost: args.allow_localhost || env_flag("SUBMILLI_ALLOW_LOCALHOST"),
            allow_private: args.allow_private || env_flag("SUBMILLI_ALLOW_PRIVATE"),
            allow_ip,
        })
    }

    /// The deployed server's policy with these grants.
    #[cfg(unix)]
    fn policy(&self) -> submilli_server::NetworkPolicy {
        self.allow_ip.iter().fold(
            submilli_server::NetworkPolicy::deny_private()
                .allow_localhost(self.allow_localhost)
                .allow_private(self.allow_private),
            |policy, net| policy.allow_cidr(*net),
        )
    }

    /// The grants by the flag that gives each, as the ready record lists them.
    pub(crate) fn grants(&self) -> Vec<String> {
        let mut grants = Vec::new();
        if self.allow_localhost {
            grants.push("allow-localhost".to_owned());
        }
        if self.allow_private {
            grants.push("allow-private".to_owned());
        }
        grants.extend(self.allow_ip.iter().map(|net| format!("allow-ip {net}")));
        grants
    }

    /// The flags that give a child the same grants.
    #[cfg(unix)]
    fn args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if self.allow_localhost {
            args.push("--allow-localhost".to_owned());
        }
        if self.allow_private {
            args.push("--allow-private".to_owned());
        }
        for net in &self.allow_ip {
            args.push("--allow-ip".to_owned());
            args.push(net.to_string());
        }
        args
    }
}

fn env_flag(key: &str) -> bool {
    std::env::var(key).is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn parse_ip_or_cidr(value: &str) -> Result<IpNet, String> {
    if let Ok(net) = value.parse::<IpNet>() {
        return Ok(net);
    }
    value
        .parse::<IpAddr>()
        .map(IpNet::from)
        .map_err(|_| format!("expected an IP address or CIDR range, got `{value}`"))
}

/// What `start` prints: the running instance's description plus a login link.
/// Never a token: the record names the app token's file, not its value.
#[cfg(unix)]
#[derive(serde::Serialize)]
pub(crate) struct ReadyRecord<'a> {
    #[serde(flatten)]
    description: &'a host::Description,
    #[serde(skip_serializing_if = "Option::is_none")]
    packages_error: Option<&'a str>,
    login_url: String,
    attached: bool,
}

#[cfg(unix)]
impl<'a> ReadyRecord<'a> {
    pub(crate) fn new(
        description: &'a host::Description,
        packages_error: Option<&'a str>,
        login_code: &str,
        attached: bool,
    ) -> Self {
        Self {
            description,
            packages_error,
            login_url: login_url(&description.url, login_code),
            attached,
        }
    }

    pub(crate) fn print(&self, output: Output) {
        match output {
            Output::Json => match serde_json::to_string(self) {
                Ok(json) => println!("{json}"),
                Err(error) => eprintln!("encoding the ready record failed: {error}"),
            },
            Output::Text => {
                let heading = if self.attached {
                    "Submilli playground is already running"
                } else {
                    "Submilli playground is running"
                };
                println!("{heading} (pid {}).", self.description.pid);
                print_description(self.description, self.packages_error);
                println!(
                    "  open:       {}  (single use, expires in {})",
                    self.login_url,
                    login_code_ttl_text()
                );
                println!(
                    "Point an app at the server with the token in the app token file. Stop it with \
                     `submilli playground stop`."
                );
            }
        }
    }
}

/// The page's address with a login code in its fragment, which the browser never
/// sends to a server.
#[cfg(unix)]
fn login_url(url: &str, code: &str) -> String {
    format!("{url}#login={code}")
}

/// How long a login code lasts, for people: "5 minutes".
#[cfg(unix)]
fn login_code_ttl_text() -> String {
    let secs = control_auth::LOGIN_CODE_TTL.as_secs();
    match (secs / 60, secs % 60) {
        (1, 0) => "1 minute".to_owned(),
        (minutes, 0) => format!("{minutes} minutes"),
        _ => format!("{secs} seconds"),
    }
}

#[cfg(unix)]
fn print_description(description: &host::Description, packages_error: Option<&str>) {
    println!("  project:    {}", description.project.display());
    println!(
        "  blueprint:  {} ({})",
        description.blueprint.name,
        description.blueprint.path.display()
    );
    println!("  page:       {}", description.url);
    println!("  server:     {}", description.server_url);
    println!("  app token:  {}", description.app_token_file.display());
    if description.egress_grants.is_empty() {
        println!("  egress:     loopback and private ranges denied (see --allow-localhost)");
    } else {
        println!("  egress:     {}", description.egress_grants.join(", "));
    }
    match &description.provider {
        Some(provider) => {
            println!("  provider:   {provider} (keys exported after start need a restart)");
        }
        None => println!(
            "  provider:   none; export ANTHROPIC_API_KEY, OPENAI_API_KEY, or GEMINI_API_KEY \
             and restart"
        ),
    }
    println!("  log:        {}", description.log_file.display());
    print_packages(&description.packages, packages_error);
}

/// The package closure, one package per line: its origin and whether a program may
/// import it.
#[cfg(unix)]
fn print_packages(packages: &[packages::ClosureEntry], error: Option<&str>) {
    if packages.is_empty() {
        println!("  packages:   none");
    }
    for (index, package) in packages.iter().enumerate() {
        let label = if index == 0 { "packages:" } else { "" };
        let mut notes = vec![
            match package.origin {
                packages::Origin::Blueprint => "blueprint",
                packages::Origin::Dependency => "dependency",
            }
            .to_owned(),
        ];
        notes.push(if package.importable {
            "importable".to_owned()
        } else {
            "not importable".to_owned()
        });
        if package.project {
            notes.push("project".to_owned());
        }
        println!(
            "  {label:<11} {} {} ({})",
            package.name,
            package.version,
            notes.join(", ")
        );
    }
    if let Some(error) = error {
        println!("  packages:   {error}");
    }
}

#[cfg(unix)]
mod unix {
    use std::process::{Command, ExitCode, Stdio};
    use std::time::{Duration, Instant};

    use anyhow::{Context, Result, bail};
    use serde_json::json;

    use super::client::{self, Busy, Probe};
    use super::host::{self, HostOptions, Status};
    use super::packages::ProjectPackages;
    use super::project::{self, DiscoveryError, Project};
    use super::scaffold;
    use super::state::{Holder, InstanceRecord, StateDir, random_hex};
    use super::watch::describe_refusal;
    use super::{
        EXIT_BUSY, EXIT_NO_PROJECT, EXIT_PACKAGE_RESOLUTION, EXIT_UNREACHABLE, Egress, Output,
        ReadyRecord, StartArgs, login_code_ttl_text, login_url, print_description,
    };

    /// How long a background start waits for its child to be ready.
    const READY_TIMEOUT: Duration = Duration::from_secs(60);
    /// How long `stop` waits for the instance to drain and exit.
    const STOP_TIMEOUT: Duration = Duration::from_secs(20);
    const POLL: Duration = Duration::from_millis(25);
    const NONCE_ENV: &str = "SUBMILLI_PLAYGROUND_START_NONCE";
    /// What a start says when what it launched was stopped before it was ready.
    const STOPPED_WHILE_STARTING: &str =
        "the playground was stopped while it was starting (`submilli playground stop` or a signal)";

    pub(super) fn start(args: StartArgs) -> Result<ExitCode> {
        let output = super::Output::from_json(args.json);
        let cwd = std::env::current_dir().context("reading the current directory")?;
        let project = match project::discover(&cwd, args.blueprint.as_deref()) {
            Ok(project) => project,
            Err(DiscoveryError::NoProject { .. }) if !args.child && offer_init(&cwd)? => {
                project::discover(&cwd, args.blueprint.as_deref())?
            }
            Err(error) => {
                eprintln!("{error}");
                return Ok(ExitCode::from(EXIT_NO_PROJECT));
            }
        };
        let egress = Egress::resolve(&args.egress)?;
        let state = StateDir::for_project(&project.root);
        state.create()?;

        if args.child {
            let nonce = std::env::var(NONCE_ENV)
                .ok()
                .filter(|nonce| !nonce.is_empty())
                .with_context(|| format!("a playground child needs ${NONCE_ENV}"))?;
            host::serve(HostOptions {
                project,
                egress,
                nonce,
                start_lock: None,
                print: None,
            })?;
            return Ok(ExitCode::SUCCESS);
        }

        let start_lock = state.start_lock()?;
        // Ensure the tokens exist before anything can be told where they are; under
        // the start lock, so concurrent first starts agree on them.
        state.tokens()?;
        match client::probe(&state)? {
            Probe::Running(running) => return attach(&running, &project, &egress, output),
            Probe::Stale(stale) => state.remove_if_ours(&stale.nonce),
            Probe::Busy(busy) => {
                // Its lock stays: the instance holding it may answer in a moment.
                eprintln!("{}", busy.message());
                return Ok(ExitCode::from(EXIT_BUSY));
            }
            Probe::NotRunning => {}
        }
        // A package the blueprint needs that cannot be built or found fails here, with
        // the command that fixes it, rather than in the detached child's log.
        if let Err(message) = check_packages(&project) {
            eprintln!("{message}");
            return Ok(ExitCode::from(EXIT_PACKAGE_RESOLUTION));
        }
        let nonce = random_hex(32)?;
        if args.foreground {
            host::serve(HostOptions {
                project,
                egress,
                nonce,
                start_lock: Some(start_lock),
                print: Some(output),
            })?;
            return Ok(ExitCode::SUCCESS);
        }
        let timeout = args
            .ready_timeout_ms
            .map_or(READY_TIMEOUT, Duration::from_millis);
        let result = spawn_and_wait(&project, &state, &egress, &nonce, timeout);
        drop(start_lock);
        match result {
            Ok(Waited::Ready) => {}
            Ok(Waited::Interrupted(signal)) => {
                eprintln!("interrupted; the playground this start launched was stopped");
                return Ok(ExitCode::from(interrupt::exit_code(signal)));
            }
            Err(error) => {
                eprintln!("{error:#}");
                return Ok(ExitCode::from(1));
            }
        }
        match client::probe(&state)? {
            Probe::Running(running) => {
                let status = running.status()?;
                if status.stopping {
                    eprintln!("{STOPPED_WHILE_STARTING}");
                    return Ok(ExitCode::from(1));
                }
                print_ready(&running, &status, output, false)
            }
            Probe::Busy(busy) if busy.is_stopping() => {
                eprintln!("{STOPPED_WHILE_STARTING}");
                Ok(ExitCode::from(1))
            }
            Probe::NotRunning | Probe::Stale(_) | Probe::Busy(_) => {
                eprintln!("the playground started but does not answer its control listener");
                Ok(ExitCode::from(1))
            }
        }
    }

    /// Build the blueprint's project packages and resolve its closure, as the child
    /// will. A blueprint that does not parse is left for the child to report.
    fn check_packages(project: &Project) -> std::result::Result<(), String> {
        let Ok(yaml) = std::fs::read_to_string(&project.blueprint) else {
            return Ok(());
        };
        let Ok(blueprint) = submilli_blueprint::parse(&yaml) else {
            return Ok(());
        };
        ProjectPackages::new(
            &project.package_dir,
            submilli_build::default_package_store_dir(),
        )
        .prepare(&blueprint)
        .map(|_| ())
        .map_err(|failure| failure.to_string())
    }

    /// Outside any project, a terminal is asked whether to create one; anything else
    /// gets the command to run.
    fn offer_init(cwd: &std::path::Path) -> Result<bool> {
        use std::io::IsTerminal;
        if !(std::io::stdin().is_terminal() && std::io::stderr().is_terminal()) {
            return Ok(false);
        }
        let root = scaffold::root_for(cwd);
        let create = dialoguer::Confirm::new()
            .with_prompt(format!(
                "No Submilli project here. Create {} with a starter billing package?",
                root.join(scaffold::FOLDER).display()
            ))
            .default(true)
            .interact()
            .context("asking whether to create a project")?;
        if !create {
            return Ok(false);
        }
        let scaffolded = scaffold::init(&root)?;
        report_init(&scaffolded, Output::Text);
        Ok(true)
    }

    pub(super) fn init(output: Output) -> Result<ExitCode> {
        let cwd = std::env::current_dir().context("reading the current directory")?;
        if let Some(root) = project::find_project_root(&cwd) {
            eprintln!(
                "{} already holds a Submilli project; start it with `submilli playground`",
                root.display()
            );
            return Ok(ExitCode::from(1));
        }
        match scaffold::init(&scaffold::root_for(&cwd)) {
            Ok(scaffolded) => {
                report_init(&scaffolded, output);
                Ok(ExitCode::SUCCESS)
            }
            Err(error) => {
                eprintln!("{error:#}");
                Ok(ExitCode::from(1))
            }
        }
    }

    fn report_init(scaffolded: &scaffold::Scaffolded, output: Output) {
        match output {
            Output::Json => println!(
                "{}",
                json!({
                    "project": scaffolded.root,
                    "files": scaffolded.files,
                    "blueprint": scaffold::BLUEPRINT_NAME,
                    "package": scaffold::PACKAGE_NAME,
                    "example": scaffolded.root.join(scaffold::FOLDER).join(scaffold::EXAMPLE),
                    "variables": { "customerId": "cus_northwind" },
                })
            ),
            Output::Text => {
                for file in &scaffolded.files {
                    eprintln!("created {}", file.display());
                }
                eprintln!(
                    "Start the playground with `submilli playground`; it builds {} and serves \
                     blueprint `{}`. The example {} runs with customerId cus_northwind.",
                    scaffold::PACKAGE_NAME,
                    scaffold::BLUEPRINT_NAME,
                    scaffold::EXAMPLE
                );
            }
        }
    }

    /// Report the running playground. A start asking for another blueprint or other
    /// grants is told the running one keeps its own, and how to change them.
    fn attach(
        running: &client::Running,
        project: &Project,
        egress: &Egress,
        output: Output,
    ) -> Result<ExitCode> {
        let status = running.status()?;
        if let Some(busy) = draining(status.stopping, status.description.pid) {
            eprintln!("{}", busy.message());
            return Ok(ExitCode::from(EXIT_BUSY));
        }
        let description = &status.description;
        let same_blueprint = same_path(&description.blueprint.path, &project.blueprint);
        let mut asked = egress.grants();
        let mut has = description.egress_grants.clone();
        asked.sort();
        has.sort();
        if !same_blueprint || asked != has {
            eprintln!(
                "note: the running playground keeps its own settings (blueprint {}, egress {}); \
                 to start it with these, run `submilli playground stop` and start it again",
                description.blueprint.path.display(),
                if has.is_empty() {
                    "with no grants".to_owned()
                } else {
                    has.join(", ")
                }
            );
        }
        print_ready(running, &status, output, true)
    }

    fn same_path(one: &std::path::Path, other: &std::path::Path) -> bool {
        match (one.canonicalize(), other.canonicalize()) {
            (Ok(one), Ok(other)) => one == other,
            _ => one == other,
        }
    }

    /// The ready record for `status` with a fresh login code: `attached` when this
    /// start found the playground already running.
    fn print_ready(
        running: &client::Running,
        status: &Status,
        output: Output,
        attached: bool,
    ) -> Result<ExitCode> {
        let code = running.mint_login_code()?;
        ReadyRecord::new(
            &status.description,
            status.packages_error.as_deref(),
            &code,
            attached,
        )
        .print(output);
        Ok(ExitCode::SUCCESS)
    }

    /// How a wait for the child ended, when it did not fail.
    enum Waited {
        Ready,
        /// This process was told to stop, by this signal, while it waited; the
        /// child was stopped too.
        Interrupted(libc::c_int),
    }

    /// Re-execute this binary as a detached child that serves the playground, and
    /// wait for its ready file. The child's output goes to the log, never to this
    /// process's pipes, so a caller reading them to the end is not held open.
    fn spawn_and_wait(
        project: &Project,
        state: &StateDir,
        egress: &Egress,
        nonce: &str,
        timeout: Duration,
    ) -> Result<Waited> {
        use std::os::unix::process::CommandExt;

        // Before the child exists: a Ctrl-C from here on stops it rather than
        // leaving it behind with no one to report it.
        let interrupts = interrupt::Guard::install()?;

        let _ = std::fs::remove_file(state.ready_path());
        let log = state.fresh_log()?;
        let executable = std::env::current_exe().context("locating this executable")?;
        let mut command = Command::new(executable);
        command
            .args(["playground", "start", "--child", "--blueprint"])
            .arg(&project.blueprint)
            .args(egress.args())
            .current_dir(&project.root)
            .env(NONCE_ENV, nonce)
            .stdin(Stdio::null())
            .stdout(log.try_clone().context("opening the playground log")?)
            .stderr(log)
            // Its own process group, so a Ctrl-C meant for the caller's terminal
            // does not reach the playground.
            .process_group(0);
        let deadline = Instant::now()
            .checked_add(timeout)
            .with_context(|| format!("a ready timeout of {}s is too long", timeout.as_secs()))?;
        let mut child = command.spawn().context("starting the playground")?;
        let waited = wait_for_ready(&mut child, state, nonce, timeout, deadline, &interrupts);
        // Whatever ended the wait other than readiness ends the child too, so no
        // start leaves behind a playground it does not report.
        if !matches!(waited, Ok(Waited::Ready)) {
            let _ = child.kill();
            let _ = child.wait();
            state.remove_if_ours(nonce);
        }
        waited
    }

    /// Poll until `child` writes its ready file, exits, or runs out of time, or this
    /// start is interrupted.
    fn wait_for_ready(
        child: &mut std::process::Child,
        state: &StateDir,
        nonce: &str,
        timeout: Duration,
        deadline: Instant,
        interrupts: &interrupt::Guard,
    ) -> Result<Waited> {
        use std::os::unix::process::ExitStatusExt;
        loop {
            if let Some(signal) = interrupts.interrupted() {
                return Ok(Waited::Interrupted(signal));
            }
            if let Some(exit) = child.try_wait().context("waiting for the playground")? {
                // It ends cleanly on SIGTERM or SIGINT once its handlers are in,
                // and by the signal itself before then.
                if exit.success() || matches!(exit.signal(), Some(libc::SIGTERM | libc::SIGINT)) {
                    bail!("{STOPPED_WHILE_STARTING}");
                }
                let log = std::fs::read_to_string(state.log_path()).unwrap_or_default();
                bail!(
                    "the playground exited before it was ready ({exit}):\n{}",
                    log.trim_end()
                );
            }
            let ready = state.read_ready()?;
            if ready
                .as_ref()
                .is_some_and(|ready| accepts(ready, nonce, child.id()))
            {
                break;
            }
            if Instant::now() >= deadline {
                bail!(
                    "the playground was not ready within {:.1}s and was stopped; its log is {}",
                    timeout.as_secs_f64(),
                    state.log_path().display()
                );
            }
            std::thread::sleep(POLL);
        }
        // A signal that arrived after the last check is still this start's to honor.
        if let Some(signal) = interrupts.interrupted() {
            return Ok(Waited::Interrupted(signal));
        }
        Ok(Waited::Ready)
    }

    /// Only the ready file this start's child wrote counts.
    fn accepts(ready: &InstanceRecord, nonce: &str, child: u32) -> bool {
        ready.nonce == nonce && ready.pid == child
    }

    pub(super) fn status(output: Output) -> Result<ExitCode> {
        let Some(state) = state_for_cwd()? else {
            return not_running(output);
        };
        let running = match client::probe(&state)? {
            Probe::Running(running) => running,
            Probe::Busy(busy) => return busy_exit(output, &busy),
            Probe::NotRunning | Probe::Stale(_) => return not_running(output),
        };
        let mut status = running.status()?;
        if let Some(busy) = draining(status.stopping, status.description.pid) {
            return busy_exit(output, &busy);
        }
        let health = if running.server_healthy() {
            "ok"
        } else {
            "server unreachable"
        };
        status.health = Some(health.to_owned());
        match output {
            Output::Json => println!(
                "{}",
                serde_json::to_string(&status).context("encoding the status")?
            ),
            Output::Text => {
                println!(
                    "Submilli playground is running (pid {}).",
                    status.description.pid
                );
                print_description(&status.description, status.packages_error.as_deref());
                println!("  health:     {health}");
                println!("  browsers:   {} signed in", status.browser_sessions);
                let blueprint = &status.blueprint_status;
                if let Some(version) = blueprint.version {
                    println!("  version:    {version} in force");
                }
                if let Some(refused) = &blueprint.refused {
                    println!("  refused:    {}", describe_refusal(refused));
                }
            }
        }
        Ok(ExitCode::SUCCESS)
    }

    pub(super) fn open(output: Output) -> Result<ExitCode> {
        let Some(state) = state_for_cwd()? else {
            return not_running(output);
        };
        let running = match client::probe(&state)? {
            Probe::Running(running) => running,
            Probe::Busy(busy) => return busy_exit(output, &busy),
            Probe::NotRunning | Probe::Stale(_) => return not_running(output),
        };
        let page = running.page()?;
        if let Some(busy) = draining(page.stopping, running.record.pid) {
            return busy_exit(output, &busy);
        }
        let code = running.mint_login_code()?;
        let url = &page.url;
        let login_url = login_url(url, &code);
        match output {
            Output::Json => println!(
                "{}",
                json!({
                    "url": url,
                    "login_url": login_url,
                    "expires_in_secs": super::control_auth::LOGIN_CODE_TTL.as_secs(),
                })
            ),
            Output::Text => {
                println!("{login_url}");
                println!("Single use; expires in {}.", login_code_ttl_text());
            }
        }
        Ok(ExitCode::SUCCESS)
    }

    pub(super) fn stop(output: Output) -> Result<ExitCode> {
        let Some(state) = state_for_cwd()? else {
            return stopped(output, None, "not running");
        };
        let running = match client::probe(&state)? {
            Probe::NotRunning => return stopped(output, None, "not running"),
            Probe::Stale(stale) => {
                state.remove_if_ours(&stale.nonce);
                return stopped(output, None, "not running (removed a stale lock)");
            }
            Probe::Busy(Busy::Starting(holder)) => {
                return match starting_target(&holder) {
                    Some(pid) => stop_starting(&state, output, pid),
                    None => busy_exit(output, &Busy::Starting(holder)),
                };
            }
            Probe::Busy(Busy::Stopping { pid: Some(pid) }) => {
                wait_for_exit(&state, pid, None)?;
                return stopped(output, Some(pid), "stopped");
            }
            Probe::Busy(busy) => return busy_exit(output, &busy),
            Probe::Running(running) => running,
        };
        running.stop()?;
        let record = running.record.clone();
        wait_for_exit(&state, record.pid, Some(&record.nonce))?;
        state.remove_if_ours(&record.nonce);
        stopped(output, Some(record.pid), "stopped")
    }

    /// The busy answer for an instance that answered that it is draining, if it did.
    fn draining(stopping: bool, pid: u32) -> Option<Busy> {
        stopping.then(|| Busy::stopping(pid))
    }

    /// The process `stop` may signal to end an instance that is still starting:
    /// only one the kernel names as the instance lock's holder. A pid the holder
    /// only wrote down may be another namespace's, or reused; it is never signaled.
    fn starting_target(holder: &Holder) -> Option<u32> {
        holder.pid_from_kernel
    }

    /// An instance still starting has no control listener yet: it is sent SIGTERM,
    /// which ends it before it serves (or drains it if it just began to).
    fn stop_starting(state: &StateDir, output: Output, pid: u32) -> Result<ExitCode> {
        // Asked again just before the signal: it goes to the process the kernel
        // says holds the instance lock, never to a pid since reused.
        if state
            .instance_holder()?
            .and_then(|holder| starting_target(&holder))
            != Some(pid)
        {
            return stopped(output, None, "not running");
        }
        let target = libc::pid_t::try_from(pid)
            .with_context(|| format!("pid {pid} is not a process id here"))?;
        // SAFETY: kill takes plain integers and touches no memory.
        if unsafe { libc::kill(target, libc::SIGTERM) } != 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(error).with_context(|| format!("signaling the playground (pid {pid})"));
            }
        }
        wait_for_exit(state, pid, None)?;
        stopped(output, Some(pid), "stopped")
    }

    /// Wait until `pid` no longer holds the instance lock. The lock goes when the
    /// process ends, however it ends; a lock file with a nonce other than `nonce`
    /// is a new instance, which is not this stop's to wait for.
    fn wait_for_exit(state: &StateDir, pid: u32, nonce: Option<&str>) -> Result<()> {
        let deadline = Instant::now()
            .checked_add(STOP_TIMEOUT)
            .context("the clock cannot represent the stop deadline")?;
        loop {
            let gone = state
                .instance_holder()?
                .is_none_or(|holder| holder.pid().is_some_and(|held| held != pid));
            let replaced = nonce.is_some_and(|nonce| {
                state
                    .read_record()
                    .ok()
                    .flatten()
                    .is_some_and(|current| current.nonce != nonce)
            });
            if gone || replaced {
                return Ok(());
            }
            if Instant::now() >= deadline {
                bail!(
                    "the playground (pid {pid}) did not stop within {}s",
                    STOP_TIMEOUT.as_secs()
                );
            }
            std::thread::sleep(POLL);
        }
    }

    /// A process serves the project but cannot be reached: said, and exit 6, with
    /// its lock left in place.
    fn busy_exit(output: Output, busy: &Busy) -> Result<ExitCode> {
        match output {
            Output::Json => println!(
                "{}",
                json!({
                    "running": !busy.is_stopping(),
                    "busy": true,
                    "stopping": busy.is_stopping(),
                    "pid": busy.pid(),
                })
            ),
            Output::Text => {}
        }
        eprintln!("{}", busy.message());
        Ok(ExitCode::from(EXIT_UNREACHABLE))
    }

    fn stopped(output: Output, pid: Option<u32>, message: &str) -> Result<ExitCode> {
        match output {
            Output::Json => println!(
                "{}",
                json!({ "running": false, "stopped": pid.is_some(), "pid": pid })
            ),
            Output::Text => match pid {
                Some(pid) => println!("Submilli playground {message} (pid {pid})."),
                None => println!("Submilli playground {message}."),
            },
        }
        Ok(ExitCode::SUCCESS)
    }

    fn not_running(output: Output) -> Result<ExitCode> {
        match output {
            Output::Json => println!("{}", json!({ "running": false })),
            Output::Text => {
                println!(
                    "Submilli playground is not running. Start it with `submilli playground`."
                );
            }
        }
        Ok(ExitCode::from(EXIT_UNREACHABLE))
    }

    /// The state directory of the project around the current directory, if any.
    /// Commands other than `start` need no blueprint.
    fn state_for_cwd() -> Result<Option<StateDir>> {
        let cwd = std::env::current_dir().context("reading the current directory")?;
        Ok(project::find_project_root(&cwd).map(|root| StateDir::for_project(&root)))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn holder(pid_from_kernel: Option<u32>, recorded_pid: Option<u32>) -> Holder {
            Holder {
                pid_from_kernel,
                recorded_pid,
                stopping: false,
            }
        }

        #[test]
        fn stop_signals_only_the_pid_the_kernel_names() {
            assert_eq!(starting_target(&holder(Some(41), Some(41))), Some(41));
            // A record left by a previous holder never redirects the signal.
            assert_eq!(starting_target(&holder(Some(41), Some(7))), Some(41));
            // The kernel named no pid (another PID namespace, a network
            // filesystem): nothing is signaled, and the recorded pid is only named.
            let unnamed = holder(None, Some(7));
            assert_eq!(starting_target(&unnamed), None);
            let busy = Busy::Starting(unnamed);
            assert_eq!(busy.pid(), Some(7));
            let message = busy.message();
            assert!(message.contains("pid 7"), "{message}");
            assert!(message.contains("sends it no signal"), "{message}");
            assert_eq!(starting_target(&holder(None, None)), None);
        }

        #[test]
        fn only_a_draining_answer_is_busy() {
            assert!(draining(false, 9).is_none());
            let busy = draining(true, 9).expect("busy");
            assert!(busy.is_stopping());
            assert_eq!(busy.pid(), Some(9));
        }
    }

    /// SIGINT, SIGTERM, and SIGHUP noted rather than acted on, while a start waits
    /// for the child it launched, so it can stop that child before it exits.
    mod interrupt {
        use std::sync::atomic::{AtomicI32, Ordering};

        use anyhow::Result;

        const SIGNALS: [libc::c_int; 3] = [libc::SIGINT, libc::SIGTERM, libc::SIGHUP];

        /// The last signal noted, or 0.
        static INTERRUPTED: AtomicI32 = AtomicI32::new(0);

        extern "C" fn note(signal: libc::c_int) {
            INTERRUPTED.store(signal, Ordering::SeqCst);
        }

        /// How the shell reports a process ended by `signal`: 128 plus its number.
        pub(super) fn exit_code(signal: libc::c_int) -> u8 {
            match signal {
                libc::SIGHUP => 129,
                libc::SIGTERM => 143,
                _ => 130,
            }
        }

        /// The handlers, installed until it drops, which puts back what was there.
        pub(super) struct Guard {
            previous: Vec<(libc::c_int, libc::sigaction)>,
        }

        impl Guard {
            pub(super) fn install() -> Result<Self> {
                INTERRUPTED.store(0, Ordering::SeqCst);
                let mut guard = Self {
                    previous: Vec::with_capacity(SIGNALS.len()),
                };
                for signal in SIGNALS {
                    // SAFETY: a zeroed sigaction is a valid "no handler, empty mask"
                    // value to fill in; `note` only stores to an atomic, which is
                    // async-signal-safe; every pointer is to a live local or null,
                    // which sigaction accepts for the action it does not set.
                    let previous = unsafe {
                        let mut previous: libc::sigaction = std::mem::zeroed();
                        if libc::sigaction(signal, std::ptr::null(), &mut previous) != 0 {
                            return Err(anyhow::anyhow!(
                                "reading a signal's handler: {}",
                                std::io::Error::last_os_error()
                            ));
                        }
                        // Ignored by whoever started this (`nohup`): left ignored.
                        if previous.sa_sigaction == libc::SIG_IGN {
                            continue;
                        }
                        let mut action: libc::sigaction = std::mem::zeroed();
                        action.sa_sigaction = note as extern "C" fn(libc::c_int) as usize;
                        libc::sigemptyset(&mut action.sa_mask);
                        if libc::sigaction(signal, &action, std::ptr::null_mut()) != 0 {
                            return Err(anyhow::anyhow!(
                                "installing a signal handler: {}",
                                std::io::Error::last_os_error()
                            ));
                        }
                        previous
                    };
                    guard.previous.push((signal, previous));
                }
                Ok(guard)
            }

            /// The signal this process was told to stop by, if any.
            pub(super) fn interrupted(&self) -> Option<libc::c_int> {
                Some(INTERRUPTED.load(Ordering::SeqCst)).filter(|signal| *signal != 0)
            }
        }

        impl Drop for Guard {
            fn drop(&mut self) {
                for (signal, previous) in &self.previous {
                    // SAFETY: `previous` is the action sigaction returned for this
                    // signal, restored as it was.
                    unsafe {
                        libc::sigaction(*signal, previous, std::ptr::null_mut());
                    }
                }
            }
        }
    }
}
