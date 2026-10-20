---
docType: how-to
title: "Connect Claude Agent SDK to Submilli"
description: "Create a Claude Agent SDK agent that uses Submilli for permitted code execution."
showDescription: false
slug: how-to/connect-claude-agent-sdk
sidebar:
  order: 6
authorship:
  label: ai-generated
  confirmed: true
  contentHash: "715cb3484cd7fe9ee1df63a9946e27e9fb2101cce434bec54db42fe3313781a0"
  confirmedAt: "2026-10-18T13:09:05.218103+00:00"
# Unpublished AI draft. Human prose rewrite required before publication.
---

Create a fresh Claude Agent SDK agent whose execution tools run through Submilli,
then try an allowed operation and a denied operation with the same Blueprint.

## Use your coding agent

Open a project directory in your coding agent.
[Install the Submilli skill](/docs/install#the-skill) if needed, then restart
the coding agent to load it. Give it this prompt:

```text agent-prompt
Use the Submilli skill to create a fresh Claude Agent SDK demo agent connected
to Submilli through MCP. Write dedicated demo files without replacing
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

Use `tools: []`, `strictMcpConfig: true`, and `settingSources: []`.
Keep only Submilli in `mcpServers` and `allowedTools`, and remove subagent
and plugin or hook configurations that add another execution route.

</details>

## Set up manually

Connect through [MCP](https://code.claude.com/docs/en/agent-sdk/mcp) with a fresh query configured
to use only Submilli tools. MCP lets an agent discover and call
tools on a server.

:::note[Before you start]
You’ll need:

- A Claude Agent SDK project with a configured model.
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

### Install Claude Agent SDK

If it is not already installed in your TypeScript project:

```sh
npm install @anthropic-ai/claude-agent-sdk
```

### Create the demo query

Create a dedicated query with only Submilli's tools. Use the message under
**Try it out** as `prompt`. `tools: []` disables built-in tools, and
`strictMcpConfig` restricts MCP to the server configured here.

```typescript title="Query configuration (fragment)"
import { query } from "@anthropic-ai/claude-agent-sdk";

const agent = query({
  prompt,
  options: {
    mcpServers: {
      submilli: {
        type: "http",
        url: `${process.env.SUBMILLI_SERVER}/mcp/${process.env.SUBMILLI_BLUEPRINT}`,
        headers: {
          Authorization: `Bearer ${process.env.SUBMILLI_SERVER_TOKEN}`,
        },
      },
    },
    allowedTools: ["mcp__submilli__*"],
    tools: [],
    strictMcpConfig: true,
    settingSources: [],
    maxTurns: 10,
  },
});
```

Process `agent` messages in one loop, as shown under **Try it out**.
Keep the query open while it uses Submilli, and call `agent.close()` when
you finish or stop consuming the messages.

<details>
<summary>Using a different Blueprint with variables or secrets</summary>

If your Blueprint requires variables, add a `submilli-variables` header.
See [MCP variable bindings](/docs/application#three-decisions-the-model-has-no-part-in).
If your Blueprint requires secrets from your application (the `harness`
source), add `submilli-secrets` containing a base64url-encoded JSON object
of their names and values. See
[Blueprint secrets](/docs/reference/blueprint-file#secrets).

</details>

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
`tool_use_id`; inspect its content for the allowed charges or the permission error.

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
