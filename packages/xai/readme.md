# @submilli/xai

Read-only X/Twitter research through Grok X Search. Ask a research question,
discover relevant posts, and examine replies with a generated answer and source
citations. Uses an xAI API key; an X developer account is not required.

## Install and configure

```sh
submilli install submilli/submilli-runtime @submilli/xai
submilli blueprint add-package @submilli/xai --no-capabilities
submilli blueprint secret add XAI_API_KEY --store xai_api_key
submilli secret put xai_api_key
submilli blueprint capability add x.ai/research
```

Run blueprint commands beside an existing blueprint. `secret put` prompts for
the key. For application-supplied credentials, bind a required harness secret:

```yaml
secrets:
  XAI_API_KEY:
    harness:
      required: true
```

The package needs HTTP POST only to `https://api.x.ai/v1/responses`, access to
`XAI_API_KEY`, and best-effort `cost.report`. Caller permissions are separate:
grant `x.ai/research`, optionally filtered by `model == "grok-4.7"`.
The package capability catalog is [capabilities.yaml](capabilities.yaml).

## Use

```ts
import xai from "@submilli/xai";

function main(): string {
    const result = xai.research({
        question: "Find developers discussing the X API. Examine relevant replies and summarize the main concerns with citations.",
        maxTurns: 2,
    });
    return JSON.stringify(result);
}
```

Only X Search is enabled; there are no operations that post or modify X content.
For a specific post, call
`getTweetSummary(url, instruction?, includeReplies?)`. It returns the same
`ResearchResult`, generating the prompt and validating/canonicalizing an HTTPS
X/Twitter post URL. Instructions default to a concise summary; replies default
to excluded from the summary. Pass true to discuss available replies separately
with citations. This flag controls the answer, not xAI's internal fetches.
Grok chooses searches and thread fetches internally. This is research with
citations, not deterministic or exhaustive tweet/reply retrieval.

The default model is `grok-4.7`, with two research rounds and 4096 output tokens
(including reasoning). Calls spend xAI credits. These controls are not a dollar
cap. Search handle/date filters do not constrain thread fetches. The runtime's
30-second HTTP timeout can interrupt a task after billable work; no automatic
retries occur. See [agent documentation](docs/readme.md) for all options, status,
refusal handling, attribution, usage, and errors.

## Development

```sh
cargo run -p submilli -- build test --skip-network -p @submilli/xai
cargo run -p submilli -- build test -p @submilli/xai --env-file .env
```

The first command runs offline contracts and checks documentation examples. The
second also makes two paid requests (search and thread reading) when `XAI_API_KEY` is bound;
otherwise the live test reports an explicit skip. Credentials are not loaded by
default. The live test checks source attribution and usage, not ranking or exact
answer text.

Provider references: [X Search](https://docs.x.ai/developers/tools/x-search),
[Responses API](https://docs.x.ai/developers/rest-api-reference/inference/responses),
[citations](https://docs.x.ai/developers/tools/citations), and
[cost tracking](https://docs.x.ai/developers/cost-tracking).
