# X research with xAI

Use `@submilli/xai` to delegate research on public X/Twitter posts and replies
to Grok. It searches X and returns a generated answer with source citations.
Credentials are supplied internally; never ask for or pass an API key in calls.

<!-- submilli:if capability="x.ai/research" -->

```ts
import { research } from "@submilli/xai";

function main(): string {
    const result = research({
        question: "Find recent discussions of WebAssembly on X. Examine relevant replies and summarize disagreements with source links.",
        maxTurns: 2,
    });
    return JSON.stringify({ status: result.status, text: result.text, citations: result.citations, usage: result.usage });
}
```

`research` requires `x.ai/research`, checked with the selected `model`.
Named imports and the default package namespace are supported.

For a specific post, use `getTweetSummary(url, instruction?, includeReplies?)`:

```ts
import { getTweetSummary } from "@submilli/xai";

function main(): string {
    const result = getTweetSummary(
        "https://x.com/XDevelopers/status/1234567890",
        "Explain the announcement and the main concerns in the replies.",
        true,
    );
    return JSON.stringify(result);
}
```

This helper generates the question and calls `research` with its defaults
(`grok-4.7`, two rounds, 4096 output tokens). It needs the same `x.ai/research`
permission and returns the same `ResearchResult`. `instruction` defaults to
"Summarize the post clearly and concisely." and rejects blank strings.
`includeReplies` defaults to false; true asks for available replies separately,
with citations, or an explicit statement that no replies are available.

Supply an HTTPS post URL on `x.com` or `twitter.com`, including `www`/`mobile`
variants, in `username/status/ID`, `i/status/ID`, or `i/web/status/ID` form.
The helper canonicalizes the link to `https://x.com/i/status/ID`, removing sharing
parameters, fragments, and media suffixes. Profiles and non-post URLs fail before
credentials or network access. If the post is unavailable, the generated question
asks Grok to say so instead of inventing its content.

The reply flag controls the requested summary, not which data xAI fetches
internally. Summaries are model-generated and may be incomplete; reply coverage
is not exhaustive. Use `research` directly for custom model or generation limits.

| Option | Behavior |
| --- | --- |
| `question` | Required nonblank research task. Include a post URL to ask about a specific conversation. |
| `model` | Defaults to `grok-4.7`; must support X Search through the Responses API. |
| `allowedHandles`, `excludedHandles` | Optional lists of 1–20 X usernames without `@`; mutually exclusive. |
| `fromDate`, `toDate` | Optional valid UTC dates in `YYYY-MM-DD`; start inclusive, end exclusive. Start must precede end. |
| `maxTurns` | Defaults to 2; package range 1–10. Bounds provider research rounds. |
| `maxOutputTokens` | Defaults to 4096; package range 128–32768, including reasoning tokens. |
| `enableImageUnderstanding`, `enableVideoUnderstanding` | Default false. Enable only when the task needs media analysis. |

Only X Search is enabled. Grok chooses keyword/semantic searches, user searches,
and thread fetches internally. The package cannot post, reply, like, follow,
or send messages. Requests use `/v1/responses` with `store: false` and no streaming.

Handle and date filters apply to keyword and semantic searches; xAI does not
apply them to thread fetches. They are relevance controls, not access boundaries.
Thread fetches can include parents, quoted posts, and replies outside the filters.
This is generated research, not an exhaustive or paginated export of posts or
replies. A citation is a source attribution, not a verified copy of a tweet.

Inspect `status` before treating the answer as complete. `incomplete` preserves
partial or empty `text` and `incompleteReason` when supplied. `refusals` preserves
model refusals. `citations` contains provider-attributed URLs and nullable titles
in output order; URLs can repeat. Provider tool calls and citation offsets are
not exposed by the package.
An answer without citations is possible and should not be treated as sourced.

`usage` exposes `inputTokens`, `outputTokens`, `totalTokens`, `searchCalls`,
`postsFetched`, `usersFetched`, and `costDollars`. Missing counters remain `null`,
not zero. Cost is the provider total converted from `cost_in_usd_ticks`, not a
local price estimate. Cost reporting to the run is best effort; the returned
usage remains available if `cost.report` is denied.

Research consumes xAI credits for fetched posts/profiles and model tokens.
Repeated fetches can be billed again. `maxTurns` and `maxOutputTokens` do not
enforce a dollar budget or post count. Narrow the task and use account spending
controls. The runtime HTTP timeout is 30 seconds; large tasks can time out.
Timeouts may follow billable work. The package does not retry automatically.

Failures throw `XaiError` with `code`, `status`, and nullable `retryAfter`.
Local validation/decoding failures use status 0. Codes: `invalid_argument`,
`missing_credentials`, `invalid_response`, `provider_error`, `invalid_request`,
`unauthorized`, `quota_exceeded`, `forbidden`, `rate_limited`, `http_error`.
HTTP errors retain their status and raw Retry-After value but omit provider error
bodies. Runtime permission and transport errors propagate unchanged.

<!-- submilli:endif -->
