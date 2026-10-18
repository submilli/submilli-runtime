---
title: "Connect Claude Agent SDK to Submilli"
description: "Add Submilli to the MCP servers used by an existing Claude Agent SDK harness."
slug: how-to/connect-claude-agent-sdk
sidebar:
  order: 6
authorship:
  label: ai-generated
  confirmed: true
  contentHash: "3a0be69be8d8baf62a748c41c313c776903ae633a92eda14e407f8b176b7fb09"
  confirmedAt: "2026-10-18T13:09:05.218103+00:00"
# Unpublished AI draft. Human prose rewrite required before publication.
---

Add Submilli to the MCP servers used by your Claude Agent SDK harness.
MCP (Model Context Protocol) connects the harness to external tools.
A harness is the software that runs your agent and handles its tool calls.

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
