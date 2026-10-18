---
title: "Connect Vercel AI SDK to Submilli"
description: "Add a Submilli code execution tool to an existing Vercel AI SDK harness."
slug: how-to/use-the-http-api
sidebar:
  order: 7
authorship:
  label: ai-generated
  confirmed: true
  contentHash: "f1c1347420690973219dc49ce9c6dde3759d3f8e3fa1f866fe581dab99cdd9af"
  confirmedAt: "2026-10-18T13:09:05.218103+00:00"
# Unpublished AI draft. Human prose rewrite required before publication.
---

Add a tool to your Vercel AI SDK harness that sends code to Submilli over
HTTP. A harness is the software that runs your agent and handles its tool
calls. The tool below uses your registered Blueprint for each request.

## Before you start

Have your harness project and these Submilli settings ready:

| Environment variable | Value |
| --- | --- |
| `SUBMILLI_SERVER` | Server address, such as `http://127.0.0.1:8128`, without a trailing slash |
| `SUBMILLI_SERVER_TOKEN` | A token accepted by that server |
| `SUBMILLI_BLUEPRINT` | The name of a registered Blueprint |

Set these variables in the process that runs your harness. A Blueprint sets
the permissions for code sent to Submilli. If needed,
[run the server](/docs/server/run-the-server) and
[register a Blueprint](/docs/server/register-a-blueprint) first.

## Install the tool dependencies

In your TypeScript project:

```sh
npm install ai@^7.0.0 zod@^4.1.8
```

## Define the Submilli tool

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

## Add the tool to your agent

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

## Verify the connection

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

## Troubleshooting

- Connection refused: check `SUBMILLI_SERVER` and whether the server is running.
- `401`: use a token accepted by that server.
- Blueprint not found: check `SUBMILLI_BLUEPRINT` against the name registered
  on that server.

See [server settings](/docs/reference/server-settings) for server configuration.
