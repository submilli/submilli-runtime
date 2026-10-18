---
title: "Connect Mastra to Submilli"
description: "Give an existing Mastra agent access to Submilli through MCP."
slug: how-to/connect-mastra
sidebar:
  order: 3
authorship:
  label: ai-generated
  confirmed: true
  contentHash: "5f9d580cf0f368b5497a257d0e76746efec7814c4ec29967fe56eea8ec23bfe6"
  confirmedAt: "2026-10-18T13:09:05.218103+00:00"
# Unpublished AI draft. Human prose rewrite required before publication.
---

Connect your Mastra harness to Submilli through MCP (Model Context Protocol),
the protocol Mastra uses to discover and call external tools. A harness is
the software that runs your agent and handles its tool calls.

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

`Connected to Submilli.` confirms that Mastra can discover Submilli's code
execution tool. Keep this request inside the `try` block so the client
disconnects if it fails.

## Troubleshooting

- Connection refused: check `SUBMILLI_SERVER` and whether the server is running.
- `401`: use a token accepted by that server.
- Blueprint not found: check `SUBMILLI_BLUEPRINT` against the name registered
  on that server.

See [server settings](/docs/reference/server-settings) for server configuration.
