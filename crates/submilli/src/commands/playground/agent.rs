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
use super::mcp_http::McpHttp;
use super::store::Recorder;
use super::store::events::{ChatRole, Position};
use super::{DEFAULT_MAX_CALLS_PER_ANSWER, MAX_CALLS_PER_ANSWER_RANGE};

/// The variable that picks the provider and model: `<provider>:<model>`.
pub(crate) const MODEL_VARIABLE: &str = "SUBMILLI_PLAYGROUND_MODEL";

/// How many times the model may be asked before the loop stops.
pub(crate) const MAX_TURNS: usize = 20;
/// How long one model request may take.
pub(crate) const PROVIDER_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_OUTPUT_TOKENS: u64 = 4096;
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
    let set = |name: &str| env(name).filter(|value| !value.trim().is_empty());
    let key_of = |row: &ProviderRow| row.keys.iter().copied().find(|key| set(key).is_some());
    let choice = |row: &'static ProviderRow, model: String, key_variable| Choice {
        provider: row.name,
        kind: row.kind,
        model,
        key_variable,
        base_url: base_url(row, set(row.base_url_variable)),
    };
    if let Some(named) = set(MODEL_VARIABLE) {
        let (provider, model) = named
            .split_once(':')
            .map(|(provider, model)| (provider.trim(), model.trim()))
            .filter(|(_, model)| !model.is_empty())
            .ok_or_else(|| {
                format!(
                    "{MODEL_VARIABLE} is `{named}`; set it to `<provider>:<model>`, the \
                     provider one of {}",
                    provider_names()
                )
            })?;
        let row = PROVIDERS
            .iter()
            .find(|row| row.name == provider)
            .ok_or_else(|| {
                format!(
                    "{MODEL_VARIABLE} names provider `{provider}`; use one of {}",
                    provider_names()
                )
            })?;
        let key = key_of(row).ok_or_else(|| {
            format!(
                "no provider configured: {MODEL_VARIABLE} names {}, but {} is not set",
                row.name,
                row.keys.join(" or ")
            )
        })?;
        return Ok(choice(row, model.to_owned(), key));
    }
    PROVIDERS
        .iter()
        .find_map(|row| key_of(row).map(|key| choice(row, row.default_model.to_owned(), key)))
        .ok_or_else(|| {
            "no provider configured: start the playground with ANTHROPIC_API_KEY, \
             OPENAI_API_KEY, or GEMINI_API_KEY set (and optionally \
             SUBMILLI_PLAYGROUND_MODEL=<provider>:<model>)"
                .to_owned()
        })
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
/// playground's or the provider's own words, with the key never in them.
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
    let mcp = McpConversation {
        http: &http,
        next_id: Cell::new(1),
    };
    let ran = converse(setup, &mcp, &key, task).map_err(clean);
    http.close();
    ran
}

fn converse(
    setup: &Setup<'_>,
    mcp: &McpConversation<'_>,
    key: &str,
    task: &str,
) -> Result<Outcome, String> {
    let tools = connect(mcp, setup.variables)?;
    let session = mcp.http.session();
    let log = Log {
        recorder: setup.recorder,
        session: session.as_deref(),
        agent: setup.label,
        model: &setup.choice.model,
    };
    log.say(0, ChatRole::User, task, false);
    let mut turns = vec![Turn::User(task.to_owned())];
    let mut tool_calls = 0;
    let mut said = 0;
    let mut last_text = None;
    for turn in 1..=MAX_TURNS {
        stop_if_cancelled(setup)?;
        let Reply {
            text,
            calls,
            finish,
        } = ask(&setup.choice, key, &turns, &tools)?;
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
            .map_err(|failure| failure.to_string())?;
        let answer = answer.ok_or_else(|| format!("the playground did not answer `{method}`"))?;
        if let Some(message) = answer["error"]["message"].as_str() {
            return Err(format!("the playground refused `{method}`: {message}"));
        }
        Ok(answer["result"].clone())
    }

    fn notify(&self, method: &str) -> Result<(), String> {
        self.http
            .send(&json!({ "jsonrpc": "2.0", "method": method }), |_| {})
            .map_err(|failure| failure.to_string())
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
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "submilli-playground-chat", "version": env!("CARGO_PKG_VERSION") },
            "_meta": { "variables": variables },
        }),
    )?;
    mcp.notify("notifications/initialized")?;
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
fn ask(choice: &Choice, key: &str, turns: &[Turn], tools: &[ToolSpec]) -> Result<Reply, String> {
    let request = build_tool_request(&ToolRequest {
        kind: choice.kind,
        base_url: &choice.base_url,
        model: &choice.model,
        system: SYSTEM,
        turns,
        tools,
        max_tokens: MAX_OUTPUT_TOKENS,
        api_key: key,
    });
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
}

impl Log<'_> {
    /// Logs one message; `cut` when the model's output cap cut it.
    fn say(&self, n: usize, role: ChatRole, text: &str, cut: bool) {
        let Some(session) = self.session else {
            return;
        };
        let turn = format!("{}-{n}", self.agent);
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
}
