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
  contentHash: "b5f0c96e8a46a944b79f2bbea1ee59aee708f91f9d064c15c6b56adcfa7a504c"
  confirmedAt: "2026-10-18T13:09:05.218103+00:00"
# Unpublished AI draft. Human prose rewrite required before publication.
---

Create a fresh Mastra agent with only Submilli execution tools, then
run a small program and see its result in your terminal.

## Use your coding agent

Open a project directory in your coding agent.
[Install the Submilli skill](/docs/install#the-skill) if needed, then restart
the coding agent to load it. Give it this prompt:

```text agent-prompt
Use the Submilli skill and this guide to create a fresh Mastra demo
in a new submilli-demo directory. Preserve my existing agent configuration.
Create the complete submilli-agent.mjs file shown in this guide, including its
command-line prompt and tool-result logging. Give it only Submilli execution
tools and apply this guide’s framework-specific exclusions.
Inspect my project’s provider/model setup. Help me choose any missing model
configuration and credentials without guessing values or revealing secrets.
Find a local Submilli server and token, or help me prepare them.
Download https://submilli.ai/docs/examples/permission-demo/basic-blueprint.yaml and
register it using submilli server blueprint apply. Use harness-demo as
SUBMILLI_BLUEPRINT. This first example requires no Package.
Install the dependencies shown here. Verify the server and basic program
without making a model request. Leave the runnable file ready for the exact
Try it out command, and tell me the directory to open and any remaining
human setup.
```

After setup, [try it out](#try-it-out). To set up the same file yourself,
follow the manual path.

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

You need the [Submilli CLI](/docs/install), a running
[local server](/docs/server/run-the-server), and a token it accepts.
You also need Node.js 22 or later and model access for Mastra.

### Prepare a demo directory

Run these commands from the directory where you keep your projects:

```sh
mkdir submilli-demo
cd submilli-demo
npm init -y
npm install @mastra/core @mastra/mcp@^1.18.0
curl -fsSLo blueprint.yaml https://submilli.ai/docs/examples/permission-demo/basic-blueprint.yaml
```

Keep this terminal in `submilli-demo` for the rest of the guide.
Set these environment variables in this terminal using your own configuration:

| Variable | Value |
| --- | --- |
| `SUBMILLI_SERVER` | Your server URL, such as `http://127.0.0.1:8128` |
| `SUBMILLI_SERVER_TOKEN` | A token accepted by that server |
| `SUBMILLI_BLUEPRINT` | `harness-demo` |
| `MASTRA_MODEL` | A `provider/model` ID from [Mastra’s model router](https://mastra.ai/models). Configure the credential environment variable documented for that provider in this terminal. |

The model credentials are separate from the Submilli server token. Reuse your
chosen provider’s setup. Do not put credentials in the file you save next.
Register the downloaded Blueprint:

```sh
export SUBMILLI_BLUEPRINT=harness-demo
SUBMILLI_SERVER_URL="$SUBMILLI_SERVER" submilli server blueprint apply blueprint.yaml
```

### Save the runnable agent

Save this complete file as `submilli-demo/submilli-agent.mjs`. If you are already
inside `submilli-demo`, save it as `submilli-agent.mjs`.

```javascript title="submilli-agent.mjs"
import { Agent } from "@mastra/core/agent";
import { MCPClient } from "@mastra/mcp";

function required(name) {
  const value = process.env[name];
  if (!value) throw new Error(`Set ${name} in this terminal before running this file.`);
  return value;
}
const server = required("SUBMILLI_SERVER").replace(/\/$/, "");
const blueprint = required("SUBMILLI_BLUEPRINT");
const token = required("SUBMILLI_SERVER_TOKEN");
const model = required("MASTRA_MODEL");
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
  if (process.argv[2] !== "--check") {
    const agent = new Agent({
      id: "submilli-demo", name: "Submilli demo", model, tools: {},
      instructions: "Run requested programs through Submilli and report tool results.",
    });
    const toolsets = await submilli.listToolsets();
    if (!toolsets.submilli) throw new Error("Submilli did not return its tools");
    const prompt = process.argv.slice(2).join(" ") || 'Run this program through Submilli and report its result: export function main(): number { return 2 + 2; }';
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

Open a terminal in `submilli-demo` with the environment variables from setup.
Run the saved file:

```sh
node submilli-agent.mjs
```

Look for a Submilli execution tool call containing `2 + 2` and its returned
value of `4`. This verifies that the agent ran the program through Submilli.
An answer of `4` without a tool call does not verify the connection.

## Troubleshooting

- Connection refused: check `SUBMILLI_SERVER` and whether the server is running.
- `401`: use a token accepted by that server.
- Blueprint not found: check `SUBMILLI_BLUEPRINT` against the registered name.
- Model authentication error: check the model provider’s credentials in this terminal.
- Tool result but no final answer: read the SDK error or turn-limit result printed by the file.

See [server settings](/docs/reference/server-settings) for server configuration.
