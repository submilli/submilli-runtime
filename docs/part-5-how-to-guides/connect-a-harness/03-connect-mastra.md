---
docType: how-to
title: "Connect Mastra to Submilli"
description: "Create a Mastra agent that uses Submilli for permitted code execution."
showDescription: false
slug: how-to/connect-mastra
sidebar:
  order: 3
authorship:
  label: ai-generated
  confirmed: true
  contentHash: "d725c02298582d41938429ee70b71141b999659a597f1edf353f065fa9b33827"
  confirmedAt: "2026-10-18T13:09:05.218103+00:00"
# Unpublished AI draft. Human prose rewrite required before publication.
---

Create a fresh Mastra agent whose execution tools run through Submilli,
then try an allowed operation and a denied operation with the same Blueprint.

## Use your coding agent

Open a project directory in your coding agent.
[Install the Submilli skill](/docs/install#the-skill) if needed, then restart
the coding agent to load it. Give it this prompt:

```text agent-prompt
Use the Submilli skill to create a fresh Mastra demo agent connected
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

Remove constructor tools, workspace tools, agents and workflows, and other
per-call tools or toolsets. Supply only the Submilli toolset shown here.

</details>

## Set up manually

Use Mastra's [MCP client](https://mastra.ai/docs/connections/mcp) to connect
your agent to the Submilli runtime. MCP lets an agent discover and call tools
on a server.

:::note[Before you start]
You’ll need:

- A Mastra project with a configured model.
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

### Install the MCP client

In your Mastra project, install the client:

```sh
npm install @mastra/mcp@^1.18.0
```

### Connect Submilli to your agent

Create the client in a dedicated demo module. The URL selects
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

<details>
<summary>Using a different Blueprint with variables or secrets</summary>

If your Blueprint requires variables, add a `submilli-variables` header
with those bindings. See [MCP variable bindings](/docs/application#three-decisions-the-model-has-no-part-in).
If your Blueprint requires secrets from your application (the `harness`
source), add `submilli-secrets` containing a base64url-encoded JSON object
of their names and values. See
[Blueprint secrets](/docs/reference/blueprint-file#secrets).

</details>

Create a fresh agent using your configured `model`. Pass only Submilli's
toolset for this run. Use the message under **Try it out** as `messages`.

```typescript title="Fresh demo agent and call"
import { Agent } from "@mastra/core/agent";

const agent = new Agent({
  id: "submilli-demo",
  name: "Submilli demo",
  model,
  instructions: "Run requested programs through Submilli and report tool results.",
  tools: {},
});

try {
  const toolsets = await submilli.listToolsets();
  if (!toolsets.submilli) {
    throw new Error("Submilli did not return its tools");
  }
  const result = await agent.generate(messages, { toolsets, maxSteps: 10 });
} finally {
  await submilli.disconnect();
}
```

Keep the connection open until the agent finishes using its tools.

### Verify the connection

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

To inspect a run in code, use the test message as `messages` in your
`agent.generate(messages, ...)` call. Keep its Submilli toolset. Add this
immediately after that call, inside the `try`
block, to see the [generation steps](https://mastra.ai/reference/agents/generate)
in your application's terminal:

```typescript
for (const step of result.steps) {
  console.dir({ calls: step.toolCalls, results: step.toolResults }, { depth: null });
}
console.log(result.text);
```

Look for a tool name ending in `submilli__typescript__execute`. Mastra wraps
tool details in `payload`: the call includes `toolName`, `toolCallId`, and
`args`, and its result includes `toolCallId` and `result`. Inspect the code
argument and the returned value inside those records.

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
