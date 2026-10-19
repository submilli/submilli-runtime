---
docType: how-to
title: "Connect Claude Agent SDK to Submilli"
description: "Add Submilli to the MCP servers used by an existing Claude Agent SDK harness."
showDescription: false
slug: how-to/connect-claude-agent-sdk
sidebar:
  order: 6
authorship:
  label: ai-generated
  confirmed: true
  contentHash: "bbb59ea9acd349aaf0d5047ff578b3adb07b6c87fdf6f77f540aa50d6a38d876"
  confirmedAt: "2026-10-18T13:09:05.218103+00:00"
# Unpublished AI draft. Human prose rewrite required before publication.
---

Connect your existing Claude Agent SDK harness to Submilli for code execution.

## Use your coding agent

Open your existing Claude Agent SDK project in your coding agent.
[Install the Submilli skill](/docs/install#the-skill) if needed, then restart
the coding agent to load it. Give it this prompt:

```text agent-prompt
Use the Submilli skill to connect this existing Claude Agent SDK project
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

Connect through [MCP](https://code.claude.com/docs/en/agent-sdk/mcp) by adding
Submilli to your existing `query` options. MCP lets an agent discover and call
tools on a server.

:::note[Before you start]
You’ll need:

- An existing Claude Agent SDK project with a configured model.
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


### Install Claude Agent SDK

If it is not already installed in your TypeScript project:

```sh
npm install @anthropic-ai/claude-agent-sdk
```

### Add Submilli to the query options

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

### Verify the connection

While the query is open, inspect its MCP connections:

```typescript
const servers = await agent.mcpServerStatus();
const submilli = servers.find((server) => server.name === "submilli");
console.log(submilli?.status);
```

The status should become `connected`, and `submilli.tools` should include
`submilli__typescript__execute`. A `pending` status means the SDK is still
connecting. Inspect `submilli.error` if the status becomes `failed`.

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

To inspect a run in code, set `prompt` to the test message in your
`query({ prompt, options: ... })` call. Keep the configured MCP server and
allowed tools. In the loop that already consumes `agent`, add this logging
alongside your application's existing message handling:

```typescript
for await (const message of agent) {
  if (message.type === "assistant" || message.type === "user") {
    const content = message.message.content;
    if (Array.isArray(content)) {
      for (const block of content) {
        if (block.type === "tool_use" || block.type === "tool_result") {
          console.dir(block, { depth: null });
        }
      }
    }
  }
  if (message.type === "result") console.dir(message, { depth: null });
}
```

Use one message loop, not two competing consumers, and close the query when
it finishes. In your application's terminal, find the `tool_use` block whose
name ends in `submilli__typescript__execute`. With the `submilli` server name
used here, Claude prefixes MCP tool names with `mcp__submilli__`. Its `input`
contains the program. The `tool_result` block refers to the call's `id` as
`tool_use_id`; inspect its content for `4` and check that `is_error` is not true.

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
