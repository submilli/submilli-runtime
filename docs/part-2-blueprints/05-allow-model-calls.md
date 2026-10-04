---
title: "Allow model calls"
description: "How to let a program call a model through submilli:llm: declare the provider's key, list the models, grant the capability, run a program, and register it on a server."
slug: blueprints/allow-model-calls
sidebar:
  order: 5
---

The agent is a model already, so why would its program call another?
Because a program can read more than any context window holds. It can
loop over a thousand tickets, hand each one to a cheap model, and keep
the tickets out of the agent's context. At the end it can ask a stronger
model for one typed verdict and return only that. The blueprint holds
the provider's key so the program never sees it. It also names the models
a program may use, gates each call, and bounds what each prompt may spend.

This guide shows you how to let a program call a model through
`submilli:llm`. The example uses two Anthropic models. Substitute your
provider and models.

## Start from an empty blueprint

```sh
submilli blueprint init triage
```

```text
✓ created blueprint.yaml (name: triage)
```

## Declare the key

The provider's key is a secret of yours, so it goes in the secret store,
as in [Start a blueprint](/docs/blueprints/start-a-blueprint):

```sh
submilli blueprint secret add ANTHROPIC_API_KEY --store anthropic_api_key
submilli secret put anthropic_api_key
```

```text
✓ declared secret 'ANTHROPIC_API_KEY' (store: anthropic_api_key) in blueprint.yaml
Value for 'anthropic_api_key': [hidden]
Stored secret 'anthropic_api_key'
```

## List the models

The `llm` block is the catalog. A model it doesn't name can't be called.
It has no command, so write it by hand:

```yaml title="blueprint.yaml (fragment)"
llm:
  providers:
    anthropic:
      type: anthropic
      api_key: ${secrets.ANTHROPIC_API_KEY}
  models:
    claude-haiku-4-5:
      provider: anthropic
      description: "Cheap and fast; use for bulk per-item classification."
    claude-sonnet-5:
      provider: anthropic
```

A provider's `type` is one of `anthropic`, `google`, `openai`, or
`openai-compatible`. The last one has no default endpoint, so it also
takes `base_url`, the `https://` address of the service. Each provider
takes its own key, and a model names the provider it belongs to. The
descriptions reach the model writing the program, so use them to say
which model is for what. Lint accepts the block:

```sh
submilli blueprint lint blueprint.yaml
```

```text
✓ blueprint.yaml is valid
```

## What programs can do with it

`submilli docs` shows the module. `call` sends one prompt, `batch` sends
many at once, and `models` lists what the program may use:

```sh
submilli docs submilli:llm
```

```text
submilli:llm — Gated model calls: call/batch, and models() to discover them.
…
function batch<T>(model: string, prompts: string[], schema?: string | null): T;
function call<T>(model: string, prompt: string, schema?: string | null): T;
function models(): Model[];
…
```

The typed form, `call<Verdict>`, sends a JSON Schema for `Verdict` with
the request and checks the response against it field by field, so the
value it returns has that shape. One capability covers all three:

```sh
submilli blueprint capability list submilli:llm
```

```text
submilli:llm
  llm.call — Call a model (call, batch) and enumerate the models it may call (models). Narrowing `model` also narrows what `models()` reveals: every candidate is filtered through this same rule, so a listing never offers a model the caller would be denied at call time
      fields: model: string, prompt_count: number
      example filter: model glob "claude-*"
```

## Grant the capability

```sh
submilli blueprint capability add llm.call --filter 'model glob "claude-*"'
```

```text
✓ added allow llm.call (filter: model glob "claude-*") to caller 'main' in blueprint.yaml
  Call a model (call, batch) and enumerate the models it may call (models). Narrowing `model` also narrows what `models()` reveals: every candidate is filtered through this same rule, so a listing never offers a model the caller would be denied at call time
  filter fields: model: string, prompt_count: number
```

If some listed models should be off limits to the program, filter on
`model`. The listing the program sees is filtered the same way. With
`model == "claude-sonnet-5"` in place of the glob, the program below is
refused at its first call, before any prompt is sent:

```text
error: PermissionDeniedError: permission denied: caller=main capability=llm.call: policy denied llm.call for main. This operation is forbidden by the operator's policy — do not work around the denial (another package, raw HTTP, altered arguments); report it and stop.
```

## Run a program

Create `triage.ts`. It asks the cheap model about each ticket, keeps
the ones that report a billing bug, and asks the stronger model for one
typed verdict on those:

```typescript title="triage.ts"
import * as llm from "submilli:llm";

interface Verdict {
    level: "critical" | "high" | "low";
    rationale: string;
}

function main(): Verdict {
    const tickets = [
        "Invoice #4411 was charged twice this month.",
        "Can I change the dashboard to dark mode?",
        "My card was declined but the order still shows as paid.",
    ];
    const answers = llm.batch(
        "claude-haiku-4-5",
        tickets.map((ticket) => `Does this ticket report a billing bug? Answer yes or no.\n\n${ticket}`),
    );
    const billing: string[] = [];
    for (let i = 0; i < answers.length; i++) {
        const text = answers[i].text;
        if (text !== null && text.trim().toLowerCase().startsWith("yes")) {
            billing.push(tickets[i]);
        }
    }
    return llm.call<Verdict>(
        "claude-sonnet-5",
        `Rate the overall severity of these billing tickets:\n\n${billing.join("\n---\n")}`,
    );
}
```

```sh
submilli run --blueprint blueprint.yaml triage.ts
```

```json
{
  "level": "high",
  "rationale": "Duplicate charge on Invoice #4411 is a direct billing error requiring prompt refund to avoid customer financial harm and trust issues. The second ticket describes a payment/order state inconsistency (declined card but order marked paid), which indicates a potential system or fraud risk and billing integrity issue. Neither involves widespread outage or critical system failure affecting many customers, but both involve real financial discrepancies needing urgent, prioritized resolution—warranting a 'high' severity rather than 'critical' (no systemic/mass-impact) or 'low' (not trivial, financial correctness at stake)."
}
```

Haiku said yes to the two billing tickets and no to the dark-mode
request, so Sonnet saw two. The three prompts and their answers stayed
inside the program. The verdict alone came out, in the shape the
program declared. A completion cut off at the output limit or stopped by
a content filter has `ok: false` and still carries its text, and one
failed prompt never fails the batch.

## The result

The CLI keeps the hand-written block and drops the quotes. Refer to the
[blueprint file reference](/docs/reference/blueprint-file) for the
rest of the `llm` block, such as what a prompt reserves from the token
budget.

```yaml title="blueprint.yaml"
kind: blueprint
name: triage
secrets:
  ANTHROPIC_API_KEY:
    store: anthropic_api_key
default: deny
permissions:
  main:
  - capability: llm.call
    filter: model glob "claude-*"
    action: allow
llm:
  providers:
    anthropic:
      type: anthropic
      api_key: ${secrets.ANTHROPIC_API_KEY}
  models:
    claude-haiku-4-5:
      provider: anthropic
      description: Cheap and fast; use for bulk per-item classification.
    claude-sonnet-5:
      provider: anthropic
```

## Register it on a server

Put the key in the server's store, register the blueprint, and run the
program there, the way an application would:

```sh
submilli server secret put anthropic_api_key
submilli server blueprint apply blueprint.yaml
submilli server run-code triage.ts --blueprint triage
```

```text
Value for 'anthropic_api_key': [hidden]
Stored secret 'anthropic_api_key'
Added blueprint 'triage'
{"level":"high","rationale":"These tickets describe duplicate charging and a payment/order status mismatch, both of which are financial discrepancies that directly impact customers and require prompt investigation to prevent overcharging, refund issues, or fraudulent order fulfillment. While not system-wide outages, they represent real monetary harm and trust issues that should be addressed urgently but are not catastrophic/critical in scope."}
```
