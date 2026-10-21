---
title: "Connect Mastra"
description: "Give a Mastra agent Submilli's MCP tools and run its TypeScript programs under a registered Blueprint."
slug: tutorials/connect-mastra
docType: how-to
sidebar:
  order: 3
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "42a2f968ad669748ec2a781931acb0f365efba017c3bc0d353291b1f7462afa2"
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
session cleanup. Run the file with --check to verify the connection without
a model request.
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

This guide connects a Mastra agent to Submilli over MCP. The agent keeps its
model and loop in Mastra, while its TypeScript programs run on
`submilli-server` under a Blueprint your application selects.

Before you start:

- [Run `submilli-server`](/docs/server/run-the-server) and keep
  `SUBMILLI_SERVER_TOKEN` exported.
- [Register a Blueprint](/docs/server/register-a-blueprint), then set
  `SUBMILLI_BLUEPRINT` to its registered name. The calculation below assumes
  the Blueprint has no required variables. The sample calls no external tools.
- Install Node.js 22.13 or later and set the API key for your model provider.

### Start the project

```sh
mkdir mastra-submilli && cd mastra-submilli
npm init -y
npm pkg set type=module
npm install @mastra/core @mastra/mcp
npm install --save-dev tsx typescript @types/node
```

### Connect the agent

Create `agent.ts` in the project directory and paste the code below:

```typescript title="agent.ts"
import { Agent } from "@mastra/core/agent";
import { MCPClient } from "@mastra/mcp";

const args = process.argv.slice(2);
const checking = args.length === 1 && args[0] === "--check";
const prompt = args.join(" ");
if (!checking && !prompt.trim()) {
  throw new Error("Usage: npx tsx agent.ts '<message>' (or --check)");
}

const server = process.env.SUBMILLI_SERVER ?? "http://127.0.0.1:8128";
const blueprint = required("SUBMILLI_BLUEPRINT");

type Model = ConstructorParameters<typeof Agent>[0]["model"];

export async function answer(
  question: string,
  // Change this model ID to use another provider-supported model.
  model: Model = "openai/gpt-5-mini",
): Promise<string> {
  const url = new URL(`${server}/mcp/${encodeURIComponent(blueprint)}`);
  const headers = { Authorization: `Bearer ${required("SUBMILLI_SERVER_TOKEN")}` };
  let sessionId: string | null = null;
  const submilli = new MCPClient({
    id: "submilli",
    servers: {
      submilli: {
        url,
        requestInit: { headers },
        fetch: async (input, init) => {
          const response = await fetch(input, init);
          sessionId = response.headers.get("mcp-session-id") ?? sessionId;
          return response;
        },
      },
    },
  });

  try {
    const toolsets = await submilli.listToolsets();
    if (toolsets.submilli === undefined) {
      throw new Error("submilli-server refused the connection");
    }

    if (checking) return "Connected to Submilli.";

    const agent = new Agent({
      id: "calculator",
      name: "Calculator",
      instructions: "Use Submilli to do calculations in TypeScript. Return the execution result.",
      model,
    });

    const result = await agent.generate(question, {
      toolsets,
      maxSteps: 10,
      onStepFinish({ toolCalls, toolResults }) {
        if (toolCalls.length > 0) console.dir({ toolCalls, toolResults }, { depth: null });
      },
    });
    return result.text;
  } finally {
    try {
      // disconnect() closes the transport, not the server's session.
      if (sessionId) {
        const response = await fetch(url, {
          method: "DELETE", headers: { ...headers, "Mcp-Session-Id": sessionId },
        });
        if (!response.ok && response.status !== 404) {
          throw new Error(`Could not close Submilli session: HTTP ${response.status}`);
        }
      }
    } finally {
      await submilli.disconnect();
    }
  }
}

function required(name: string): string {
  const value = process.env[name];
  if (!value) throw new Error(`${name} is not set`);
  return value;
}

console.log(await answer(prompt));
```

Mastra receives Submilli's tools in `toolsets`. Pass them to `generate` for
this request rather than putting a session-bound toolset on a shared agent.
Checking `toolsets.submilli` prevents the model from continuing without
Submilli if the MCP connection fails. The `finally` block deletes the server session, then closes the client
transport.

This configuration gives the model the tools returned by the one `submilli`
MCP server. It does not add Mastra tools or agents to the request. Mastra tools
can still be added directly when the application needs them, but those calls
bypass Submilli and keep Mastra's own authorization and approval path. Current Submilli MCP connections do not support application-tool
callbacks. To let a TypeScript program call an application tool through
Submilli, use an HTTP session with the `tools` field on `POST /v1/sessions`,
grant `tools.call` in the Blueprint, and answer each suspended run through
`POST /v1/runs/{run_id}/resume`. See the [HTTP API reference](/docs/reference/http-api#application-tools).

If the selected Blueprint declares required variables, add a
`submilli-variables` header whose values come from trusted application state,
as described in [Your application](/docs/application#three-decisions-the-model-has-no-part-in).
Never take those values from the conversation.

### Check the connection

With the setup environment active, run this without a model request:

```sh
npx tsx agent.ts --check
```

Look for `Connected to Submilli.`. This checks the server and registered
Blueprint; the model runs only when you supply a message.

## Try it out

```sh
npx tsx agent.ts \
  'Run this program through Submilli and report its result:
export function main(): number { return 2 + 2; }'
```

The step callback prints Mastra's tool calls and results. Confirm a call to
Submilli's execute tool and a result containing `4`; the final answer
alone does not verify where the calculation ran.
This sample requires model-provider access for the Mastra conversation. The
TypeScript program itself only computes a value; it does not call a Package,
application tool, or external service.

For the division of responsibilities between Mastra, Submilli, and the
Blueprint, see [How Submilli works with
harnesses](/docs/tutorials/connect-a-harness).
