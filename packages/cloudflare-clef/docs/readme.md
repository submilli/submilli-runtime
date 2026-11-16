# Cloudflare Clef

Use `@submilli/cloudflare-clef` when a workflow needs a focused semantic judgment
over text or application state: routing a request, verifying evidence, selecting
a known candidate, or rating relevance. Clef returns typed answers and
probabilities that code combines. Keep exact lookups, calculations, policy, and
actions in code. Clef does not generate prose and cannot fetch missing evidence.

Clef answers the same three question types as TypeSafe's Jev, with the same
functions:

| Make an HTTP call | Build a question locally |
| --- | --- |
| `noul(state, instructions, criteria?, options?)` → `NoulAnswer` | `noulQuestion(instructions, criteria?)` |
| `choice(state, instructions, criteria, options?)` → `ChoiceAnswer` | `choiceQuestion(instructions, criteria)` |
| `score(state, instructions, levels, options?)` → `ScoreAnswer` | `scoreQuestion(instructions, levels)` |
| `batch({ state, questions, model? })` → `BatchResponse` | Combine the builders in a `Map<string, Question>` |

```ts
import { noul } from "@submilli/cloudflare-clef";

function main(): number {
    return noul("Please refund my duplicate payment.", "Does the customer explicitly request a refund?").noul;
}
```

## Write good questions

- Put the evidence in `state` under named fields, and refer to those fields in
  the instructions, such as `ticket.text`.
- Ask one thing per question. Batch independent questions over the same state
  in one `batch` call instead of making several calls.
- Give `choice` an option for "none of these" when no option may fit.
- Give `score` 2 to 10 levels, each meaningful on its own. The answer's `score` is
  a weighted mean of level indices, not a probability.
- `noul` is the probability of yes. `confidence` on choice and score answers is
  how concentrated the distribution is, not whether the workflow is correct.
  Decide thresholds in code, and handle low confidence explicitly.

## Choose a model

| `model` | Size | When |
| --- | --- | --- |
| `clef` (default) | 27B | The most accurate judgments. |
| `clef-flash` | 9B | Cheaper and faster judgments over short state. Cloudflare's hosted version has a shorter context than the open weights. |

Keep state short and relevant: send the passages a question needs rather than
whole documents. Clef can also read images, but this Package sends text and
JSON state only.

## Errors

Validation, credential, and HTTP-status failures throw `ClefError`, with `code`,
`status`, and `retryAfter`. A denied capability throws `PermissionDeniedError`, and
a transport failure such as a timeout throws a plain `Error` from `submilli:http`.
Invalid questions, state, or model names fail with `invalid_argument` before any
request. Request failures use `missing_credentials`, `invalid_credentials`,
`invalid_request`, `unauthorized`, `forbidden`, `rate_limited`, `http_error`, or
`invalid_response`. Nothing retries automatically.
