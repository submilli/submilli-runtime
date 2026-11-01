---
title: "Connect Vercel AI SDK"
description: "Connect a Vercel AI SDK application to a registered Submilli Blueprint with @submilli/ai-sdk."
slug: tutorials/connect-vercel-ai-sdk
docType: how-to
sidebar:
  order: 7
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "e49cf307cd47f983bfada65368ac325806648a67243ac09dea51aaf8bf631147"
  confirmedAt: "2026-10-20T09:28:54Z"
---

## Use your coding agent

Choose an icon to open a prefilled prompt. Select your project, review, and send.

```text agent-prompt
Read https://submilli.ai/docs/tutorials/connect-vercel-ai-sdk/ as the source for this setup.

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

Connect a Vercel AI SDK application to Submilli, run a small arithmetic
program, and close the server session when the conversation ends. You need
Node.js 22 or later, AI SDK 7, `submilli-server` 0.4.1 or later, a
model-provider key, and a Blueprint already registered on your server. For
this example, use a Blueprint with no required variables. The calculation
does not need Package or tool permissions.

Start the [server](/docs/server/run-the-server), [create a Blueprint with no
grants](/docs/blueprints/start-a-blueprint#start-from-nothing-allowed), and
[register it](/docs/server/register-a-blueprint#register-it) before running this
example. Keep `SUBMILLI_SERVER_TOKEN` exported in the shell where you run this
example.
If your Blueprint requires variables or harness secrets, pass them as
`submilliTools`' `variables` and `secrets` options from your application's
state. The values are bound when the session opens; the model cannot set them.
The [`@submilli/ai-sdk` reference](https://github.com/submilli/submilli-typescript/tree/main/packages/ai-sdk#reference)
describes the binding options.

### Install the integration

Create a project and install the AI SDK, the Submilli integration, and the
OpenAI provider:

```sh
mkdir vercel-ai-sdk && cd vercel-ai-sdk
npm init -y
npm pkg set type=module
npm install @submilli/ai-sdk ai @ai-sdk/openai
npm install --save-dev tsx typescript @types/node
```

### Write the agent

Create `agent.ts` in the project directory and paste the code below:

```typescript title="agent.ts"
import { openai } from "@ai-sdk/openai";
import { submilliTools } from "@submilli/ai-sdk";
import { generateText, isStepCount } from "ai";

const prompt = process.argv.slice(2).join(" ");
if (!prompt.trim()) throw new Error("Usage: npx tsx agent.ts '<message>'");
const blueprint = process.env.SUBMILLI_BLUEPRINT;
if (!blueprint) throw new Error("SUBMILLI_BLUEPRINT is not set");

const submilli = await submilliTools({ blueprint, conversationId: "submilli-example" });
try {
  const result = await generateText({
    model: openai("gpt-5-mini"),
    system: "Use Submilli to run the requested TypeScript code.",
    tools: submilli.tools,
    prompt,
    stopWhen: isStepCount(10),
  });
  for (const step of result.steps) {
    for (const output of step.toolResults) {
      if (output.toolName === "submilli__typescript__execute") {
        console.error("Submilli execute result:", JSON.stringify(output.output));
      }
    }
  }
  console.log(result.text);
} finally {
  await submilli.close();
}
```

`submilliTools` reads the server address from `SUBMILLI_SERVER` or uses
`http://127.0.0.1:8128`, and reads the token from `SUBMILLI_SERVER_TOKEN`.
Before the first model call, it fetches the tool descriptions for the
Blueprint from the server.

Requests with the same `conversationId` share one server session. In an
application, call `submilliTools` once per request with the chat's id, and call
`close` when the conversation ends, not after every request. `close` here, in
the `finally` block, ends this run's session. To keep sessions across restarts
or between processes, give `submilliTools` a `store`, as the
[sessions reference](https://github.com/submilli/submilli-typescript/tree/main/packages/ai-sdk#sessions-retries-and-results)
describes.

`tools: submilli.tools` gives the model only Submilli's tools. Keep application
tools out of `generateText`'s `tools` if their calls must pass through
Blueprint permissions: tools added there are called directly by the model and
bypass Submilli. `stopWhen` lets the SDK continue after a tool result; the
default stops after one step, before the model can use that result.

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

You now have a Vercel AI SDK application whose program execution is governed
by an already registered Blueprint and whose conversation closes its server
session. For the same connection through another harness, see the
[Mastra](/docs/tutorials/connect-mastra) tutorial.

## Use application tools

<a id="your-tools-in-programs"></a>

To reuse application tools under Blueprint permissions, pass your AI SDK tools
to `submilliTools`' `ptc` option. They become callable from Submilli programs;
the integration does not expose them directly to the model. Keep them out of
`generateText`'s `tools`: adding the same tools there creates a direct path
that bypasses Submilli.

Mark calls that need a person's approval the way the AI SDK does, with
`needsApproval` on the tool or with `submilliTools`' `toolApproval` option. The
program waits on the server while the AI SDK's approval flow asks for the
decision, or while your `onApprovalRequired` callback decides it. The
[harness tools overview](/docs/tutorials/connect-a-harness#call-harness-tools-from-programs)
describes the shared lifecycle; the [`@submilli/ai-sdk` reference](https://github.com/submilli/submilli-typescript/tree/main/packages/ai-sdk#your-tools-in-programs)
covers the integration contract.

To build tools on the server's endpoints yourself, without the integration,
see the [HTTP API reference](/docs/reference/http-api).
