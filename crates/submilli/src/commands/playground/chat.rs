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
//! - A stand-in the assistant resumes after it stopped goes on in a new connection, so
//!   its calls run in a new session: what it says from then on goes to the session its
//!   next call runs in, and what it said before stays where it was.

use std::collections::{HashMap, HashSet};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::state::STAND_IN_TOKEN;
use super::store::events::{ChatReplaced, ChatRole, ChatTurn, EventBody, Position};
use super::store::{Recorder, Store};

/// The stand-in agent's name in the assistant: only its hooks post chat.
pub(crate) const STAND_IN_AGENT: &str = "submilli-standin";
/// The tool a Claude Code subagent hands its final report back with.
const HANDBACK_TOOL: &str = "SubagentHandback";
/// How Claude Code begins a message it relays to a stand-in after its task.
const COORDINATOR_LEAD: &str = "The coordinator sent a message while you were working:";

/// The longest message kept; the rest is cut and the message marked.
const MAX_MESSAGE_BYTES: usize = 16 * 1024;
/// The longest model name kept.
const MAX_MODEL_BYTES: usize = 200;
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
    /// The model that wrote an assistant message, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) model: Option<String>,
}

// ---- the transcript ------------------------------------------------------------------------

/// What a stand-in's transcript holds: its messages, and the ids of its tool calls.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Transcript {
    pub(crate) messages: Vec<Message>,
    pub(crate) calls: Vec<String>,
}

/// Reads a Claude Code subagent transcript (JSON lines). The task is its first plain user
/// message, and each message relayed to it later is one too; the stand-in's text and its
/// handed-back report are its messages. Reminders
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
            Some("user") if entry["isMeta"] != true || entry["origin"]["kind"] == "coordinator" => {
                // A message sent to a stand-in already at work, or resumed, comes marked
                // as the assistant's own, with a lead-in (Claude Code 2.1.300).
                let text = content.as_str().map(|text| {
                    text.strip_prefix(COORDINATOR_LEAD)
                        .unwrap_or(text)
                        .trim_start()
                });
                if let Some(text) = text.filter(|text| !text.trim().is_empty()) {
                    waiting.push(read.messages.len());
                    read.messages.push(message(turn, ChatRole::User, text));
                }
            }
            Some("assistant") => {
                let model = entry["message"]["model"].as_str().and_then(model_name);
                let blocks = content.as_array().map(Vec::as_slice).unwrap_or_default();
                let said: Vec<&str> = blocks
                    .iter()
                    .filter(|block| block["type"] == "text")
                    .filter_map(|block| block["text"].as_str())
                    .filter(|text| !text.trim().is_empty())
                    .collect();
                if !said.is_empty() {
                    waiting.push(read.messages.len());
                    let mut said = message(turn, ChatRole::Assistant, &said.join("\n\n"));
                    said.model.clone_from(&model);
                    read.messages.push(said);
                }
                for block in blocks.iter().filter(|block| block["type"] == "tool_use") {
                    if block["name"] == HANDBACK_TOOL {
                        if let Some(report) = block["input"]["message"].as_str() {
                            let turn = format!("{turn}:report");
                            let mut report = message(&turn, ChatRole::Assistant, report);
                            report.model.clone_from(&model);
                            read.messages.push(report);
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

/// A model name as kept, cut to [`MAX_MODEL_BYTES`]; none for an empty name or a
/// stand-in for one such as `<synthetic>`.
fn model_name(model: &str) -> Option<String> {
    let model = model.trim();
    if model.is_empty() || model.starts_with('<') {
        return None;
    }
    Some(super::within_bytes(model, MAX_MODEL_BYTES).0.to_owned())
}

/// One message, cut to [`MAX_MESSAGE_BYTES`].
pub(crate) fn message(turn: &str, role: ChatRole, text: &str) -> Message {
    let (text, truncated) = super::within_bytes(text, MAX_MESSAGE_BYTES);
    Message {
        turn: turn.to_owned(),
        role,
        text: text.to_owned(),
        truncated,
        before_call: None,
        model: None,
    }
}

// ---- the hook ------------------------------------------------------------------------------

/// `submilli playground chat-hook`: posts the stand-in's chat for the hook on stdin.
/// Always exits 0 and prints nothing, so it never changes the assistant's session, whether
/// or not the playground is running, and its post is bounded so it never holds it up.
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
        let _ = running.post_bounded::<Value>("/api/chat", &body);
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
    // Lossy: the line being written may end inside a character.
    let read = path
        .and_then(|path| std::fs::read(path).ok())
        .map(|bytes| transcript(&String::from_utf8_lossy(&bytes)))
        .unwrap_or_default();
    let mut messages = read.messages;
    // Claude Code 2.1.289 gave the final answer here, and not in the transcript.
    if complete
        && let Some(last) = hook["last_assistant_message"]
            .as_str()
            .filter(|last| !last.trim().is_empty())
    {
        let last = message(&format!("{agent}:last"), ChatRole::Assistant, last);
        if !messages.iter().any(|message| message.text == last.text) {
            messages.push(last);
        }
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
    stand_ins: Mutex<HashMap<String, StandInChat>>,
}

/// One stand-in's chat: where it goes, and what is logged and held.
struct StandInChat {
    session: Option<String>,
    /// Turns in the log, and where each was placed.
    logged: HashMap<String, Position>,
    /// Chat waiting for the session, each message placed when it arrived.
    held: Vec<(Position, Message)>,
    held_bytes: usize,
    last_seen: Instant,
    /// The complete conversation was logged; later live posts are late copies, unless
    /// they bring a turn the stand-in hasn't said yet: then it was resumed.
    complete: bool,
    /// Turns logged in the sessions it chatted in before it was resumed.
    earlier: HashSet<String>,
}

impl StandInChat {
    fn new(now: Instant) -> Self {
        Self {
            session: None,
            logged: HashMap::new(),
            held: Vec::new(),
            held_bytes: 0,
            last_seen: now,
            complete: false,
            earlier: HashSet::new(),
        }
    }

    /// Whether `post` is a resumed stand-in's first word: a live post, after its stop,
    /// with a turn it never said.
    fn resumed_by(&self, post: &ChatPost) -> bool {
        self.complete
            && !post.complete
            && post.messages.iter().any(|message| {
                !self.logged.contains_key(&message.turn) && !self.earlier.contains(&message.turn)
            })
    }

    /// Whether all it holds is which turns it logged: its conversation is complete, or it
    /// was resumed after one was. Forgetting such a stand-in would move those turns into
    /// the next session it chats in, so only a full intake does.
    fn only_remembers(&self) -> bool {
        self.held.is_empty() && (self.complete || !self.earlier.is_empty())
    }

    /// Forgets the session it chatted in, keeping its turns there as earlier ones: its
    /// chat is held until a call says where it goes on.
    fn restart_after_resume(&mut self) {
        let logged = std::mem::take(&mut self.logged);
        self.earlier.extend(logged.into_keys());
        self.session = None;
        self.complete = false;
    }

    fn hold(&mut self, position: Position, message: Message) {
        if self.held.iter().any(|(_, held)| held.turn == message.turn) {
            return;
        }
        let size =
            message.text.len() + message.turn.len() + message.model.as_ref().map_or(0, String::len);
        if self.held_bytes + size > MAX_HELD_BYTES {
            return;
        }
        self.held_bytes += size;
        self.held.push((position, message));
    }
}

/// What one post did.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Accepted {
    /// How many messages it logged.
    pub(crate) logged: usize,
    /// The session its stand-in now chats in, when this post placed it there.
    pub(crate) placed: Option<String>,
}

impl ChatIntake {
    /// Takes one post.
    pub(crate) fn accept(&self, mut post: ChatPost, recorder: &Recorder, now: Instant) -> Accepted {
        let at = Position::at(super::now_micros());
        within_limits(&mut post);
        let found = self.session_for(&post, recorder, now);
        // Read before the lock too: the log may be long.
        let first_events = match (&found, post.complete) {
            (Some(session), true) => first_event_of_calls(recorder.store(), session),
            _ => HashMap::new(),
        };
        let mut stand_ins = self.lock();
        // Again: the map may have changed while the lock was released.
        prune(&mut stand_ins, now);
        if !stand_ins.contains_key(&post.agent) && stand_ins.len() >= MAX_AGENTS {
            make_room(&mut stand_ins);
        }
        if !stand_ins.contains_key(&post.agent) && stand_ins.len() >= MAX_AGENTS {
            return Accepted::default();
        }
        let stand_in = stand_ins
            .entry(post.agent.clone())
            .or_insert_with(|| StandInChat::new(now));
        stand_in.last_seen = now;
        if stand_in.resumed_by(&post) {
            stand_in.restart_after_resume();
        }
        post.messages
            .retain(|message| !stand_in.earlier.contains(&message.turn));
        let mut placed = None;
        if stand_in.session.is_none() {
            stand_in.session = found.clone();
            placed.clone_from(&found);
        }
        let Some(session) = stand_in.session.clone() else {
            if post.complete {
                stand_in.held.clear();
                stand_in.held_bytes = 0;
            }
            for message in post.messages {
                stand_in.hold(at, message);
            }
            return Accepted { logged: 0, placed };
        };
        let mut logged = 0;
        for (position, message) in std::mem::take(&mut stand_in.held) {
            logged += log_live(stand_in, recorder, &session, &post.agent, position, message);
        }
        stand_in.held_bytes = 0;
        if post.complete {
            let first_events = if found.as_deref() == Some(session.as_str()) {
                first_events
            } else {
                first_event_of_calls(recorder.store(), &session)
            };
            logged += log_complete(stand_in, recorder, &session, &post, at, &first_events);
        } else if !stand_in.complete {
            for message in post.messages {
                logged += log_live(stand_in, recorder, &session, &post.agent, at, message);
            }
        }
        Accepted { logged, placed }
    }

    /// The session `post`'s stand-in is known to chat in, else the one its latest tool
    /// call ran in, if either. A stand-in that stopped may have been resumed, in another
    /// session, so for one its calls are looked up again. Looking may read session logs,
    /// so it runs with the lock released; stand-ins a later prune would forget are pruned
    /// first, so one back after a long silence looks again.
    fn session_for(&self, post: &ChatPost, recorder: &Recorder, now: Instant) -> Option<String> {
        let known = {
            let mut stand_ins = self.lock();
            prune(&mut stand_ins, now);
            stand_ins
                .get(&post.agent)
                .filter(|stand_in| !stand_in.complete)
                .and_then(|stand_in| stand_in.session.clone())
        };
        known.or_else(|| recorder.session_of_call(post.calls.iter().rev().map(String::as_str)))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, StandInChat>> {
        self.stand_ins
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// Caps `post` to what the intake keeps: so many messages, each so long, and a model
/// name only on an answer, as the hook keeps it, whoever posted.
fn within_limits(post: &mut ChatPost) {
    post.messages.truncate(MAX_MESSAGES);
    for message in &mut post.messages {
        let (text, cut) = super::within_bytes(&message.text, MAX_MESSAGE_BYTES);
        if cut {
            message.text = text.to_owned();
            message.truncated = true;
        }
        message.model = match message.role {
            ChatRole::Assistant => message.model.as_deref().and_then(model_name),
            _ => None,
        };
    }
}

/// Forgets the stand-in heard from longest ago among those that only remember what they
/// logged, if any: a full intake makes room for a new one.
fn make_room(stand_ins: &mut HashMap<String, StandInChat>) {
    let done = stand_ins
        .iter()
        .filter(|(_, stand_in)| stand_in.only_remembers())
        .min_by_key(|(_, stand_in)| stand_in.last_seen)
        .map(|(agent, _)| agent.clone());
    if let Some(agent) = done {
        stand_ins.remove(&agent);
    }
}

/// Drops the chat held by stand-ins not heard from for [`HELD_FOR`], as it never found
/// its session, and forgets those stand-ins unless they [remember](StandInChat::only_remembers)
/// turns they logged: resumed later, those stay in the session they were said in.
/// [`make_room`] forgets those.
fn prune(stand_ins: &mut HashMap<String, StandInChat>, now: Instant) {
    stand_ins.retain(|_, stand_in| {
        if now.saturating_duration_since(stand_in.last_seen) < HELD_FOR {
            return true;
        }
        stand_in.held.clear();
        stand_in.held_bytes = 0;
        stand_in.only_remembers()
    });
}

fn log_live(
    stand_in: &mut StandInChat,
    recorder: &Recorder,
    session: &str,
    agent_id: &str,
    position: Position,
    message: Message,
) -> usize {
    if stand_in.logged.contains_key(&message.turn) {
        return 0;
    }
    stand_in.logged.insert(message.turn.clone(), position);
    let event_id = format!("chat-{agent_id}-{}", message.turn);
    recorder.append_chat(session, event_id, position, chat_body(agent_id, message));
    1
}

/// Replaces what was logged live with the complete conversation, each message just before
/// the session's first event of the next tool call after it (`first_events`). A live
/// message the copy lacks, as one cut to fit its post may, is kept.
fn log_complete(
    stand_in: &mut StandInChat,
    recorder: &Recorder,
    session: &str,
    post: &ChatPost,
    at: Position,
    first_events: &HashMap<String, Position>,
) -> usize {
    let copied: std::collections::HashSet<&str> = post
        .messages
        .iter()
        .map(|message| message.turn.as_str())
        .collect();
    let mut turns: Vec<String> = stand_in
        .logged
        .keys()
        .filter(|turn| copied.contains(turn.as_str()))
        .cloned()
        .collect();
    if !turns.is_empty() {
        turns.sort();
        recorder.append_chat(
            session,
            format!("chat-{}-replaced", post.agent),
            at,
            EventBody::ChatReplaced(ChatReplaced { turns }),
        );
    }
    // A live message the copy lacks stays logged where it was.
    let mut logged: HashMap<String, Position> = stand_in
        .logged
        .iter()
        .filter(|(turn, _)| !copied.contains(turn.as_str()))
        .map(|(turn, position)| (turn.clone(), *position))
        .collect();
    for message in &post.messages {
        // After the last call, a message is placed now, as the copy arrives at the
        // stand-in's stop. Before a call the session has no event of, it keeps where it
        // was posted live.
        let position = match message.before_call.as_deref() {
            None => at,
            Some(call) => first_events
                .get(call)
                .map(|first| Position::at(first.at_micros.saturating_sub(1)))
                .or_else(|| stand_in.logged.get(&message.turn).copied())
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
    stand_in.logged = logged;
    stand_in.complete = true;
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

pub(crate) fn chat_body(agent: &str, message: Message) -> EventBody {
    EventBody::Chat(ChatTurn {
        turn: message.turn,
        role: message.role,
        text: message.text,
        truncated: message.truncated,
        agent: agent.to_owned(),
        model: message.model,
    })
}

#[cfg(test)]
mod tests;
