---
docType: how-to
title: "Connect Vercel AI SDK to Submilli"
description: "Configure Vercel AI SDK to use Submilli for code execution over HTTP."
showDescription: false
slug: how-to/use-the-http-api
sidebar:
  order: 7
authorship:
  label: ai-generated
  confirmed: true
  contentHash: "f54a599a383fc6210266db9ce99afecf72cd89bbc5e62fc2c515a58f47742af9"
  confirmedAt: "2026-10-18T13:09:05.218103+00:00"
# Unpublished AI draft. Human prose rewrite required before publication.
---

Connect your existing Vercel AI SDK harness to Submilli for code execution.

## Use your coding agent

Open your existing Vercel AI SDK project in your coding agent.
[Install the Submilli skill](/docs/install#the-skill) if needed, then restart
the coding agent to load it. Give it this prompt:

```text agent-prompt
Use the Submilli skill to connect this existing Vercel AI SDK project
to Submilli over MCP.
Inspect the project. Preserve the existing agent, model, instructions,
tools, and configuration. Make only the changes needed to add Submilli.
Find any existing Submilli server, credentials, and registered Blueprint
in local configuration. If setup is missing, help me prepare it. Ask me
which operations the agent needs before defining Blueprint permissions.
Install the integration and configure the connection. Keep tokens out of
source code and responses, and help me configure missing credentials locally.
Verify the connection and tool discovery without a model request.
Tell me what changed and anything I still need to do.
```

After setup, [try it out](#try-it-out). To configure the connection yourself,
follow the manual steps below.

## Set up manually

Use the HTTP API to send code from a Vercel AI SDK
[tool](https://ai-sdk.dev/docs/ai-sdk-core/tools-and-tool-calling) to the Submilli runtime.
A tool describes a function the model can call and the inputs it accepts.

:::note[Before you start]
You’ll need:

- An existing Vercel AI SDK project with a configured model.
- A [running Submilli server](/docs/server/run-the-server) and a token it accepts.
- A [registered Blueprint](/docs/server/register-a-blueprint), which sets the
  permissions for code sent to Submilli.
:::

### Set connection variables

Use the server address, token, and Blueprint name prepared in **Before you start**.
Assign those values to these environment variables in the process that runs your agent:

| Environment variable | Value |
| --- | --- |
| `SUBMILLI_SERVER` | Server address, such as `http://127.0.0.1:8128`, without a trailing slash |
| `SUBMILLI_SERVER_TOKEN` | Server token |
| `SUBMILLI_BLUEPRINT` | Registered Blueprint name |


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

If your Blueprint requires variables, include them as `variables` in the
request body. For secrets supplied by your application (the `harness`
source), include a mapping of names to values as `secrets`.
See [Blueprint secrets](/docs/reference/blueprint-file#secrets) and
[the execute request](/docs/reference/http-api#one-off-runs) for the fields.

### Add the tool to your agent

Include `submilli` in the `tools` passed to your existing `generateText`
or `streamText` call. Here, `options` is your existing call configuration,
including the model, messages, and step limit. Preserve it when adding the tool:

```typescript title="Agent call (fragment)"
import { generateText } from "ai";
import { submilli } from "./submilli";

const result = await generateText({
  ...options,
  tools: { ...options.tools, submilli__typescript__execute: submilli },
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

1. Open your application's chat or test input with the connected agent.
2. Send this message:

   > Run this TypeScript program in Submilli and report its return value:

   ```typescript
   export function main(): number {
     return 2 + 2;
   }
   ```

3. Check for the answer `4` and a Submilli execution call returning `4` in
   your application's tool activity. If your app hides tool activity, use the
   inspection steps below.

The tool call confirms that your agent reached Submilli and ran the program.
The answer alone does not prove execution. Test your business permissions separately.

<details>
<summary>Inspect the Submilli call</summary>

To inspect a run in code, put the test message in your `generateText`
input, either as its `prompt` or as a user entry in its `messages`. Keep the
model, Submilli tool, and other options. After `await generateText(...)`, print
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
successful tool result's `output` should be `4`. If setup connected through
MCP, inspect the execution tool supplied by that connection; its output may
wrap the value in MCP text content.

Match the tool call and result by their call ID and check for an error.

If there is no execution call, check that this run uses the agent configuration
with Submilli attached and that its tool settings allow the call. If the call
fails, read the tool error and [check the connection settings](#troubleshooting).
If the tool returns `4` but the agent gives no final answer, check your agent's
step or turn limit so it can continue after the tool result.

</details>

## Troubleshooting

- Connection refused: check `SUBMILLI_SERVER` and whether the server is running.
- `401`: use a token accepted by that server.
- Blueprint not found: check `SUBMILLI_BLUEPRINT` against the name registered
  on that server.

See [server settings](/docs/reference/server-settings) for server configuration.
