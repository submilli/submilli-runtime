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
    pub arguments: Value,
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
}

/// The request for the conversation's next turn.
#[allow(clippy::too_many_arguments)]
pub fn build_tool_request(
    kind: ProviderKind,
    base_url: &str,
    model: &str,
    system: &str,
    turns: &[Turn],
    tools: &[ToolSpec],
    max_tokens: u64,
    api_key: &str,
) -> WireRequest {
    let base = base_url.trim_end_matches('/');
    match kind {
        ProviderKind::Anthropic => {
            anthropic(base, model, system, turns, tools, max_tokens, api_key)
        }
        ProviderKind::Google => google(base, model, system, turns, tools, max_tokens, api_key),
        ProviderKind::OpenAi | ProviderKind::OpenAiCompatible => {
            openai(base, model, system, turns, tools, max_tokens, api_key)
        }
    }
}

/// The model's answer in a 2xx body, or why there is none.
pub fn parse_tool_response(kind: ProviderKind, body: &str) -> Result<Reply, String> {
    let value: Value =
        serde_json::from_str(body).map_err(|_| "the model's answer is not JSON".to_owned())?;
    match kind {
        ProviderKind::Anthropic => parse_anthropic(&value),
        ProviderKind::Google => parse_google(&value),
        ProviderKind::OpenAi | ProviderKind::OpenAiCompatible => parse_openai(&value),
    }
}

/// The provider's own message in an error body, or the body.
pub fn error_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| value.pointer("/error/message")?.as_str().map(str::to_owned))
        .unwrap_or_else(|| body.trim().to_owned())
}

// ---- Anthropic -----------------------------------------------------------------------------

fn anthropic(
    base: &str,
    model: &str,
    system: &str,
    turns: &[Turn],
    tools: &[ToolSpec],
    max_tokens: u64,
    api_key: &str,
) -> WireRequest {
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
                            "input": call.arguments })
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
                arguments: object_or_empty(&block["input"]),
            })
        })
        .collect();
    Ok(Reply { text, calls })
}

// ---- OpenAI --------------------------------------------------------------------------------

fn openai(
    base: &str,
    model: &str,
    system: &str,
    turns: &[Turn],
    tools: &[ToolSpec],
    max_tokens: u64,
    api_key: &str,
) -> WireRequest {
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
                                    "arguments": call.arguments.to_string(),
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
            "model": model, "max_completion_tokens": max_tokens,
            "messages": messages, "tools": tools,
        }),
    }
}

fn parse_openai(value: &Value) -> Result<Reply, String> {
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
            let arguments = function["arguments"]
                .as_str()
                .and_then(|text| serde_json::from_str(text).ok())
                .unwrap_or(Value::Null);
            Some(ToolCall {
                id: call["id"].as_str()?.to_owned(),
                name: function["name"].as_str()?.to_owned(),
                arguments: object_or_empty(&arguments),
            })
        })
        .collect();
    Ok(Reply { text, calls })
}

// ---- Google --------------------------------------------------------------------------------

fn google(
    base: &str,
    model: &str,
    system: &str,
    turns: &[Turn],
    tools: &[ToolSpec],
    max_tokens: u64,
    api_key: &str,
) -> WireRequest {
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
                parts.extend(calls.iter().map(
                    |call| json!({ "functionCall": { "name": call.name, "args": call.arguments } }),
                ));
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
    let parts = value["candidates"][0]["content"]["parts"]
        .as_array()
        .ok_or("the model's answer has no content")?;
    let text = joined(parts.iter().filter_map(|part| part["text"].as_str()));
    let calls = parts
        .iter()
        .filter_map(|part| part.get("functionCall"))
        .enumerate()
        .filter_map(|(n, call)| {
            Some(ToolCall {
                id: call["id"]
                    .as_str()
                    .map_or_else(|| format!("call-{n}"), str::to_owned),
                name: call["name"].as_str()?.to_owned(),
                arguments: object_or_empty(&call["args"]),
            })
        })
        .collect();
    Ok(Reply { text, calls })
}

// ---- shared --------------------------------------------------------------------------------

fn joined<'a>(texts: impl Iterator<Item = &'a str>) -> Option<String> {
    let texts: Vec<&str> = texts.filter(|text| !text.trim().is_empty()).collect();
    (!texts.is_empty()).then(|| texts.join("\n\n"))
}

/// A tool's arguments are an object; anything else is sent as none.
fn object_or_empty(value: &Value) -> Value {
    if value.is_object() {
        value.clone()
    } else {
        json!({})
    }
}

#[cfg(test)]
mod tests;
