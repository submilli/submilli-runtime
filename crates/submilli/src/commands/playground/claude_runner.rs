//! Chat on Claude Code: each message runs the developer's own `claude` headless, with
//! only the playground's tools, joined to the playground session the conversation is in
//! through the bridge. Claude Code keeps the conversation itself, under the session's id,
//! and a follow-up resumes it. Only the developer's login carries over: Claude Code gets a
//! config directory of its own, and none of their settings, hooks, plugins, memory or
//! CLAUDE.md files apply.
//!
//! agent-server's runner (`agent-server/src/harness/claude-code/auth.ts` in
//! submilli-private) isolates the config the same way, but strips API keys from the
//! environment and refuses any login that is not a subscription. Anthropic's terms say a
//! product running Claude Code may not restrict its sign-in methods, so this one shows the
//! billing and has the developer confirm it instead.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read as _, Write as _};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use submilli_shared::llm::tools::{ToolCall, ToolResult};

use super::agent::{Log, MAX_TURNS, Outcome, PROTOCOL_VERSION, Stopped};
use super::state::StateDir;
use super::store::events::ChatRole;

/// What the choice is named: `claude-code:<model>`.
pub(crate) const PROVIDER: &str = "claude-code";
/// Claude Code's own model names, the only ones taken.
pub(crate) const MODELS: [&str; 3] = ["sonnet", "opus", "haiku"];
pub(crate) const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];
/// The spending limit per message a session starts with when none is set, and the most
/// one may set, in US dollars.
pub(crate) const DEFAULT_MAX_BUDGET_USD: f64 = 1.0;
pub(crate) const MAX_BUDGET_USD: f64 = 100.0;
/// The version every option used here was checked on.
pub(crate) const MIN_VERSION: (u32, u32, u32) = (2, 1, 310);
/// Its tools, as Claude Code names them.
const TOOL_PREFIX: &str = "mcp__playground__";
/// How long `--version` and `auth status` may take.
const CHECK_TIMEOUT: Duration = Duration::from_secs(10);
/// How long Claude Code has to stop cleanly before its process group is killed.
const STOP_GRACE: Duration = Duration::from_secs(5);
/// The most of Claude Code's stderr kept for an error.
const MAX_STDERR_BYTES: usize = 8 * 1024;

/// Who Claude Code bills: its sign-in method, as `auth status` names it, and the address
/// `ANTHROPIC_BASE_URL` sends its requests to, when set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Billing {
    pub(crate) source: String,
    pub(crate) base_url: Option<String>,
}

impl Billing {
    /// What the developer confirms: the source, and the address when one is set.
    pub(crate) fn confirmation(&self) -> String {
        match &self.base_url {
            Some(address) => format!("{} via {address}", self.source),
            None => self.source.clone(),
        }
    }

    /// Only a claude.ai login, sending to Anthropic, is the subscription.
    pub(crate) fn is_subscription(&self) -> bool {
        self.source == "claude.ai" && self.base_url.is_none()
    }

    /// Whether the key source Claude Code's start message reports fits this billing.
    fn reported_as(&self, api_key_source: &str) -> bool {
        match self.source.as_str() {
            "claude.ai" | "oauth_token" | "third_party" => api_key_source == "none",
            "api_key" => matches!(api_key_source, "ANTHROPIC_API_KEY" | "/login managed key"),
            "api_key_helper" => api_key_source == "apiKeyHelper",
            _ => true,
        }
    }
}

/// Whether Claude Code can answer, and who it would bill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Availability {
    Available {
        version: String,
        billing: Billing,
    },
    Missing,
    TooOld {
        version: String,
    },
    SignedOut {
        version: String,
    },
    /// `--version` or `auth status` failed, or took too long.
    Failed(String),
}

/// How often the status asks `claude` again, at most.
const STATUS_MAX_AGE: Duration = Duration::from_secs(60);

/// Claude Code's availability as the status shows it, for the page's agent choice.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Shown {
    /// `available`, `missing`, `too-old`, `signed-out`, or `failed`.
    pub(crate) state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    /// Who it bills, as `auth status` names it, and the address it sends to when
    /// `ANTHROPIC_BASE_URL` sets one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) base_url: Option<String>,
    /// Whether it bills the developer's Claude subscription, needing no confirmation.
    pub(crate) subscription: bool,
    /// What a message confirms to bill anything else.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) confirmation: Option<String>,
    /// Why it is unavailable, and what to do.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reason: Option<String>,
}

impl From<&Availability> for Shown {
    fn from(availability: &Availability) -> Self {
        let mut shown = Self {
            state: match availability {
                Availability::Available { .. } => "available",
                Availability::Missing => "missing",
                Availability::TooOld { .. } => "too-old",
                Availability::SignedOut { .. } => "signed-out",
                Availability::Failed(_) => "failed",
            }
            .to_owned(),
            version: None,
            source: None,
            base_url: None,
            subscription: false,
            confirmation: None,
            reason: None,
        };
        match availability {
            Availability::Available { version, billing } => {
                shown.version = Some(version.clone());
                shown.source = Some(billing.source.clone());
                shown.base_url.clone_from(&billing.base_url);
                shown.subscription = billing.is_subscription();
                shown.confirmation = Some(billing.confirmation());
            }
            Availability::TooOld { version } | Availability::SignedOut { version } => {
                shown.version = Some(version.clone());
                shown.reason = Some(unavailable_reason(availability));
            }
            Availability::Missing | Availability::Failed(_) => {
                shown.reason = Some(unavailable_reason(availability));
            }
        }
        shown
    }
}

/// The status's last look at Claude Code. A look older than a minute starts one fresh
/// one in the background and is shown meanwhile, so the page's polling never waits on
/// `claude`, and nothing runs while no one asks.
#[derive(Default)]
pub(crate) struct StatusCache {
    looked: Mutex<Option<(Instant, Shown)>>,
    looking: AtomicBool,
}

impl StatusCache {
    /// The last look, or `None` before the first is done.
    pub(crate) fn shown(self: &Arc<Self>, project_root: &Path) -> Option<Shown> {
        let looked = self
            .looked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let stale = looked
            .as_ref()
            .is_none_or(|(at, _)| at.elapsed() >= STATUS_MAX_AGE);
        if stale && !self.looking.swap(true, Ordering::Relaxed) {
            let cache = Arc::clone(self);
            let state = StateDir::for_project(project_root);
            std::thread::spawn(move || {
                let shown = Shown::from(&availability(&state));
                *cache.looked.lock().unwrap_or_else(PoisonError::into_inner) =
                    Some((Instant::now(), shown));
                cache.looking.store(false, Ordering::Relaxed);
            });
        }
        looked.map(|(_, shown)| shown)
    }
}

/// What to tell the developer when Claude Code is not [`Availability::Available`].
pub(crate) fn unavailable_reason(why: &Availability) -> String {
    let (major, minor, patch) = MIN_VERSION;
    match why {
        Availability::Available { .. } => "Claude Code is available".to_owned(),
        Availability::Missing => "Claude Code isn't installed: `claude` is not on the \
            playground's PATH. Install it (https://code.claude.com), then restart the playground"
            .to_owned(),
        Availability::TooOld { version } => format!(
            "Claude Code {version} is older than {major}.{minor}.{patch}; update it with \
             `claude update`"
        ),
        Availability::SignedOut { .. } => {
            "Claude Code is signed out; run `claude` once to sign in".to_owned()
        }
        Availability::Failed(why) => format!("Claude Code could not be checked: {why}"),
    }
}

/// Asks the developer's `claude` its version and login, in the environment and directory
/// a message runs in, so what it says is what a message would bill. No model is asked.
pub(crate) fn availability(state: &StateDir) -> Availability {
    let dirs = match Dirs::create(state) {
        Ok(dirs) => dirs,
        Err(error) => return Availability::Failed(format!("{error:#}")),
    };
    let version = match checked(&dirs, &["--version"]) {
        Ok(output) => output,
        Err(Checked::Missing) => return Availability::Missing,
        Err(Checked::Failed(why)) => return Availability::Failed(why),
    };
    let Some(version) = parse_version(&version) else {
        return Availability::Failed(format!("`claude --version` printed `{}`", version.trim()));
    };
    let shown = format!("{}.{}.{}", version.0, version.1, version.2);
    if version < MIN_VERSION {
        return Availability::TooOld { version: shown };
    }
    let status = match checked(&dirs, &["auth", "status", "--json"]) {
        Ok(output) => output,
        Err(Checked::Missing) => return Availability::Missing,
        Err(Checked::Failed(why)) => return Availability::Failed(why),
    };
    let Ok(status) = serde_json::from_str::<Value>(&status) else {
        return Availability::Failed("`claude auth status` did not print JSON".to_owned());
    };
    match status["authMethod"].as_str() {
        Some(source) if status["loggedIn"] == true && source != "none" => Availability::Available {
            version: shown,
            billing: Billing {
                source: source.to_owned(),
                base_url: std::env::var("ANTHROPIC_BASE_URL")
                    .ok()
                    .filter(|address| !address.trim().is_empty()),
            },
        },
        _ => Availability::SignedOut { version: shown },
    }
}

/// `claude --version`'s `2.1.310 (Claude Code)`.
fn parse_version(printed: &str) -> Option<(u32, u32, u32)> {
    let mut parts = printed.split_whitespace().next()?.split('.');
    let mut part = || parts.next()?.parse().ok();
    Some((part()?, part()?, part()?))
}

enum Checked {
    Missing,
    Failed(String),
}

/// `claude <args>`'s stdout, or why there is none.
fn checked(dirs: &Dirs, args: &[&str]) -> Result<String, Checked> {
    let mut command = isolated(dirs);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let shown = format!("`claude {}`", args.join(" "));
    let mut child = command.spawn().map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => Checked::Missing,
        _ => Checked::Failed(format!("{shown} did not start: {error}")),
    })?;
    let mut stdout = child.stdout.take().expect("piped");
    let read = std::thread::spawn(move || {
        let mut output = String::new();
        let _ = stdout.read_to_string(&mut output);
        output
    });
    let deadline = Instant::now() + CHECK_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                kill_group(child.id());
                let _ = child.wait();
                return Err(Checked::Failed(format!(
                    "{shown} did not answer within {} seconds",
                    CHECK_TIMEOUT.as_secs()
                )));
            }
            Err(error) => return Err(Checked::Failed(format!("{shown} failed: {error}"))),
        }
    };
    let output = read.join().unwrap_or_default();
    if !status.success() {
        return Err(Checked::Failed(format!("{shown} exited with {status}")));
    }
    Ok(output)
}

/// Claude Code's own directories under the playground's state: its config, where its
/// conversations are saved, and an empty directory it runs in.
struct Dirs {
    root: PathBuf,
    config: PathBuf,
    cwd: PathBuf,
}

impl Dirs {
    fn create(state: &StateDir) -> anyhow::Result<Self> {
        let [root, config, cwd] = state.claude_code_dirs()?;
        Ok(Self { root, config, cwd })
    }
}

/// `claude`, keeping the developer's login and nothing else of their setup.
fn isolated(dirs: &Dirs) -> Command {
    let mut command = Command::new("claude");
    command.current_dir(&dirs.cwd).process_group(0);
    // Options that run code before Claude Code starts, and the marks of a Claude Code
    // session this one would otherwise take itself to be inside.
    for name in [
        "NODE_OPTIONS",
        "BASH_ENV",
        "ENV",
        "CLAUDECODE",
        "CLAUDE_CODE_SESSION_ID",
    ] {
        command.env_remove(name);
    }
    // An empty secure-storage directory keeps the login in the system keychain, where
    // the developer's own Claude Code keeps it, while the config is the playground's.
    command
        .env("CLAUDE_CONFIG_DIR", &dirs.config)
        .env("CLAUDE_SECURESTORAGE_CONFIG_DIR", "")
        .env("CLAUDE_CODE_PROJECT_DIR_NAME", "playground")
        .env("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1")
        .env("ENABLE_CLAUDEAI_MCP_SERVERS", "false")
        .env("DISABLE_AUTOUPDATER", "1")
        .env("NO_COLOR", "1");
    command
}

/// Where one message runs, and what it may spend.
pub(crate) struct Setup<'a> {
    /// One of [`MODELS`].
    pub(crate) model: &'a str,
    /// One of [`EFFORTS`].
    pub(crate) effort: Option<&'a str>,
    pub(crate) max_budget_usd: f64,
    /// The most tool calls the message may make.
    pub(crate) max_calls: usize,
    pub(crate) state: &'a StateDir,
    pub(crate) project_root: &'a Path,
    pub(crate) blueprint: &'a str,
    /// The token's name, which the bridge joins with and the runs are labeled by.
    pub(crate) label: &'a str,
    /// Its value, kept out of every error.
    pub(crate) token: &'a str,
    /// The playground session, whose id is the Claude Code conversation's too.
    pub(crate) session: &'a str,
    /// Whether Claude Code has saved the conversation, so this message resumes it.
    pub(crate) continued: bool,
    pub(crate) system_prompt: &'a str,
    /// Who `auth status` said is billed, as the developer confirmed it.
    pub(crate) billing: &'a Billing,
    pub(crate) log: &'a Log<'a>,
    /// Set when whoever asked is gone: Claude Code is stopped.
    pub(crate) cancelled: &'a AtomicBool,
    /// Told once Claude Code has started the conversation under the session's id.
    pub(crate) conversation_started: &'a dyn Fn(),
}

/// Why a message got no answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Failure {
    /// The conversation the session continues is no longer saved.
    ConversationMissing,
    /// Claude Code reported billing other than what was confirmed.
    BillingChanged(String),
    Failed(String),
}

/// Runs `task` through Claude Code to its answer or a limit. Blocking.
pub(crate) fn run(setup: &Setup<'_>, task: &str) -> Result<Outcome, Failure> {
    let clean = |failure: Failure| match failure {
        Failure::Failed(message) if !setup.token.is_empty() => {
            Failure::Failed(message.replace(setup.token, "[redacted]"))
        }
        other => other,
    };
    setup.log.say(0, ChatRole::User, task, false);
    let dirs = Dirs::create(setup.state).map_err(|error| Failure::Failed(format!("{error:#}")))?;
    let config = dirs.root.join(format!("mcp-{}.json", setup.session));
    let servers = mcp_config(setup).map_err(clean)?;
    super::state::write_private(&config, servers.to_string().as_bytes())
        .map_err(|error| Failure::Failed(format!("{error:#}")))?;
    // A first message whose conversation Claude Code already saved, as when the
    // playground stopped before it noted that, resumes it instead.
    let mut resume = setup.continued;
    let ended = loop {
        let mut command = isolated(&dirs);
        command.args(arguments(setup, &config, resume));
        match attempt(setup, command, task) {
            Err(Attempt::InUse) if !resume => resume = true,
            Err(Attempt::InUse) => {
                break Err(Failure::Failed(
                    "Claude Code is already answering in this session".to_owned(),
                ));
            }
            Err(Attempt::NotFound) if setup.continued => break Err(Failure::ConversationMissing),
            Err(Attempt::NotFound) => {
                break Err(Failure::Failed(
                    "Claude Code has no saved conversation for this session".to_owned(),
                ));
            }
            Err(Attempt::Ended(failure)) => break Err(failure),
            Ok(outcome) => break Ok(outcome),
        }
    };
    let _ = std::fs::remove_file(&config);
    ended.map_err(clean)
}

/// The bridge Claude Code starts as its one MCP server, joined to the session.
fn mcp_config(setup: &Setup<'_>) -> Result<Value, Failure> {
    let program = std::env::current_exe()
        .map_err(|error| Failure::Failed(format!("finding this program failed: {error}")))?;
    let root = setup.project_root.to_string_lossy();
    let max_calls = setup.max_calls.to_string();
    Ok(json!({
        "mcpServers": {
            "playground": {
                "type": "stdio",
                "command": program,
                "args": [
                    "playground", "mcp",
                    "--root", root,
                    "--token", setup.label,
                    "--blueprint", setup.blueprint,
                    "--protocol", PROTOCOL_VERSION,
                    "--join", setup.session,
                    "--max-calls", max_calls,
                ],
                // Its tools are always loaded, never deferred behind a search.
                "alwaysLoad": true,
            }
        }
    }))
}

/// `claude`'s arguments for one message.
fn arguments(setup: &Setup<'_>, config: &Path, resume: bool) -> Vec<OsString> {
    let mut args: Vec<OsString> = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        // None of Claude Code's own tools: only the playground's, without asking.
        "--tools",
        "",
        "--strict-mcp-config",
        "--permission-mode",
        "dontAsk",
        "--allowedTools",
        "mcp__playground__*",
        "--permission-prompts",
        "none",
        "--setting-sources",
        "",
        "--settings",
        r#"{"disableAllHooks":true,"autoMemoryEnabled":false}"#,
        "--disable-slash-commands",
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    args.extend([
        OsString::from("--mcp-config"),
        config.as_os_str().to_owned(),
    ]);
    let pairs = [
        ("--system-prompt", setup.system_prompt.to_owned()),
        ("--model", setup.model.to_owned()),
        ("--max-budget-usd", setup.max_budget_usd.to_string()),
        ("--max-turns", MAX_TURNS.to_string()),
        (
            if resume { "--resume" } else { "--session-id" },
            setup.session.to_owned(),
        ),
    ];
    for (flag, value) in pairs {
        args.extend([OsString::from(flag), OsString::from(value)]);
    }
    if let Some(effort) = setup.effort {
        args.extend([OsString::from("--effort"), OsString::from(effort)]);
    }
    args
}

/// How one start of Claude Code ended.
#[derive(Debug)]
enum Attempt {
    /// Another start holds the conversation's id.
    InUse,
    /// There is no saved conversation to resume.
    NotFound,
    Ended(Failure),
}

fn attempt(setup: &Setup<'_>, mut command: Command, task: &str) -> Result<Outcome, Attempt> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = command.spawn().map_err(|error| {
        Attempt::Ended(Failure::Failed(match error.kind() {
            std::io::ErrorKind::NotFound => {
                "Claude Code is not installed: `claude` is not on the playground's PATH".to_owned()
            }
            _ => format!("starting Claude Code failed: {error}"),
        }))
    })?;
    drive(setup, child, task)
}

/// Writes `task` to a started `claude`, follows its stream to the end, and reaps it.
fn drive(setup: &Setup<'_>, mut child: Child, task: &str) -> Result<Outcome, Attempt> {
    let stopper = Stopper::new(child.id());
    let stderr = child.stderr.take().map(|stderr| {
        std::thread::spawn(move || {
            let mut kept = Vec::new();
            let _ = stderr.take(MAX_STDERR_BYTES as u64).read_to_end(&mut kept);
            String::from_utf8_lossy(&kept).into_owned()
        })
    });
    let watching = AtomicBool::new(true);
    let stream = std::thread::scope(|scope| {
        scope.spawn(|| {
            while watching.load(Ordering::Relaxed) {
                if setup.cancelled.load(Ordering::Relaxed) {
                    stopper.stop(Stop::Cancelled);
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        let stream = follow(setup, &mut child, &stopper, task);
        let _ = child.wait();
        stopper.exited();
        // Its MCP server too, which a Claude Code that died mid-call leaves running with
        // the stderr this reads.
        kill_group(child.id());
        watching.store(false, Ordering::Relaxed);
        stream
    });
    let stderr = stderr.and_then(|read| read.join().ok()).unwrap_or_default();
    stream.ended(stopper.reason(), &stderr)
}

/// Writes `task`, then reads the stream until Claude Code closes it.
fn follow<'s, 'a>(
    setup: &'s Setup<'a>,
    child: &mut Child,
    stopper: &Stopper,
    task: &str,
) -> Stream<'s, 'a> {
    let mut stdin = child.stdin.take();
    let prompt = json!({ "type": "user", "message": { "role": "user", "content": task } });
    if let Some(input) = stdin.as_mut() {
        let _ = writeln!(input, "{prompt}");
        let _ = input.flush();
    }
    let mut stream = Stream::new(setup);
    let Some(stdout) = child.stdout.take() else {
        return stream;
    };
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(stop) = stream.read(&message) {
            stopper.stop(stop);
        }
        if stream.result.is_some() && stdin.is_some() {
            // With input on stdin, Claude Code waits for more until stdin closes.
            drop(stdin.take());
            stopper.end_after_grace();
        }
    }
    stream
}

/// Why the playground stopped a message.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Stop {
    Cancelled,
    CallLimit,
    /// Claude Code's start message did not fit what was checked.
    Start(Failure),
}

/// Stops Claude Code once: an interrupt, so it ends its turn and saves the conversation,
/// then its whole process group, the bridge included, if it has not exited in time.
struct Stopper {
    pid: u32,
    reason: Mutex<Option<Stop>>,
    exited: Arc<AtomicBool>,
    timed: AtomicBool,
}

impl Stopper {
    fn new(pid: u32) -> Self {
        Self {
            pid,
            reason: Mutex::new(None),
            exited: Arc::new(AtomicBool::new(false)),
            timed: AtomicBool::new(false),
        }
    }

    fn stop(&self, why: Stop) {
        {
            let mut reason = self.reason.lock().unwrap_or_else(PoisonError::into_inner);
            if reason.is_some() {
                return;
            }
            *reason = Some(why);
        }
        // Claude Code alone: the bridge exits at once on a signal, which would fail the
        // calls Claude Code is still finishing.
        if let Ok(pid) = i32::try_from(self.pid) {
            // SAFETY: kill takes only scalar arguments.
            unsafe {
                libc::kill(pid, libc::SIGINT);
            }
        }
        self.end_after_grace();
    }

    /// Kills the group if it has not exited within [`STOP_GRACE`].
    fn end_after_grace(&self) {
        if self.timed.swap(true, Ordering::Relaxed) {
            return;
        }
        let exited = Arc::clone(&self.exited);
        let pid = self.pid;
        std::thread::spawn(move || {
            let deadline = Instant::now() + STOP_GRACE;
            while Instant::now() < deadline {
                if exited.load(Ordering::Relaxed) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            if !exited.load(Ordering::Relaxed) {
                kill_group(pid);
            }
        });
    }

    fn exited(&self) {
        self.exited.store(true, Ordering::Relaxed);
    }

    fn reason(&self) -> Option<Stop> {
        self.reason
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

fn kill_group(pid: u32) {
    if let Ok(pid) = i32::try_from(pid) {
        // SAFETY: the child leads its own process group; kill takes only scalar
        // arguments. A group already gone is not an error worth reporting.
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
}

/// What Claude Code's stream said so far.
struct Stream<'s, 'a> {
    setup: &'s Setup<'a>,
    started: bool,
    said: usize,
    /// Tool calls run, counted across the message.
    calls: usize,
    turn: usize,
    /// The calls of the latest answer, until all their results are in.
    pending: Vec<ToolCall>,
    results: Vec<ToolResult>,
    usage_limited: bool,
    result: Option<Value>,
}

impl<'s, 'a> Stream<'s, 'a> {
    fn new(setup: &'s Setup<'a>) -> Self {
        Self {
            setup,
            started: false,
            said: 0,
            calls: 0,
            turn: 0,
            pending: Vec::new(),
            results: Vec::new(),
            usage_limited: false,
            result: None,
        }
    }

    /// Takes one line of the stream; why the message must stop, if it must.
    fn read(&mut self, message: &Value) -> Option<Stop> {
        match message["type"].as_str() {
            Some("system") if message["subtype"] == "init" => self.start(message),
            // An error is Claude Code's own words standing in for an answer; the
            // result says it too.
            Some("assistant") if message.get("error").is_some() => {
                self.usage_limited |= message["error"] == "rate_limit";
                None
            }
            Some("assistant") => self.answer(&message["message"]["content"]),
            Some("user") => {
                self.tool_results(&message["message"]["content"]);
                None
            }
            Some("rate_limit_event") => {
                if message["rate_limit_info"]["status"] == "rejected" {
                    self.usage_limited = true;
                }
                None
            }
            Some("result") => {
                self.flush_tools();
                self.result = Some(message.clone());
                None
            }
            _ => None,
        }
    }

    /// Checks the start message against what was checked before Claude Code started.
    fn start(&mut self, init: &Value) -> Option<Stop> {
        if self.started {
            return None;
        }
        self.started = true;
        let reported = init["apiKeySource"].as_str().unwrap_or("none");
        if !self.setup.billing.reported_as(reported) {
            return Some(Stop::Start(Failure::BillingChanged(format!(
                "Claude Code started billing {reported}, not {} as confirmed",
                self.setup.billing.confirmation()
            ))));
        }
        let connected = init["mcp_servers"].as_array().is_some_and(|servers| {
            servers
                .iter()
                .any(|server| server["name"] == "playground" && server["status"] == "connected")
        });
        let erred = init["mcp_server_errors"]
            .as_array()
            .is_some_and(|errors| errors.iter().any(|error| error["name"] == "playground"));
        if !connected || erred {
            return Some(Stop::Start(Failure::Failed(
                "the playground's tools did not connect to Claude Code".to_owned(),
            )));
        }
        if init["session_id"] != self.setup.session {
            return Some(Stop::Start(Failure::Failed(format!(
                "Claude Code started conversation {}, not the session's own",
                init["session_id"]
            ))));
        }
        if !self.setup.continued {
            (self.setup.conversation_started)();
        }
        None
    }

    fn answer(&mut self, content: &Value) -> Option<Stop> {
        let mut stop = None;
        for block in content.as_array().map(Vec::as_slice).unwrap_or_default() {
            match block["type"].as_str() {
                Some("text") => {
                    let text = block["text"].as_str().unwrap_or_default();
                    if !text.trim().is_empty() {
                        self.said += 1;
                        self.setup
                            .log
                            .say(self.said, ChatRole::Assistant, text, false);
                    }
                }
                Some("tool_use") => {
                    let name = block["name"].as_str().unwrap_or_default();
                    let Some(name) = name.strip_prefix(TOOL_PREFIX) else {
                        continue;
                    };
                    self.calls += 1;
                    if self.calls > self.setup.max_calls {
                        stop = Some(Stop::CallLimit);
                    }
                    let arguments = match &block["input"] {
                        Value::Object(arguments) => Ok(arguments.clone()),
                        other => Err(other.to_string()),
                    };
                    self.pending.push(ToolCall {
                        id: block["id"].as_str().unwrap_or_default().to_owned(),
                        name: name.to_owned(),
                        arguments,
                        signature: None,
                    });
                }
                _ => {}
            }
        }
        stop
    }

    fn tool_results(&mut self, content: &Value) {
        for block in content.as_array().map(Vec::as_slice).unwrap_or_default() {
            if block["type"] != "tool_result" {
                continue;
            }
            let call_id = block["tool_use_id"].as_str().unwrap_or_default();
            let Some(call) = self.pending.iter().find(|call| call.id == call_id) else {
                continue;
            };
            let content = match &block["content"] {
                Value::String(text) => text.clone(),
                Value::Array(parts) => parts
                    .iter()
                    .filter_map(|part| part["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
                _ => String::new(),
            };
            self.results.push(ToolResult {
                call_id: call_id.to_owned(),
                name: call.name.clone(),
                content,
                is_error: block["is_error"] == true,
            });
        }
        if !self.pending.is_empty() && self.results.len() >= self.pending.len() {
            self.flush_tools();
        }
    }

    fn flush_tools(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        self.turn += 1;
        self.setup
            .log
            .tools(self.turn, &self.pending, &self.results);
        self.pending.clear();
        self.results.clear();
    }

    /// How the message ended, from the result line, why the playground stopped it, and
    /// Claude Code's stderr.
    fn ended(mut self, stopped: Option<Stop>, stderr: &str) -> Result<Outcome, Attempt> {
        self.flush_tools();
        let outcome = |stopped: Stopped, reply: Option<String>| Outcome {
            session: Some(self.setup.session.to_owned()),
            provider: PROVIDER,
            model: self.setup.model.to_owned(),
            reply,
            tool_calls: self.calls.min(self.setup.max_calls),
            stopped,
            stop_reason: None,
        };
        match stopped {
            Some(Stop::Cancelled) => {
                return Err(Attempt::Ended(Failure::Failed(
                    "the chat was cancelled: whoever started it is gone".to_owned(),
                )));
            }
            Some(Stop::CallLimit) => return Ok(outcome(Stopped::CallLimit, None)),
            Some(Stop::Start(failure)) => return Err(Attempt::Ended(failure)),
            None => {}
        }
        let Some(result) = self.result.take() else {
            if !self.started && stderr.contains("is already in use") {
                return Err(Attempt::InUse);
            }
            if !self.started && stderr.contains("No conversation found") {
                return Err(Attempt::NotFound);
            }
            return Err(Attempt::Ended(Failure::Failed(failed(
                "Claude Code stopped without an answer",
                stderr,
            ))));
        };
        let reply = result["result"]
            .as_str()
            .filter(|reply| !reply.trim().is_empty())
            .map(str::to_owned);
        let errors = errors(&result);
        match result["subtype"].as_str() {
            Some("success") if result["is_error"] != true => Ok(outcome(Stopped::Answered, reply)),
            Some("error_max_turns") => Ok(outcome(Stopped::TurnLimit, None)),
            Some("error_max_budget_usd") => Ok(outcome(Stopped::SpendingLimit, None)),
            _ if self.usage_limited => Ok(outcome(Stopped::UsageLimit, None)),
            _ if !self.started && errors.contains("No conversation found") => {
                Err(Attempt::NotFound)
            }
            _ => {
                let said = reply.unwrap_or(errors);
                Err(Attempt::Ended(Failure::Failed(failed(
                    &format!("Claude Code failed: {said}"),
                    stderr,
                ))))
            }
        }
    }
}

/// A result's `errors`, joined.
fn errors(result: &Value) -> String {
    result["errors"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join("; ")
}

/// `message`, with the last line Claude Code wrote to stderr.
fn failed(message: &str, stderr: &str) -> String {
    match stderr.lines().rev().find(|line| !line.trim().is_empty()) {
        Some(last) => format!("{message}: {}", last.trim()),
        None => message.to_owned(),
    }
}

#[cfg(test)]
mod tests;
