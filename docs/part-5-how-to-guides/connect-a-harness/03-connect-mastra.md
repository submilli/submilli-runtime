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
  contentHash: "3db7d492d0881ecf7251b2ded5a78baf6c82f91cbf1cc877207c0ad8d6e80058"
  confirmedAt: "2026-10-18T13:09:05.218103+00:00"
# Unpublished AI draft. Human prose rewrite required before publication.
---

Create a fresh Mastra agent that exposes only the Submilli MCP tools, then run
a small program and see the tool result in your terminal.

## Use your coding agent

Choose an icon to open a prefilled prompt. Select your project, review, and send.

```text agent-prompt
Read https://submilli.ai/docs/how-to/connect-mastra/ as the source for this setup.

Inspect my project and use this guide to create a fresh Mastra agent in the
current project (or an empty neutral directory if I ask for one). Preserve my
existing agent configuration. Create the complete `submilli-agent.mjs` file
shown in this guide, including its command-line prompt and tool-result
logging. Configure the fresh agent with only the Submilli MCP toolset.
Inspect the installed Mastra source and this project’s configuration to verify
the MCP client, toolset, agent, workflow, and delegation APIs before claiming
that another route is excluded. Remove other provider tools, workspace or file
tools, workflow tools, and delegation or subagent routes from this fresh agent.
Use the model shown here unless my project already has a valid model setup.
Ask only for missing credentials, server URL, server token, or registered
Blueprint name, without
guessing values or revealing secrets. Reuse the running Submilli server, token,
and registered Blueprint I provide; do not download or register a Blueprint.
Install only the dependencies shown here. Verify the server and Blueprint connection with `--check`
without making a model request. Leave the runnable file ready
for the exact Try it out command and report the file path and any remaining
human setup.
```

After setup, [try it out](#try-it-out). To set up the same file yourself,
follow the manual path.

<details>
<summary>Connecting an existing agent</summary>

1. In the existing Mastra `Agent` configuration, clear `tools`, `workflows`,
   `agents`, and `workspace`.
2. Remove `clientTools` from `generate` or `stream` options.
3. Replace other `toolsets` with the `submilli` toolset returned by
   `submilli.listToolsets()`.
4. Check those settings in the installed Mastra version before running the
   agent.

A Blueprint governs programs run through Submilli. Another execution route
would bypass that boundary.

</details>

## Set up manually

### Prerequisites

Before this guide, install the [Submilli CLI](/docs/install), start a
[server](/docs/server/run-the-server), and register a Blueprint using the
[server registration guide](/docs/server/register-a-blueprint). Keep the server
running and have its token and the registered Blueprint name available. You
also need Node.js 22 or later and model access for Mastra.

### Install the Mastra dependencies

Run these commands from the directory containing the agent file:

```sh
npm init -y
npm install @mastra/core @mastra/mcp@^1.18.0
```

Replace the example values, then run these commands in bash or zsh. The file
reads this terminal's environment and does not automatically load `.env` files.
Use your running server's base URL for `SUBMILLI_SERVER`. For example, use
`http://127.0.0.1:8128` only when that server runs on this machine at port 8128.

```sh
export SUBMILLI_SERVER='your-server-base-url'
export SUBMILLI_SERVER_TOKEN='your-server-token'
export SUBMILLI_BLUEPRINT='your-registered-blueprint'
export OPENAI_API_KEY='your-openai-api-key'
```

### Create the agent file

Create `submilli-agent.mjs` in the directory where you ran the install command,
and paste the code below.

```javascript title="submilli-agent.mjs"
import { Agent } from "@mastra/core/agent";
import { MCPClient } from "@mastra/mcp";

const args = process.argv.slice(2);
const checking = args.length === 1 && args[0] === "--check";
const prompt = args.join(" ");
if (!checking && !prompt.trim()) {
  console.error("Usage: node submilli-agent.mjs '<message>' (or --check)");
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
const submilli = new MCPClient({
  id: "submilli",
  servers: {
    submilli: {
      url: new URL(`${server}/mcp/${blueprint}`),
      requestInit: { headers: { Authorization: `Bearer ${token}` } },
    },
  },
});
try {
  const tools = await submilli.listTools();
  if (!Object.keys(tools).some(name => name.endsWith("submilli__typescript__execute"))) {
    throw new Error("Submilli's execute tool was not found");
  }
  console.log("Connected to Submilli.");
  if (!checking) {
    const agent = new Agent({
      // Change the provider/model ID here to use another model.
      id: "submilli-agent", name: "Submilli agent", model: "openai/gpt-5-mini", tools: {},
      instructions: "Run requested programs through Submilli and report tool results.",
    });
    const toolsets = await submilli.listToolsets();
    if (!toolsets.submilli) throw new Error("Submilli did not return its tools");
    if (Object.keys(toolsets).some(name => name !== "submilli")) {
      throw new Error("The fresh agent has a non-Submilli toolset");
    }

    const result = await agent.generate(prompt, { toolsets, maxSteps: 10 });
    for (const step of result.steps) {
      console.dir({ calls: step.toolCalls, results: step.toolResults }, { depth: null });
    }
    console.log("Agent answer:", result.text);
  }
} finally {
  await submilli.disconnect();
}
```

Check the connection without a model request:

```sh
node submilli-agent.mjs --check
```

Look for `Connected to Submilli.`. This confirms the server and Blueprint are reachable.

## Try it out

Run the saved file from its directory with the environment variables from setup:

```sh
node submilli-agent.mjs \
  'Run this program through Submilli and report its result:
export function main(): number { return 2 + 2; }'
```

Look for one `submilli__typescript__execute` call containing `2 + 2` and its
returned value of `4`. This verifies that the agent ran the program through
Submilli. An answer of `4` without that tool result does not verify the
connection.

## Troubleshooting

- Connection refused: check `SUBMILLI_SERVER` and whether the server is running.
- `401`: use a token accepted by that server.
- Blueprint not found: check `SUBMILLI_BLUEPRINT` against the registered name.
- Model authentication error: check the model provider’s credentials in this terminal.
- Tool result but no final answer: read the SDK error or turn-limit result printed by the file.

See [server settings](/docs/reference/server-settings) for server configuration.
