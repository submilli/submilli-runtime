---
title: "Connect Vercel AI SDK to Submilli"
description: "Configure Vercel AI SDK to use Submilli for code execution over HTTP."
showDescription: false
slug: how-to/use-the-http-api
sidebar:
  order: 7
authorship:
  label: ai-generated
  confirmed: true
  contentHash: "932cc0cfd535e1de29270ee240d0af2ca3e7c4022aa6f02bd684401aceee4ae3"
  confirmedAt: "2026-10-18T13:09:05.218103+00:00"
# Unpublished AI draft. Human prose rewrite required before publication.
---

Use the HTTP API to send code from a Vercel AI SDK tool to the Submilli runtime.

:::note[Before you start]
- An existing Vercel AI SDK project with a configured model.
- A [running Submilli server](/docs/server/run-the-server) and a token it accepts.
- A [registered Blueprint](/docs/server/register-a-blueprint), which sets the
  permissions for code sent to Submilli.
:::

## Set connection variables

Set these environment variables in the process that runs your agent:

| Environment variable | Value |
| --- | --- |
| `SUBMILLI_SERVER` | Server address, such as `http://127.0.0.1:8128`, without a trailing slash |
| `SUBMILLI_SERVER_TOKEN` | Server token |
| `SUBMILLI_BLUEPRINT` | Registered Blueprint name |

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
