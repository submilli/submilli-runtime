# @submilli/typesafe

TypeSafe supplies focused semantic judgments for routing, ranking, selecting known
values, and verifying evidence. Batch Choice, Noul, and Score questions over shared
state; use typed answers and probabilities in code. Jev handles language judgment;
ordinary code handles exact rules and actions. It does not generate prose or fetch
missing evidence. Read the self-contained [agent guide](docs/readme.md) for when to
use it, question design, examples, batching, and uncertainty handling, adapted from
TypeSafe's official guidance.

## Install

```sh
submilli install submilli/submilli-runtime @submilli/typesafe
submilli blueprint add-package @submilli/typesafe --no-capabilities
submilli blueprint secret add TYPESAFE_AI_KEY --store typesafe_ai_key
submilli secret put typesafe_ai_key
submilli blueprint capability add typesafe.ai/systemone
```

Supply the key at the hidden prompt. A trusted harness can instead bind:

```yaml
secrets:
  TYPESAFE_AI_KEY:
    harness:
      required: true
```

The package reads `TYPESAFE_AI_KEY` through `submilli:secrets`. This deliberately
uses our binding name, rather than the official SDK's `TYPESAFE_API_KEY` default.
HTTP permissions are restricted to `POST api.typesafe.ai/v1/systemone`.
No Node SDK is needed.

## Use

For one judgment, call `noul(state, instructions)`,
`choice(state, instructions, criteria)`, or `score(state, instructions, levels)`.
Each makes one HTTP request and returns its concrete answer type.
For several judgments over the same state, build questions locally and send them
in one request with `batch`:

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

Builders make no HTTP calls. `batch` returns answers under the supplied IDs,
plus model and usage metadata. Code decides how to act on the judgments; see the
[agent guide](docs/readme.md) for probability interpretation and uncertainty.

## Development

```sh
cargo run -p submilli -- build test -p @submilli/typesafe
```

Offline tests exercise the public builders and validation of single and batched
requests. The live test reads `TYPESAFE_AI_KEY` from the environment or repository
`.env`, skips when missing, and exercises one mixed batch plus each single-question
function (four small requests using provider credits). Agent documentation examples
are compile-checked by the same command.
