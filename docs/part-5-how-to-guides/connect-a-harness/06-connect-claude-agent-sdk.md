---
title: "Connect Claude Agent SDK to Submilli"
description: "Add Submilli to the MCP servers used by an existing Claude Agent SDK harness."
showDescription: false
slug: how-to/connect-claude-agent-sdk
sidebar:
  order: 6
authorship:
  label: ai-generated
  confirmed: true
  contentHash: "fda1d222fe4b65469cd5c529d31287f9828277ceafce7c244015273af414cf14"
  confirmedAt: "2026-10-18T13:09:05.218103+00:00"
# Unpublished AI draft. Human prose rewrite required before publication.
---

Connect through [MCP](https://code.claude.com/docs/en/agent-sdk/mcp)
(Model Context Protocol) by adding Submilli to your existing `query` options.
MCP lets an agent discover and call tools on a server.

:::note[Before you start]
- An existing Claude Agent SDK project with a configured model.
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
Use the Submilli skill to connect this existing Claude Agent SDK project
to Submilli over MCP. Reuse the prepared server and Blueprint. Read
SUBMILLI_SERVER, SUBMILLI_SERVER_TOKEN, and SUBMILLI_BLUEPRINT from the
local environment or project configuration. Map those values to the
integration's settings.
Keep my model and existing agent configuration. Keep token values out of
source code and responses. Verify the connection without a model request.
```

For manual setup, continue below.

## Install Claude Agent SDK

If it is not already installed in your TypeScript project:

```sh
npm install @anthropic-ai/claude-agent-sdk
```

## Add Submilli to the query options

Add a `submilli` entry under `mcpServers` and allow its tools.
The URL selects the Blueprint, and the header supplies the server token.

In this fragment, `prompt` and `options` are the values from your existing
`query` call. The spread operators preserve your other server and tool settings.

```typescript title="Query configuration (fragment)"
import { query } from "@anthropic-ai/claude-agent-sdk";

const agent = query({
  prompt,
  options: {
    ...options,
    mcpServers: {
      ...options.mcpServers,
      submilli: {
        type: "http",
        url: `${process.env.SUBMILLI_SERVER}/mcp/${process.env.SUBMILLI_BLUEPRINT}`,
        headers: {
          Authorization: `Bearer ${process.env.SUBMILLI_SERVER_TOKEN}`,
        },
      },
    },
    allowedTools: [...(options.allowedTools ?? []), "mcp__submilli__*"],
  },
});
```

Continue processing `agent` messages as in your existing application.
Keep the query open while it uses Submilli, and call `agent.close()` when
you finish or stop consuming the messages.

If your Blueprint requires variables, add a `submilli-variables` header.
See [MCP variable bindings](/docs/application#three-decisions-the-model-has-no-part-in).
If your Blueprint requires secrets from your application (the `harness`
source), add `submilli-secrets` containing a base64url-encoded JSON object
of their names and values. See
[Blueprint secrets](/docs/reference/blueprint-file#secrets).

## Verify the connection

While the query is open, inspect its MCP connections:

```typescript
const servers = await agent.mcpServerStatus();
const submilli = servers.find((server) => server.name === "submilli");
console.log(submilli?.status);
```

The status should become `connected`, and `submilli.tools` should include
`submilli__typescript__execute`. A `pending` status means the SDK is still
connecting. Inspect `submilli.error` if the status becomes `failed`.

## Troubleshooting

- Connection refused: check `SUBMILLI_SERVER` and whether the server is running.
- `401`: use a token accepted by that server.
- Blueprint not found: check `SUBMILLI_BLUEPRINT` against the name registered
  on that server.

See [server settings](/docs/reference/server-settings) for server configuration.
