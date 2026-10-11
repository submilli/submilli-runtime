//! Chat with the developer's own model key: a task run by a model from Anthropic, OpenAI,
//! or Google against the same tools a production agent sees, through the playground's
//! MCP endpoint. The CLI's `chat` and the page both start it here, in the playground, so
//! the key is the one the playground started with. Its runs are labeled by who started
//! it (`chat` from the CLI, `browser-chat` from the page), and its turns go to its
//! session's log as chat.

use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::{Value, json};
use submilli_shared::llm::ProviderKind;
use submilli_shared::llm::tools::{
    Reply, ToolCall, ToolResult, ToolSpec, Turn, build_tool_request, error_message,
    parse_tool_response,
};

use super::mcp_http::McpHttp;
use super::store::Recorder;
use super::store::events::{ChatRole, ChatTurn, EventBody, Position};

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
    let choice = |row: &ProviderRow, model: String, key_variable| {
        let base_url = set(row.base_url_variable).map_or_else(
            || row.default_base_url.to_owned(),
            |url| {
                let url = url.trim_end_matches('/');
                url.strip_suffix("/v1").unwrap_or(url).to_owned()
            },
        );
        Choice {
            provider: row.name,
            kind: row.kind,
            model,
            key_variable,
            base_url,
        }
    };
    let names = || {
        PROVIDERS
            .iter()
            .map(|row| row.name)
            .collect::<Vec<_>>()
            .join(", ")
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
                    names()
                )
            })?;
        let row = PROVIDERS
            .iter()
            .find(|row| row.name == provider)
            .ok_or_else(|| {
                format!(
                    "{MODEL_VARIABLE} names provider `{provider}`; use one of {}",
                    names()
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
    /// `answered`, or `turn-limit` when it was stopped at [`MAX_TURNS`].
    pub(crate) stopped: &'static str,
}

/// Runs `task` to the model's answer or the turn limit. Blocking. Errors are the
/// playground's or the provider's own words, with the key never in them.
pub(crate) fn run(setup: &Setup<'_>, task: &str) -> Result<Outcome, String> {
    let key = std::env::var(setup.choice.key_variable).unwrap_or_default();
    let clean = |message: String| {
        if key.is_empty() {
            message
        } else {
            message.replace(&key, "[redacted]")
        }
    };
    let mcp = McpHttp::new(
        setup.server_port,
        setup.blueprint,
        setup.token,
        setup.secrets,
    );
    let ran = converse(setup, &mcp, &key, task).map_err(clean);
    mcp.close();
    ran
}

fn converse(setup: &Setup<'_>, mcp: &McpHttp, key: &str, task: &str) -> Result<Outcome, String> {
    let tools = connect(mcp, setup.variables)?;
    let session = mcp.session();
    let log = Log {
        recorder: setup.recorder,
        session: session.as_deref(),
        agent: setup.label,
    };
    log.say(0, ChatRole::User, task);
    let mut turns = vec![Turn::User(task.to_owned())];
    let mut tool_calls = 0;
    let mut said = 0;
    for _ in 0..MAX_TURNS {
        let reply = ask(&setup.choice, key, &turns, &tools)?;
        if let Some(text) = &reply.text {
            said += 1;
            log.say(said, ChatRole::Assistant, text);
        }
        let Reply { text, calls } = reply;
        if calls.is_empty() {
            return Ok(outcome(setup, session, text, tool_calls, "answered"));
        }
        let results: Vec<ToolResult> = calls.iter().map(|call| call_tool(mcp, call)).collect();
        tool_calls += calls.len();
        turns.push(Turn::Assistant { text, calls });
        turns.push(Turn::Results(results));
    }
    Ok(outcome(setup, session, None, tool_calls, "turn-limit"))
}

fn outcome(
    setup: &Setup<'_>,
    session: Option<String>,
    reply: Option<String>,
    tool_calls: usize,
    stopped: &'static str,
) -> Outcome {
    Outcome {
        session,
        provider: setup.choice.provider,
        model: setup.choice.model.clone(),
        reply,
        tool_calls,
        stopped,
    }
}

/// Starts the MCP session under the binding and lists its tools.
fn connect(mcp: &McpHttp, variables: &BTreeMap<String, String>) -> Result<Vec<ToolSpec>, String> {
    rpc(
        mcp,
        1,
        "initialize",
        json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "submilli-playground-chat", "version": env!("CARGO_PKG_VERSION") },
            "_meta": { "variables": variables },
        }),
    )?;
    mcp.send(
        &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        |_| {},
    )
    .map_err(|failure| failure.to_string())?;
    let listed = rpc(mcp, 2, "tools/list", json!({}))?;
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

/// One JSON-RPC request's result, or its error's message.
fn rpc(mcp: &McpHttp, id: u64, method: &str, params: Value) -> Result<Value, String> {
    let mut answer = None;
    mcp.send(
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

/// Calls one tool; a refusal or a failure is the result the model sees.
fn call_tool(mcp: &McpHttp, call: &ToolCall) -> ToolResult {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(100);
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let (content, is_error) = match rpc(
        mcp,
        id,
        "tools/call",
        json!({ "name": call.name, "arguments": call.arguments }),
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
    ToolResult {
        call_id: call.id.clone(),
        name: call.name.clone(),
        content: capped(content),
        is_error,
    }
}

fn capped(mut text: String) -> String {
    if text.len() > MAX_RESULT_BYTES {
        let mut end = MAX_RESULT_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str("\n[cut]");
    }
    text
}

/// One model request.
fn ask(choice: &Choice, key: &str, turns: &[Turn], tools: &[ToolSpec]) -> Result<Reply, String> {
    let request = build_tool_request(
        choice.kind,
        &choice.base_url,
        &choice.model,
        SYSTEM,
        turns,
        tools,
        MAX_OUTPUT_TOKENS,
        key,
    );
    let agent: ureq::Agent = ureq::Agent::config_builder()
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
            error_message(&body)
        ));
    }
    parse_tool_response(choice.kind, &body).map_err(|message| format!("{provider}: {message}"))
}

/// The chat's turns, into its session's log.
struct Log<'a> {
    recorder: &'a Recorder,
    session: Option<&'a str>,
    agent: &'a str,
}

impl Log<'_> {
    fn say(&self, n: usize, role: ChatRole, text: &str) {
        let Some(session) = self.session else {
            return;
        };
        let (text, truncated) = capped_chat(text);
        let turn = format!("{}-{n}", self.agent);
        self.recorder.append_chat(
            session,
            format!("chat-{session}-{turn}"),
            Position {
                at_micros: super::now_micros(),
                run: None,
                call_index: None,
                rank: 0,
            },
            EventBody::Chat(ChatTurn {
                turn,
                role,
                text,
                truncated,
                agent: self.agent.to_owned(),
            }),
        );
    }
}

fn capped_chat(text: &str) -> (String, bool) {
    const MAX: usize = 16 * 1024;
    if text.len() <= MAX {
        return (text.to_owned(), false);
    }
    let mut end = MAX;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_owned(), true)
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
    fn a_base_url_override_is_taken_with_or_without_v1() {
        let chosen = choose(env(&[
            ("OPENAI_API_KEY", "o"),
            ("OPENAI_BASE_URL", "http://127.0.0.1:9/v1/"),
        ]))
        .unwrap();
        assert_eq!(chosen.base_url, "http://127.0.0.1:9");
    }
}
