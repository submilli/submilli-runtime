//! Chat capture against a stand-in run recorded with Claude Code 2.1.296: its hooks, the
//! stand-in's transcript, and what Claude Code sent the bridge. Paths, session and agent
//! ids are placeholders, and Claude Code's own entries are cut down to their type. A
//! Claude Code change that drops a field this relies on fails here.

use std::collections::HashSet;
use std::sync::Arc;

use base64::Engine as _;
use serde_json::json;
use submilli_server::record::{EVENT_SCHEMA, EventKind, RunRecorderFactory, SessionEvent};

use super::*;
use crate::commands::playground::store::KnownSecrets;
use crate::commands::playground::store::events::{EventLog, StoredEvent};

const HOOKS: &str = include_str!("fixtures/hooks.jsonl");
const TRANSCRIPT: &str = include_str!("fixtures/transcript.jsonl");
const BRIDGE_REQUESTS: &str = include_str!("fixtures/bridge-requests.jsonl");

/// The home directory the fixtures name.
const HOME: &str = "/home/dev";
const AGENT: &str = "a000000000000001";
const SESSION: &str = "00000000-0000-4000-8000-000000000001";
const SECRET: &str = "sk_test_51HxQ2eLkdIwHu7ix";

/// The first line of the stand-in's task, as the main session sent it.
const TASK_START: &str = "Please do the following through the Submilli playground";

struct World {
    dir: tempfile::TempDir,
    store: Arc<Store>,
    recorder: Recorder,
    secrets: KnownSecrets,
    intake: ChatIntake,
    seq: u64,
}

impl World {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(&dir.path().join("store")).unwrap());
        let secrets = KnownSecrets::default();
        let recorder = Recorder::new(Arc::clone(&store), secrets.clone());
        Self {
            dir,
            store,
            recorder,
            secrets,
            intake: ChatIntake::default(),
            seq: 0,
        }
    }

    /// The recorded hooks, naming this world's home, with the stand-in's transcript
    /// where Claude Code writes it.
    fn hooks(&self) -> Vec<Value> {
        let home = self.dir.path().join("home");
        let live = home.join(format!(
            ".claude/projects/-home-dev-project/{SESSION}/subagents/agent-{AGENT}.jsonl"
        ));
        std::fs::create_dir_all(live.parent().unwrap()).unwrap();
        std::fs::write(&live, TRANSCRIPT).unwrap();
        HOOKS
            .lines()
            .map(|line| {
                let line = line.replace(HOME, &home.display().to_string());
                serde_json::from_str(&line).unwrap()
            })
            .collect()
    }

    /// The server recording MCP tool call `call` in `session`.
    fn call(&mut self, session: &str, call: &str) {
        self.seq += 1;
        self.recorder.event(SessionEvent {
            schema: EVENT_SCHEMA,
            event_id: format!("srv-{}", self.seq),
            seq: self.seq,
            at_micros: crate::commands::playground::now_micros(),
            session_id: Some(session.to_owned()),
            run_id: None,
            tool_call_id: Some(call.to_owned()),
            kind: EventKind::ToolCall {
                tool: "submilli__typescript__execute".into(),
                ok: true,
                result_bytes: 1,
                wall_ms: 1,
            },
        });
    }

    fn post(&self, post: ChatPost) -> usize {
        self.post_at(post, Instant::now())
    }

    fn post_at(&self, post: ChatPost, now: Instant) -> usize {
        self.intake.accept(post, &self.recorder, now)
    }

    fn log(&self, session: &str) -> EventLog {
        self.store.read_events(Some(session)).unwrap()
    }

    /// Plays the recorded run: each hook posts as `chat-hook` would, and each playground
    /// tool call reaches the server between its `PreToolUse` and its `PostToolUse`.
    fn replay(&mut self, session: &str) {
        for hook in self.hooks() {
            let event = hook["hook_event_name"].as_str().unwrap();
            if let Some(post) = hook_post(&hook) {
                self.post(post);
            }
            let tool = hook["tool_name"].as_str().unwrap_or_default();
            if event == "PreToolUse" && tool.starts_with("mcp__") {
                self.call(session, hook["tool_use_id"].as_str().unwrap());
            }
        }
    }
}

/// The log in causal order, as short words: `chat <role>`, `call <id>`, or `replaced`.
fn order(log: &EventLog) -> Vec<String> {
    let current: HashSet<&str> = log
        .chat()
        .into_iter()
        .map(|(event, _)| event.event_id.as_str())
        .collect();
    log.causal()
        .into_iter()
        .filter_map(|event: &StoredEvent| match &event.body {
            EventBody::Chat(turn) if current.contains(event.event_id.as_str()) => {
                Some(format!("chat {:?}", turn.role).to_lowercase())
            }
            EventBody::Chat(_)
            | EventBody::ChatReplaced(_)
            | EventBody::ChatTools(_)
            | EventBody::Gap(_) => None,
            EventBody::Event(event) => Some(format!(
                "call {}",
                event.tool_call_id.as_deref().unwrap_or("-")
            )),
        })
        .collect()
}

fn texts(log: &EventLog) -> Vec<String> {
    log.chat()
        .into_iter()
        .map(|(_, turn)| turn.text.clone())
        .collect()
}

fn says(turn: &str, text: &str) -> Message {
    Message {
        turn: turn.into(),
        role: ChatRole::Assistant,
        text: text.into(),
        truncated: false,
        before_call: None,
        model: None,
    }
}

fn live(agent: &str, call: &str, messages: Vec<Message>) -> ChatPost {
    ChatPost {
        agent: agent.into(),
        calls: vec![call.into()],
        messages,
        complete: false,
    }
}

// ---- what Claude Code gives ----------------------------------------------------------------

#[test]
fn the_transcript_gives_the_task_the_report_and_every_call_claude_code_sent() {
    let read = transcript(TRANSCRIPT);
    let roles: Vec<ChatRole> = read.messages.iter().map(|message| message.role).collect();
    assert_eq!(roles, [ChatRole::User, ChatRole::Assistant]);
    let (task, report) = (&read.messages[0], &read.messages[1]);
    assert!(task.text.starts_with(TASK_START), "{}", task.text);
    assert!(report.text.contains("$61.50"), "{}", report.text);
    assert!(
        !read
            .messages
            .iter()
            .any(|message| message.text.contains("system-reminder")),
        "the assistant's reminders are not chat"
    );

    // Every call the bridge relayed carried its id in `_meta`, in the order the
    // transcript has them; the task leads to the first and the report follows the last.
    let sent: Vec<String> = BRIDGE_REQUESTS
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|request| request["method"] == "tools/call")
        .map(|request| {
            request["params"]["_meta"]["claudecode/toolUseId"]
                .as_str()
                .expect("Claude Code names each tool call")
                .to_owned()
        })
        .collect();
    assert_eq!(read.calls, sent);
    assert_eq!(task.before_call.as_ref(), sent.first());
    assert_eq!(report.before_call, None);
}

#[test]
fn only_the_stand_ins_own_hooks_post() {
    let world = World::new();
    let hooks = world.hooks();
    let read = transcript(TRANSCRIPT);
    let mut posted = 0;
    for hook in &hooks {
        let post = hook_post(hook);
        let event = hook["hook_event_name"].as_str().unwrap();
        if hook["agent_type"] != STAND_IN_AGENT || event == "SubagentStart" {
            assert_eq!(post, None, "{event} posts nothing");
            continue;
        }
        let post = post.unwrap_or_else(|| panic!("{event} of the stand-in posts"));
        posted += 1;
        assert_eq!(post.agent, AGENT);
        if event == "SubagentStop" {
            assert!(post.complete);
            assert_eq!(post.calls, read.calls);
            assert_eq!(post.messages, read.messages);
        } else {
            assert!(!post.complete);
            assert_eq!(post.calls, [hook["tool_use_id"].as_str().unwrap()]);
            assert!(!post.messages.is_empty());
        }
        let mut other = hook.clone();
        other["agent_type"] = json!("general-purpose");
        assert_eq!(
            hook_post(&other),
            None,
            "another agent's {event} posts nothing"
        );
    }
    assert_eq!(posted, 15, "every tool call of the stand-in, and its stop");
}

#[test]
fn an_assistant_message_names_the_model_its_transcript_entry_does() {
    let entries = [
        json!({ "type": "user", "uuid": "u1", "message": { "content": "Total the charges." } }),
        json!({ "type": "assistant", "uuid": "a1", "message": {
            "model": "claude-haiku-5-5",
            "content": [{ "type": "text", "text": "The total is $61.50." }],
        } }),
        json!({ "type": "assistant", "uuid": "a2", "message": {
            "model": "<synthetic>",
            "content": [{ "type": "text", "text": "No response requested." }],
        } }),
        json!({ "type": "assistant", "uuid": "a3", "message": {
            "model": 7,
            "content": [{ "type": "text", "text": "Seven." }],
        } }),
        json!({ "type": "assistant", "uuid": "a4", "message": {
            "model": "m".repeat(5000),
            "content": [{ "type": "tool_use", "id": "toolu_h", "name": "SubagentHandback",
                          "input": { "message": "Done: $61.50." } }],
        } }),
    ];
    let text: Vec<String> = entries.iter().map(Value::to_string).collect();
    let read = transcript(&text.join("\n"));
    let models: Vec<Option<usize>> = read
        .messages
        .iter()
        .map(|message| message.model.as_ref().map(String::len))
        .collect();
    // The report names the model too, cut to what is kept.
    assert_eq!(
        models,
        [None, Some("claude-haiku-5-5".len()), None, None, Some(200)]
    );
}

#[test]
fn a_posted_model_is_kept_only_on_an_answer_and_cut_whoever_posted_it() {
    let mut world = World::new();
    world.call(SESSION, "toolu_m");
    let named = |turn: &str, role: ChatRole, model: &str| Message {
        role,
        model: Some(model.into()),
        ..says(turn, turn)
    };
    world.post(live(
        "agent-m",
        "toolu_m",
        vec![
            named("task", ChatRole::User, "claude-haiku-5-5"),
            named("long", ChatRole::Assistant, &"m".repeat(5000)),
            named("synthetic", ChatRole::Assistant, "<synthetic>"),
            named("answer", ChatRole::Assistant, "claude-haiku-5-5"),
        ],
    ));
    let log = world.log(SESSION);
    let models: Vec<Option<usize>> = log
        .chat()
        .into_iter()
        .map(|(_, turn)| turn.model.as_ref().map(String::len))
        .collect();
    assert_eq!(
        models,
        [None, Some(200), None, Some("claude-haiku-5-5".len())]
    );
}

#[test]
fn a_hook_with_no_transcript_posts_no_messages_and_the_stop_still_does() {
    let world = World::new();
    let hooks = world.hooks();
    let home = world.dir.path().join("home");
    let live = home.join(format!(
        ".claude/projects/-home-dev-project/{SESSION}/subagents/agent-{AGENT}.jsonl"
    ));
    let elsewhere = world.dir.path().join("moved.jsonl");
    std::fs::rename(&live, &elsewhere).unwrap();
    let tool_call = hooks
        .iter()
        .find(|hook| {
            hook["hook_event_name"] == "PostToolUse" && hook["agent_type"] == STAND_IN_AGENT
        })
        .unwrap();
    let post = hook_post(tool_call).unwrap();
    assert!(post.messages.is_empty());
    assert_eq!(
        post.calls.len(),
        1,
        "it still ties the stand-in to its session"
    );

    let mut stop = hooks
        .iter()
        .find(|hook| hook["hook_event_name"] == "SubagentStop")
        .unwrap()
        .clone();
    stop["agent_transcript_path"] = json!(elsewhere);
    assert_eq!(hook_post(&stop).unwrap().messages.len(), 2);
}

#[test]
fn an_older_claude_codes_last_message_is_the_report_when_the_transcript_lacks_it() {
    let world = World::new();
    let mut stop = world
        .hooks()
        .into_iter()
        .find(|hook| hook["hook_event_name"] == "SubagentStop")
        .unwrap();
    stop["last_assistant_message"] = json!("All done.");
    let messages = hook_post(&stop).unwrap().messages;
    assert_eq!(messages.last().unwrap().text, "All done.");
    // One already in the transcript is not added twice.
    let report = transcript(TRANSCRIPT).messages[1].text.clone();
    stop["last_assistant_message"] = json!(report);
    assert_eq!(hook_post(&stop).unwrap().messages.len(), 2);
}

// ---- into the session ------------------------------------------------------------------------

#[test]
fn a_long_last_message_already_in_the_transcript_is_not_added_twice_and_a_half_written_line_is_skipped()
 {
    let world = World::new();
    let long = "é".repeat(MAX_MESSAGE_BYTES);
    let lines = [
        json!({ "type": "user", "uuid": "u-1", "message": { "content": "Total the charges." } }),
        json!({ "type": "assistant", "uuid": "a-1",
                "message": { "content": [{ "type": "text", "text": long }] } }),
    ];
    let mut bytes: Vec<u8> = lines
        .iter()
        .flat_map(|line| format!("{line}\n").into_bytes())
        .collect();
    // The line being written ends inside a character.
    bytes.extend_from_slice(b"{\"type\":\"assistant\",\"text\":\"\xc3");
    let path = world.dir.path().join("long.jsonl");
    std::fs::write(&path, bytes).unwrap();
    let mut stop = world
        .hooks()
        .into_iter()
        .find(|hook| hook["hook_event_name"] == "SubagentStop")
        .unwrap();
    stop["agent_transcript_path"] = json!(path);
    stop["last_assistant_message"] = json!(long);
    let messages = hook_post(&stop).unwrap().messages;
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(messages[1].truncated);
}

#[test]
fn a_recorded_stand_in_run_puts_its_task_and_report_around_its_calls_once() {
    let mut world = World::new();
    world.replay("sess-1");
    let log = world.log("sess-1");
    let calls = transcript(TRANSCRIPT).calls;
    let mut expected = vec!["chat user".to_owned()];
    expected.extend(calls.iter().map(|call| format!("call {call}")));
    expected.push("chat assistant".to_owned());
    assert_eq!(order(&log), expected);
    let said = texts(&log);
    assert_eq!(
        said.len(),
        2,
        "the task and the report, once each: {said:?}"
    );
    assert!(said[0].starts_with(TASK_START));
    // The complete copy replaced what was posted live.
    assert!(
        log.events
            .iter()
            .any(|event| matches!(event.body, EventBody::ChatReplaced(_)))
    );
    // Nothing went anywhere else.
    assert!(world.log("other").events.is_empty());
}

#[test]
fn chat_is_held_until_a_tool_call_ties_the_stand_in_to_its_session() {
    let mut world = World::new();
    let first = live("agent-1", "toolu_1", vec![says("t1", "I will look first.")]);
    assert_eq!(world.post(first.clone()), 0, "no call recorded yet: held");
    world.call("sess-1", "toolu_1");
    // The same call's next hook ties the stand-in to the session; the held message is
    // logged where it arrived, before the call.
    assert_eq!(
        world.post(ChatPost {
            messages: Vec::new(),
            ..first
        }),
        1
    );
    let log = world.log("sess-1");
    assert_eq!(order(&log), ["chat assistant", "call toolu_1"]);
}

#[test]
fn a_live_message_is_logged_once_however_often_it_is_posted() {
    let mut world = World::new();
    world.call("sess-1", "toolu_1");
    let post = live("agent-1", "toolu_1", vec![says("t1", "first")]);
    assert_eq!(world.post(post.clone()), 1);
    assert_eq!(world.post(post), 0);
    assert_eq!(texts(&world.log("sess-1")), ["first"]);
}

#[test]
fn two_stand_ins_at_once_each_get_their_own_session() {
    let mut world = World::new();
    world.call("sess-a", "toolu_a");
    world.call("sess-b", "toolu_b");
    // Both start with the same task and the same first call.
    for (agent, call) in [("agent-a", "toolu_a"), ("agent-b", "toolu_b")] {
        let task = Message {
            role: ChatRole::User,
            before_call: Some(call.into()),
            ..says("task", "Total the charges.")
        };
        world.post(live(agent, call, vec![task]));
        world.post(live(
            agent,
            call,
            vec![says(&format!("{agent}-said"), agent)],
        ));
    }
    assert_eq!(
        texts(&world.log("sess-a")),
        ["Total the charges.", "agent-a"]
    );
    assert_eq!(
        texts(&world.log("sess-b")),
        ["Total the charges.", "agent-b"]
    );
}

#[test]
fn chat_that_never_finds_its_session_is_dropped() {
    let mut world = World::new();
    let start = Instant::now();
    world.post_at(live("agent-1", "toolu_1", vec![says("t1", "held")]), start);
    let later = start + HELD_FOR + Duration::from_secs(1);
    world.call("sess-1", "toolu_1");
    assert_eq!(
        world.post_at(live("agent-1", "toolu_1", Vec::new()), later),
        0
    );
    assert!(world.log("sess-1").chat().is_empty());
}

#[test]
fn a_stand_in_back_after_a_long_silence_still_finds_its_session() {
    let mut world = World::new();
    let start = Instant::now();
    world.call("sess-1", "toolu_1");
    assert_eq!(
        world.post_at(live("agent-1", "toolu_1", vec![says("t1", "first")]), start),
        1
    );
    let later = start + HELD_FOR + Duration::from_secs(1);
    let stop = ChatPost {
        agent: "agent-1".into(),
        calls: vec!["toolu_1".into()],
        messages: vec![says("t1", "first"), says("t2", "report")],
        complete: true,
    };
    assert_eq!(world.post_at(stop, later), 2);
    assert_eq!(texts(&world.log("sess-1")), ["first", "report"]);
}

#[test]
fn a_complete_copy_cut_to_fit_keeps_the_live_messages_it_lacks() {
    let mut world = World::new();
    world.call("sess-1", "toolu_1");
    let messages = vec![says("t1", "task"), says("t2", "middle"), says("t3", "end")];
    assert_eq!(world.post(live("agent-1", "toolu_1", messages)), 3);
    // The copy kept the task and the newest message only.
    let stop = ChatPost {
        agent: "agent-1".into(),
        calls: vec!["toolu_1".into()],
        messages: vec![says("t1", "task"), says("t3", "end")],
        complete: true,
    };
    assert_eq!(world.post(stop), 2);
    let mut kept = texts(&world.log("sess-1"));
    kept.sort();
    assert_eq!(kept, ["end", "middle", "task"]);
}

#[test]
fn a_full_intake_makes_room_by_forgetting_a_finished_stand_in() {
    let mut world = World::new();
    world.call("sess-1", "toolu_done");
    let done = ChatPost {
        agent: "done".into(),
        calls: vec!["toolu_done".into()],
        messages: vec![says("d1", "finished")],
        complete: true,
    };
    assert_eq!(world.post(done), 1);
    for n in 1..MAX_AGENTS {
        world.post(live(
            &format!("waiting-{n}"),
            &format!("toolu_w{n}"),
            vec![],
        ));
    }
    world.call("sess-2", "toolu_new");
    assert_eq!(
        world.post(live("new", "toolu_new", vec![says("n1", "hello")])),
        1
    );
}

#[test]
fn held_chat_is_capped() {
    let world = World::new();
    let big = "x".repeat(MAX_MESSAGE_BYTES);
    let messages: Vec<Message> = (0..MAX_HELD_BYTES / MAX_MESSAGE_BYTES + 5)
        .map(|n| says(&format!("t{n}"), &big))
        .collect();
    world.post(live("agent-1", "toolu_1", messages));
    let stand_ins = world.intake.stand_ins.lock().unwrap();
    assert!(stand_ins["agent-1"].held_bytes <= MAX_HELD_BYTES);
}

#[test]
fn a_long_message_is_cut_and_marked() {
    let mut world = World::new();
    world.call("sess-1", "toolu_1");
    let long = "é".repeat(MAX_MESSAGE_BYTES);
    world.post(live("agent-1", "toolu_1", vec![says("t1", &long)]));
    let log = world.log("sess-1");
    let (_, turn) = log.chat()[0];
    assert!(turn.truncated);
    assert!(turn.text.len() <= MAX_MESSAGE_BYTES);
}

#[test]
fn a_known_secret_in_the_chat_never_reaches_the_log() {
    let mut world = World::new();
    world.secrets.add(SECRET);
    world.call("sess-1", "toolu_1");
    let base64 = base64::engine::general_purpose::STANDARD.encode(SECRET);
    let url: String = url::form_urlencoded::byte_serialize(SECRET.as_bytes()).collect();
    let text = format!("plain={SECRET} b64={base64} url={url}");
    world.post(live("agent-1", "toolu_1", vec![says("t1", &text)]));
    world.post(ChatPost {
        complete: true,
        ..live("agent-1", "toolu_1", vec![says("t1", &text)])
    });
    let bytes = std::fs::read_to_string(world.store.events_path(Some("sess-1"))).unwrap();
    for form in [SECRET, base64.as_str(), url.as_str()] {
        assert!(!bytes.contains(form), "the log holds {form}");
    }
    assert_eq!(texts(&world.log("sess-1")).len(), 1);
}

#[test]
fn after_a_restart_the_complete_copy_still_finds_its_session() {
    let mut world = World::new();
    world.call("sess-1", "toolu_1");
    world.call("sess-1", "toolu_2");
    // A playground started again over the same store remembers no tool call.
    let restarted = Recorder::new(Arc::clone(&world.store), world.secrets.clone());
    let intake = ChatIntake::default();
    let post = ChatPost {
        agent: "agent-1".into(),
        calls: vec!["toolu_2".into()],
        messages: vec![says("t1", "done")],
        complete: true,
    };
    assert_eq!(intake.accept(post, &restarted, Instant::now()), 1);
    assert_eq!(texts(&world.log("sess-1")), ["done"]);
}
