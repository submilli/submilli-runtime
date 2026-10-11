use serde_json::json;

use super::*;

const KEY: &str = "dummy-key-value";

fn tools() -> Vec<ToolSpec> {
    vec![ToolSpec {
        name: "submilli__typescript__execute".into(),
        description: "Run a program.".into(),
        input_schema: json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "properties": { "code": { "type": "string" } },
            "required": ["code"],
        }),
    }]
}

/// The task, a call, its result: the conversation a second request carries.
fn turns() -> Vec<Turn> {
    vec![
        Turn::User("Total the charges.".into()),
        Turn::Assistant {
            text: Some("Running it.".into()),
            calls: vec![ToolCall {
                id: "call_1".into(),
                name: "submilli__typescript__execute".into(),
                arguments: json!({ "code": "function main() {}" }),
            }],
        },
        Turn::Results(vec![ToolResult {
            call_id: "call_1".into(),
            name: "submilli__typescript__execute".into(),
            content: "{\"result\":42}".into(),
            is_error: false,
        }]),
    ]
}

fn request(kind: ProviderKind, base: &str, model: &str) -> WireRequest {
    build_tool_request(
        kind,
        base,
        model,
        "Be brief.",
        &turns(),
        &tools(),
        1024,
        KEY,
    )
}

/// The key is in a header, and nowhere in the URL or body.
fn key_only_in(request: &WireRequest, header: &str) {
    assert!(!request.url.contains(KEY));
    assert!(!request.body.to_string().contains(KEY));
    assert!(
        request
            .headers
            .iter()
            .any(|(name, value)| name == header && value.contains(KEY))
    );
}

#[test]
fn anthropic_request_carries_tools_calls_and_results() {
    let request = request(ProviderKind::Anthropic, "https://api.anthropic.com/", "m");
    assert_eq!(request.url, "https://api.anthropic.com/v1/messages");
    key_only_in(&request, "x-api-key");
    assert_eq!(
        request.body,
        json!({
            "model": "m", "max_tokens": 1024, "system": "Be brief.",
            "messages": [
                { "role": "user", "content": "Total the charges." },
                { "role": "assistant", "content": [
                    { "type": "text", "text": "Running it." },
                    { "type": "tool_use", "id": "call_1",
                      "name": "submilli__typescript__execute",
                      "input": { "code": "function main() {}" } },
                ]},
                { "role": "user", "content": [
                    { "type": "tool_result", "tool_use_id": "call_1",
                      "content": "{\"result\":42}", "is_error": false },
                ]},
            ],
            "tools": [{ "name": "submilli__typescript__execute",
                        "description": "Run a program.",
                        "input_schema": tools()[0].input_schema }],
        })
    );
}

#[test]
fn openai_request_carries_tools_calls_and_results() {
    let request = request(ProviderKind::OpenAi, "https://api.openai.com", "m");
    assert_eq!(request.url, "https://api.openai.com/v1/chat/completions");
    key_only_in(&request, "authorization");
    assert_eq!(
        request.body,
        json!({
            "model": "m", "max_completion_tokens": 1024,
            "messages": [
                { "role": "system", "content": "Be brief." },
                { "role": "user", "content": "Total the charges." },
                { "role": "assistant", "content": "Running it.", "tool_calls": [
                    { "id": "call_1", "type": "function", "function": {
                        "name": "submilli__typescript__execute",
                        "arguments": "{\"code\":\"function main() {}\"}" } },
                ]},
                { "role": "tool", "tool_call_id": "call_1", "content": "{\"result\":42}" },
            ],
            "tools": [{ "type": "function", "function": {
                "name": "submilli__typescript__execute",
                "description": "Run a program.",
                "parameters": tools()[0].input_schema } }],
        })
    );
}

#[test]
fn google_request_carries_tools_calls_and_results_with_the_schema_as_given() {
    let request = request(
        ProviderKind::Google,
        "https://generativelanguage.googleapis.com",
        "gemini-flash-latest",
    );
    assert_eq!(
        request.url,
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-flash-latest:generateContent"
    );
    key_only_in(&request, "x-goog-api-key");
    assert_eq!(
        request.body,
        json!({
            "systemInstruction": { "parts": [{ "text": "Be brief." }] },
            "contents": [
                { "role": "user", "parts": [{ "text": "Total the charges." }] },
                { "role": "model", "parts": [
                    { "text": "Running it." },
                    { "functionCall": { "name": "submilli__typescript__execute",
                                        "args": { "code": "function main() {}" } } },
                ]},
                { "role": "user", "parts": [{ "functionResponse": {
                    "name": "submilli__typescript__execute",
                    "response": { "content": "{\"result\":42}", "isError": false } } }] },
            ],
            "tools": [{ "functionDeclarations": [{
                "name": "submilli__typescript__execute",
                "description": "Run a program.",
                "parametersJsonSchema": tools()[0].input_schema }] }],
            "generationConfig": { "maxOutputTokens": 1024 },
        })
    );
}

#[test]
fn each_provider_answer_parses_into_text_and_calls() {
    let call = ToolCall {
        id: "call_1".into(),
        name: "submilli__typescript__execute".into(),
        arguments: json!({ "code": "x" }),
    };
    let anthropic = json!({ "content": [
        { "type": "thinking", "thinking": "" },
        { "type": "text", "text": "Running it." },
        { "type": "tool_use", "id": "call_1", "name": "submilli__typescript__execute",
          "input": { "code": "x" } },
    ], "stop_reason": "tool_use" });
    let openai = json!({ "choices": [{ "message": { "content": "Running it.", "tool_calls": [
        { "id": "call_1", "type": "function", "function": {
            "name": "submilli__typescript__execute", "arguments": "{\"code\":\"x\"}" } },
    ]}, "finish_reason": "tool_calls" }] });
    let google = json!({ "candidates": [{ "content": { "parts": [
        { "text": "Running it." },
        { "functionCall": { "id": "call_1", "name": "submilli__typescript__execute",
                            "args": { "code": "x" } } },
    ]}}]});
    for (kind, body) in [
        (ProviderKind::Anthropic, anthropic),
        (ProviderKind::OpenAi, openai),
        (ProviderKind::Google, google),
    ] {
        let reply = parse_tool_response(kind, &body.to_string()).unwrap();
        assert_eq!(reply.text.as_deref(), Some("Running it."), "{kind:?}");
        assert_eq!(reply.calls, std::slice::from_ref(&call), "{kind:?}");
    }
}

#[test]
fn a_text_only_answer_has_no_calls() {
    for (kind, body) in [
        (
            ProviderKind::Anthropic,
            json!({ "content": [{ "type": "text", "text": "Done." }] }),
        ),
        (
            ProviderKind::OpenAi,
            json!({ "choices": [{ "message": { "content": "Done." } }] }),
        ),
        (
            ProviderKind::Google,
            json!({ "candidates": [{ "content": { "parts": [{ "text": "Done." }] } }] }),
        ),
    ] {
        let reply = parse_tool_response(kind, &body.to_string()).unwrap();
        assert_eq!(reply.text.as_deref(), Some("Done."));
        assert!(reply.calls.is_empty());
    }
}

#[test]
fn google_calls_without_ids_get_distinct_ones() {
    let body = json!({ "candidates": [{ "content": { "parts": [
        { "functionCall": { "name": "a", "args": {} } },
        { "functionCall": { "name": "b" } },
    ]}}]});
    let reply = parse_tool_response(ProviderKind::Google, &body.to_string()).unwrap();
    let ids: Vec<&str> = reply.calls.iter().map(|call| call.id.as_str()).collect();
    assert_eq!(ids, ["call-0", "call-1"]);
    assert_eq!(reply.calls[1].arguments, json!({}));
}

#[test]
fn a_malformed_answer_says_so_and_errors_give_the_providers_message() {
    assert!(parse_tool_response(ProviderKind::Anthropic, "nope").is_err());
    assert!(parse_tool_response(ProviderKind::OpenAi, "{}").is_err());
    assert!(parse_tool_response(ProviderKind::Google, "{}").is_err());
    assert_eq!(
        error_message(r#"{"error":{"message":"invalid x-api-key"}}"#),
        "invalid x-api-key"
    );
    assert_eq!(error_message("Bad gateway\n"), "Bad gateway");
}
