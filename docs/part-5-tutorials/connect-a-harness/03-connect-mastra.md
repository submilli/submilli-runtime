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
  contentHash: "59fb286c98c79d2f555a939e59d5a9826a93fbb07f091f900d4931937b9da598"
  confirmedAt: "2026-10-20T09:28:54Z"
---

This guide connects a Mastra agent to Submilli over MCP. The agent keeps its
model and loop in Mastra, while its TypeScript programs run on
`submilli-server` under a Blueprint your application selects.

Before you start:

- [Run `submilli-server`](/docs/server/run-the-server) and keep
  `SUBMILLI_SERVER_TOKEN` exported.
- [Register a Blueprint](/docs/server/register-a-blueprint), then set
  `SUBMILLI_BLUEPRINT` to its registered name. The calculation below assumes
  the Blueprint has no required variables. The sample calls no external tools.
- Install Node.js 20 or later and set the API key for your model provider.

## Start the project

```sh
mkdir mastra-submilli && cd mastra-submilli
npm init -y
npm pkg set type=module
npm install @mastra/core @mastra/mcp
npm install --save-dev tsx typescript @types/node
```

## Connect the agent

Save this as `agent.ts`:

```typescript title="agent.ts"
import { Agent } from "@mastra/core/agent";
import { MCPClient } from "@mastra/mcp";

const server = process.env.SUBMILLI_SERVER ?? "http://127.0.0.1:8128";
const blueprint = required("SUBMILLI_BLUEPRINT");

type Model = ConstructorParameters<typeof Agent>[0]["model"];

export async function answer(
  question: string,
  model: Model = "anthropic/claude-sonnet-5",
): Promise<string> {
  const agent = new Agent({
    id: "calculator",
    name: "Calculator",
    instructions: "Use Submilli to do calculations in TypeScript. Return the execution result.",
    model,
  });

  const submilli = new MCPClient({
    id: "submilli",
    servers: {
      submilli: {
        url: new URL(`${server}/mcp/${encodeURIComponent(blueprint)}`),
        requestInit: {
          headers: { Authorization: `Bearer ${required("SUBMILLI_SERVER_TOKEN")}` },
        },
      },
    },
  });

  try {
    const toolsets = await submilli.listToolsets();
    if (toolsets.submilli === undefined) {
      throw new Error("submilli-server refused the connection");
    }

    const result = await agent.generate(question, {
      toolsets,
      maxSteps: 10,
      onStepFinish({ toolCalls, toolResults }) {
        if (toolCalls.length > 0) console.dir({ toolCalls, toolResults }, { depth: null });
      },
    });
    return result.text;
  } finally {
    await submilli.disconnect();
  }
}

function required(name: string): string {
  const value = process.env[name];
  if (!value) throw new Error(`${name} is not set`);
  return value;
}

console.log(
  await answer("Use Submilli to compute the 40th Fibonacci number, with F(0)=0 and F(1)=1."),
);
```

Mastra receives Submilli's tools in `toolsets`. Pass them to `generate` for
this request rather than putting a session-bound toolset on a shared agent.
Checking `toolsets.submilli` prevents the model from continuing without
Submilli if the MCP connection fails. The `finally` block closes the MCP
session.

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

## Run it

```sh
export SUBMILLI_BLUEPRINT=<registered-blueprint-name>
ANTHROPIC_API_KEY=... npx tsx agent.ts
```

The step callback prints Mastra's tool calls and results. Confirm a call to
Submilli's execute tool and a result containing `102334155`; the final answer
alone does not verify where the calculation ran.
This sample requires model-provider access for the Mastra conversation. The
TypeScript program itself only computes a value; it does not call a Package,
application tool, or external service.

For the division of responsibilities between Mastra, Submilli, and the
Blueprint, see [How Submilli works with
harnesses](/docs/tutorials/connect-a-harness).
