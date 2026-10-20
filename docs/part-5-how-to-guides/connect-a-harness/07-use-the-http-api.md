---
docType: how-to
title: "Connect Vercel AI SDK to Submilli"
description: "Create a Vercel AI SDK agent that uses Submilli for permitted code execution."
showDescription: false
slug: how-to/use-the-http-api
sidebar:
  order: 7
authorship:
  label: ai-generated
  confirmed: true
  contentHash: "2b5572bf6834c31c00c05f59562f451e4123f0d5ff8248c187a2eed76c21a66c"
  confirmedAt: "2026-10-18T13:09:05.218103+00:00"
# Unpublished AI draft. Human prose rewrite required before publication.
---

Create a fresh Vercel AI SDK agent with only a Submilli execution tool, then run
a small program and see the tool result in your terminal.

## Use your coding agent

Open a project directory in your coding agent.
[Install the Submilli skill](/docs/install#the-skill) if needed, then restart
the coding agent to load it. Give it this prompt:

```text agent-prompt
Read https://submilli.ai/docs/how-to/use-the-http-api/ as the source for this setup.

Use the Submilli skill and this guide to create a fresh Vercel AI SDK agent in
the current project (or an empty neutral directory if I ask for one). Preserve
my existing agent configuration. Create the complete `submilli-agent.mjs` file
shown in this guide, including its command-line prompt and tool-result logging.
Configure the fresh agent with only the Submilli execution tool. Inspect the
installed AI SDK source and this project’s configuration to verify
`generateText`/`streamText` options, `tools`, `prepareStep`, provider options,
and delegation tools before claiming that another route is excluded. Remove
other provider, workspace, file, network, and delegation tools and options from
this fresh agent. Keep my existing provider/model setup; ask only for missing
model configuration, credentials, server URL, server token, or registered
Blueprint name, without guessing values or revealing secrets. Reuse the
running Submilli server, token, and registered Blueprint I provide; do not
download or register a Blueprint. Install only the dependencies shown here.
Verify the server and the basic `2 + 2` program without making a model request.
Leave the runnable file ready for the exact Try it out command and report the
file path and any remaining human setup.
```

After setup, [try it out](#try-it-out). To set up the same file yourself,
follow the manual path.

<details>
<summary>Connecting an existing agent</summary>

1. Replace the existing `generateText` or `streamText` call's `tools` map with
   `{ submilli__typescript__execute: submilli }`.
2. Remove provider-defined and delegation tools from that map.
3. Review `prepareStep` callbacks for changes to `model` or `providerOptions`
   that enable other execution routes.
4. Preserve the provider authentication and model options the chosen model
   needs.

A Blueprint governs programs run through Submilli. Another execution route
would bypass that boundary.

</details>

## Set up manually

### Prerequisites

Before this guide, install the [Submilli CLI](/docs/install), start a
[server](/docs/server/run-the-server), and register a Blueprint using the
[server registration guide](/docs/server/register-a-blueprint). Keep the server
running and have its token and the registered Blueprint name available. You
also need Node.js 22 or later and model access for Vercel AI SDK.

### Install the Vercel AI SDK dependencies

Run these commands from the directory containing the agent file:

```sh
npm init -y
npm install ai@^7.0.0 zod@^4.1.8 @ai-sdk/openai
```

Replace the example values, then run these commands in bash or zsh. The file
reads this terminal's environment and does not automatically load `.env` files.

```sh
export SUBMILLI_SERVER='http://127.0.0.1:8128'
export SUBMILLI_SERVER_TOKEN='your-server-token'
export SUBMILLI_BLUEPRINT='your-registered-blueprint'
export OPENAI_API_KEY='your-openai-api-key'
```

This example uses OpenAI's `gpt-5-mini`. To choose another model, edit the
argument to `openai()` in the file.

### Create the agent file

Create `submilli-agent.mjs` in the directory where you ran the install command,
and paste the code below.

```javascript title="submilli-agent.mjs"
import { generateText, stepCountIs, tool } from "ai";
import { z } from "zod";
import { openai } from "@ai-sdk/openai";

function required(name) {
  const value = process.env[name];
  if (!value) throw new Error(`Set ${name} in this terminal before running this file.`);
  return value;
}
const server = required("SUBMILLI_SERVER").replace(/\/$/, "");
const blueprint = required("SUBMILLI_BLUEPRINT");
const token = required("SUBMILLI_SERVER_TOKEN");
required("OPENAI_API_KEY");
const headers = {
  Authorization: `Bearer ${token}`, "Content-Type": "application/json",
};
const response = await fetch(`${server}/v1/blueprints/${blueprint}/prompt`, { headers });
if (!response.ok) throw new Error(`Cannot read the tool description: ${response.status}`);
const description = await response.json();
async function execute(code) {
  const response = await fetch(`${server}/v1/execute`, {
    method: "POST", headers, body: JSON.stringify({ blueprint, code }),
  });
  if (!response.ok) throw new Error(`Submilli request failed: ${response.status}`);
  const run = await response.json();
  console.dir({ submilliResult: run }, { depth: null });
  if (run.error) throw new Error(JSON.stringify(run.error));
  return JSON.parse(run.result);
}
const submilli = tool({
  description: description.prompt,
  inputSchema: z.object({ code: z.string() }),
  execute: ({ code }) => execute(code),
});
if (process.argv[2] === "--check") {
  const result = await execute('export function main(): number { return 2 + 2; }');
  if (result !== 4) throw new Error(`Expected 4, received ${JSON.stringify(result)}`);
  console.log("Connected to Submilli.");
} else {
  const prompt = process.argv.slice(2).join(" ") || 'Run this program through Submilli and report its result: export function main(): number { return 2 + 2; }';
  const result = await generateText({
    model: openai("gpt-5-mini"), prompt, tools: { submilli__typescript__execute: submilli },
    stopWhen: stepCountIs(10),
  });
  for (const step of result.steps) {
    console.dir({ calls: step.toolCalls, results: step.toolResults }, { depth: null });
  }
  console.log("Agent answer:", result.text);
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
node submilli-agent.mjs
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
