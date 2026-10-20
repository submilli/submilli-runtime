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
  contentHash: "7ca865b2def63de090f815ba8e146a965a35c5105f9a1ebf4948f6473766b8bb"
  confirmedAt: "2026-10-18T13:09:05.218103+00:00"
# Unpublished AI draft. Human prose rewrite required before publication.
---

Create a fresh Claude Agent SDK agent that sends its execution requests to your
existing Submilli server and registered Blueprint, then run a small program and
see its result in your terminal.

## Use your coding agent

Open a project directory in your coding agent.
[Install the Submilli skill](/docs/install#the-skill) if needed, then restart
the coding agent to load it. Give it this prompt:

```text agent-prompt
Use the Submilli skill and this guide to create a fresh Claude Agent SDK
agent in a new or existing project folder. Preserve my existing agent
configuration. Create the complete submilli-agent.mjs file shown in this guide,
including its command-line prompt and tool-result logging. Connect it to my
existing Submilli server URL, server token, and registered Blueprint. Configure
the query with `tools: []`, `allowedTools: ["mcp__submilli__*"]`,
`strictMcpConfig: true`, and `settingSources: []`, so Submilli is its only
execution route. Inspect my provider/model setup and ask only for essential
missing choices; do not guess values or reveal secrets. Install the dependencies
shown here and prepare the file without making a model request. Leave it ready
for the exact Try it out command. Tell me any remaining human setup.
```

After setup, [try it out](#try-it-out). To set up the same file yourself,
follow the manual path.

<details>
<summary>Connecting an existing agent</summary>

Configure the query's `mcpServers.submilli` entry with the server URL and
Blueprint path `SUBMILLI_SERVER/mcp/SUBMILLI_BLUEPRINT`, plus the bearer token
in `SUBMILLI_SERVER_TOKEN`. Set `tools: []`, `strictMcpConfig: true`, and
`settingSources: []`; keep `allowedTools: ["mcp__submilli__*"]` and the
Submilli entry in `mcpServers`. Remove other MCP servers and any subagent,
plugin, or hook configuration that adds another execution route. These are
Claude Agent SDK options; the Blueprint still governs what the server can
execute.

</details>

## Set up manually

Before this guide, use the [Submilli install guide](/docs/install), [server
guide](/docs/server/run-the-server), and [Blueprint registration
guide](/docs/server/register-a-blueprint) if you still need to prepare the
service. This page assumes you have a running server, a token it accepts, and a
Blueprint already registered on it.
You also need Node.js 22 or later and model access for Claude Agent SDK.

### Prepare the agent folder

Open the folder where you want to save the agent and install its dependencies:

```sh
npm init -y
npm install @anthropic-ai/claude-agent-sdk
```

Set these environment variables in this terminal using your existing server,
token, Blueprint, and model configuration:

| Variable | Value |
| --- | --- |
| `SUBMILLI_SERVER` | The server URL where your Blueprint is registered |
| `SUBMILLI_SERVER_TOKEN` | A token accepted by that server |
| `SUBMILLI_BLUEPRINT` | The name of your registered Blueprint |
| `CLAUDE_MODEL` | A Claude model ID supported by your configured [Agent SDK authentication](https://platform.claude.com/docs/en/agent-sdk/overview). Configure that authentication in this terminal before running the file. |

The model credentials are separate from the Submilli server token. Reuse your
chosen provider’s setup. Do not put credentials in the file you save next.

### Save the runnable agent

Save this complete file as `submilli-agent.mjs` in the folder you opened.

The query disables built-in tools and project settings, and restricts MCP to the configured Submilli server.

```javascript title="submilli-agent.mjs"
import { query } from "@anthropic-ai/claude-agent-sdk";

function required(name) {
  const value = process.env[name];
  if (!value) throw new Error(`Set ${name} in this terminal before running this file.`);
  return value;
}
const server = required("SUBMILLI_SERVER").replace(/\/$/, "");
const blueprint = required("SUBMILLI_BLUEPRINT");
const token = required("SUBMILLI_SERVER_TOKEN");
const model = required("CLAUDE_MODEL");
const prompt = process.argv.slice(2).join(" ") || 'Run this program through Submilli and report its result: export function main(): number { return 2 + 2; }';
const agent = query({
  prompt,
  options: {
    model,
    mcpServers: {
      submilli: {
        type: "http", url: `${server}/mcp/${blueprint}`,
        headers: { Authorization: `Bearer ${token}` },
      },
    },
    allowedTools: ["mcp__submilli__*"],
    tools: [], strictMcpConfig: true, settingSources: [], maxTurns: 10,
  },
});
try {
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
} finally {
  agent.close();
}
```

## Try it out

With the environment variables from setup, run the saved file with its
built-in 2 + 2 prompt:

```sh
node submilli-agent.mjs
```

Look for a Submilli execution tool call containing `2 + 2` and its returned
value of `4`. The tool call and returned value prove that the program ran
through the configured server and Blueprint.

## Troubleshooting

- Connection refused: check `SUBMILLI_SERVER` and whether the server is running.
- `401`: use a token accepted by that server.
- Blueprint not found: check `SUBMILLI_BLUEPRINT` against the registered name.
- Model authentication error: check the model provider’s credentials in this terminal.
- Tool result but no final answer: read the SDK error or turn-limit result printed by the file.

See [server settings](/docs/reference/server-settings) for server configuration.
