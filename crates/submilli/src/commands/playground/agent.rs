//! Chat with the developer's own model key: a task run by a model from Anthropic, OpenAI,
//! or Google against the same tools a production agent sees, through the playground's
//! MCP endpoint. The CLI's `chat` and the page both start it here, in the playground, so
//! the key is the one the playground started with. Its runs are labeled by who started
//! it (`chat` from the CLI, `browser-chat` from the page), and its turns go to its
//! session's log as chat.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::{Value, json};
use submilli_shared::llm::ProviderKind;
use submilli_shared::llm::tools::{
    Finish, Reply, ToolCall, ToolRequest, ToolResult, ToolSpec, Turn, build_tool_request,
    error_message, parse_tool_response,
};

use super::chat::{chat_body, message};
use super::mcp_http::{Failure, McpHttp};
use super::store::Recorder;
use super::store::events::{
    ChatRole, ChatToolCall, ChatToolResult, ChatTools, EventBody, Position,
};
use super::{DEFAULT_MAX_CALLS_PER_ANSWER, MAX_CALLS_PER_ANSWER_RANGE};

/// The variable that picks the provider and model: `<provider>:<model>`.
pub(crate) const MODEL_VARIABLE: &str = "SUBMILLI_PLAYGROUND_MODEL";

/// How many times the model may be asked before the loop stops.
pub(crate) const MAX_TURNS: usize = 20;
/// How long one model request may take.
pub(crate) const PROVIDER_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_OUTPUT_TOKENS: u64 = 4096;
/// The largest JSON of provider settings taken.
const MAX_PROVIDER_SETTINGS_BYTES: usize = 16 * 1024;
/// The MCP protocol version chat speaks.
const PROTOCOL_VERSION: &str = "2025-06-18";
/// The most of a tool's result handed back to the model.
const MAX_RESULT_BYTES: usize = 64 * 1024;

const SYSTEM: &str = "You are the production agent for this application, running in a \
    local playground. Do the task with the tools you are given: write TypeScript-subset \
    programs and run them. When the playground refuses something, report the refusal; do \
    not work around it. Finish with a short answer.";

/// The providers, each with the variables its key may be in and its default model.
struct ProviderRow {
    name: &'static str,
    kind: ProviderKind,
    keys: &'static [&'static str],
    default_model: &'static str,
    base_url_variable: &'static str,
    default_base_url: &'static str,
}

const PROVIDERS: [ProviderRow; 3] = [
    ProviderRow {
        name: "anthropic",
        kind: ProviderKind::Anthropic,
        keys: &["ANTHROPIC_API_KEY"],
        default_model: "claude-sonnet-5-5",
        base_url_variable: "ANTHROPIC_BASE_URL",
        default_base_url: "https://api.anthropic.com",
    },
    ProviderRow {
        name: "openai",
        kind: ProviderKind::OpenAi,
        keys: &["OPENAI_API_KEY"],
        default_model: "gpt-5.5",
        base_url_variable: "OPENAI_BASE_URL",
        default_base_url: "https://api.openai.com",
    },
    ProviderRow {
        name: "google",
        kind: ProviderKind::Google,
        keys: &["GEMINI_API_KEY", "GOOGLE_API_KEY"],
        default_model: "gemini-flash-latest",
        base_url_variable: "GOOGLE_GEMINI_BASE_URL",
        default_base_url: "https://generativelanguage.googleapis.com",
    },
];

/// The provider and model chat uses, and where its key is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Choice {
    pub(crate) provider: &'static str,
    pub(crate) kind: ProviderKind,
    pub(crate) model: String,
    key_variable: &'static str,
    base_url: String,
}

/// The provider and model the environment `env` names: `SUBMILLI_PLAYGROUND_MODEL` when
/// set, else the first provider whose key is set, with its default model. Reads which
/// key variable is set, never a key's value.
pub(crate) fn choose(env: impl Fn(&str) -> Option<String>) -> Result<Choice, String> {
    if let Some(named) = set(&env, MODEL_VARIABLE) {
        return choose_named(&env, &named, MODEL_VARIABLE);
    }
    PROVIDERS
        .iter()
        .find_map(|row| {
            key_of(&env, row).map(|key| choice(&env, row, row.default_model.to_owned(), key))
        })
        .ok_or_else(|| {
            "no provider configured: start the playground with ANTHROPIC_API_KEY, \
             OPENAI_API_KEY, or GEMINI_API_KEY set (and optionally \
             SUBMILLI_PLAYGROUND_MODEL=<provider>:<model>)"
                .to_owned()
        })
}

/// The provider and model `named` picks, as `<provider>:<model>`, when the environment
/// `env` holds that provider's key. `source` says where `named` came from.
pub(crate) fn choose_named(
    env: impl Fn(&str) -> Option<String>,
    named: &str,
    source: &str,
) -> Result<Choice, String> {
    let (provider, model) = named
        .split_once(':')
        .map(|(provider, model)| (provider.trim(), model.trim()))
        .filter(|(_, model)| !model.is_empty())
        .ok_or_else(|| {
            format!(
                "{source} is `{named}`; set it to `<provider>:<model>`, the provider one of {}",
                provider_names()
            )
        })?;
    // The name goes into the provider's request, and into the URL for some; only a
    // plain model name is taken.
    let plain = |part: &str| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    };
    if !plain(model.strip_prefix("models/").unwrap_or(model)) {
        return Err(format!(
            "{source} names model `{model}`; a model name is letters, digits, `.`, `_`, and `-`"
        ));
    }
    let row = PROVIDERS
        .iter()
        .find(|row| row.name == provider)
        .ok_or_else(|| {
            format!(
                "{source} names provider `{provider}`; use one of {}",
                provider_names()
            )
        })?;
    let key = key_of(&env, row).ok_or_else(|| {
        format!(
            "no provider configured: {source} names {}, but {} is not set",
            row.name,
            row.keys.join(" or ")
        )
    })?;
    Ok(choice(&env, row, model.to_owned(), key))
}

/// The models chat can use, as `<provider>:<model>`: the one [`choose`] picks first, then
/// every other provider whose key `env` holds, with its default model.
pub(crate) fn offered(env: impl Fn(&str) -> Option<String>) -> Vec<String> {
    let first = choose(&env).ok();
    let mut offered: Vec<String> = first.iter().map(Choice::named).collect();
    for row in &PROVIDERS {
        if first
            .as_ref()
            .is_some_and(|first| first.provider == row.name)
        {
            continue;
        }
        if key_of(&env, row).is_some() {
            offered.push(format!("{}:{}", row.name, row.default_model));
        }
    }
    offered
}

fn set(env: &impl Fn(&str) -> Option<String>, name: &str) -> Option<String> {
    env(name).filter(|value| !value.trim().is_empty())
}

fn key_of(env: &impl Fn(&str) -> Option<String>, row: &ProviderRow) -> Option<&'static str> {
    row.keys.iter().copied().find(|key| set(env, key).is_some())
}

fn choice(
    env: &impl Fn(&str) -> Option<String>,
    row: &'static ProviderRow,
    model: String,
    key_variable: &'static str,
) -> Choice {
    Choice {
        provider: row.name,
        kind: row.kind,
        model,
        key_variable,
        base_url: base_url(row, set(env, row.base_url_variable)),
    }
}

/// The provider's address: its override with any version path left off, else its own.
fn base_url(row: &ProviderRow, configured: Option<String>) -> String {
    let Some(url) = configured else {
        return row.default_base_url.to_owned();
    };
    let url = url.trim_end_matches('/');
    ["/v1beta", "/v1"]
        .iter()
        .find_map(|version| url.strip_suffix(version))
        .unwrap_or(url)
        .to_owned()
}

fn provider_names() -> String {
    PROVIDERS
        .iter()
        .map(|row| row.name)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The chat's limit of tool calls per answer: `requested`, else the default.
pub(crate) fn max_calls_per_answer(requested: Option<u16>) -> Result<usize, String> {
    let limit = requested.unwrap_or(DEFAULT_MAX_CALLS_PER_ANSWER);
    if !MAX_CALLS_PER_ANSWER_RANGE.contains(&limit) {
        return Err(format!(
            "the most tool calls per answer is between {} and {}",
            MAX_CALLS_PER_ANSWER_RANGE.start(),
            MAX_CALLS_PER_ANSWER_RANGE.end()
        ));
    }
    Ok(usize::from(limit))
}

/// [`choose`] from this process's environment.
pub(crate) fn configured() -> Result<Choice, String> {
    choose(|name| std::env::var(name).ok())
}

/// [`choose_named`] from this process's environment.
pub(crate) fn configured_named(named: &str, source: &str) -> Result<Choice, String> {
    choose_named(|name| std::env::var(name).ok(), named, source)
}

/// [`offered`] from this process's environment.
pub(crate) fn offered_here() -> Vec<String> {
    offered(|name| std::env::var(name).ok())
}

impl Choice {
    /// `<provider>:<model>`, as `--model` and `SUBMILLI_PLAYGROUND_MODEL` take it.
    pub(crate) fn named(&self) -> String {
        format!("{}:{}", self.provider, self.model)
    }
}

/// How a session's model is asked, fixed for the session like its model.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Settings {
    /// How hard the model reasons: `low`, `medium`, or `high`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) effort: Option<String>,
    /// The most tokens one answer may take.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) max_output: Option<u64>,
    /// Fields added to every request to the provider, as its API names them.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub(crate) provider: serde_json::Map<String, Value>,
}

impl Settings {
    pub(crate) fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Checks `settings` for `choice`'s provider by building a request with them, so what
/// is refused here is exactly what a request would refuse.
pub(crate) fn check_settings(choice: &Choice, settings: &Settings) -> Result<(), String> {
    if let Some(effort) = &settings.effort
        && !matches!(effort.as_str(), "low" | "medium" | "high")
    {
        return Err(format!("effort is `{effort}`; use low, medium, or high"));
    }
    if settings.max_output == Some(0) {
        return Err("the most output tokens is at least 1".to_owned());
    }
    let size = serde_json::to_string(&settings.provider).map_or(0, |json| json.len());
    if size > MAX_PROVIDER_SETTINGS_BYTES {
        return Err(format!(
            "the provider settings are {size} bytes; the most taken is {MAX_PROVIDER_SETTINGS_BYTES}"
        ));
    }
    request(choice, "", &[], &[], settings).map(|_| ())
}

/// The request for the conversation's next turn under `settings`: the effort's fields,
/// then the provider settings, each refused where it would change a field the playground
/// sets itself.
fn request(
    choice: &Choice,
    key: &str,
    turns: &[Turn],
    tools: &[ToolSpec],
    settings: &Settings,
) -> Result<submilli_shared::llm::wire::WireRequest, String> {
    let mut request = build_tool_request(&ToolRequest {
        kind: choice.kind,
        base_url: &choice.base_url,
        model: &choice.model,
        system: SYSTEM,
        turns,
        tools,
        max_tokens: settings.max_output.unwrap_or(MAX_OUTPUT_TOKENS),
        api_key: key,
    });
    if let Some(effort) = &settings.effort {
        let fields = match choice.kind {
            ProviderKind::OpenAi => json!({ "reasoning": { "effort": effort } }),
            ProviderKind::Anthropic => {
                return Err(format!(
                    "effort is not available for {} yet: Claude's thinking has to be handed \
                     back with each tool result, which chat does not do yet",
                    choice.provider
                ));
            }
            _ => {
                return Err(format!(
                    "effort is not available for {} yet: its thinking setting differs by \
                     model; set it with the provider settings",
                    choice.provider
                ));
            }
        };
        merge(&mut request.body, &fields, "").map_err(|path| {
            format!("effort would change `{path}`, which the playground sets itself")
        })?;
    }
    if settings.provider.contains_key("stream") {
        return Err(
            "the provider settings cannot set `stream`: chat reads whole answers".to_owned(),
        );
    }
    if choice.kind == ProviderKind::Anthropic && settings.provider.contains_key("thinking") {
        return Err(
            "the provider settings cannot turn on Claude's `thinking` yet: it has to be \
             handed back with each tool result, which chat does not do yet"
                .to_owned(),
        );
    }
    merge(
        &mut request.body,
        &Value::Object(settings.provider.clone()),
        "",
    )
    .map_err(|path| {
        let set_by = if settings.effort.is_some() {
            " or effort"
        } else {
            ""
        };
        format!("the provider settings set `{path}`, which the playground{set_by} sets itself")
    })?;
    Ok(request)
}

/// Adds `fields` to `body`, object into object; the path of the first field `body`
/// already holds otherwise.
fn merge(body: &mut Value, fields: &Value, path: &str) -> Result<(), String> {
    let (Some(body), Some(fields)) = (body.as_object_mut(), fields.as_object()) else {
        return Err(path.to_owned());
    };
    for (name, value) in fields {
        let at = if path.is_empty() {
            name.clone()
        } else {
            format!("{path}.{name}")
        };
        match body.get_mut(name) {
            None => {
                body.insert(name.clone(), value.clone());
            }
            Some(held) if held.is_object() && value.is_object() => merge(held, value, &at)?,
            Some(_) => return Err(at),
        }
    }
    Ok(())
}

/// Where a chat runs: the endpoint, its token, the binding, and the log its turns go to.
pub(crate) struct Setup<'a> {
    pub(crate) choice: Choice,
    pub(crate) server_port: u16,
    pub(crate) blueprint: &'a str,
    /// The token's name labels the runs; its value authorizes them.
    pub(crate) label: &'a str,
    pub(crate) token: &'a str,
    pub(crate) variables: &'a BTreeMap<String, String>,
    pub(crate) secrets: &'a BTreeMap<String, String>,
    pub(crate) recorder: &'a Recorder,
    /// The most tool calls run for one answer.
    pub(crate) max_calls_per_answer: usize,
    /// Set when whoever asked is gone: the chat is cancelled before its next model
    /// request or tool call.
    pub(crate) cancelled: &'a AtomicBool,
    /// The session this message continues, when it is not the first.
    pub(crate) resume: Option<Resume>,
    /// Told a new chat's session id as soon as the session starts.
    pub(crate) started: &'a dyn Fn(&str),
    /// How the model is asked: the session's own when it is carried on.
    pub(crate) settings: &'a Settings,
}

/// An earlier chat to carry on: its session, and what was said in it.
pub(crate) struct Resume {
    pub(crate) session: String,
    /// The conversation so far, as [`history`] rebuilds it.
    pub(crate) history: Vec<Turn>,
    /// How many messages the session's log holds, so new ones are numbered after them.
    pub(crate) said: usize,
}

/// One entry of a session's chat log, as [`history`] reads it.
pub(crate) enum Logged<'m> {
    /// A message.
    Said(ChatRole, &'m str),
    /// The tools one answer called, and what they returned.
    Tools(&'m ChatTools),
}

/// The conversation a session's chat log holds, oldest first, as the model is shown it:
/// what was said, and the tools each answer called with what they returned, so a later
/// message is answered knowing what the earlier ones ran. Consecutive messages from the
/// same side are joined, so the sides alternate. A session logged before its tool calls
/// were has only what was said.
pub(crate) fn history<'m>(logged: impl IntoIterator<Item = Logged<'m>>) -> Vec<Turn> {
    let mut turns: Vec<Turn> = Vec::new();
    for entry in logged {
        match entry {
            Logged::Said(role, text) => said(&mut turns, role, text),
            Logged::Tools(tools) => {
                let calls = tools.calls.iter().map(kept_call).collect();
                // The calls belong to the answer's words, when it said any first.
                match turns.last_mut() {
                    Some(Turn::Assistant { calls: held, .. }) if held.is_empty() => {
                        *held = calls;
                    }
                    _ => turns.push(Turn::Assistant { text: None, calls }),
                }
                turns.push(Turn::Results(
                    tools.results.iter().map(kept_result).collect(),
                ));
            }
        }
    }
    // A model is asked with the user's turn first.
    while turns
        .first()
        .is_some_and(|turn| !matches!(turn, Turn::User(_)))
    {
        turns.remove(0);
    }
    turns
}

fn said(turns: &mut Vec<Turn>, role: ChatRole, text: &str) {
    match (role, turns.last_mut()) {
        (ChatRole::User, Some(Turn::User(said))) => {
            said.push_str("\n\n");
            said.push_str(text);
        }
        (
            ChatRole::Assistant,
            Some(Turn::Assistant {
                text: Some(said),
                calls,
            }),
        ) if calls.is_empty() => {
            said.push_str("\n\n");
            said.push_str(text);
        }
        (ChatRole::User, _) => turns.push(Turn::User(text.to_owned())),
        (ChatRole::Assistant, _) => turns.push(Turn::Assistant {
            text: Some(text.to_owned()),
            calls: Vec::new(),
        }),
    }
}

/// A logged call as the model is handed it back.
fn kept_call(call: &ChatToolCall) -> ToolCall {
    ToolCall {
        id: call.id.clone(),
        name: call.name.clone(),
        arguments: call
            .arguments
            .clone()
            .ok_or_else(|| call.arguments_text.clone().unwrap_or_default()),
        signature: call.signature.clone(),
    }
}

/// A logged result as the model is handed it back, saying where it was cut.
fn kept_result(result: &ChatToolResult) -> ToolResult {
    let mut content = result.content.clone();
    if result.truncated {
        content.push_str("\n[cut here: the rest was not kept]");
    }
    ToolResult {
        call_id: result.call_id.clone(),
        name: result.name.clone(),
        content,
        is_error: result.is_error,
    }
}

/// [`history`] with `task` said after it: joined to the last turn when that is the
/// user's own, which an answer never followed.
fn with_task(mut turns: Vec<Turn>, task: &str) -> Vec<Turn> {
    if let Some(Turn::User(said)) = turns.last_mut() {
        said.push_str("\n\n");
        said.push_str(task);
    } else {
        turns.push(Turn::User(task.to_owned()));
    }
    turns
}

/// Cancels a chat when dropped. The chat runs on a thread of its own; the request that
/// started it holds this, so when the request is dropped, as when the CLI that sent it
/// is stopped or the page is closed, the chat is cancelled too.
pub(crate) struct CancelOnDrop(pub(crate) Arc<AtomicBool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// How a chat ended.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct Outcome {
    pub(crate) session: Option<String>,
    pub(crate) provider: &'static str,
    pub(crate) model: String,
    /// The model's last words.
    pub(crate) reply: Option<String>,
    pub(crate) tool_calls: usize,
    pub(crate) stopped: Stopped,
    /// The provider's reason, when it stopped the answer.
    pub(crate) stop_reason: Option<String>,
}

/// Why a chat ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Stopped {
    /// The model answered without asking for a tool.
    Answered,
    /// It was stopped at [`MAX_TURNS`].
    TurnLimit,
    /// The model's answer stopped at its output cap.
    OutputLimit,
    /// The provider stopped the answer: a refusal, a safety filter.
    Provider,
}

/// Runs `task` to the model's answer or the turn limit. Blocking. Errors are the
/// playground's or the provider's own words, with the key never in them. The session
/// stays open, so a later message can carry the conversation on; the server ends it when
/// it sits idle past the blueprint's idle timeout, or `session end` does.
pub(crate) fn run(setup: &Setup<'_>, task: &str) -> Result<Outcome, String> {
    // Trimmed as `choose` read it, so the header and the redaction use the same text.
    let key = std::env::var(setup.choice.key_variable)
        .map(|key| key.trim().to_owned())
        .unwrap_or_default();
    let clean = |message: String| {
        if key.is_empty() {
            message
        } else {
            message.replace(&key, "[redacted]")
        }
    };
    let http = McpHttp::new(
        setup.server_port,
        setup.blueprint,
        setup.token,
        setup.secrets,
    );
    if let Some(resume) = &setup.resume {
        http.resume(&resume.session, PROTOCOL_VERSION);
    }
    let mcp = McpConversation {
        http: &http,
        next_id: Cell::new(1),
    };
    converse(setup, &mcp, &key, task).map_err(clean)
}

fn converse(
    setup: &Setup<'_>,
    mcp: &McpConversation<'_>,
    key: &str,
    task: &str,
) -> Result<Outcome, String> {
    let tools = if setup.resume.is_some() {
        list_tools(mcp)?
    } else {
        let tools = connect(mcp, setup.variables);
        if let Some(session) = mcp.http.session() {
            (setup.started)(&session);
        }
        tools?
    };
    let session = mcp.http.session();
    let (history, earlier) = setup.resume.as_ref().map_or((Vec::new(), 0), |resume| {
        (resume.history.clone(), resume.said)
    });
    let log = Log {
        recorder: setup.recorder,
        session: session.as_deref(),
        agent: setup.label,
        model: &setup.choice.model,
        earlier,
    };
    log.say(0, ChatRole::User, task, false);
    let mut turns = with_task(history, task);
    let mut tool_calls = 0;
    let mut said = 0;
    let mut last_text = None;
    for turn in 1..=MAX_TURNS {
        stop_if_cancelled(setup)?;
        let Reply {
            text,
            calls,
            finish,
        } = ask(&setup.choice, key, &turns, &tools, setup.settings)?;
        if let Some(text) = &text {
            said += 1;
            log.say(
                said,
                ChatRole::Assistant,
                text,
                finish == Finish::OutputLimit,
            );
        }
        if calls.is_empty() {
            let (stopped, stop_reason) = match finish {
                Finish::Complete => (Stopped::Answered, None),
                Finish::OutputLimit => (Stopped::OutputLimit, None),
                Finish::Other(reason) => (Stopped::Provider, Some(reason)),
            };
            let mut ended = outcome(setup, session, text, tool_calls, stopped);
            ended.stop_reason = stop_reason;
            return Ok(ended);
        }
        // No model would see what the last turn's calls returned, so they are not run.
        if turn == MAX_TURNS {
            last_text = text;
            break;
        }
        let (results, ran) = run_calls(setup, mcp, &calls, &finish);
        tool_calls += ran;
        log.tools(turn, &calls, &results);
        turns.push(Turn::Assistant { text, calls });
        turns.push(Turn::Results(results));
    }
    Ok(outcome(
        setup,
        session,
        last_text,
        tool_calls,
        Stopped::TurnLimit,
    ))
}

/// Runs what one answer asked for: a result for every call, in order, and how many ran.
/// Once the chat is cancelled, calls past its limit per answer, calls without an
/// arguments object, and an answer's last call when the output cap cut it are refused to
/// the model.
fn run_calls(
    setup: &Setup<'_>,
    mcp: &McpConversation<'_>,
    calls: &[ToolCall],
    finish: &Finish,
) -> (Vec<ToolResult>, usize) {
    let mut ran = 0;
    let results = calls
        .iter()
        .enumerate()
        .map(|(n, call)| {
            if setup.cancelled.load(Ordering::Relaxed) {
                refused(call, "not run: the chat was cancelled".to_owned())
            } else if n >= setup.max_calls_per_answer {
                refused(
                    call,
                    format!(
                        "not run: at most {} tool calls per answer",
                        setup.max_calls_per_answer
                    ),
                )
            } else if call.arguments.is_err() {
                refused(
                    call,
                    "not run: the arguments are not a JSON object".to_owned(),
                )
            } else if *finish == Finish::OutputLimit && n + 1 == calls.len() {
                refused(
                    call,
                    "not run: the answer was cut at the output limit, so this call may be \
                     incomplete"
                        .to_owned(),
                )
            } else {
                ran += 1;
                call_tool(mcp, call)
            }
        })
        .collect();
    (results, ran)
}

fn stop_if_cancelled(setup: &Setup<'_>) -> Result<(), String> {
    if setup.cancelled.load(Ordering::Relaxed) {
        return Err("the chat was cancelled: whoever started it is gone".to_owned());
    }
    Ok(())
}

fn outcome(
    setup: &Setup<'_>,
    session: Option<String>,
    reply: Option<String>,
    tool_calls: usize,
    stopped: Stopped,
) -> Outcome {
    Outcome {
        session,
        provider: setup.choice.provider,
        model: setup.choice.model.clone(),
        reply,
        tool_calls,
        stopped,
        stop_reason: None,
    }
}

/// The chat's MCP session, numbering its requests.
struct McpConversation<'a> {
    http: &'a McpHttp,
    next_id: Cell<u64>,
}

impl McpConversation<'_> {
    /// One JSON-RPC request's result, or its error's message.
    fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        let mut answer = None;
        self.http
            .send(
                &json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
                |reply| {
                    if reply["id"] == id {
                        answer = Some(reply);
                    }
                },
            )
            .map_err(chat_failure)?;
        let answer = answer.ok_or_else(|| format!("the playground did not answer `{method}`"))?;
        if let Some(message) = answer["error"]["message"].as_str() {
            return Err(format!("the playground refused `{method}`: {message}"));
        }
        Ok(answer["result"].clone())
    }

    fn notify(&self, method: &str) -> Result<(), String> {
        self.http
            .send(&json!({ "jsonrpc": "2.0", "method": method }), |_| {})
            .map_err(chat_failure)
    }
}

/// Why a message got no reply, in chat's terms.
fn chat_failure(failure: Failure) -> String {
    match failure {
        Failure::SessionGone => "the session has ended (it sat idle past its idle timeout, \
            `session end` ended it, or the playground restarted); start a new chat"
            .to_owned(),
        other => other.to_string(),
    }
}

/// Starts the MCP session under the binding and lists its tools.
fn connect(
    mcp: &McpConversation<'_>,
    variables: &BTreeMap<String, String>,
) -> Result<Vec<ToolSpec>, String> {
    mcp.request(
        "initialize",
        json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": { "name": "submilli-playground-chat", "version": env!("CARGO_PKG_VERSION") },
            "_meta": { "variables": variables },
        }),
    )?;
    mcp.notify("notifications/initialized")?;
    list_tools(mcp)
}

/// The tools the session offers.
fn list_tools(mcp: &McpConversation<'_>) -> Result<Vec<ToolSpec>, String> {
    let listed = mcp.request("tools/list", json!({}))?;
    Ok(listed["tools"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|tool| {
            Some(ToolSpec {
                name: tool["name"].as_str()?.to_owned(),
                description: tool["description"].as_str().unwrap_or_default().to_owned(),
                input_schema: tool["inputSchema"].clone(),
            })
        })
        .collect())
}

/// Calls one tool; a refusal or a failure is the result the model sees.
fn call_tool(mcp: &McpConversation<'_>, call: &ToolCall) -> ToolResult {
    let arguments = call.arguments.clone().unwrap_or_default();
    let (content, is_error) = match mcp.request(
        "tools/call",
        json!({ "name": call.name, "arguments": arguments }),
    ) {
        Ok(result) => {
            let text: Vec<&str> = result["content"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .filter_map(|part| part["text"].as_str())
                .collect();
            (text.join("\n"), result["isError"] == true)
        }
        Err(message) => (message, true),
    };
    let (kept, cut) = super::within_bytes(&content, MAX_RESULT_BYTES);
    ToolResult {
        call_id: call.id.clone(),
        name: call.name.clone(),
        content: if cut {
            format!("{kept}\n[cut]")
        } else {
            content
        },
        is_error,
    }
}

/// The error result for a call that was not run.
fn refused(call: &ToolCall, why: String) -> ToolResult {
    ToolResult {
        call_id: call.id.clone(),
        name: call.name.clone(),
        content: why,
        is_error: true,
    }
}

/// One model request.
fn ask(
    choice: &Choice,
    key: &str,
    turns: &[Turn],
    tools: &[ToolSpec],
    settings: &Settings,
) -> Result<Reply, String> {
    let request = request(choice, key, turns, tools, settings)?;
    // No redirects: a redirect would carry the key's header to wherever it points.
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(PROVIDER_TIMEOUT))
        .build()
        .into();
    let mut builder = ureq::http::Request::builder()
        .method("POST")
        .uri(&request.url)
        .header("content-type", "application/json");
    for (name, value) in &request.headers {
        builder = builder.header(name, value);
    }
    let http = builder
        .body(request.body.to_string())
        .map_err(|error| error.to_string())?;
    let provider = choice.provider;
    let mut response = agent
        .run(http)
        .map_err(|error| format!("calling {provider} failed: {error}"))?;
    let status = response.status();
    let body = response
        .body_mut()
        .with_config()
        .limit(16 << 20)
        .read_to_string()
        .map_err(|error| format!("reading {provider}'s answer failed: {error}"))?;
    if !status.is_success() {
        return Err(format!(
            "{provider} refused the request (HTTP {}): {}",
            status.as_u16(),
            error_message(&body, key)
        ));
    }
    parse_tool_response(choice.kind, &body).map_err(|message| format!("{provider}: {message}"))
}

/// The chat's turns, into its session's log.
struct Log<'a> {
    recorder: &'a Recorder,
    session: Option<&'a str>,
    agent: &'a str,
    /// The model answering, named on its messages.
    model: &'a str,
    /// How many messages the session held before this chat's, numbered after them.
    earlier: usize,
}

impl Log<'_> {
    /// Logs one message; `cut` when the model's output cap cut it.
    fn say(&self, n: usize, role: ChatRole, text: &str, cut: bool) {
        let Some(session) = self.session else {
            return;
        };
        let turn = format!("{}-{}", self.agent, self.earlier + n);
        let mut said = message(&turn, role, text);
        said.truncated |= cut;
        if role == ChatRole::Assistant {
            said.model = Some(self.model.to_owned());
        }
        self.recorder.append_chat(
            session,
            format!("chat-{session}-{turn}"),
            Position::at(super::now_micros()),
            chat_body(self.agent, said),
        );
    }

    /// Logs the tools answer `turn` called and what they returned, each capped as the
    /// model was handed it.
    fn tools(&self, turn: usize, calls: &[ToolCall], results: &[ToolResult]) {
        let Some(session) = self.session else {
            return;
        };
        let id = format!("{}-{}-tools-{turn}", self.agent, self.earlier);
        let calls = calls
            .iter()
            .map(|call| {
                let (arguments, arguments_text) = match &call.arguments {
                    Ok(arguments)
                        if serde_json::to_vec(arguments)
                            .is_ok_and(|bytes| bytes.len() <= MAX_RESULT_BYTES) =>
                    {
                        (Some(arguments.clone()), None)
                    }
                    Ok(arguments) => (None, Some(Value::Object(arguments.clone()).to_string())),
                    Err(text) => (None, Some(text.clone())),
                };
                ChatToolCall {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    arguments,
                    arguments_text: arguments_text
                        .map(|text| super::within_bytes(&text, MAX_RESULT_BYTES).0.to_owned()),
                    signature: call.signature.clone(),
                }
            })
            .collect();
        let results = results
            .iter()
            .map(|result| {
                let (content, truncated) = super::within_bytes(&result.content, MAX_RESULT_BYTES);
                ChatToolResult {
                    call_id: result.call_id.clone(),
                    name: result.name.clone(),
                    content: content.to_owned(),
                    truncated,
                    is_error: result.is_error,
                }
            })
            .collect();
        self.recorder.append_chat(
            session,
            format!("chat-{session}-{id}"),
            Position::at(super::now_micros()),
            EventBody::ChatTools(ChatTools {
                turn: id,
                agent: self.agent.to_owned(),
                calls,
                results,
            }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
        }
    }

    #[test]
    fn the_model_variable_picks_the_provider_and_model() {
        let chosen = choose(env(&[
            ("ANTHROPIC_API_KEY", "a"),
            ("GEMINI_API_KEY", "g"),
            (MODEL_VARIABLE, "google:gemini-3.8-flash"),
        ]))
        .unwrap();
        assert_eq!(chosen.provider, "google");
        assert_eq!(chosen.model, "gemini-3.8-flash");
        assert_eq!(chosen.key_variable, "GEMINI_API_KEY");
    }

    #[test]
    fn without_it_the_first_key_set_picks_with_its_default_model() {
        let chosen = choose(env(&[("OPENAI_API_KEY", "o"), ("GOOGLE_API_KEY", "g")])).unwrap();
        assert_eq!(chosen.provider, "openai");
        assert_eq!(chosen.model, "gpt-5.5");
        assert_eq!(chosen.base_url, "https://api.openai.com");
        let chosen = choose(env(&[("GOOGLE_API_KEY", "g")])).unwrap();
        assert_eq!(chosen.key_variable, "GOOGLE_API_KEY");
    }

    #[test]
    fn no_key_or_a_bad_model_variable_says_what_to_set() {
        let none = choose(env(&[])).unwrap_err();
        assert!(none.starts_with("no provider configured"), "{none}");
        let unkeyed = choose(env(&[
            ("OPENAI_API_KEY", "o"),
            (MODEL_VARIABLE, "anthropic:m"),
        ]))
        .unwrap_err();
        assert!(
            unkeyed.contains("ANTHROPIC_API_KEY is not set"),
            "{unkeyed}"
        );
        for bad in ["claude", "mistral:large", "openai:"] {
            let refused = choose(env(&[("OPENAI_API_KEY", "o"), (MODEL_VARIABLE, bad)]));
            assert!(refused.is_err(), "{bad}");
        }
    }

    #[test]
    fn a_base_url_override_is_taken_with_or_without_its_version() {
        let chosen = choose(env(&[
            ("OPENAI_API_KEY", "o"),
            ("OPENAI_BASE_URL", "http://127.0.0.1:9/v1/"),
        ]))
        .unwrap();
        assert_eq!(chosen.base_url, "http://127.0.0.1:9");
        let chosen = choose(env(&[
            ("GEMINI_API_KEY", "g"),
            (
                "GOOGLE_GEMINI_BASE_URL",
                "https://generativelanguage.googleapis.com/v1beta",
            ),
        ]))
        .unwrap();
        assert_eq!(chosen.base_url, "https://generativelanguage.googleapis.com");
    }

    #[test]
    fn a_named_model_needs_its_providers_key_and_says_where_it_came_from() {
        let keys = env(&[("OPENAI_API_KEY", "o"), ("ANTHROPIC_API_KEY", "a")]);
        let chosen = choose_named(&keys, "anthropic:claude-haiku-5-5", "--model").unwrap();
        assert_eq!(chosen.named(), "anthropic:claude-haiku-5-5");
        let unkeyed = choose_named(&keys, "google:gemini-flash-latest", "--model").unwrap_err();
        assert!(
            unkeyed
                .contains("--model names google, but GEMINI_API_KEY or GOOGLE_API_KEY is not set"),
            "{unkeyed}"
        );
        let bad = choose_named(&keys, "gpt-5.5", "--model").unwrap_err();
        assert!(bad.starts_with("--model is `gpt-5.5`"), "{bad}");
        // Only a plain name goes into the provider's request.
        for odd in ["openai:../../v1/files", "openai:gpt 5", "openai:gpt?x=1"] {
            assert!(choose_named(&keys, odd, "--model").is_err(), "{odd}");
        }
        let google = env(&[("GEMINI_API_KEY", "g")]);
        assert!(choose_named(&google, "google:models/gemini-flash-latest", "--model").is_ok());
    }

    #[test]
    fn the_models_offered_are_the_configured_one_then_one_per_other_keyed_provider() {
        assert_eq!(
            offered(env(&[
                ("OPENAI_API_KEY", "o"),
                ("ANTHROPIC_API_KEY", "a"),
                (MODEL_VARIABLE, "openai:gpt-6-luna"),
            ])),
            ["openai:gpt-6-luna", "anthropic:claude-sonnet-5-5"]
        );
        assert_eq!(
            offered(env(&[("GEMINI_API_KEY", "g")])),
            ["google:gemini-flash-latest"]
        );
        assert!(offered(env(&[])).is_empty());
    }

    #[test]
    fn the_history_alternates_starts_with_the_user_and_takes_the_next_message() {
        let turns = history([
            Logged::Said(ChatRole::Assistant, "stray"),
            Logged::Said(ChatRole::User, "Total cus_acme's charges."),
            Logged::Said(ChatRole::Assistant, "Running it."),
            Logged::Said(ChatRole::Assistant, "That was refused."),
            Logged::Said(ChatRole::User, "Why?"),
        ]);
        assert_eq!(
            turns,
            [
                Turn::User("Total cus_acme's charges.".into()),
                Turn::Assistant {
                    text: Some("Running it.\n\nThat was refused.".into()),
                    calls: Vec::new(),
                },
                Turn::User("Why?".into()),
            ]
        );
        // The last message went unanswered, so the next is said with it.
        let next = with_task(turns, "Then last month's only.");
        assert_eq!(next.len(), 3);
        assert_eq!(
            next[2],
            Turn::User("Why?\n\nThen last month's only.".into())
        );
        let answered = with_task(
            history([
                Logged::Said(ChatRole::User, "a"),
                Logged::Said(ChatRole::Assistant, "b"),
            ]),
            "c",
        );
        assert_eq!(answered.last(), Some(&Turn::User("c".into())));
    }

    fn tools(calls: &[(&str, Option<Value>)], results: &[(&str, &str, bool)]) -> ChatTools {
        ChatTools {
            turn: "chat-0-tools-1".into(),
            agent: "chat".into(),
            calls: calls
                .iter()
                .map(|(id, arguments)| ChatToolCall {
                    id: (*id).into(),
                    name: "execute".into(),
                    arguments: arguments.as_ref().and_then(|a| a.as_object().cloned()),
                    arguments_text: arguments.is_none().then(|| "not json".into()),
                    signature: None,
                })
                .collect(),
            results: results
                .iter()
                .map(|(id, content, truncated)| ChatToolResult {
                    call_id: (*id).into(),
                    name: "execute".into(),
                    content: (*content).into(),
                    truncated: *truncated,
                    is_error: false,
                })
                .collect(),
        }
    }

    #[test]
    fn the_history_hands_back_each_answers_tool_calls_and_what_they_returned() {
        let ran = tools(
            &[
                ("call_1", Some(json!({ "code": "main()" }))),
                ("call_2", None),
            ],
            &[("call_1", "{\"total\":5}", false), ("call_2", "abc", true)],
        );
        let again = tools(&[("call_3", Some(json!({})))], &[("call_3", "ok", false)]);
        let turns = history([
            Logged::Tools(&again),
            Logged::Said(ChatRole::User, "Total my charges."),
            Logged::Said(ChatRole::Assistant, "Running it."),
            Logged::Tools(&ran),
            Logged::Said(ChatRole::Assistant, "Your total is 5."),
            Logged::Tools(&again),
            Logged::Said(ChatRole::Assistant, "Checked again."),
        ]);
        let call = |id: &str, arguments: Result<Value, &str>| ToolCall {
            id: id.into(),
            name: "execute".into(),
            arguments: arguments
                .map(|a| a.as_object().cloned().unwrap())
                .map_err(str::to_owned),
            signature: None,
        };
        let result = |id: &str, content: &str| ToolResult {
            call_id: id.into(),
            name: "execute".into(),
            content: content.into(),
            is_error: false,
        };
        assert_eq!(
            turns,
            [
                // Tools before the first message have no question to answer, so they go.
                Turn::User("Total my charges.".into()),
                // The calls go with the words said before them.
                Turn::Assistant {
                    text: Some("Running it.".into()),
                    calls: vec![
                        call("call_1", Ok(json!({ "code": "main()" }))),
                        call("call_2", Err("not json")),
                    ],
                },
                Turn::Results(vec![
                    result("call_1", "{\"total\":5}"),
                    result("call_2", "abc\n[cut here: the rest was not kept]"),
                ]),
                // Words after the results start a turn of their own.
                Turn::Assistant {
                    text: Some("Your total is 5.".into()),
                    calls: vec![call("call_3", Ok(json!({})))],
                },
                Turn::Results(vec![result("call_3", "ok")]),
                Turn::Assistant {
                    text: Some("Checked again.".into()),
                    calls: Vec::new(),
                },
            ]
        );
    }

    fn settings(effort: Option<&str>, max_output: Option<u64>, provider: Value) -> Settings {
        Settings {
            effort: effort.map(str::to_owned),
            max_output,
            provider: provider.as_object().cloned().unwrap_or_default(),
        }
    }

    #[test]
    fn settings_go_into_the_request_and_never_over_what_the_playground_sets() {
        let keys = env(&[("OPENAI_API_KEY", "o"), ("ANTHROPIC_API_KEY", "a")]);
        let openai = choose_named(&keys, "openai:gpt-6-luna", "--model").unwrap();
        let asked = settings(
            Some("high"),
            Some(8000),
            json!({ "temperature": 0.2, "reasoning": { "summary": "auto" } }),
        );
        let body = request(&openai, "o", &[], &[], &asked).unwrap().body;
        assert_eq!(
            body["reasoning"],
            json!({ "effort": "high", "summary": "auto" })
        );
        assert_eq!(body["temperature"], 0.2);
        assert_eq!(body["max_output_tokens"], 8000);

        for (provider, refused) in [
            (json!({ "model": "other" }), "`model`"),
            (json!({ "input": [] }), "`input`"),
            (json!({ "stream": true }), "`stream`"),
            (
                json!({ "reasoning": { "effort": "low" } }),
                "`reasoning.effort`",
            ),
        ] {
            let error =
                check_settings(&openai, &settings(Some("high"), None, provider)).unwrap_err();
            assert!(error.contains(refused), "{error}");
        }
        let bad = check_settings(&openai, &settings(Some("max"), None, json!({}))).unwrap_err();
        assert!(bad.contains("low, medium, or high"), "{bad}");

        // Claude's thinking is not carried between tool calls yet, so neither way in is open.
        let claude = choose_named(&keys, "anthropic:claude-haiku-5-5", "--model").unwrap();
        let effort = check_settings(&claude, &settings(Some("low"), None, json!({}))).unwrap_err();
        assert!(effort.contains("not available for anthropic"), "{effort}");
        let thinking = check_settings(
            &claude,
            &settings(None, None, json!({ "thinking": { "type": "enabled" } })),
        )
        .unwrap_err();
        assert!(thinking.contains("`thinking`"), "{thinking}");
        let fine = request(
            &claude,
            "a",
            &[],
            &[],
            &settings(None, Some(1234), json!({ "top_k": 5 })),
        )
        .unwrap()
        .body;
        assert_eq!(
            (fine["max_tokens"].clone(), fine["top_k"].clone()),
            (json!(1234), json!(5))
        );
    }
}
