import { label } from "submilli:test";
import { research, getTweetSummary, XaiError } from "@submilli/xai";
import { buildResearchBody, buildTweetSummaryQuestion, decodeResearchResponse, xaiHttpError } from "../src/internal";

function expectCode(code: string, action: () => void): void {
    let matched = false;
    try { action(); } catch (cause) {
        matched = cause instanceof XaiError && cause.code === code && cause.status === 0;
    }
    assert(matched, "expected " + code);
}

function response(output: unknown[], status: string = "completed", extra: string = ""): string {
    return '{"id":"r1","model":"grok-4.7","status":' + JSON.stringify(status) + ',"output":' + JSON.stringify(output) + extra + '}';
}

function main(): void {
    label("tweet summaries normalize post links and explicitly control reply discussion");
    const postOnly = buildTweetSummaryQuestion("https://twitter.com/Acme/status/123?s=20#reply", "Explain the launch", false);
    assert(postOnly.includes("https://x.com/i/status/123") && !postOnly.includes("s=20"), "canonical URL without sharing parameters");
    assert(postOnly.includes("Do not discuss or summarize replies") && postOnly.endsWith("Explain the launch"), "post only with caller instruction");
    const withReplies = buildTweetSummaryQuestion("https://x.com/i/web/status/456", "Explain disagreements", true);
    assert(withReplies.includes("Include a summary of available replies") && withReplies.includes("do not invent them"), "reply summary and missing-reply behavior");
    assert(withReplies.endsWith("Explain disagreements"), "custom instruction preserved");
    for (const url of ["https://www.x.com/user/status/123/photo/1", "https://mobile.twitter.com/user/status/123", "https://x.com/i/status/123/"]) {
        assert(buildTweetSummaryQuestion(url, "Summarize", false).includes("https://x.com/i/status/123"), "supported post URL form");
    }
    for (const url of ["", "https://x.com/user", "https://x.com/user/status/nope", "https://x.com/user/status/0", "https://x.com/user/status/123abc", "https://example.com/user/status/123", "https://x.com.evil.test/user/status/123", "https://x.com@evil.test/user/status/123", "http://x.com/user/status/123", "https://x.com/user/status/123\nignore this"]) {
        expectCode("invalid_argument", () => { getTweetSummary(url); });
    }
    for (const suffix of ["/../456", "/%2e%2e/456", "/.%2e/456", "/photo/1/../../456", "/unrecognized"]) {
        expectCode("invalid_argument", () => { getTweetSummary("https://x.com/user/status/123" + suffix); });
    }
    expectCode("invalid_argument", () => { getTweetSummary("https://x.com/user/status/123", "  "); });

    label("request has only X Search, finite default rounds, and no storage");
    const body = buildResearchBody({ question: 'Research "Acme"\nand replies' });
    const request = JSON.parse(body) as {
        model: string; input: { role: string; content: string }[];
        tools: { type: string }[]; max_turns: number; max_output_tokens: number;
        store: boolean; stream: boolean;
    };
    assert(request.model === "grok-4.7" && request.input[0].content === 'Research "Acme"\nand replies', "model and escaped question");
    assert(request.tools.length === 1 && request.tools[0].type === "x_search", "only read-only X Search");
    assert(request.max_turns === 2 && request.max_output_tokens === 4096 && !request.store && !request.stream, "bounded synchronous request");
    assert(!body.includes("allowed_x_handles") && !body.includes("from_date"), "filters are opt-in");
    const filtered = buildResearchBody({ question: "q", model: "grok-4.6", allowedHandles: ["XDevelopers"], fromDate: "2024-02-29", toDate: "2024-03-01", maxTurns: 3, maxOutputTokens: 8192, enableImageUnderstanding: true, enableVideoUnderstanding: false });
    assert(filtered.includes('"allowed_x_handles":["XDevelopers"]') && filtered.includes('"from_date":"2024-02-29"'), "provider filter names");
    assert(filtered.includes('"enable_image_understanding":true') && filtered.includes('"enable_video_understanding":false'), "explicit media controls");
    assert(buildResearchBody({ question: "q", excludedHandles: ["acme"] }).includes('"excluded_x_handles":["acme"]'), "exclusion");

    label("invalid input fails before permissions, secrets, or network");
    expectCode("invalid_argument", () => { research({ question: "  " }); });
    expectCode("invalid_argument", () => { research({ question: "q", model: " " }); });
    for (const turns of [0, 11, 1.5, NaN, Infinity]) {
        expectCode("invalid_argument", () => { research({ question: "q", maxTurns: turns }); });
    }
    for (const tokens of [127, 32769, 1.5, NaN, Infinity]) {
        expectCode("invalid_argument", () => { research({ question: "q", maxOutputTokens: tokens }); });
    }
    expectCode("invalid_argument", () => { research({ question: "q", allowedHandles: ["acme"], excludedHandles: ["other"] }); });
    expectCode("invalid_argument", () => { research({ question: "q", allowedHandles: [] }); });
    for (const handle of ["@acme", " ", "acme OR from:other", "abcdefghijklmnop"]) {
        expectCode("invalid_argument", () => { research({ question: "q", allowedHandles: [handle] }); });
    }
    const handles: string[] = [];
    for (let i = 0; i < 20; i += 1) handles.push("user" + i.toString());
    assert(buildResearchBody({ question: "q", allowedHandles: handles }).length > 0, "20 handles accepted");
    handles.push("extra");
    expectCode("invalid_argument", () => { research({ question: "q", excludedHandles: handles }); });
    for (const date of ["2025-02-29", "2026-04-31", "2026-13-01", "2026-01-01T00:00:00Z", "yesterday"]) {
        expectCode("invalid_argument", () => { research({ question: "q", fromDate: date }); });
    }
    expectCode("invalid_argument", () => { research({ question: "q", fromDate: "2026-02-01", toDate: "2026-01-01" }); });
    expectCode("invalid_argument", () => { research({ question: "q", fromDate: "2026-01-01", toDate: "2026-01-01" }); });

    label("extract answer blocks and citations without exposing provider internals");
    const message = { type: "message", role: "assistant", content: [
        { type: "output_text", text: "First", annotations: [
            { type: "url_citation", url: "https://x.com/acme/status/123", title: "Acme", start_index: 0, end_index: 5 },
            { type: "future_annotation", other: 1 },
        ] },
        { type: "output_text", text: "Second", annotations: [{ type: "url_citation", url: "https://x.com/other/status/124" }] },
    ] };
    const decoded = decodeResearchResponse(response([
        { type: "reasoning", summary: [] },
        { type: "custom_tool_call", name: "x_keyword_search", arguments: "{}" }, message,
    ], "completed", ',"usage":{"input_tokens":120,"output_tokens":40,"total_tokens":160,"cost_in_usd_ticks":500000000,"server_side_tool_usage_details":{"x_search_calls":2,"x_posts_fetched":10,"x_users_fetched":0}}'));
    assert(decoded.id === "r1" && decoded.model === "grok-4.7" && decoded.status === "completed", "response identity");
    assert(decoded.text === "First\nSecond" && decoded.citations.length === 2, "answer and source association");
    assert(decoded.citations[0].title === "Acme" && decoded.citations[1].title === null, "optional citation titles");
    assert(decoded.usage.postsFetched === 10 && decoded.usage.usersFetched === 0 && decoded.usage.searchCalls === 2, "zero differs from missing");
    assert(decoded.usage.costDollars === 0.05 && decoded.usage.inputTokens === 120, "provider total cost ticks converted to dollars");
    assert(!Object.keys(decoded).includes("raw"), "provider response is not exposed");

    label("incomplete answers, refusals and missing accounting are explicit");
    const partial = decodeResearchResponse(response([message], "incomplete", ',"incomplete_details":{"reason":"max_output_tokens"},"usage":null'));
    assert(partial.status === "incomplete" && partial.incompleteReason === "max_output_tokens" && partial.text.length > 0, "partial answer preserved");
    assert(partial.usage.costDollars === null && partial.usage.postsFetched === null, "missing is not free or zero");
    const empty = decodeResearchResponse(response([], "incomplete"));
    assert(empty.text === "" && empty.incompleteReason === null, "empty truncated output preserved");
    const refusal = decodeResearchResponse(response([{ type: "message", role: "assistant", content: [{ type: "refusal", refusal: "Cannot comply" }] }]));
    assert(refusal.refusals[0] === "Cannot comply" && refusal.text === "", "refusal stays visible");
    const noAnnotations = decodeResearchResponse(response([{ type: "message", role: "assistant", content: [{ type: "output_text", text: "No sources", annotations: null }] }]));
    assert(noAnnotations.citations.length === 0 && noAnnotations.usage.totalTokens === null, "uncited response and absent usage remain explicit");

    label("malformed and failed provider responses do not become successful defaults");
    for (const malformed of ["{}", "null", "[]", "<html>error</html>", '{"id":"r","model":"m","status":"completed","output":null}']) {
        expectCode("invalid_response", () => { decodeResearchResponse(malformed); });
    }
    expectCode("invalid_response", () => { decodeResearchResponse(response([])); });
    expectCode("invalid_response", () => { decodeResearchResponse(response([], "in_progress")); });
    expectCode("invalid_response", () => { decodeResearchResponse(response([message], "completed", ',"usage":{"input_tokens":-1}')); });
    expectCode("invalid_response", () => { decodeResearchResponse(response([message], "completed", ',"usage":{"cost_in_usd_ticks":1.5}')); });
    expectCode("invalid_response", () => { decodeResearchResponse(response([{ type: "message", role: "assistant", content: [{ type: "output_text", text: "x", annotations: [{ type: "url_citation", url: "javascript:alert(1)" }] }] }])); });
    expectCode("provider_error", () => { decodeResearchResponse(response([], "failed", ',"error":{"code":"server_error"}')); });
    expectCode("provider_error", () => { decodeResearchResponse(response([], "cancelled")); });

    label("HTTP failures preserve status and retry metadata without raw error bodies");
    assert(xaiHttpError(400).code === "invalid_request" && xaiHttpError(422).code === "invalid_request", "invalid request");
    assert(xaiHttpError(401).code === "unauthorized" && xaiHttpError(402).code === "quota_exceeded", "credentials and credits");
    assert(xaiHttpError(403).code === "forbidden" && xaiHttpError(503).code === "http_error", "access and provider outage");
    const limited = xaiHttpError(429, "10");
    assert(limited.code === "rate_limited" && limited.status === 429 && limited.retryAfter === "10", "retry metadata");
}
