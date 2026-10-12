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
                arguments: Ok(object(json!({ "code": "function main() {}" }))),
                signature: None,
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

fn object(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap()
}

fn request_for(kind: ProviderKind, base: &str, model: &str, turns: &[Turn]) -> WireRequest {
    build_tool_request(&ToolRequest {
        kind,
        base_url: base,
        model,
        system: "Be brief.",
        turns,
        tools: &tools(),
        max_tokens: 1024,
        api_key: KEY,
    })
}

fn request(kind: ProviderKind, base: &str, model: &str) -> WireRequest {
    request_for(kind, base, model, &turns())
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
    assert_eq!(request.url, "https://api.openai.com/v1/responses");
    key_only_in(&request, "authorization");
    assert_eq!(
        request.body,
        json!({
            "model": "m", "max_output_tokens": 1024, "instructions": "Be brief.",
            "input": [
                { "role": "user", "content": "Total the charges." },
                { "role": "assistant", "content": "Running it." },
                { "type": "function_call", "call_id": "call_1",
                  "name": "submilli__typescript__execute",
                  "arguments": "{\"code\":\"function main() {}\"}" },
                { "type": "function_call_output", "call_id": "call_1",
                  "output": "{\"result\":42}" },
            ],
            "tools": [{ "type": "function", "name": "submilli__typescript__execute",
                        "description": "Run a program.",
                        "parameters": tools()[0].input_schema, "strict": false }],
        })
    );
}

#[test]
fn an_openai_compatible_request_keeps_chat_completions() {
    let request = request(
        ProviderKind::OpenAiCompatible,
        "http://localhost:11434",
        "m",
    );
    assert_eq!(request.url, "http://localhost:11434/v1/chat/completions");
    key_only_in(&request, "authorization");
    assert_eq!(
        request.body,
        json!({
            "model": "m", "max_tokens": 1024,
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
        arguments: Ok(object(json!({ "code": "x" }))),
        signature: None,
    };
    let anthropic = json!({ "content": [
        { "type": "thinking", "thinking": "" },
        { "type": "text", "text": "Running it." },
        { "type": "tool_use", "id": "call_1", "name": "submilli__typescript__execute",
          "input": { "code": "x" } },
    ], "stop_reason": "tool_use" });
    let openai = json!({ "output": [
        { "type": "reasoning", "id": "rs_1", "summary": [] },
        { "type": "message", "role": "assistant", "content": [
            { "type": "output_text", "text": "Running it." } ] },
        { "type": "function_call", "id": "fc_1", "call_id": "call_1",
          "name": "submilli__typescript__execute", "arguments": "{\"code\":\"x\"}" },
    ]});
    let compatible = json!({ "choices": [{ "message": { "content": "Running it.", "tool_calls": [
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
        (ProviderKind::OpenAiCompatible, compatible),
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
            json!({ "output": [{ "type": "message", "role": "assistant",
                                 "content": [{ "type": "output_text", "text": "Done." }] }] }),
        ),
        (
            ProviderKind::OpenAiCompatible,
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
    assert_eq!(reply.calls[1].arguments, Ok(Map::new()));
}

#[test]
fn a_google_calls_thought_signature_goes_back_with_it() {
    let body = json!({ "candidates": [{ "content": { "parts": [
        { "functionCall": { "name": "submilli__typescript__execute", "args": { "code": "x" } },
          "thoughtSignature": "c2lnbmVk" },
    ]}}]});
    let reply = parse_tool_response(ProviderKind::Google, &body.to_string()).unwrap();
    assert_eq!(reply.calls[0].signature.as_deref(), Some("c2lnbmVk"));
    let turns = [
        Turn::User("Run it.".into()),
        Turn::Assistant {
            text: None,
            calls: reply.calls,
        },
    ];
    let request = request_for(
        ProviderKind::Google,
        "https://generativelanguage.googleapis.com",
        "m",
        &turns,
    );
    assert_eq!(
        request.body["contents"][1]["parts"][0],
        json!({ "functionCall": { "name": "submilli__typescript__execute",
                                  "args": { "code": "x" } },
                "thoughtSignature": "c2lnbmVk" })
    );
}

#[test]
fn arguments_that_are_not_an_object_are_kept_as_sent() {
    let cut = "{\"path\":\"src/sec";
    let body = json!({ "output": [
        { "type": "function_call", "call_id": "a", "name": "t", "arguments": cut },
        { "type": "function_call", "call_id": "b", "name": "t", "arguments": "[1]" },
    ]});
    let reply = parse_tool_response(ProviderKind::OpenAi, &body.to_string()).unwrap();
    assert_eq!(reply.calls[0].arguments, Err(cut.to_owned()));
    assert_eq!(reply.calls[1].arguments, Err("[1]".to_owned()));
    let anthropic = json!({ "content": [
        { "type": "tool_use", "id": "a", "name": "t", "input": "x" } ] });
    let reply = parse_tool_response(ProviderKind::Anthropic, &anthropic.to_string()).unwrap();
    assert_eq!(reply.calls[0].arguments, Err("\"x\"".to_owned()));
    // Sent back as they came to a provider that takes text, as none to one that takes
    // an object.
    let turns = [Turn::Assistant {
        text: None,
        calls: vec![ToolCall {
            id: "a".into(),
            name: "t".into(),
            arguments: Err(cut.to_owned()),
            signature: None,
        }],
    }];
    let openai = request_for(ProviderKind::OpenAi, "https://api.openai.com", "m", &turns);
    assert_eq!(openai.body["input"][0]["arguments"], json!(cut));
    let anthropic = request_for(
        ProviderKind::Anthropic,
        "https://api.anthropic.com",
        "m",
        &turns,
    );
    assert_eq!(
        anthropic.body["messages"][0]["content"][0]["input"],
        json!({})
    );
}

#[test]
fn a_malformed_answer_says_so_and_errors_give_the_providers_message() {
    assert!(parse_tool_response(ProviderKind::Anthropic, "nope").is_err());
    assert!(parse_tool_response(ProviderKind::OpenAi, "{}").is_err());
    assert!(parse_tool_response(ProviderKind::OpenAiCompatible, "{}").is_err());
    assert!(parse_tool_response(ProviderKind::Google, "{}").is_err());
    assert_eq!(
        error_message(r#"{"error":{"message":"invalid x-api-key"}}"#, ""),
        "invalid x-api-key"
    );
    assert_eq!(error_message("Bad gateway\n", ""), "Bad gateway");
    // An odd prefix puts a two-byte character across the cap.
    let page = format!("<html>!{}</html>", "é".repeat(4000));
    let quoted = error_message(&page, "");
    assert!(
        quoted.len() <= MAX_ERROR_BYTES + " [cut]".len(),
        "{}",
        quoted.len()
    );
    assert!(quoted.ends_with("é [cut]"), "{quoted}");
    let exact = "x".repeat(MAX_ERROR_BYTES);
    assert_eq!(error_message(&exact, ""), exact);
}

#[test]
fn an_echoed_key_is_replaced_before_the_error_is_cut() {
    // The key straddles the cap: cut first, its start would be left.
    let body = format!("{}{KEY} echoed", "x".repeat(MAX_ERROR_BYTES - 5));
    let quoted = error_message(&body, KEY);
    assert!(!quoted.contains(&KEY[..5]), "{quoted}");
    let json = json!({ "error": { "message": format!("bad key {KEY}") } }).to_string();
    assert_eq!(error_message(&json, KEY), "bad key [redacted]");
    // Quoted as JSON text by a proxy that escapes `/`.
    let key = "sk-a/b+c";
    let body = r#"{"detail":"bad key sk-a\/b+c"}"#;
    let quoted = error_message(body, key);
    assert!(!quoted.contains("sk-a"), "{quoted}");
}

#[test]
fn an_answer_cut_at_the_output_cap_says_so() {
    for (kind, body) in [
        (
            ProviderKind::Anthropic,
            json!({ "content": [{ "type": "text", "text": "Partial" }], "stop_reason": "max_tokens" }),
        ),
        (
            ProviderKind::OpenAi,
            json!({ "status": "incomplete",
                    "incomplete_details": { "reason": "max_output_tokens" },
                    "output": [{ "type": "reasoning", "id": "rs_1", "summary": [] }] }),
        ),
        (
            ProviderKind::OpenAiCompatible,
            json!({ "choices": [{ "message": { "content": "Partial" }, "finish_reason": "length" }] }),
        ),
        (
            ProviderKind::Google,
            json!({ "candidates": [{ "content": { "parts": [{ "text": "Partial" }] },
                                     "finishReason": "MAX_TOKENS" }] }),
        ),
    ] {
        let reply = parse_tool_response(kind, &body.to_string()).unwrap();
        assert_eq!(reply.finish, Finish::OutputLimit, "{kind:?}");
    }
    // A thinking model cut before it said anything sends no parts.
    let thought_only =
        json!({ "candidates": [{ "content": { "role": "model" }, "finishReason": "MAX_TOKENS" }] });
    let reply = parse_tool_response(ProviderKind::Google, &thought_only.to_string()).unwrap();
    assert_eq!(reply.finish, Finish::OutputLimit);
    assert_eq!((reply.text, reply.calls.len()), (None, 0));
    let done =
        json!({ "content": [{ "type": "text", "text": "Done." }], "stop_reason": "end_turn" });
    let reply = parse_tool_response(ProviderKind::Anthropic, &done.to_string()).unwrap();
    assert_eq!(reply.finish, Finish::Complete);
}

#[test]
fn an_answer_the_provider_stopped_says_why() {
    for (kind, body, why) in [
        (
            ProviderKind::Anthropic,
            json!({ "content": [], "stop_reason": "refusal" }),
            "refusal",
        ),
        (
            ProviderKind::OpenAi,
            json!({ "status": "incomplete",
                    "incomplete_details": { "reason": "content_filter" }, "output": [] }),
            "content_filter",
        ),
        (
            ProviderKind::OpenAiCompatible,
            json!({ "choices": [{ "message": { "content": null },
                                  "finish_reason": "content_filter" }] }),
            "content_filter",
        ),
        (
            ProviderKind::Google,
            json!({ "candidates": [{ "finishReason": "SAFETY" }] }),
            "SAFETY",
        ),
        (
            ProviderKind::Google,
            json!({ "promptFeedback": { "blockReason": "PROHIBITED_CONTENT" } }),
            "PROHIBITED_CONTENT",
        ),
    ] {
        let reply = parse_tool_response(kind, &body.to_string()).unwrap();
        assert_eq!(reply.finish, Finish::Other(why.to_owned()), "{kind:?}");
    }
}

#[test]
fn parallel_google_calls_keep_the_signature_on_the_first_only() {
    let body = json!({ "candidates": [{ "content": { "parts": [
        { "text": "Two at once." },
        { "functionCall": { "name": "a", "args": {} }, "thoughtSignature": "c2ln" },
        { "functionCall": { "name": "b", "args": { "x": 1 } } },
    ]}}]});
    let reply = parse_tool_response(ProviderKind::Google, &body.to_string()).unwrap();
    let signatures: Vec<Option<&str>> = reply
        .calls
        .iter()
        .map(|call| call.signature.as_deref())
        .collect();
    assert_eq!(signatures, [Some("c2ln"), None]);
    let turns = [Turn::Assistant {
        text: reply.text,
        calls: reply.calls,
    }];
    let request = request_for(
        ProviderKind::Google,
        "https://generativelanguage.googleapis.com",
        "m",
        &turns,
    );
    let parts = &request.body["contents"][0]["parts"];
    assert_eq!(parts[0], json!({ "text": "Two at once." }));
    assert_eq!(parts[1]["thoughtSignature"], "c2ln");
    assert!(parts[2].get("thoughtSignature").is_none(), "{parts}");
}

#[test]
fn blank_argument_text_is_no_arguments() {
    let body = json!({ "output": [
        { "type": "function_call", "call_id": "a", "name": "t", "arguments": "" } ] });
    let reply = parse_tool_response(ProviderKind::OpenAi, &body.to_string()).unwrap();
    assert_eq!(reply.calls[0].arguments, Ok(Map::new()));
}
