//! The conversation that drove a session, in its event log beside the runs.
//!
//! For a coding assistant's stand-in, the assistant's hooks call `submilli playground
//! chat-hook` with each hook's JSON on stdin. Only the stand-in's own hooks post anything,
//! read from the stand-in's own transcript: its task, what it says, and its final report.
//! Nothing from the developer's conversation is ever read.
//!
//! - Each tool call the stand-in makes posts what its transcript holds so far. A message
//!   posted live is placed when it arrived: a tool call's hook runs before the call is
//!   sent, so what led to the call sorts before it.
//! - When the stand-in stops, the complete conversation replaces what was posted live,
//!   each message placed just before the session's first event of the next tool call
//!   after it in the transcript.
//! - A stand-in's chat goes to the session its tool calls ran in: the hook's tool-call id
//!   is the one the assistant sent in the call's `_meta`, which the server recorded. Chat
//!   that arrives before any call is recorded is held, within a size and age cap, until
//!   one is.

use std::collections::HashMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::store::events::{ChatReplaced, ChatRole, ChatTurn, EventBody, Position};
use super::store::{Recorder, Store};

/// The stand-in agent's name in the assistant: only its hooks post chat.
pub(crate) const STAND_IN_AGENT: &str = "submilli-standin";
const STAND_IN_TOKEN: &str = "stand-in";
/// The tool a Claude Code subagent hands its final report back with.
const HANDBACK_TOOL: &str = "SubagentHandback";

/// The longest message kept; the rest is cut and the message marked.
const MAX_MESSAGE_BYTES: usize = 16 * 1024;
/// The most messages one post carries, and their total size: well inside the control
/// API's body limit.
const MAX_MESSAGES: usize = 1000;
const MAX_POST_BYTES: usize = 1 << 20;
/// The most of a hook's input read.
const MAX_HOOK_BYTES: u64 = 4 << 20;
/// Chat held for a stand-in no session is known for yet: at most this much, for this
/// long, for this many stand-ins.
const MAX_HELD_BYTES: usize = 256 * 1024;
const HELD_FOR: Duration = Duration::from_secs(10 * 60);
const MAX_AGENTS: usize = 64;

/// What `chat-hook` posts: one stand-in's messages, and the tool calls that tie it to its
/// session.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ChatPost {
    pub(crate) agent: String,
    #[serde(default)]
    pub(crate) calls: Vec<String>,
    #[serde(default)]
    pub(crate) messages: Vec<Message>,
    /// The whole conversation, replacing what was posted live.
    #[serde(default)]
    pub(crate) complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Message {
    pub(crate) turn: String,
    pub(crate) role: ChatRole,
    pub(crate) text: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) truncated: bool,
    /// The next tool call after the message in the conversation, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) before_call: Option<String>,
}

// ---- the transcript ------------------------------------------------------------------------

/// What a stand-in's transcript holds: its messages, and the ids of its tool calls.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Transcript {
    pub(crate) messages: Vec<Message>,
    pub(crate) calls: Vec<String>,
}

/// Reads a Claude Code subagent transcript (JSON lines). The task is its first plain user
/// message; the stand-in's text and its handed-back report are its messages. Reminders
/// the assistant adds, tool results, and reasoning are left out. A line that does not
/// parse, as the one being written may not, is skipped.
pub(crate) fn transcript(text: &str) -> Transcript {
    let mut read = Transcript::default();
    // Messages waiting for the next tool call after them.
    let mut waiting: Vec<usize> = Vec::new();
    for line in text.lines() {
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let turn = entry["uuid"].as_str().unwrap_or_default();
        let content = &entry["message"]["content"];
        match entry["type"].as_str() {
            Some("user") if entry["isMeta"] != true => {
                if let Some(text) = content.as_str().filter(|text| !text.trim().is_empty()) {
                    waiting.push(read.messages.len());
                    read.messages.push(message(turn, ChatRole::User, text));
                }
            }
            Some("assistant") => {
                let blocks = content.as_array().map(Vec::as_slice).unwrap_or_default();
                let said: Vec<&str> = blocks
                    .iter()
                    .filter(|block| block["type"] == "text")
                    .filter_map(|block| block["text"].as_str())
                    .filter(|text| !text.trim().is_empty())
                    .collect();
                if !said.is_empty() {
                    waiting.push(read.messages.len());
                    read.messages
                        .push(message(turn, ChatRole::Assistant, &said.join("\n\n")));
                }
                for block in blocks.iter().filter(|block| block["type"] == "tool_use") {
                    if block["name"] == HANDBACK_TOOL {
                        if let Some(report) = block["input"]["message"].as_str() {
                            let turn = format!("{turn}:report");
                            read.messages
                                .push(message(&turn, ChatRole::Assistant, report));
                        }
                        continue;
                    }
                    let Some(id) = block["id"].as_str() else {
                        continue;
                    };
                    for index in waiting.drain(..) {
                        read.messages[index].before_call = Some(id.to_owned());
                    }
                    read.calls.push(id.to_owned());
                }
            }
            _ => {}
        }
    }
    read
}

fn message(turn: &str, role: ChatRole, text: &str) -> Message {
    let (text, truncated) = capped(text);
    Message {
        turn: turn.to_owned(),
        role,
        text,
        truncated,
        before_call: None,
    }
}

/// `text` cut to [`MAX_MESSAGE_BYTES`] at a character boundary, and whether it was cut.
fn capped(text: &str) -> (String, bool) {
    if text.len() <= MAX_MESSAGE_BYTES {
        return (text.to_owned(), false);
    }
    let mut end = MAX_MESSAGE_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_owned(), true)
}

// ---- the hook ------------------------------------------------------------------------------

/// `submilli playground chat-hook`: posts the stand-in's chat for the hook on stdin.
/// Always exits 0 and prints nothing, so it never blocks or changes the assistant's
/// session, whether or not the playground is running.
pub(crate) fn execute_hook() -> ExitCode {
    let mut input = String::new();
    let read = std::io::stdin()
        .lock()
        .take(MAX_HOOK_BYTES)
        .read_to_string(&mut input);
    if read.is_err() {
        return ExitCode::SUCCESS;
    }
    let Some(post) = serde_json::from_str::<Value>(&input)
        .ok()
        .and_then(|hook| hook_post(&hook))
    else {
        return ExitCode::SUCCESS;
    };
    if let Ok(Ok(running)) = super::controls::act::connect_as(STAND_IN_TOKEN)
        && let Ok(body) = serde_json::to_value(&post)
    {
        let _ = running.send::<Value>("POST", "/api/chat", Some(&body));
    }
    ExitCode::SUCCESS
}

/// What a hook posts: nothing unless it is the stand-in's own tool call or its stop.
pub(crate) fn hook_post(hook: &Value) -> Option<ChatPost> {
    if hook["agent_type"] != STAND_IN_AGENT {
        return None;
    }
    let agent = hook["agent_id"].as_str().filter(|id| !id.is_empty())?;
    let event = hook["hook_event_name"].as_str()?;
    let complete = match event {
        "PreToolUse" | "PostToolUse" | "PostToolUseFailure" => false,
        "SubagentStop" => true,
        _ => return None,
    };
    let path = if complete {
        hook["agent_transcript_path"]
            .as_str()
            .map(PathBuf::from)
            .or_else(|| live_transcript(hook, agent))
    } else {
        live_transcript(hook, agent)
    };
    let read = path
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|text| transcript(&text))
        .unwrap_or_default();
    let mut messages = read.messages;
    // Claude Code 2.1.289 gave the final answer here, and not in the transcript.
    if complete
        && let Some(last) = hook["last_assistant_message"]
            .as_str()
            .filter(|last| !last.trim().is_empty())
        && !messages.iter().any(|message| message.text == *last)
    {
        messages.push(message(&format!("{agent}:last"), ChatRole::Assistant, last));
    }
    let calls = if complete {
        read.calls
    } else {
        hook["tool_use_id"]
            .as_str()
            .map(|id| vec![id.to_owned()])
            .unwrap_or_default()
    };
    Some(ChatPost {
        agent: agent.to_owned(),
        calls,
        messages: within_budget(messages),
        complete,
    })
}

/// Where Claude Code writes a subagent's transcript as it works (observed with Claude
/// Code 2.1.289 and 2.1.296; not documented): beside the session's own transcript.
fn live_transcript(hook: &Value, agent: &str) -> Option<PathBuf> {
    let session = hook["session_id"].as_str()?;
    let main = Path::new(hook["transcript_path"].as_str()?);
    if session.contains(['/', '\\']) || agent.contains(['/', '\\']) {
        return None;
    }
    Some(
        main.parent()?
            .join(session)
            .join("subagents")
            .join(format!("agent-{agent}.jsonl")),
    )
}

/// At most [`MAX_MESSAGES`] and [`MAX_POST_BYTES`] of them: the first (the task), then
/// the latest that fit.
fn within_budget(mut messages: Vec<Message>) -> Vec<Message> {
    let size = |message: &Message| message.text.len() + message.turn.len() + 128;
    let total: usize = messages.iter().map(size).sum();
    if messages.len() <= MAX_MESSAGES && total <= MAX_POST_BYTES {
        return messages;
    }
    let first = messages.remove(0);
    let mut budget = MAX_POST_BYTES.saturating_sub(size(&first));
    let mut kept: Vec<Message> = Vec::new();
    for message in messages.into_iter().rev() {
        if kept.len() + 1 >= MAX_MESSAGES || size(&message) > budget {
            break;
        }
        budget -= size(&message);
        kept.push(message);
    }
    kept.push(first);
    kept.reverse();
    kept
}

// ---- the intake ----------------------------------------------------------------------------

/// The playground's side of the chat route: which session each stand-in's chat goes to,
/// what it already logged, and what it holds until it knows.
#[derive(Default)]
pub(crate) struct ChatIntake {
    agents: Mutex<HashMap<String, Agent>>,
}

struct Agent {
    session: Option<String>,
    /// Turns in the log, and where each was placed.
    logged: HashMap<String, Position>,
    /// Chat waiting for the session, each message placed when it arrived.
    held: Vec<(Position, Message)>,
    held_bytes: usize,
    last_seen: Instant,
    /// The complete conversation was logged; later live posts are late copies.
    complete: bool,
}

impl Agent {
    fn new(now: Instant) -> Self {
        Self {
            session: None,
            logged: HashMap::new(),
            held: Vec::new(),
            held_bytes: 0,
            last_seen: now,
            complete: false,
        }
    }

    fn hold(&mut self, position: Position, message: Message) {
        if self.held.iter().any(|(_, held)| held.turn == message.turn) {
            return;
        }
        let size = message.text.len() + message.turn.len();
        if self.held_bytes + size > MAX_HELD_BYTES {
            return;
        }
        self.held_bytes += size;
        self.held.push((position, message));
    }
}

impl ChatIntake {
    /// Takes one post; how many messages it logged.
    pub(crate) fn accept(&self, mut post: ChatPost, recorder: &Recorder, now: Instant) -> usize {
        let at = Position {
            at_micros: super::now_micros(),
            run: None,
            call_index: None,
            rank: 0,
        };
        post.messages.truncate(MAX_MESSAGES);
        for message in &mut post.messages {
            let (text, cut) = capped(&message.text);
            message.text = text;
            message.truncated |= cut;
        }
        let mut agents = self.agents.lock().unwrap_or_else(PoisonError::into_inner);
        prune(&mut agents, now);
        if !agents.contains_key(&post.agent) && agents.len() >= MAX_AGENTS {
            return 0;
        }
        let agent = agents
            .entry(post.agent.clone())
            .or_insert_with(|| Agent::new(now));
        agent.last_seen = now;
        if agent.session.is_none() {
            agent.session = recorder.session_of_call(post.calls.iter().map(String::as_str));
        }
        let Some(session) = agent.session.clone() else {
            if post.complete {
                agent.held.clear();
                agent.held_bytes = 0;
            }
            for message in post.messages {
                agent.hold(at, message);
            }
            return 0;
        };
        let mut logged = 0;
        for (position, message) in std::mem::take(&mut agent.held) {
            logged += log_live(agent, recorder, &session, &post.agent, position, message);
        }
        agent.held_bytes = 0;
        if post.complete {
            logged += log_complete(agent, recorder, &session, &post, at);
        } else if !agent.complete {
            for message in post.messages {
                logged += log_live(agent, recorder, &session, &post.agent, at, message);
            }
        }
        logged
    }
}

/// Forgets stand-ins not heard from for [`HELD_FOR`]: held chat that never found its
/// session is dropped.
fn prune(agents: &mut HashMap<String, Agent>, now: Instant) {
    agents.retain(|_, agent| now.saturating_duration_since(agent.last_seen) < HELD_FOR);
}

fn log_live(
    agent: &mut Agent,
    recorder: &Recorder,
    session: &str,
    agent_id: &str,
    position: Position,
    message: Message,
) -> usize {
    if agent.logged.contains_key(&message.turn) {
        return 0;
    }
    agent.logged.insert(message.turn.clone(), position);
    let event_id = format!("chat-{agent_id}-{}", message.turn);
    recorder.append_chat(session, event_id, position, chat_body(agent_id, message));
    1
}

/// Replaces what was logged live with the complete conversation, each message just before
/// the session's first event of the next tool call after it.
fn log_complete(
    agent: &mut Agent,
    recorder: &Recorder,
    session: &str,
    post: &ChatPost,
    at: Position,
) -> usize {
    let first_events = first_event_of_calls(recorder.store(), session);
    if !agent.logged.is_empty() {
        let mut turns: Vec<String> = agent.logged.keys().cloned().collect();
        turns.sort();
        recorder.append_chat(
            session,
            format!("chat-{}-replaced", post.agent),
            at,
            EventBody::ChatReplaced(ChatReplaced { turns }),
        );
    }
    let mut logged = HashMap::new();
    for message in &post.messages {
        // After the last call, a message is placed now, as the copy arrives at the
        // stand-in's stop. Before a call the session has no event of, it keeps where it
        // was posted live.
        let position = match message.before_call.as_deref() {
            None => at,
            Some(call) => first_events
                .get(call)
                .map(|first| Position {
                    at_micros: first.at_micros.saturating_sub(1),
                    run: None,
                    call_index: None,
                    rank: 0,
                })
                .or_else(|| agent.logged.get(&message.turn).copied())
                .unwrap_or(at),
        };
        logged.insert(message.turn.clone(), position);
        let event_id = format!("chat-{}-{}-complete", post.agent, message.turn);
        recorder.append_chat(
            session,
            event_id,
            position,
            chat_body(&post.agent, message.clone()),
        );
    }
    agent.logged = logged;
    agent.complete = true;
    post.messages.len()
}

/// Where each tool call's first event in `session`'s log is, by the call's id.
fn first_event_of_calls(store: &Store, session: &str) -> HashMap<String, Position> {
    let Ok(log) = store.read_events(Some(session)) else {
        return HashMap::new();
    };
    let mut first: HashMap<String, Position> = HashMap::new();
    for event in &log.events {
        let EventBody::Event(session_event) = &event.body else {
            continue;
        };
        if let Some(call) = &session_event.tool_call_id {
            first
                .entry(call.clone())
                .and_modify(|position| *position = (*position).min(event.position))
                .or_insert(event.position);
        }
    }
    first
}

fn chat_body(agent: &str, message: Message) -> EventBody {
    EventBody::Chat(ChatTurn {
        turn: message.turn,
        role: message.role,
        text: message.text,
        truncated: message.truncated,
        agent: agent.to_owned(),
    })
}

#[cfg(test)]
mod tests;
