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
  contentHash: "e40a02bcac3e1921d6b0b9d583d5fe4470bedf13eb7e2f2cef2ab7fca28dd5db"
  confirmedAt: "2026-10-18T13:09:05.218103+00:00"
# Unpublished AI draft. Human prose rewrite required before publication.
---

Create a fresh Claude Agent SDK agent that sends its execution requests to your
existing Submilli server and registered Blueprint, then run a small program and
see its result in your terminal.

## Use your coding agent

Choose an icon to open a prefilled prompt. Select your project, review, and send.

```text agent-prompt
Read https://submilli.ai/docs/how-to/connect-claude-agent-sdk/ as the source for this setup.

Inspect my project and use this guide to create a fresh Claude Agent SDK
agent in a new or existing project folder. Preserve my existing agent
configuration. Create the complete submilli-agent.mjs file shown in this guide,
including its command-line prompt and tool-result logging. Connect it to my
existing Submilli server URL, server token, and registered Blueprint. Configure
the query with `tools: []`, `allowedTools: ["mcp__submilli__*"]`,
`strictMcpConfig: true`, and `settingSources: []`, so Submilli is its only
execution route. Use the model shown here unless my project already has a valid model setup.
Ask only for missing credentials or Submilli connection values; do not guess
values or reveal secrets. Install the dependencies
shown here and prepare the file without making a model request. Leave it ready
for the exact Try it out command. Tell me any remaining human setup.
```

After setup, [try it out](#try-it-out). To set up the same file yourself,
follow the manual path.

<details>
<summary>Connecting an existing agent</summary>

1. Configure the query's `mcpServers.submilli` entry with the server URL and
   Blueprint path `SUBMILLI_SERVER/mcp/SUBMILLI_BLUEPRINT`.
2. Use the bearer token in `SUBMILLI_SERVER_TOKEN`.
3. Set `tools: []`, `strictMcpConfig: true`, and `settingSources: []`.
4. Keep `allowedTools: ["mcp__submilli__*"]` and the Submilli entry in
   `mcpServers`.
5. Remove other MCP servers and any subagent, plugin, or hook configuration
   that adds another execution route.

These are Claude Agent SDK options. The Blueprint still governs what the
server can execute.

</details>

## Set up manually

### Prerequisites

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

Replace the example values, then run these commands in bash or zsh. The file
reads this terminal's environment and does not automatically load `.env` files.
Use your running server's base URL for `SUBMILLI_SERVER`. For example, use
`http://127.0.0.1:8128` only when that server runs on this machine at port 8128.

```sh
export SUBMILLI_SERVER='your-server-base-url'
export SUBMILLI_SERVER_TOKEN='your-server-token'
export SUBMILLI_BLUEPRINT='your-registered-blueprint'
```

Configure [Claude Agent SDK authentication](https://platform.claude.com/docs/en/agent-sdk/overview)
in this terminal before running the file. Keep credentials out of the file.

### Create the agent file

The query disables built-in tools and project settings, and restricts MCP to
the configured Submilli server.

Create `submilli-agent.mjs` in the folder where you installed the dependencies,
and paste the code below.

```javascript title="submilli-agent.mjs"
import { query } from "@anthropic-ai/claude-agent-sdk";

const args = process.argv.slice(2);
const prompt = args.join(" ");
if (!prompt.trim()) {
  console.error("Usage: node submilli-agent.mjs '<message>'");
  process.exit(1);
}

function required(name) {
  const value = process.env[name];
  if (!value) throw new Error(`Set ${name} in this terminal before running this file.`);
  return value;
}
const server = required("SUBMILLI_SERVER").replace(/\/$/, "");
const blueprint = required("SUBMILLI_BLUEPRINT");
const token = required("SUBMILLI_SERVER_TOKEN");

const agent = query({
  prompt,
  options: {
    // Change this model ID to use another supported Claude model.
    model: "claude-sonnet-5",
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

With the environment variables from setup, run the saved file with this
message:

```sh
node submilli-agent.mjs \
  'Run this program through Submilli and report its result:
export function main(): number { return 2 + 2; }'
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
