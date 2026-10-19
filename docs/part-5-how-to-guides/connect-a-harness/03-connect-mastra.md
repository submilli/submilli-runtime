---
docType: how-to
title: "Connect Mastra to Submilli"
description: "Give an existing Mastra agent access to Submilli through MCP."
showDescription: false
slug: how-to/connect-mastra
sidebar:
  order: 3
authorship:
  label: ai-generated
  confirmed: true
  contentHash: "e621ccde9fb1b8bc896611bdd3a22ad2385aca92d90794a5610ce341c2c091e6"
  confirmedAt: "2026-10-18T13:09:05.218103+00:00"
# Unpublished AI draft. Human prose rewrite required before publication.
---

Connect your existing Mastra harness to Submilli for code execution.

## Use your coding agent

Open your existing Mastra project in your coding agent.
[Install the Submilli skill](/docs/install#the-skill) if needed, then restart
the coding agent to load it. Give it this prompt:

```text agent-prompt
Use the Submilli skill to connect this existing Mastra project
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

Use Mastra's [MCP client](https://mastra.ai/docs/connections/mcp) to connect
your agent to the Submilli runtime. MCP lets an agent discover and call tools
on a server.

:::note[Before you start]
You’ll need:

- An existing Mastra project with a configured model.
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


### Install the MCP client

In your Mastra project, install the client:

```sh
npm install @mastra/mcp@^1.18.0
```

### Connect Submilli to your agent

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

To inspect a run in code, use the test message as `messages` in your
`agent.generate(messages, ...)` call. Keep its Submilli toolsets and your
other call options. Add this immediately after that call, inside the `try`
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
If the tool returns `4` but the agent gives no final answer, check your agent's
step or turn limit so it can continue after the tool result.

</details>

## Troubleshooting

- Connection refused: check `SUBMILLI_SERVER` and whether the server is running.
- `401`: use a token accepted by that server.
- Blueprint not found: check `SUBMILLI_BLUEPRINT` against the name registered
  on that server.

See [server settings](/docs/reference/server-settings) for server configuration.
