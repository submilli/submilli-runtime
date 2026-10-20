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
  contentHash: "6f8bd781cdf52e110f2dad35286bce1b4979156dbbbfd94a6f067d2631acb574"
  confirmedAt: "2026-10-20T09:28:54Z"
---

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

## Start the project

```sh
mkdir claude-agent-submilli && cd claude-agent-submilli
npm init -y
npm pkg set type=module
npm install @anthropic-ai/claude-agent-sdk
npm install --save-dev tsx typescript @types/node
```

## Connect the agent

Save this as `agent.ts`:

```typescript title="agent.ts"
import { query, type Options } from "@anthropic-ai/claude-agent-sdk";

const server = process.env.SUBMILLI_SERVER ?? "http://127.0.0.1:8128";
const blueprint = required("SUBMILLI_BLUEPRINT");

const options: Options = {
  model: "claude-sonnet-5",
  systemPrompt: "Use Submilli to do calculations in TypeScript. Return the execution result.",
  mcpServers: {
    submilli: {
      type: "http",
      url: `${server}/mcp/${encodeURIComponent(blueprint)}`,
      headers: {
        Authorization: `Bearer ${required("SUBMILLI_SERVER_TOKEN")}`,
      },
    },
  },
  strictMcpConfig: true,
  allowedTools: ["mcp__submilli__*"],
  tools: [],
  settingSources: [],
  maxTurns: 10,
};

const prompt =
  "Use Submilli to compute the 40th Fibonacci number, with F(0)=0 and F(1)=1.";

for await (const message of query({ prompt, options })) {
  // Keep the SDK events visible so the tool call and its result can be checked.
  console.dir(message, { depth: null });
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

## Run it

```sh
export SUBMILLI_BLUEPRINT=<registered-blueprint-name>
ANTHROPIC_API_KEY=... npx tsx agent.ts
```

The script prints the SDK messages. Confirm that they contain a call to
Submilli's execute tool and a tool result containing `102334155`; the final
assistant text alone does not verify where the calculation ran. This example
uses the model provider for the agent conversation. The TypeScript program
itself only computes a value and calls no Package, application tool, or
external service.

A Blueprint applies only to programs executed through Submilli. If you enable
other Claude Agent SDK tools, they keep the SDK's own access and approval
rules. See [How Submilli works with
harnesses](/docs/tutorials/connect-a-harness) for the full boundary.
