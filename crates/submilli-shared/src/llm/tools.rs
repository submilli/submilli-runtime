//! Tool calling for Anthropic, OpenAI, and Google, as pure functions: a conversation
//! and its tools become one request body, and a response body becomes the model's text
//! and the tool calls it asks for. Beside [`super::wire`], whose text-only path the
//! server uses, and unchanged by it. The HTTP round trip belongs to the caller.
//!
//! The key goes in a header only, as in [`super::wire`], never in the URL or the body.

use serde_json::{Map, Value, json};

use super::wire::{ProviderKind, WireRequest};

const ANTHROPIC_VERSION: &str = "2023-06-01";

/// A tool the model may call: an MCP tool's name, description, and input schema.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    /// The provider's id for the call, or one made up for a provider that gives none.
    pub id: String,
    pub name: String,
    /// The arguments object, or, when the model sent something else, that as JSON text.
    pub arguments: Result<Map<String, Value>, String>,
    /// Google's thought signature on the call: opaque, and handed back with it as sent.
    pub signature: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolResult {
    pub call_id: String,
    pub name: String,
    pub content: String,
    pub is_error: bool,
}

/// One turn of the conversation.
#[derive(Debug, Clone, PartialEq)]
pub enum Turn {
    User(String),
    Assistant {
        text: Option<String>,
        calls: Vec<ToolCall>,
    },
    /// What the tools the previous turn called returned.
    Results(Vec<ToolResult>),
}

/// What the model answered: what it said, and the tools it wants called. No calls means
/// it is done.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub text: Option<String>,
    pub calls: Vec<ToolCall>,
    pub finish: Finish,
}

/// How the model's answer ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finish {
    /// It finished, with its answer or the calls it wants.
    Complete,
    /// It stopped at the output cap: what it said, and its last call, may be cut short.
    OutputLimit,
    /// The provider stopped it for this reason: a refusal, a safety filter.
    Other(String),
}

/// What the conversation's next request is made of.
#[derive(Debug, Clone, Copy)]
pub struct ToolRequest<'a> {
    pub kind: ProviderKind,
    pub base_url: &'a str,
    pub model: &'a str,
    pub system: &'a str,
    pub turns: &'a [Turn],
    pub tools: &'a [ToolSpec],
    pub max_tokens: u64,
    pub api_key: &'a str,
}

/// The request for the conversation's next turn.
pub fn build_tool_request(request: &ToolRequest<'_>) -> WireRequest {
    let base = request.base_url.trim_end_matches('/');
    match request.kind {
        ProviderKind::Anthropic => anthropic(request, base),
        ProviderKind::Google => google(request, base),
        ProviderKind::OpenAi => openai(request, base),
        ProviderKind::OpenAiCompatible => chat_completions(request, base),
    }
}

/// The model's answer in a 2xx body, or why there is none.
pub fn parse_tool_response(kind: ProviderKind, body: &str) -> Result<Reply, String> {
    let value: Value =
        serde_json::from_str(body).map_err(|_| "the model's answer is not JSON".to_owned())?;
    match kind {
        ProviderKind::Anthropic => parse_anthropic(&value),
        ProviderKind::Google => parse_google(&value),
        ProviderKind::OpenAi => parse_openai(&value),
        ProviderKind::OpenAiCompatible => parse_chat_completions(&value),
    }
}

/// The most of an error body quoted, so a proxy's error page is not printed whole.
const MAX_ERROR_BYTES: usize = 2048;

/// The provider's own message in an error body, or the body, either cut at
/// [`MAX_ERROR_BYTES`]. The key is replaced first, so the cut cannot leave part of it.
pub fn error_message(body: &str, api_key: &str) -> String {
    let message = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| value.pointer("/error/message")?.as_str().map(str::to_owned))
        .unwrap_or_else(|| body.trim().to_owned());
    let message = if api_key.is_empty() {
        message
    } else {
        // A body quoted as JSON text may hold the key escaped, as `\/` for `/`.
        let escaped = serde_json::to_string(api_key).unwrap_or_default();
        let escaped = escaped.trim_matches('"').replace('/', "\\/");
        message
            .replace(api_key, "[redacted]")
            .replace(&escaped, "[redacted]")
    };
    if message.len() <= MAX_ERROR_BYTES {
        return message;
    }
    let mut end = MAX_ERROR_BYTES;
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    format!("{} [cut]", &message[..end])
}

// ---- Anthropic -----------------------------------------------------------------------------

fn anthropic(request: &ToolRequest<'_>, base: &str) -> WireRequest {
    let ToolRequest {
        model,
        system,
        turns,
        tools,
        max_tokens,
        api_key,
        ..
    } = *request;
    let messages: Vec<Value> = turns
        .iter()
        .map(|turn| match turn {
            Turn::User(text) => json!({ "role": "user", "content": text }),
            Turn::Assistant { text, calls } => {
                let mut content: Vec<Value> = text
                    .iter()
                    .map(|text| json!({ "type": "text", "text": text }))
                    .collect();
                content.extend(calls.iter().map(|call| {
                    json!({ "type": "tool_use", "id": call.id, "name": call.name,
                            "input": arguments_object(call) })
                }));
                json!({ "role": "assistant", "content": content })
            }
            Turn::Results(results) => json!({
                "role": "user",
                "content": results.iter().map(|result| json!({
                    "type": "tool_result", "tool_use_id": result.call_id,
                    "content": result.content, "is_error": result.is_error,
                })).collect::<Vec<_>>(),
            }),
        })
        .collect();
    let tools: Vec<Value> = tools
        .iter()
        .map(|tool| {
            json!({ "name": tool.name, "description": tool.description,
                    "input_schema": tool.input_schema })
        })
        .collect();
    WireRequest {
        url: format!("{base}/v1/messages"),
        headers: vec![
            ("anthropic-version".into(), ANTHROPIC_VERSION.into()),
            ("x-api-key".into(), api_key.into()),
        ],
        body: json!({
            "model": model, "max_tokens": max_tokens, "system": system,
            "messages": messages, "tools": tools,
        }),
    }
}

fn parse_anthropic(value: &Value) -> Result<Reply, String> {
    let blocks = value["content"]
        .as_array()
        .ok_or("the model's answer has no content")?;
    let text = joined(
        blocks
            .iter()
            .filter(|block| block["type"] == "text")
            .filter_map(|block| block["text"].as_str()),
    );
    let calls = blocks
        .iter()
        .filter(|block| block["type"] == "tool_use")
        .filter_map(|block| {
            Some(ToolCall {
                id: block["id"].as_str()?.to_owned(),
                name: block["name"].as_str()?.to_owned(),
                arguments: as_object(&block["input"]),
                signature: None,
            })
        })
        .collect();
    let finish = match value["stop_reason"].as_str() {
        None | Some("end_turn" | "tool_use" | "stop_sequence" | "pause_turn") => Finish::Complete,
        Some("max_tokens") => Finish::OutputLimit,
        Some(other) => Finish::Other(other.to_owned()),
    };
    Ok(Reply {
        text,
        calls,
        finish,
    })
}

// ---- OpenAI --------------------------------------------------------------------------------

// The Responses API: OpenAI's reasoning models refuse tools on chat completions.
fn openai(request: &ToolRequest<'_>, base: &str) -> WireRequest {
    let ToolRequest {
        model,
        system,
        turns,
        tools,
        max_tokens,
        api_key,
        ..
    } = *request;
    let mut input = Vec::new();
    for turn in turns {
        match turn {
            Turn::User(text) => input.push(json!({ "role": "user", "content": text })),
            Turn::Assistant { text, calls } => {
                input.extend(
                    text.iter()
                        .map(|text| json!({ "role": "assistant", "content": text })),
                );
                input.extend(calls.iter().map(|call| {
                    json!({ "type": "function_call", "call_id": call.id, "name": call.name,
                            "arguments": arguments_text(call) })
                }));
            }
            Turn::Results(results) => input.extend(results.iter().map(|result| {
                json!({ "type": "function_call_output", "call_id": result.call_id,
                        "output": result.content })
            })),
        }
    }
    // Strict mode would demand every property be required, which MCP schemas are not.
    let tools: Vec<Value> = tools
        .iter()
        .map(|tool| {
            json!({ "type": "function", "name": tool.name, "description": tool.description,
                    "parameters": tool.input_schema, "strict": false })
        })
        .collect();
    WireRequest {
        url: format!("{base}/v1/responses"),
        headers: vec![("authorization".into(), format!("Bearer {api_key}"))],
        body: json!({
            "model": model, "max_output_tokens": max_tokens, "instructions": system,
            "input": input, "tools": tools,
        }),
    }
}

fn parse_openai(value: &Value) -> Result<Reply, String> {
    let output = value["output"]
        .as_array()
        .ok_or("the model's answer has no output")?;
    let text = joined(
        output
            .iter()
            .filter(|item| item["type"] == "message")
            .filter_map(|item| item["content"].as_array())
            .flatten()
            .filter(|part| part["type"] == "output_text")
            .filter_map(|part| part["text"].as_str()),
    );
    let calls = output
        .iter()
        .filter(|item| item["type"] == "function_call")
        .filter_map(|item| {
            Some(ToolCall {
                id: item["call_id"].as_str()?.to_owned(),
                name: item["name"].as_str()?.to_owned(),
                arguments: parsed_arguments(&item["arguments"]),
                signature: None,
            })
        })
        .collect();
    let finish = if value["status"] == "incomplete" {
        match value["incomplete_details"]["reason"].as_str() {
            Some("max_output_tokens") => Finish::OutputLimit,
            reason => Finish::Other(reason.unwrap_or("incomplete").to_owned()),
        }
    } else {
        Finish::Complete
    };
    Ok(Reply {
        text,
        calls,
        finish,
    })
}

// ---- OpenAI-compatible ---------------------------------------------------------------------

// Chat completions, which compatible servers offer where they may lack the Responses API.
// The cap is `max_tokens`: most compatible servers ignore `max_completion_tokens`.
fn chat_completions(request: &ToolRequest<'_>, base: &str) -> WireRequest {
    let ToolRequest {
        model,
        system,
        turns,
        tools,
        max_tokens,
        api_key,
        ..
    } = *request;
    let mut messages = vec![json!({ "role": "system", "content": system })];
    for turn in turns {
        match turn {
            Turn::User(text) => messages.push(json!({ "role": "user", "content": text })),
            Turn::Assistant { text, calls } => {
                let mut message = Map::from_iter([
                    ("role".into(), json!("assistant")),
                    ("content".into(), json!(text)),
                ]);
                if !calls.is_empty() {
                    message.insert(
                        "tool_calls".into(),
                        calls
                            .iter()
                            .map(|call| {
                                json!({ "id": call.id, "type": "function", "function": {
                                    "name": call.name,
                                    "arguments": arguments_text(call),
                                }})
                            })
                            .collect(),
                    );
                }
                messages.push(Value::Object(message));
            }
            Turn::Results(results) => messages.extend(results.iter().map(|result| {
                json!({ "role": "tool", "tool_call_id": result.call_id,
                        "content": result.content })
            })),
        }
    }
    let tools: Vec<Value> = tools
        .iter()
        .map(|tool| {
            json!({ "type": "function", "function": {
                "name": tool.name, "description": tool.description,
                "parameters": tool.input_schema,
            }})
        })
        .collect();
    WireRequest {
        url: format!("{base}/v1/chat/completions"),
        headers: vec![("authorization".into(), format!("Bearer {api_key}"))],
        body: json!({
            "model": model, "max_tokens": max_tokens,
            "messages": messages, "tools": tools,
        }),
    }
}

fn parse_chat_completions(value: &Value) -> Result<Reply, String> {
    let message = &value["choices"][0]["message"];
    if !message.is_object() {
        return Err("the model's answer has no message".to_owned());
    }
    let text = message["content"]
        .as_str()
        .filter(|text| !text.trim().is_empty())
        .map(str::to_owned);
    let calls = message["tool_calls"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|call| {
            let function = &call["function"];
            Some(ToolCall {
                id: call["id"].as_str()?.to_owned(),
                name: function["name"].as_str()?.to_owned(),
                arguments: parsed_arguments(&function["arguments"]),
                signature: None,
            })
        })
        .collect();
    let finish = match value["choices"][0]["finish_reason"].as_str() {
        Some("length") => Finish::OutputLimit,
        Some("content_filter") => Finish::Other("content_filter".to_owned()),
        _ => Finish::Complete,
    };
    Ok(Reply {
        text,
        calls,
        finish,
    })
}

// ---- Google --------------------------------------------------------------------------------

fn google(request: &ToolRequest<'_>, base: &str) -> WireRequest {
    let ToolRequest {
        model,
        system,
        turns,
        tools,
        max_tokens,
        api_key,
        ..
    } = *request;
    let path_model = if model.starts_with("models/") {
        model.to_owned()
    } else {
        format!("models/{model}")
    };
    let contents: Vec<Value> = turns
        .iter()
        .map(|turn| match turn {
            Turn::User(text) => json!({ "role": "user", "parts": [{ "text": text }] }),
            Turn::Assistant { text, calls } => {
                let mut parts: Vec<Value> =
                    text.iter().map(|text| json!({ "text": text })).collect();
                parts.extend(calls.iter().map(|call| {
                    let mut part = json!({ "functionCall": {
                        "name": call.name, "args": arguments_object(call) } });
                    if let Some(signature) = &call.signature {
                        part["thoughtSignature"] = json!(signature);
                    }
                    part
                }));
                json!({ "role": "model", "parts": parts })
            }
            Turn::Results(results) => json!({
                "role": "user",
                "parts": results.iter().map(|result| json!({ "functionResponse": {
                    "name": result.name,
                    "response": { "content": result.content, "isError": result.is_error },
                }})).collect::<Vec<_>>(),
            }),
        })
        .collect();
    // `parametersJsonSchema` takes a JSON Schema as it is; `parameters` takes only
    // Google's subset and refuses a schema naming its `$schema`, as MCP tools' do.
    let declarations: Vec<Value> = tools
        .iter()
        .map(|tool| {
            json!({ "name": tool.name, "description": tool.description,
                    "parametersJsonSchema": tool.input_schema })
        })
        .collect();
    WireRequest {
        url: format!("{base}/v1beta/{path_model}:generateContent"),
        headers: vec![("x-goog-api-key".into(), api_key.into())],
        body: json!({
            "systemInstruction": { "parts": [{ "text": system }] },
            "contents": contents,
            "tools": [{ "functionDeclarations": declarations }],
            "generationConfig": { "maxOutputTokens": max_tokens },
        }),
    }
}

fn parse_google(value: &Value) -> Result<Reply, String> {
    let candidate = &value["candidates"][0];
    if !candidate.is_object() {
        // A prompt Google blocked has no candidate, only why.
        return match value["promptFeedback"]["blockReason"].as_str() {
            Some(reason) => Ok(Reply {
                text: None,
                calls: Vec::new(),
                finish: Finish::Other(reason.to_owned()),
            }),
            None => Err("the model's answer has no content".to_owned()),
        };
    }
    // An answer cut before it said anything, as a thinking model's can be, has no parts.
    let parts = candidate["content"]["parts"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let text = joined(parts.iter().filter_map(|part| part["text"].as_str()));
    // Gemini 3 refuses a conversation whose calls come back without their signatures.
    let calls = parts
        .iter()
        .filter_map(|part| Some((part.get("functionCall")?, part)))
        .enumerate()
        .filter_map(|(n, (call, part))| {
            Some(ToolCall {
                id: call["id"]
                    .as_str()
                    .map_or_else(|| format!("call-{n}"), str::to_owned),
                name: call["name"].as_str()?.to_owned(),
                arguments: match call.get("args") {
                    None => Ok(Map::new()),
                    Some(args) => as_object(args),
                },
                signature: part["thoughtSignature"].as_str().map(str::to_owned),
            })
        })
        .collect();
    let finish = match candidate["finishReason"].as_str() {
        None | Some("STOP" | "FINISH_REASON_UNSPECIFIED") => Finish::Complete,
        Some("MAX_TOKENS") => Finish::OutputLimit,
        Some(other) => Finish::Other(other.to_owned()),
    };
    Ok(Reply {
        text,
        calls,
        finish,
    })
}

// ---- shared --------------------------------------------------------------------------------

fn joined<'a>(texts: impl Iterator<Item = &'a str>) -> Option<String> {
    let texts: Vec<&str> = texts.filter(|text| !text.trim().is_empty()).collect();
    (!texts.is_empty()).then(|| texts.join("\n\n"))
}

/// The arguments object, or what came instead as JSON text.
fn as_object(value: &Value) -> Result<Map<String, Value>, String> {
    value.as_object().cloned().ok_or_else(|| value.to_string())
}

/// OpenAI sends a call's arguments as JSON text; text that is not an object is kept as
/// it came. Blank text is no arguments, as some compatible servers send it.
fn parsed_arguments(arguments: &Value) -> Result<Map<String, Value>, String> {
    let Some(text) = arguments.as_str() else {
        return as_object(arguments);
    };
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str(text) {
        Ok(Value::Object(object)) => Ok(object),
        _ => Err(text.to_owned()),
    }
}

/// A call's arguments for a provider that takes an object: none when they were not one.
fn arguments_object(call: &ToolCall) -> Value {
    Value::Object(call.arguments.clone().unwrap_or_default())
}

/// A call's arguments as the JSON text the model sent.
fn arguments_text(call: &ToolCall) -> String {
    match &call.arguments {
        Ok(object) => Value::Object(object.clone()).to_string(),
        Err(text) => text.clone(),
    }
}

#[cfg(test)]
mod tests;
