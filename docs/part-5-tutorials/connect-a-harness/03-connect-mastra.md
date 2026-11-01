---
title: "Connect Mastra"
description: "Connect a Mastra agent to a registered Submilli Blueprint with @submilli/mastra."
slug: tutorials/connect-mastra
docType: how-to
sidebar:
  order: 3
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "c0cb4b851d38032c4b8e74d576991da5fb70f1bd64b2f84f271075fc426b563d"
  confirmedAt: "2026-10-20T09:28:54Z"
---

## Use your coding agent

Choose an icon to open a prefilled prompt. Select your project, review, and send.

```text agent-prompt
Read https://submilli.ai/docs/tutorials/connect-mastra/ as the source for this setup.

Inspect my project and follow this guide to create agent.ts.
Reuse valid local Submilli connection settings and provider credentials. If
SUBMILLI_SERVER, SUBMILLI_SERVER_TOKEN, or SUBMILLI_BLUEPRINT is missing, ask
for that value or its local configuration source. Do not invent values,
expose secrets, create a Blueprint, or change the server. Ask for missing
provider credentials without printing them. Install only the
missing dependencies shown in this guide. Preserve its tool restrictions and
session cleanup. Prepare the file without running it or making a model request.
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
export OPENAI_API_KEY='your-openai-api-key'
```

Connect a Mastra agent to Submilli, run a small arithmetic program, and close
the server session when the memory thread ends. You need Node.js 22 or later,
`@mastra/core` 1.79 or later, `submilli-server` 0.4.1 or later, a
model-provider key, and a Blueprint already registered on your server. For
this example, use a Blueprint with no required variables. The calculation
does not need Package or tool permissions.

Start the [server](/docs/server/run-the-server), [create a Blueprint with no
grants](/docs/blueprints/start-a-blueprint#start-from-nothing-allowed), and
[register it](/docs/server/register-a-blueprint#register-it) before running this
example. Keep `SUBMILLI_SERVER_TOKEN` exported in the shell where you run this
example.
If your Blueprint requires variables or harness secrets, pass them as
`createSubmilliTools`' `variables` and `secrets` options: fixed values, or
functions of each agent run's resource, thread, and request context. The values
are bound when the session opens; the model cannot set them. The
[`@submilli/mastra` reference](https://github.com/submilli/submilli-typescript/tree/main/packages/mastra#reference)
describes the binding options.

### Install the integration

Create a project and install Mastra, its memory, and the Submilli integration:

```sh
mkdir mastra && cd mastra
npm init -y
npm pkg set type=module
npm install @submilli/mastra @mastra/core @mastra/memory
npm install --save-dev tsx typescript @types/node
```

### Write the agent

Create `agent.ts` in the project directory and paste the code below:

```typescript title="agent.ts"
import { Mastra } from "@mastra/core";
import { Agent } from "@mastra/core/agent";
import { InMemoryStore } from "@mastra/core/storage";
import { Memory } from "@mastra/memory";
import { createSubmilliTools } from "@submilli/mastra";

const prompt = process.argv.slice(2).join(" ");
if (!prompt.trim()) throw new Error("Usage: npx tsx agent.ts '<message>'");
const blueprint = process.env.SUBMILLI_BLUEPRINT;
if (!blueprint) throw new Error("SUBMILLI_BLUEPRINT is not set");

const submilli = createSubmilliTools({ blueprint });
const memory = new Memory();
const mastra = new Mastra({
  agents: {
    calculator: new Agent({
      id: "calculator",
      name: "Calculator",
      instructions: "Use Submilli to run the requested TypeScript code.",
      model: "openai/gpt-5-mini",
      memory,
      tools: () => submilli.tools(),
    }),
  },
  storage: new InMemoryStore(),
  logger: false,
});

const threadId = "submilli-example";
try {
  const result = await mastra.getAgent("calculator").generate(prompt, {
    memory: { thread: threadId, resource: "submilli-example-user" },
    maxSteps: 10,
  });
  for (const { payload } of result.toolResults) {
    if (payload.toolName === "submilli__typescript__execute") {
      console.error("Submilli execute result:", JSON.stringify(payload.result));
    }
  }
  console.log(result.text);
} finally {
  await submilli.close({ threadId, memory });
}
```

`createSubmilliTools` reads the server address from `SUBMILLI_SERVER` or uses
`http://127.0.0.1:8128`, and reads the token from `SUBMILLI_SERVER_TOKEN`.
Create it once per process. Before the first model call, it fetches the tool
descriptions for the Blueprint from the server.

Each memory thread has one server session, kept in the thread's metadata, so
later agent runs on the thread reuse it. `close` ends the thread's session in
the `finally` block; in an application, call it when the thread is deleted or
archived, not after every run. This example keeps memory in `InMemoryStore`;
with persistent storage, such as `LibSQLStore`, threads keep their sessions
across restarts.

`tools: () => submilli.tools()` gives the model only Submilli's tools. Keep
application tools out of the agent's `tools` if their calls must pass through
Blueprint permissions: tools added there are called directly by the agent and
bypass Submilli.

## Try it out

Run this from the project directory with the setup environment active:

```sh
npx tsx agent.ts \
  'Run this program through Submilli and report its result:
export function main(): number { return 2 + 2; }'
```

The agent should make a `submilli__typescript__execute` tool call and print
its tool result on standard error. The final answer should contain:

```text
4
```

The model passed the supplied program to Submilli, which compiled and ran it in the
registered Blueprint's session. Check the printed execute-tool result as well
as the final answer to confirm that the connection completed.

You now have a Mastra agent whose program execution is governed by an already
registered Blueprint and whose memory thread closes its server session. For the
same connection through another harness, see the [Vercel AI
SDK](/docs/tutorials/connect-vercel-ai-sdk) tutorial.

## Use application tools

<a id="your-tools-in-programs"></a>

To reuse application tools under Blueprint permissions, pass your Mastra tools
to `createSubmilliTools`' `ptc` option. They become callable from Submilli
programs; the integration does not expose them directly to the model. Keep them
out of the agent's `tools`: adding the same tools there creates a direct path
that bypasses Submilli.

Mark calls that need a person's approval the way Mastra does, with
`requireApproval` on the tool or with the `requireToolApproval` option. The
agent run suspends while the program waits, and `resumeGenerate` carries it on
with the person's decision. The
[harness tools overview](/docs/tutorials/connect-a-harness#call-harness-tools-from-programs)
describes the shared lifecycle; the [`@submilli/mastra` reference](https://github.com/submilli/submilli-typescript/tree/main/packages/mastra#your-tools-in-programs)
covers the integration contract.
