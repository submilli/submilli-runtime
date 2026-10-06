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
use serde_json::Value;

#[cfg(unix)]
mod client;
#[cfg(unix)]
mod control_auth;
#[cfg(unix)]
mod host;
#[cfg(unix)]
mod labels;
#[cfg(unix)]
mod project;
#[cfg(unix)]
mod state;
#[cfg(unix)]
mod store;

/// Not running, or a lock whose listener failed the nonce challenge.
const EXIT_NOT_RUNNING: u8 = 6;
/// No project to serve, or no single blueprint in it.
const EXIT_NO_PROJECT: u8 = 2;

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
pub(crate) struct ReadyRecord(Value);

impl ReadyRecord {
    pub(crate) fn new(mut description: Value, login_code: &str, attached: bool) -> Self {
        if let Some(object) = description.as_object_mut() {
            object.remove("running");
            object.remove("browser_sessions");
            let url = object
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            object.insert(
                "login_url".into(),
                Value::String(format!("{url}#login={login_code}")),
            );
            object.insert("attached".into(), Value::Bool(attached));
        }
        Self(description)
    }

    pub(crate) fn print(&self, output: Output) {
        match output {
            Output::Json => println!("{}", self.0),
            Output::Text => {
                let record = &self.0;
                let heading = if record["attached"] == true {
                    "Submilli playground is already running"
                } else {
                    "Submilli playground is running"
                };
                println!("{heading} (pid {}).", record["pid"]);
                print_description(record);
                println!(
                    "  open:       {}  (single use, expires in 5 minutes)",
                    text(&record["login_url"])
                );
                println!(
                    "Point an app at the server with the token in the app token file. Stop it with \
                     `submilli playground stop`."
                );
            }
        }
    }
}

fn print_description(record: &Value) {
    println!("  project:    {}", text(&record["project"]));
    println!(
        "  blueprint:  {} ({})",
        text(&record["blueprint"]["name"]),
        text(&record["blueprint"]["path"])
    );
    println!("  page:       {}", text(&record["url"]));
    println!("  server:     {}", text(&record["server_url"]));
    println!("  app token:  {}", text(&record["app_token_file"]));
    let grants: Vec<&str> = record["egress_grants"]
        .as_array()
        .map(|grants| grants.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if grants.is_empty() {
        println!("  egress:     loopback and private ranges denied (see --allow-localhost)");
    } else {
        println!("  egress:     {}", grants.join(", "));
    }
    match record["provider"].as_str() {
        Some(provider) => {
            println!("  provider:   {provider} (keys exported after start need a restart)");
        }
        None => println!(
            "  provider:   none; export ANTHROPIC_API_KEY, OPENAI_API_KEY, or GEMINI_API_KEY \
             and restart"
        ),
    }
    println!("  log:        {}", text(&record["log_file"]));
}

fn text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => "-".to_owned(),
        other => other.to_string(),
    }
}

#[cfg(unix)]
mod unix {
    use std::process::{Command, ExitCode, Stdio};
    use std::time::{Duration, Instant};

    use anyhow::{Context, Result, bail};
    use serde_json::json;

    use super::client::{self, Probe};
    use super::host::{self, HostOptions};
    use super::project::{self, Project};
    use super::state::{Lock, StateDir, random_hex};
    use super::{
        EXIT_NO_PROJECT, EXIT_NOT_RUNNING, Egress, Output, ReadyRecord, StartArgs,
        print_description,
    };

    /// How long a background start waits for its child to be ready.
    const READY_TIMEOUT: Duration = Duration::from_secs(60);
    /// How long `stop` waits for the instance to drain and exit.
    const STOP_TIMEOUT: Duration = Duration::from_secs(20);
    const POLL: Duration = Duration::from_millis(25);
    const NONCE_ENV: &str = "SUBMILLI_PLAYGROUND_START_NONCE";

    pub(super) fn start(args: StartArgs) -> Result<ExitCode> {
        let output = super::Output::from_json(args.json);
        let cwd = std::env::current_dir().context("reading the current directory")?;
        let project = match project::discover(&cwd, args.blueprint.as_deref()) {
            Ok(project) => project,
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

        // Ensure the tokens exist before anything can be told where they are.
        state.tokens()?;
        let start_lock = state.start_lock()?;
        match client::probe(&state)? {
            Probe::Running(running) => return attach(&running, output),
            Probe::Stale => state.remove_stale(),
            Probe::NotRunning => {}
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
            Ok(()) => {}
            Err(error) => {
                eprintln!("{error:#}");
                return Ok(ExitCode::from(1));
            }
        }
        match client::probe(&state)? {
            Probe::Running(running) => print_ready(&running, output, false),
            Probe::NotRunning | Probe::Stale => {
                eprintln!("the playground started but does not answer its control listener");
                Ok(ExitCode::from(1))
            }
        }
    }

    fn attach(running: &client::Running, output: Output) -> Result<ExitCode> {
        print_ready(running, output, true)
    }

    fn print_ready(running: &client::Running, output: Output, attached: bool) -> Result<ExitCode> {
        let code = running.mint_login_code()?;
        let status = running.status()?;
        ReadyRecord::new(status, &code, attached).print(output);
        Ok(ExitCode::SUCCESS)
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
    ) -> Result<()> {
        use std::os::unix::process::CommandExt;

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
        let mut child = command.spawn().context("starting the playground")?;
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(exit) = child.try_wait().context("waiting for the playground")? {
                state.remove_if_ours(nonce);
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
                return Ok(());
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                state.remove_if_ours(nonce);
                bail!(
                    "the playground was not ready within {:.1}s and was stopped; its log is {}",
                    timeout.as_secs_f64(),
                    state.log_path().display()
                );
            }
            std::thread::sleep(POLL);
        }
    }

    /// Only the ready file this start's child wrote counts.
    fn accepts(ready: &Lock, nonce: &str, child: u32) -> bool {
        ready.nonce == nonce && ready.pid == child
    }

    pub(super) fn status(output: Output) -> Result<ExitCode> {
        let Some(state) = state_for_cwd()? else {
            return not_running(output);
        };
        let Probe::Running(running) = client::probe(&state)? else {
            return not_running(output);
        };
        let mut status = running.status()?;
        let health = if running.server_healthy() {
            "ok"
        } else {
            "server unreachable"
        };
        if let Some(object) = status.as_object_mut() {
            object.insert("health".into(), json!(health));
        }
        match output {
            Output::Json => println!("{status}"),
            Output::Text => {
                println!("Submilli playground is running (pid {}).", status["pid"]);
                print_description(&status);
                println!("  health:     {health}");
                println!(
                    "  browsers:   {} signed in",
                    status["browser_sessions"].as_u64().unwrap_or(0)
                );
            }
        }
        Ok(ExitCode::SUCCESS)
    }

    pub(super) fn open(output: Output) -> Result<ExitCode> {
        let Some(state) = state_for_cwd()? else {
            return not_running(output);
        };
        let Probe::Running(running) = client::probe(&state)? else {
            return not_running(output);
        };
        let code = running.mint_login_code()?;
        let status = running.status()?;
        let url = status["url"].as_str().unwrap_or_default();
        let login_url = format!("{url}#login={code}");
        match output {
            Output::Json => println!(
                "{}",
                json!({ "url": url, "login_url": login_url, "expires_in_secs": 300 })
            ),
            Output::Text => {
                println!("{login_url}");
                println!("Single use; expires in 5 minutes.");
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
            Probe::Stale => {
                state.remove_stale();
                return stopped(output, None, "not running (removed a stale lock)");
            }
            Probe::Running(running) => running,
        };
        running.stop()?;
        let lock = running.lock.clone();
        let deadline = Instant::now() + STOP_TIMEOUT;
        // The instance removes its lock as the last thing it does; a dead pid
        // means it ended without getting there.
        while state.read_lock()?.is_some_and(|current| current == lock)
            && client::pid_alive(lock.pid)
        {
            if Instant::now() >= deadline {
                bail!(
                    "the playground (pid {}) did not stop within {}s",
                    lock.pid,
                    STOP_TIMEOUT.as_secs()
                );
            }
            std::thread::sleep(POLL);
        }
        state.remove_if_ours(&lock.nonce);
        stopped(output, Some(lock.pid), "stopped")
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
        Ok(ExitCode::from(EXIT_NOT_RUNNING))
    }

    /// The state directory of the project around the current directory, if any.
    /// Commands other than `start` need no blueprint.
    fn state_for_cwd() -> Result<Option<StateDir>> {
        let cwd = std::env::current_dir().context("reading the current directory")?;
        Ok(project::find_project_root(&cwd).map(|root| StateDir::for_project(&root)))
    }
}
