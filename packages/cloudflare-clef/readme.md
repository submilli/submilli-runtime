# @submilli/cloudflare-clef

Cloudflare's Clef is a decision model in the same family as TypeSafe's Jev: it
reads shared state and returns a probability for every option of every typed
question. This Package calls Clef on Cloudflare Workers AI with the same
questions, builders, and answers as [`@submilli/typesafe`](../typesafe/readme.md),
so code written for one moves to the other by changing the import. Read the
[agent guide](docs/readme.md) for when to use it and how its limits differ from Jev.

## Install

```sh
submilli install submilli/submilli-runtime @submilli/cloudflare-clef
submilli blueprint add-package @submilli/cloudflare-clef --no-capabilities
submilli blueprint secret add CLOUDFLARE_ACCOUNT_ID --store cloudflare_account_id
submilli blueprint secret add CLOUDFLARE_API_TOKEN --store cloudflare_api_token
submilli secret put cloudflare_account_id
submilli secret put cloudflare_api_token
submilli blueprint capability add cloudflare.com/clef
```

The token needs Workers AI Read and Edit permissions. The account ID is the
32-character ID shown in the Cloudflare dashboard.
The Package requests only `POST https://api.cloudflare.com/client/v4/accounts/<id>/ai/run/@cf/cloudflare/clef`
(or `clef-flash`) and those two secrets. It imports question building and answer
checking from `@submilli/typesafe` but makes no TypeSafe calls, so
`add-package` may warn that `TYPESAFE_AI_KEY` is undeclared; it isn't needed.

## Use

```ts
import { batch, choiceQuestion, noulQuestion, Question,
    isChoiceAnswer, isNoulAnswer } from "@submilli/cloudflare-clef";

function main(): string {
    const routes = new Map<string, unknown>();
    routes.set("billing", "Charges, invoices, and refunds");
    routes.set("technical", "Broken features or service failures");
    routes.set("other", "Neither billing nor technical support");

    const questions = new Map<string, Question>();
    questions.set("route", choiceQuestion("Which team should handle ticket.text?", routes));
    questions.set("refund", noulQuestion("Does ticket.text explicitly request a refund?"));
    const result = batch({
        state: { ticket: { text: "Please refund my duplicate payment today." } },
        questions: questions,
        model: "clef-flash"
    });
    const route = result.answers.get("route");
    const refund = result.answers.get("refund");
    if (!isChoiceAnswer(route)) throw new Error("Unexpected route answer");
    if (!isNoulAnswer(refund)) throw new Error("Unexpected refund answer");
    return JSON.stringify({ route: route.choice, refundProbability: refund.noul, usage: result.usage });
}
```

`model` is `clef` (27B, the default) or `clef-flash` (9B). `noul`, `choice`, and
`score` make one request for one question, as in `@submilli/typesafe`.
Validation, credential, and HTTP-status failures throw `ClefError`. A denied
capability throws `PermissionDeniedError`, and a transport failure such as a
timeout throws a plain `Error` from `submilli:http`. The Package reports token
usage to the run's cost without a dollar amount, because Workers AI bills in
neurons after a daily free allowance.

## Development

```sh
cargo run -p submilli -- build test -p @submilli/cloudflare-clef
```

Offline tests cover validation, model names, and errors. The live test reads
`CLOUDFLARE_ACCOUNT_ID` and `CLOUDFLARE_API_TOKEN` from the environment or the
repository `.env`, skips when either is missing, and makes five small requests
across both models.
