---
title: "Connect Mastra to Submilli"
description: "Give an existing Mastra agent access to Submilli through MCP."
showDescription: false
slug: how-to/connect-mastra
sidebar:
  order: 3
authorship:
  label: ai-generated
  confirmed: true
  contentHash: "13bea7cb240081bea2ec6f0b60b0a1fe12953aaf177b244abe095aff723dc9e2"
  confirmedAt: "2026-10-18T13:09:05.218103+00:00"
# Unpublished AI draft. Human prose rewrite required before publication.
---

Use Mastra's [MCP client](https://mastra.ai/docs/connections/mcp) to connect
your agent to the Submilli runtime. MCP (Model Context Protocol) lets an
agent discover and call tools on a server.

:::note[Before you start]
- An existing Mastra project with a configured model.
- A [running Submilli server](/docs/server/run-the-server) and a token it accepts.
- A [registered Blueprint](/docs/server/register-a-blueprint), which sets the
  permissions for code sent to Submilli.
:::

## Set connection variables

Use the server address, token, and Blueprint name prepared in **Before you start**.
Assign those values to these environment variables in the process that runs your agent:

| Environment variable | Value |
| --- | --- |
| `SUBMILLI_SERVER` | Server address, such as `http://127.0.0.1:8128`, without a trailing slash |
| `SUBMILLI_SERVER_TOKEN` | Server token |
| `SUBMILLI_BLUEPRINT` | Registered Blueprint name |

## Use your coding agent (optional)

[Install the Submilli skill](/docs/install#the-skill), restart your coding agent,
and open this project. Make the prepared values available to its terminal through
your local environment or existing project configuration, then give it this prompt:

```text title="Prompt for your coding agent"
Use the Submilli skill to connect this existing Mastra project
to Submilli over MCP. Reuse the prepared server and Blueprint. Read
SUBMILLI_SERVER, SUBMILLI_SERVER_TOKEN, and SUBMILLI_BLUEPRINT from the
local environment or project configuration. Map those values to the
integration's settings.
Keep my model and existing agent configuration. Keep token values out of
source code and responses. Verify the connection without a model request.
```

For manual setup, continue below.

## Install the MCP client

In your Mastra project, install the client:

```sh
npm install @mastra/mcp@^1.18.0
```

## Connect Submilli to your agent

Create the client in the code that runs your existing agent. The URL selects
the Blueprint, and the header supplies the server token.

```typescript title="MCP connection (fragment)"
import { MCPClient } from "@mastra/mcp";

const submilli = new MCPClient({
  id: "submilli",
  servers: {
    submilli: {
      url: new URL(
        `${process.env.SUBMILLI_SERVER}/mcp/${process.env.SUBMILLI_BLUEPRINT}`,
      ),
      requestInit: {
        headers: {
          Authorization: `Bearer ${process.env.SUBMILLI_SERVER_TOKEN}`,
        },
      },
    },
  },
});
```

If your Blueprint requires variables, add a `submilli-variables` header
with those bindings. See [MCP variable bindings](/docs/application#three-decisions-the-model-has-no-part-in).
If your Blueprint requires secrets from your application (the `harness`
source), add `submilli-secrets` containing a base64url-encoded JSON object
of their names and values. See
[Blueprint secrets](/docs/reference/blueprint-file#secrets).

Pass Submilli's toolset to your existing `agent.generate` call. Here,
`agent`, `messages`, and `options` are your existing agent, input, and
call options.

```typescript title="Agent call (fragment)"
try {
  const toolsets = await submilli.listToolsets();
  if (!toolsets.submilli) {
    throw new Error("Submilli did not return its tools");
  }
  const result = await agent.generate(messages, {
    ...options,
    toolsets: { ...options.toolsets, ...toolsets },
  });
} finally {
  await submilli.disconnect();
}
```

Keep the connection open until the agent finishes using its tools.

## Verify the connection

To verify the connection before calling the model, place this after creating
the client and before `agent.generate`:

```typescript
const tools = await submilli.listTools();
if (!Object.keys(tools).some((name) => name.endsWith("submilli__typescript__execute"))) {
  throw new Error("Submilli's execute tool was not found");
}
console.log("Connected to Submilli.");
```

`Connected to Submilli.` confirms that Mastra can discover
`submilli__typescript__execute`. Keep this request inside the `try` block so the client
disconnects if it fails.

## Troubleshooting

- Connection refused: check `SUBMILLI_SERVER` and whether the server is running.
- `401`: use a token accepted by that server.
- Blueprint not found: check `SUBMILLI_BLUEPRINT` against the name registered
  on that server.

See [server settings](/docs/reference/server-settings) for server configuration.
