# TypeSafe

Use `@submilli/typesafe` when a workflow needs a focused semantic judgment over
text or application state: routing a request, comparing evidence, selecting a
known candidate, or rating relevance. TypeSafe's Jev model supplies typed answers
and probabilities that your code can combine. Keep exact lookups, calculations,
policy enforcement, and execution in code. Use a reasoning model for open-ended
analysis, generated prose, code, or explanations; Jev does not generate those.

## Decide when to call it

Start with the behavior needed, then identify the judgments that require language
understanding. Useful applications include:

- Route to a known handler and select its bounded arguments.
- Rerank retrieved passages against a query using the same Score rubric per item.
- Verify whether supplied source evidence supports a claim or extracted field.
- Extract by finding candidate spans in code, asking which matches, and copying
  the chosen source value. A missing candidate cannot be selected.
- Score independent dimensions once, then adjust weights or filters in code
  without rerunning inference when evidence and question meanings are unchanged.

Retrieve evidence before calling. Jev only receives the state and questions you
send; it cannot browse, read your agent conversation, or fetch missing records.
Give named fields for source text, relationships, policies, and current facts.
Images, audio, and video must first be converted to useful text.

## One judgment or several

The request and question-building functions have two roles:

| Make an HTTP call | Build a question locally |
| --- | --- |
| `noul(state, instructions, criteria?, options?)` → `NoulAnswer` | `noulQuestion(instructions, criteria?)` |
| `choice(state, instructions, criteria, options?)` → `ChoiceAnswer` | `choiceQuestion(instructions, criteria)` |
| `score(state, instructions, levels, options?)` → `ScoreAnswer` | `scoreQuestion(instructions, levels)` |
| `batch({ state, questions, model? })` → `BatchResponse` | Combine the builders in a `Map<string, Question>` |

For one judgment, call the primitive directly. You get a concrete answer type
without creating a question Map or narrowing an answer union:

```ts
import { noul } from "@submilli/typesafe";

function main(): number {
    const answer = noul(
        "Please refund my duplicate payment.",
        "Does the customer explicitly request a refund?"
    );
    return answer.noul;
}
```

Each direct function makes one HTTP request. Its final `options` argument accepts
`{ model: "jev-latest" }`; for a Noul without criteria, pass `undefined` as the
third argument before supplying options. Single calls return the typed answer;
use `batch` (even with one question) when you need the answering model and token
usage as well. Builders validate question definitions and make no network calls.
When several independent judgments share state, use the builders and `batch`
to evaluate them in one HTTP request, rather than calling the direct functions
repeatedly. `batch` is synchronous evaluation, not an asynchronous job API.

## Choose the judgment

| Need | Builder | Interpret the result |
| --- | --- | --- |
| Exactly one option | `choiceQuestion(instructions, criteria)` | `choice` is the winning key; probabilities compare competing options. Include other/no-match when appropriate. |
| Whether a condition holds | `noulQuestion(instructions, criteria?)` | `noul` is probability of yes. Near 0.5 means uncertainty, not medium intensity. Use separate Nouls when multiple labels may apply. |
| Degree along one dimension | `scoreQuestion(instructions, levels)` | `score` is a weighted position from 0 to levels.length - 1, possibly fractional. Use comparable per-item Scores for graded ranking. |

Question instructions and descriptions accept strings, JSON objects, or arrays.
Choice criteria are a `Map<string, unknown>` with up to 255 options; an option's
value can be null when its name is sufficient. Score takes 2–10 ordered level
descriptions. Optional Noul criteria use only `"true"` and `"false"` keys.

Write one coherent judgment per question. Include its entire meaning in
instructions: question IDs are for code and are not sent to the model. Refer to
state fields with paths such as `ticket.text`. Describe Score levels as concrete
situations that stand alone; avoid bare numeric labels or “more than the previous
level.” Split independently useful dimensions instead of mixing them into one
ambiguous rating.

## Batch independent questions

`batch({ state, questions, model? })` is synchronous. Use Maps for dynamic
question IDs and criteria. Answers are returned in a Map under the same IDs;
use `isNoulAnswer`, `isChoiceAnswer`, or `isScoreAnswer` to narrow an answer before
reading its fields. These local type guards accept `Answer | null | undefined`, return false
for a missing answer or another variant, and make no HTTP calls. Their TypeScript
predicates (for example, `answer is ChoiceAnswer`) let the compiler expose the
variant's fields inside a successful branch or after a rejecting guard.
They classify already-decoded answers; they do not validate arbitrary JSON.
Probability maps support `.get(key)`. To serialize a Map, use
`Array.from(map)` to get an array of key/value pairs; Submilli does not stringify
Maps directly. State and nested descriptions must be JSON-serializable values;
use ordinary objects/arrays there, not Maps or class instances.

```ts
import { batch, choiceQuestion, noulQuestion, scoreQuestion, Question,
    isNoulAnswer, isChoiceAnswer, isScoreAnswer } from "@submilli/typesafe";

function main(): string {
    const routes = new Map<string, unknown>();
    routes.set("billing", "Charges, invoices, and refunds");
    routes.set("technical", "Broken features or service failures");
    routes.set("other", "Neither billing nor technical support");

    const questions = new Map<string, Question>();
    questions.set("route", choiceQuestion("Which team should handle ticket.text?", routes));
    questions.set("refund", noulQuestion("Does ticket.text explicitly request a refund?"));
    questions.set("urgency", scoreQuestion("How time-sensitive is ticket.text?", [
        "No deadline or time pressure mentioned",
        "Explicit deadline within the next week",
        "Immediate action requested today"
    ]));
    const result = batch({
        state: { ticket: { text: "Please refund my duplicate payment today." } },
        questions: questions
    });
    const route = result.answers.get("route");
    const refund = result.answers.get("refund");
    const urgency = result.answers.get("urgency");
    if (!isChoiceAnswer(route)) throw new Error("Unexpected route answer");
    if (!isNoulAnswer(refund)) throw new Error("Unexpected refund answer");
    if (!isScoreAnswer(urgency)) throw new Error("Unexpected urgency answer");
    return JSON.stringify({
        route: route.choice, routeConfidence: route.confidence,
        refundProbability: refund.noul, urgency: urgency.score,
        urgencyConfidence: urgency.confidence, model: result.model, usage: result.usage
    });
}
```

Questions in one call run independently and cannot see each other's answers.
Ask useful branch-specific questions speculatively, state their premises, and
consume only applicable results. Ignore uncertainty on unused branches. Make a
second call when an earlier result is needed to retrieve evidence or construct
new state or options. Extra questions consume tokens; measure total usage and
latency rather than assuming batching is free.

## Turn answers into behavior

Choice and Score include full `probabilities` and `confidence` in [0, 1].
Confidence summarizes distribution concentration; it is not the probability that
your whole workflow is correct, and it never grants permission to act. Several
acceptable options can spread probability without making a harmless choice bad.
Noul has no separate confidence field. Score also includes a `legend` Map from
zero-based level strings to descriptions.

Keep raw judgments available. Choose thresholds on representative data and the
consequences of mistakes; no universal cutoff is supplied by this package.
Uncertain or failing evidence checks can trigger more evidence, a reasoning model,
or human review. Weighted scores allow strengths to compensate for weaknesses;
use separate conditions for rules where any serious violation must block action.
Recheck freshness if the underlying state changes before applying a result.

Typed output guarantees an interface, not truth. Test both judgments and downstream
behavior. For failures inspect the exact state, instructions, available candidates,
answers, and composition; distinguish missing evidence from model, code, or service
errors. Examples illustrate decomposition, not measured accuracy for your domain.

## Credentials, permissions, and failures

Credentials are read internally from `TYPESAFE_AI_KEY`; never pass a key in calls.
The caller needs `typesafe.ai/systemone`. The package requests only
`POST https://api.typesafe.ai/v1/systemone` and that secret.

The default model is `jev-latest`. Set `model` to a versioned ID when reproducible
threshold evaluation matters; batch responses report the model that answered.
`usage.input_tokens` and `usage.output_tokens` preserve provider usage counts.
This package exposes evaluation, not model discovery or automatic execution.

`TypeSafeError` carries `code`, HTTP `status` (0 for local errors), and nullable raw
`retryAfter`. Codes: `invalid_argument`, `missing_credentials`, `invalid_request`,
`unauthorized`, `forbidden`, `rate_limited`, `overloaded`, `http_error`, and
`invalid_response`. Permission and transport errors propagate from the runtime.
There are no automatic retries. For 429/529, let the calling workflow schedule
bounded exponential backoff and respect Retry-After when present. Do not retry
invalid inputs or credentials unchanged.

All four HTTP functions validate requests before reading credentials or sending
HTTP. The shared decoder checks response answer IDs/types, probability ranges,
criteria coverage, and usage before returning a result.

## Official guidance

This self-contained guide adapts TypeSafe's official
[System One](https://docs.typesafe.ai/concepts/system-one),
[agent skill](https://docs.typesafe.ai/agent-skill),
[Choice](https://docs.typesafe.ai/primitives/choice),
[Noul](https://docs.typesafe.ai/primitives/noul),
[Score](https://docs.typesafe.ai/primitives/score),
[fan-out](https://docs.typesafe.ai/patterns/fan-out), and
[confidence](https://docs.typesafe.ai/confidence) guidance.
The [API reference](https://docs.typesafe.ai/api) defines the wire contract.
