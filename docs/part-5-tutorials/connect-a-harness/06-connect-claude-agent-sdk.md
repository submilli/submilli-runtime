---
title: "Connect Claude Agent SDK"
description: "Give a Claude Agent SDK agent Submilli's MCP tools and run its TypeScript programs under a registered Blueprint."
slug: tutorials/connect-claude-agent-sdk
docType: how-to
sidebar:
  order: 6
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "16361a428ce10c5946a2049f3f7c373c387128391af268fd830efcf07a9c69a0"
  confirmedAt: "2026-10-20T09:28:54Z"
---

## Use your coding agent

Choose an icon to open a prefilled prompt. Select your project, review, and send.

```text agent-prompt
Read https://submilli.ai/docs/tutorials/connect-claude-agent-sdk/ as the source for this setup.

Inspect my project and follow this guide to create agent.ts.
Reuse valid local Submilli connection settings and provider credentials. If
SUBMILLI_SERVER, SUBMILLI_SERVER_TOKEN, or SUBMILLI_BLUEPRINT is missing, ask
for that value or its local configuration source. Do not invent values,
expose secrets, create a Blueprint, or change the server. Ask for missing
provider credentials without printing them. Install only the
missing dependencies shown in this guide. Preserve its tool restrictions and
session cleanup. Prepare the files without running the agent or making a
model request.
Leave the files ready for the exact Try it out command.
```

After setup, [try it out](#try-it-out).

## Set up manually

The files read this terminal's environment; they do not automatically load
`.env` files. Use your running server's base URL and registered Blueprint.
Replace these values before running the files:

```sh
export SUBMILLI_SERVER='your-server-base-url'
export SUBMILLI_SERVER_TOKEN='your-server-token'
export SUBMILLI_BLUEPRINT='your-registered-blueprint'
```

This guide connects a Claude Agent SDK agent to Submilli over MCP. The SDK
keeps control of the agent loop, and Submilli executes the TypeScript programs
under a Blueprint your application selects.

Before you start:

- [Run `submilli-server`](/docs/server/run-the-server) and keep
  `SUBMILLI_SERVER_TOKEN` exported.
- [Register a Blueprint](/docs/server/register-a-blueprint), then set
  `SUBMILLI_BLUEPRINT` to its registered name. The calculation below assumes
  the Blueprint has no required variables. The sample calls no external tools.
- Install Node.js 20 or later and configure the Claude Agent SDK with an
  Anthropic API key, Amazon Bedrock, Google Vertex AI, or a Claude Code login.

### Start the project

```sh
mkdir claude-agent-submilli && cd claude-agent-submilli
npm init -y
npm pkg set type=module
npm install @anthropic-ai/claude-agent-sdk @modelcontextprotocol/sdk
npm install --save-dev tsx typescript @types/node
```

### Connect the agent

Create `agent.ts` in the project directory and paste the code below:

```typescript title="agent.ts"
import { query, type Options } from "@anthropic-ai/claude-agent-sdk";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StreamableHTTPClientTransport } from "@modelcontextprotocol/sdk/client/streamableHttp.js";
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { CallToolRequestSchema, ListToolsRequestSchema } from "@modelcontextprotocol/sdk/types.js";

const prompt = process.argv.slice(2).join(" ");
if (!prompt.trim()) throw new Error("Usage: npx tsx agent.ts '<message>'");

const server = process.env.SUBMILLI_SERVER ?? "http://127.0.0.1:8128";
const blueprint = required("SUBMILLI_BLUEPRINT");
const endpoint = new URL(`${server}/mcp/${encodeURIComponent(blueprint)}`);
const requestInit = { headers: { Authorization: `Bearer ${required("SUBMILLI_SERVER_TOKEN")}` } };
const transport = new StreamableHTTPClientTransport(endpoint, { requestInit });
const client = new Client({ name: "claude-submilli", version: "1.0.0" });
const abortController = new AbortController();
let closeOnSignal: Promise<void> | undefined;
const interrupt = () => stop(130);
const terminate = () => stop(143);
process.on("SIGINT", interrupt);
process.on("SIGTERM", terminate);

function stop(exitCode: number) {
  if (abortController.signal.aborted) return;
  process.exitCode = exitCode;
  abortController.abort();
  closeOnSignal = client.close();
  // Observe rejection now; the finally block awaits and reports it.
  void closeOnSignal.catch(() => {});
}

try {
  await client.connect(transport, { signal: abortController.signal });
  const catalog = await client.listTools({}, { signal: abortController.signal });
  abortController.signal.throwIfAborted();
  if (!catalog.tools.some((tool) => tool.name === "submilli__typescript__execute")) {
    throw new Error("Submilli's execute tool is unavailable");
  }

  const mcp = new McpServer(
    { name: "submilli", version: "1.0.0" },
    { capabilities: { tools: {} }, instructions: client.getInstructions() },
  );
  mcp.server.setRequestHandler(ListToolsRequestSchema, (request) => client.listTools(request.params));
  mcp.server.setRequestHandler(CallToolRequestSchema, (request) => client.callTool(request.params));
  const options: Options = {
    abortController,
    // Change this model ID to use another supported Claude model.
    model: "claude-sonnet-5",
    systemPrompt: "Use Submilli to do calculations in TypeScript. Return the execution result.",
    mcpServers: { submilli: { type: "sdk", name: "submilli", instance: mcp } },
    strictMcpConfig: true,
    allowedTools: ["mcp__submilli__*"],
    tools: [],
    settingSources: [],
    maxTurns: 10,
  };
  const agent = query({ prompt, options });
  try {
    for await (const message of agent) {
      // Keep the SDK events visible so the tool call and its result can be checked.
      console.dir(message, { depth: null });
      if (message.type === "result" && message.is_error) {
        throw new Error("Claude reported an error; see the SDK result above");
      }
    }
  } finally {
    agent.close();
    await mcp.close();
  }
} catch (error) {
  if (!abortController.signal.aborted) throw error;
} finally {
  try {
    if (transport.sessionId) {
      // Initialization failures can auto-close the original transport.
      const cleanup = new StreamableHTTPClientTransport(endpoint, {
        requestInit, sessionId: transport.sessionId,
      });
      if (transport.protocolVersion) cleanup.setProtocolVersion(transport.protocolVersion);
      try {
        await cleanup.terminateSession();
      } finally {
        await cleanup.close();
      }
    }
  } finally {
    try {
      await (closeOnSignal ?? client.close());
    } finally {
      process.off("SIGINT", interrupt);
      process.off("SIGTERM", terminate);
    }
  }
}

function required(name: string): string {
  const value = process.env[name];
  if (!value) throw new Error(`${name} is not set`);
  return value;
}
```

`strictMcpConfig` limits MCP servers to the entry declared here.
`allowedTools` allows calls to that server without an interactive permission
prompt. The SDK can also give the agent shell, filesystem, web, and user-level
MCP tools that run outside Submilli. `tools: []` and `settingSources: []` keep
this example limited to the configured Submilli server.

The exact boundary in this example is one `mcpServers.submilli` entry, the
`mcp__submilli__*` allowlist, `tools: []`, and `settingSources: []`. No Claude
built-in tool or delegation tool is enabled. Direct Claude Agent SDK tools can
still be added when the application needs them, but those calls bypass
Submilli and keep the SDK's own authorization and approval path.
Current Submilli MCP connections do not support application-tool callbacks. To
let a TypeScript program call an application tool through Submilli, use an HTTP
session with the `tools` field on `POST /v1/sessions`, grant `tools.call` in the
Blueprint, and answer each suspended run through
`POST /v1/runs/{run_id}/resume`. See the [HTTP API reference](/docs/reference/http-api#application-tools).

If the selected Blueprint declares required variables, add a
`submilli-variables` header whose values come from trusted application state,
as described in [Your application](/docs/application#three-decisions-the-model-has-no-part-in).
Never take those values from the conversation.

## Try it out

```sh
npx tsx agent.ts \
  'Run this program through Submilli and report its result:
export function main(): number { return 2 + 2; }'
```

The script prints the SDK messages. Confirm that they contain a call to
Submilli's execute tool and a tool result containing `4`; the final
assistant text alone does not verify where the calculation ran. This example
uses the model provider for the agent conversation. The TypeScript program
itself only computes a value and calls no Package, application tool, or
external service.

A Blueprint applies only to programs executed through Submilli. If you enable
other Claude Agent SDK tools, they keep the SDK's own access and approval
rules. See [How Submilli works with
harnesses](/docs/tutorials/connect-a-harness) for the full boundary.
