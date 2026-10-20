---
docType: how-to
title: "Connect Vercel AI SDK to Submilli"
description: "Create a Vercel AI SDK agent that uses Submilli for permitted code execution."
showDescription: false
slug: how-to/use-the-http-api
sidebar:
  order: 7
authorship:
  label: ai-generated
  confirmed: true
  contentHash: "31dd7a2403a759d8427c67afcf47405d3371263ea38d14d6ab3320beb9cbc527"
  confirmedAt: "2026-10-18T13:09:05.218103+00:00"
# Unpublished AI draft. Human prose rewrite required before publication.
---

Create a fresh Vercel AI SDK agent whose execution tools run through Submilli,
then try an allowed operation and a denied operation with the same Blueprint.

## Use your coding agent

Open a project directory in your coding agent.
[Install the Submilli skill](/docs/install#the-skill) if needed, then restart
the coding agent to load it. Give it this prompt:

```text agent-prompt
Use the Submilli skill to create a fresh Vercel AI SDK demo agent connected
to Submilli through the HTTP API. Write dedicated demo files without replacing
an existing agent or its configuration. Inspect the project and help me choose
and configure a model if needed. Keep credentials local and out of responses.
Download https://submilli.ai/docs/examples/permission-demo/blueprint.yaml
and https://submilli.ai/docs/examples/permission-demo/billing-source.zip
into a new demo directory. Prepare the billing Package with submilli build
publish-local and register the Blueprint with submilli server blueprint apply.
Find a local Submilli server and token, or help me prepare them. Publish the
Package under the same user and SUBMILLI_HOME as that server. Use the
permission-demo Blueprint. Give the new agent only Submilli execution tools.
Disable built-in execution tools, other tool providers, and delegation routes
that could act outside Submilli. Follow this guide's framework-specific setup.
Verify connection and the allowed cus_northwind and denied cus_initech
listCharges calls without a model request. Tell me how to start the agent
and where to inspect tool results, plus any remaining setup I must complete.
```

After setup, [try it out](#try-it-out). To configure the connection yourself,
follow the manual steps below.

<details>
<summary>Connecting an existing agent</summary>

Replace its execution tools with Submilli. Remove other tool providers,
file and shell tools, network tools, and delegation routes that can perform
operations outside Submilli. Blueprint permissions apply to code run by
Submilli; leaving another execution route available bypasses that boundary.

Replace the call's `tools` with the single Submilli tool shown here.
Remove provider tools, `prepareStep` tool overrides, and tools that delegate to other agents.

</details>

## Set up manually

Use the HTTP API to send code from a Vercel AI SDK
[tool](https://ai-sdk.dev/docs/ai-sdk-core/tools-and-tool-calling) to the Submilli runtime.
A tool describes a function the model can call and the inputs it accepts.

:::note[Before you start]
You’ll need:

- A Vercel AI SDK project with a configured model.
- The [Submilli CLI](/docs/install) and a [local server](/docs/server/run-the-server)
  running under the same user and `SUBMILLI_HOME`, with a token it accepts.
:::

### Set connection variables

Set these environment variables in your shell and the process that runs your agent:

| Environment variable | Value |
| --- | --- |
| `SUBMILLI_SERVER` | Server address, such as `http://127.0.0.1:8128`, without a trailing slash |
| `SUBMILLI_SERVER_TOKEN` | Server token |
| `SUBMILLI_BLUEPRINT` | `permission-demo` |

### Prepare the permission demo

Download the [demo Blueprint](/docs/examples/permission-demo/blueprint.yaml)
and [billing Package source](/docs/examples/permission-demo/billing-source.zip).
The Package uses local sample charges and needs no billing credentials.
Save both downloads in a new `permission-demo` directory, then prepare and
register the demo with your running server:

```sh
cd permission-demo
unzip billing-source.zip
submilli build publish-local
SUBMILLI_SERVER_URL="$SUBMILLI_SERVER" submilli server blueprint apply blueprint.yaml
```

The Blueprint permits
listing charges for `cus_northwind` and denies that operation for other customers.

### Install the tool dependencies

In your TypeScript project:

```sh
npm install ai@^7.0.0 zod@^4.1.8
```

### Define the Submilli tool

Add this to a module in your project. It fetches the tool description from
Submilli, then creates a tool that sends code to `POST /v1/execute`.

```typescript title="submilli.ts"
import { tool } from "ai";
import { z } from "zod";

const server = process.env.SUBMILLI_SERVER;
const blueprint = process.env.SUBMILLI_BLUEPRINT;
const headers = {
  Authorization: `Bearer ${process.env.SUBMILLI_SERVER_TOKEN}`,
  "Content-Type": "application/json",
};

const response = await fetch(`${server}/v1/blueprints/${blueprint}/prompt`, { headers });
if (!response.ok) throw new Error(`Cannot read the tool description: ${response.status}`);
const description = await response.json();

export const submilli = tool({
  description: description.prompt,
  inputSchema: z.object({ code: z.string() }),
  execute: async ({ code }) => {
    const response = await fetch(`${server}/v1/execute`, {
      method: "POST",
      headers,
      body: JSON.stringify({ blueprint, code }),
    });
    if (!response.ok) throw new Error(`Submilli request failed: ${response.status}`);
    const run = await response.json();
    if (run.error) throw new Error(JSON.stringify(run.error));
    return run.result;
  },
});
```

<details>
<summary>Using a different Blueprint with variables or secrets</summary>

If your Blueprint requires variables, include them as `variables` in the
request body. For secrets supplied by your application (the `harness`
source), include a mapping of names to values as `secrets`.
See [Blueprint secrets](/docs/reference/blueprint-file#secrets) and
[the execute request](/docs/reference/http-api#one-off-runs) for the fields.

</details>

### Create the demo call

Use your configured `model` and the message under **Try it out** as `prompt`.
This fresh call gives the model only the Submilli tool.

```typescript title="Fresh demo call"
import { generateText, stepCountIs } from "ai";
import { submilli } from "./submilli";

const result = await generateText({
  model,
  prompt,
  tools: { submilli__typescript__execute: submilli },
  stopWhen: stepCountIs(10),
});
```

Each call runs in a new Submilli session that ends when the program returns.
For a connection that keeps state across calls, see
[HTTP sessions](/docs/reference/http-api#sessions).

### Verify the connection

Call the tool directly before making a model request:

```typescript
const connectionResult = await submilli.execute!(
  { code: 'function main(): string { return "connected"; }' },
  { toolCallId: "connection-check", messages: [], context: {} },
);
if (connectionResult !== "connected") throw new Error(`Unexpected result: ${connectionResult}`);
console.log("Connected to Submilli.");
```

`Connected to Submilli.` confirms that the SDK tool sent code to the
server and received the expected result. This request does not call a model.

## Try it out

1. Open your application's chat or test input with the fresh demo agent.
2. Ask it to perform the allowed operation:

   > Use the billing Package to list charges for customer `cus_northwind`.

   Check the Submilli tool result for `ch_a1` (4900) and `ch_a2` (1250).
3. Ask it to attempt the denied operation:

   > Use the billing Package to list charges for customer `cus_initech`.
   > Attempt the operation through Submilli and report the runtime result.
   > Keep the customer ID and Blueprint permissions unchanged.

   Check for a Submilli call returning `permission_denied` for
   `acme.com/charges.list`, with no charges.

An agent's answer or refusal alone does not demonstrate the permission check.
Look for both runtime results in tool activity. Expand the inspection steps
below if your app hides them.

<details>
<summary>If the agent declines to attempt the denied operation</summary>

Ask it to run this exact program through Submilli and report the tool result:

```typescript
import { listCharges, Charge } from "@acme/billing";

function main(): Charge[] {
  return listCharges("cus_initech");
}
```

If it still declines without making a tool call, the runtime denial has not
been tested. Use the direct demo check below to verify the Blueprint independently.

```sh
curl -fsS "$SUBMILLI_SERVER/v1/execute" \
  -H "Authorization: Bearer $SUBMILLI_SERVER_TOKEN" \
  -H "Content-Type: application/json" \
  --data '{"blueprint":"permission-demo","code":"import { listCharges, Charge } from \"@acme/billing\"; function main(): Charge[] { return listCharges(\"cus_initech\"); }"}'
```

Check for `error.kind` equal to `"permission_denied"`, Capability
`acme.com/charges.list`, and `result: null`. Older runtimes may return
`runtime_error` with a `PermissionDeniedError` message instead.

</details>

<details>
<summary>Inspect the Submilli call</summary>

To inspect a run in code, put the test message in your `generateText`
input, either as its `prompt` or as a user entry in its `messages`. Keep the
demo model and Submilli tool. After `await generateText(...)`, print
the [completed steps](https://ai-sdk.dev/docs/reference/ai-sdk-core/generate-text)
in your application's terminal:

```typescript
for (const step of result.steps) {
  console.dir({ calls: step.toolCalls, results: step.toolResults }, { depth: null });
}
console.log(result.text);
```

For an existing `streamText` call, inspect `await result.steps` after consuming
the stream. Find `submilli__typescript__execute` in `toolName`, inspect its
`input.code`, and match its `toolCallId` to the result's `toolCallId`. The
successful tool result's `output` should contain the Northwind charges. If setup connected through
MCP, inspect the execution tool supplied by that connection; its output may
wrap the value in MCP text content.

Match the tool call and result by their call ID and check for an error.

If there is no execution call, check that this run uses the agent configuration
with Submilli attached and that its tool settings allow the call. If the call
fails, read the tool error and [check the connection settings](#troubleshooting).
If the tool returns charges but the agent gives no final answer, check your agent's
step or turn limit so it can continue after the tool result.

</details>

## Troubleshooting

- Connection refused: check `SUBMILLI_SERVER` and whether the server is running.
- `401`: use a token accepted by that server.
- Blueprint not found: check `SUBMILLI_BLUEPRINT` against the name registered
  on that server.

See [server settings](/docs/reference/server-settings) for server configuration.
