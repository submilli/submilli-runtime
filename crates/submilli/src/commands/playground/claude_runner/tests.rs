use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use super::*;
use crate::commands::playground::store::events::EventBody;
use crate::commands::playground::store::redact::KnownSecrets;
use crate::commands::playground::store::{Recorder, Store};

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/commands/playground/claude_runner/fixtures"
);
const SESSION: &str = "SESSION";
const TOKEN: &str = "tok_secret_value_9f2";
/// Waits for the interrupt Claude Code would get, then ends as it does on one. Gives up
/// after ten seconds, so a test whose interrupt never comes fails rather than hangs.
const ON_INTERRUPT: &str =
    "trap 'exit 0' INT; i=0; while [ $i -lt 100 ]; do sleep 0.1; i=$((i + 1)); done";

struct World {
    dir: tempfile::TempDir,
    store: Arc<Store>,
    recorder: Recorder,
    state: StateDir,
}

impl World {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(&dir.path().join("store")).unwrap());
        let recorder =
            Recorder::new(Arc::clone(&store), KnownSecrets::default()).with_events_sent_by_hand();
        let state = StateDir::for_project(&dir.path().join("project"));
        state.create().unwrap();
        Self {
            dir,
            store,
            recorder,
            state,
        }
    }

    /// What the session's log holds: each message's text, and each tools entry's calls
    /// and results.
    fn logged(&self) -> Vec<String> {
        let log = self.store.read_events(Some(SESSION)).unwrap();
        log.events
            .iter()
            .filter_map(|event| match &event.body {
                EventBody::Chat(turn) => Some(format!("{:?}: {}", turn.role, turn.text)),
                EventBody::ChatTools(tools) => Some(format!(
                    "tools: {} -> {}",
                    tools
                        .calls
                        .iter()
                        .map(|call| call.name.as_str())
                        .collect::<Vec<_>>()
                        .join(","),
                    tools
                        .results
                        .iter()
                        .map(|result| result.content.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                )),
                _ => None,
            })
            .collect()
    }
}

/// How a test message is set up.
struct Given {
    billing: Billing,
    max_calls: usize,
    continued: bool,
    cancel_after: Option<Duration>,
}

impl Default for Given {
    fn default() -> Self {
        Self {
            billing: subscription(),
            max_calls: 16,
            continued: false,
            cancel_after: None,
        }
    }
}

fn subscription() -> Billing {
    Billing {
        source: "claude.ai".to_owned(),
        base_url: None,
    }
}

/// Runs `script` as Claude Code for one message; how it ended, whether the conversation
/// was noted as started, and how long it took.
fn drive_fake(
    world: &World,
    given: Given,
    script: &str,
) -> (Result<Outcome, Attempt>, bool, Duration) {
    let log = Log {
        recorder: &world.recorder,
        session: Some(SESSION),
        agent: "chat",
        model: "sonnet",
        earlier: 0,
    };
    let cancelled = Arc::new(AtomicBool::new(false));
    let noted = AtomicBool::new(false);
    let note = || noted.store(true, Ordering::Relaxed);
    let root = world.dir.path().join("project");
    let setup = Setup {
        model: "sonnet",
        effort: None,
        max_budget_usd: 1.0,
        max_calls: given.max_calls,
        state: &world.state,
        project_root: &root,
        blueprint: "demo",
        label: "chat",
        token: TOKEN,
        session: SESSION,
        continued: given.continued,
        system_prompt: "You are a test agent.",
        billing: &given.billing,
        log: &log,
        cancelled: &cancelled,
        conversation_started: &note,
    };
    if let Some(after) = given.cancel_after {
        let cancelled = Arc::clone(&cancelled);
        std::thread::spawn(move || {
            std::thread::sleep(after);
            cancelled.store(true, Ordering::Relaxed);
        });
    }
    let prompt = world.dir.path().join("prompt");
    let script = format!(
        "read -r prompt; printf '%s\\n' \"$prompt\" > {}; {script}",
        prompt.display()
    );
    let child = Command::new("sh")
        .args(["-c", &script])
        .current_dir(FIXTURES)
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let began = Instant::now();
    let ended = drive(&setup, child, "Total the charges.");
    let took = began.elapsed();
    let written = std::fs::read_to_string(&prompt).unwrap_or_default();
    let written: Value = serde_json::from_str(&written).unwrap();
    assert_eq!(written["message"]["content"], "Total the charges.");
    (ended, noted.load(Ordering::Relaxed), took)
}

fn init(fields: &Value) -> String {
    let mut init = json!({
        "type": "system", "subtype": "init", "session_id": SESSION, "apiKeySource": "none",
        "mcp_servers": [{ "name": "playground", "status": "connected" }],
    });
    for (name, value) in fields.as_object().unwrap() {
        init[name] = value.clone();
    }
    format!("printf '%s\\n' '{init}'")
}

fn ended(ended: Result<Outcome, Attempt>) -> Result<Outcome, Failure> {
    match ended {
        Ok(outcome) => Ok(outcome),
        Err(Attempt::Ended(failure)) => Err(failure),
        Err(other) => panic!("not ended: {other:?}"),
    }
}

#[test]
fn an_answer_is_logged_as_it_streams_and_the_conversation_noted() {
    let world = World::new();
    // Claude Code keeps reading stdin after its result, until stdin closes.
    let (outcome, noted, took) = drive_fake(
        &world,
        Given::default(),
        "cat answered.jsonl; cat > /dev/null",
    );
    assert!(took < STOP_GRACE, "stdin closed after the result: {took:?}");
    let outcome = ended(outcome).unwrap();
    assert_eq!(outcome.stopped, Stopped::Answered);
    assert_eq!(outcome.tool_calls, 1);
    assert_eq!(outcome.provider, "claude-code");
    assert_eq!(
        outcome.reply.as_deref(),
        Some("The execute tool returned \"forty-two\".")
    );
    assert!(noted);
    // The developer's message is logged before Claude Code starts, by `run`.
    assert_eq!(
        world.logged(),
        [
            "tools: submilli__typescript__execute -> {\"result\": \"forty-two\", \"ran\": 1}",
            "Assistant: The execute tool returned \"forty-two\".",
        ]
    );
}

#[test]
fn a_flood_on_stderr_never_stalls_the_answer() {
    let world = World::new();
    // Far more than a pipe holds, before the answer.
    let (outcome, _, took) = drive_fake(
        &world,
        Given::default(),
        "head -c 1000000 /dev/zero | tr '\\0' x >&2; cat answered.jsonl",
    );
    assert!(took < STOP_GRACE, "{took:?}");
    assert_eq!(ended(outcome).unwrap().stopped, Stopped::Answered);
}

#[test]
fn a_resumed_conversation_is_not_noted_again() {
    let world = World::new();
    let given = Given {
        continued: true,
        ..Given::default()
    };
    let (outcome, noted, _) = drive_fake(&world, given, "cat answered.jsonl");
    assert_eq!(ended(outcome).unwrap().stopped, Stopped::Answered);
    assert!(!noted);
}

#[test]
fn each_way_a_result_ends_maps_to_a_stop_or_an_error() {
    let world = World::new();
    let stopped = |fixture: &str| {
        let (outcome, _, _) = drive_fake(&world, Given::default(), &format!("cat {fixture}"));
        ended(outcome).map(|outcome| outcome.stopped)
    };
    assert_eq!(stopped("turn-limit.jsonl"), Ok(Stopped::TurnLimit));
    assert_eq!(stopped("spending-limit.jsonl"), Ok(Stopped::SpendingLimit));
    let usage = format!(
        "{}; printf '%s\\n' '{}' '{}'",
        init(&json!({})),
        json!({ "type": "rate_limit_event", "rate_limit_info": { "status": "rejected" } }),
        json!({ "type": "result", "subtype": "success", "is_error": true, "result": "limit" }),
    );
    let (outcome, _, _) = drive_fake(&world, Given::default(), &usage);
    assert_eq!(ended(outcome).unwrap().stopped, Stopped::UsageLimit);

    // A rejected key ends in an error result marked as a success.
    let api_key = Given {
        billing: Billing {
            source: "api_key".to_owned(),
            base_url: None,
        },
        ..Given::default()
    };
    let (outcome, _, _) = drive_fake(&world, api_key, "cat key-rejected.jsonl");
    match ended(outcome) {
        Err(Failure::Failed(message)) => assert!(message.contains("Invalid API key"), "{message}"),
        other => panic!("{other:?}"),
    }
    // A crash: no result, and Claude Code's last words on stderr.
    let (outcome, _, _) = drive_fake(
        &world,
        Given::default(),
        &format!("{}; echo 'out of memory' >&2; exit 1", init(&json!({}))),
    );
    match ended(outcome) {
        Err(Failure::Failed(message)) => assert!(message.contains("out of memory"), "{message}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_conversation_that_cannot_be_resumed_or_is_already_open_is_told_apart() {
    let world = World::new();
    let (outcome, _, _) = drive_fake(
        &world,
        Given::default(),
        "echo 'No conversation found with session ID: SESSION' >&2; cat no-conversation.jsonl; exit 1",
    );
    assert!(matches!(outcome, Err(Attempt::NotFound)), "{outcome:?}");
    let (outcome, _, _) = drive_fake(
        &world,
        Given::default(),
        "echo 'Error: Session ID SESSION is already in use.' >&2; exit 1",
    );
    assert!(matches!(outcome, Err(Attempt::InUse)), "{outcome:?}");
}

#[test]
fn a_start_message_that_does_not_fit_stops_the_message_at_once() {
    let world = World::new();
    let start = |fields: Value| {
        let script = format!("{}; {ON_INTERRUPT}", init(&fields));
        let (outcome, noted, took) = drive_fake(&world, Given::default(), &script);
        assert!(
            took < STOP_GRACE,
            "stopped by interrupt, not killed: {took:?}"
        );
        assert!(!noted);
        ended(outcome).unwrap_err()
    };
    let billed = start(json!({ "apiKeySource": "ANTHROPIC_API_KEY" }));
    assert!(
        matches!(&billed, Failure::BillingChanged(why) if why.contains("ANTHROPIC_API_KEY")),
        "{billed:?}"
    );
    let not_connected = [
        json!({ "mcp_servers": [{ "name": "playground", "status": "pending" }] }),
        json!({ "mcp_servers": [{ "name": "playground", "status": "failed" }] }),
        json!({ "mcp_servers": [] }),
        json!({ "mcp_server_errors": [{ "name": "playground", "error": "spawn failed" }] }),
    ];
    for fields in not_connected {
        let failed = start(fields);
        assert!(
            matches!(&failed, Failure::Failed(why) if why.contains("did not connect")),
            "{failed:?}"
        );
    }
}

#[test]
fn a_call_past_the_limit_stops_the_message() {
    let world = World::new();
    let call = |id: &str| {
        json!({ "type": "assistant", "message": { "role": "assistant", "content": [{
            "type": "tool_use", "id": id, "name": "mcp__playground__submilli__typescript__execute",
            "input": { "code": "x" },
        }] } })
    };
    let script = format!(
        "{}; printf '%s\\n' '{}' '{}'; {ON_INTERRUPT}",
        init(&json!({})),
        call("toolu_1"),
        call("toolu_2"),
    );
    let given = Given {
        max_calls: 1,
        ..Given::default()
    };
    let (outcome, _, _) = drive_fake(&world, given, &script);
    let outcome = ended(outcome).unwrap();
    assert_eq!(outcome.stopped, Stopped::CallLimit);
    assert_eq!(outcome.tool_calls, 1);
}

#[test]
fn a_cancelled_message_is_interrupted_and_one_that_ignores_it_is_killed() {
    let world = World::new();
    let given = || Given {
        cancel_after: Some(Duration::from_millis(300)),
        ..Given::default()
    };
    let (outcome, _, took) = drive_fake(
        &world,
        given(),
        &format!("{}; {ON_INTERRUPT}", init(&json!({}))),
    );
    assert!(took < STOP_GRACE, "{took:?}");
    assert!(matches!(ended(outcome), Err(Failure::Failed(why)) if why.contains("cancelled")));

    let (outcome, _, took) = drive_fake(
        &world,
        given(),
        &format!("trap '' INT; {}; sleep 20", init(&json!({}))),
    );
    assert!(took >= STOP_GRACE && took < STOP_GRACE * 3, "{took:?}");
    assert!(matches!(ended(outcome), Err(Failure::Failed(why)) if why.contains("cancelled")));
}

#[test]
fn the_command_keeps_only_the_login_and_never_carries_the_token() {
    let world = World::new();
    let dirs = Dirs::create(&world.state).unwrap();
    let log = Log {
        recorder: &world.recorder,
        session: Some(SESSION),
        agent: "chat",
        model: "opus",
        earlier: 0,
    };
    let billing = subscription();
    let cancelled = AtomicBool::new(false);
    let root = world.dir.path().join("project");
    let setup = Setup {
        model: "opus",
        effort: Some("high"),
        max_budget_usd: 0.5,
        max_calls: 16,
        state: &world.state,
        project_root: &root,
        blueprint: "demo",
        label: "browser-chat",
        token: TOKEN,
        session: SESSION,
        continued: true,
        system_prompt: "You are a test agent.",
        billing: &billing,
        log: &log,
        cancelled: &cancelled,
        conversation_started: &|| {},
    };
    let config = dirs.root.join("mcp.json");
    let mut command = isolated(&dirs);
    command.args(arguments(&setup, &config, true));
    let args: Vec<String> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let joined = args.join(" ");
    for expected in [
        "--tools  --strict-mcp-config",
        "--permission-mode dontAsk --allowedTools mcp__playground__*",
        "--setting-sources  --settings {\"disableAllHooks\":true,\"autoMemoryEnabled\":false}",
        "--disable-slash-commands",
        "--model opus",
        "--max-budget-usd 0.5",
        "--max-turns 20",
        "--resume SESSION",
        "--effort high",
        "--system-prompt You are a test agent.",
    ] {
        assert!(joined.contains(expected), "{expected}: {joined}");
    }
    assert!(!joined.contains("--session-id"), "{joined}");
    let set: Vec<(String, Option<String>)> = command
        .get_envs()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.map(|value| value.to_string_lossy().into_owned()),
            )
        })
        .collect();
    for (name, value) in [
        (
            "CLAUDE_CONFIG_DIR",
            Some(dirs.config.to_string_lossy().into_owned()),
        ),
        ("CLAUDE_SECURESTORAGE_CONFIG_DIR", Some(String::new())),
        ("CLAUDE_CODE_DISABLE_AUTO_MEMORY", Some("1".to_owned())),
        ("CLAUDECODE", None),
        ("NODE_OPTIONS", None),
    ] {
        assert!(
            set.contains(&(name.to_owned(), value.clone())),
            "{name}: {set:?}"
        );
    }
    assert_eq!(command.get_current_dir(), Some(dirs.cwd.as_path()));

    let servers = mcp_config(&setup).unwrap().to_string();
    assert!(servers.contains("\"--join\",\"SESSION\""), "{servers}");
    assert!(
        servers.contains("\"--token\",\"browser-chat\""),
        "{servers}"
    );
    assert!(!joined.contains(TOKEN) && !servers.contains(TOKEN));
}

#[test]
fn billing_names_the_address_and_fits_only_its_own_key_source() {
    let mut billing = subscription();
    assert_eq!(billing.confirmation(), "claude.ai");
    assert!(billing.reported_as("none") && !billing.reported_as("ANTHROPIC_API_KEY"));
    billing.base_url = Some("https://proxy.example".to_owned());
    assert_eq!(
        billing.confirmation(),
        "claude.ai via https://proxy.example"
    );
    let key = Billing {
        source: "api_key".to_owned(),
        base_url: None,
    };
    assert!(key.reported_as("ANTHROPIC_API_KEY") && !key.reported_as("none"));
    assert_eq!(parse_version("2.1.310 (Claude Code)\n"), Some((2, 1, 310)));
    assert!(parse_version("2.1.299 (Claude Code)").unwrap() < MIN_VERSION);
}
