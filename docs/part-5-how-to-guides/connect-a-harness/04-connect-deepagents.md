---
docType: how-to
title: "Connect deepagents to Submilli"
description: "Add Submilli tools to an existing LangChain deepagents harness."
showDescription: false
slug: how-to/connect-deepagents
sidebar:
  order: 4
authorship:
  label: ai-generated
  confirmed: true
  contentHash: "3dc9d577047b8fb32c69c31c34770830c68c36eec277d030702befd3ea3af23f"
  confirmedAt: "2026-10-18T13:09:05.218103+00:00"
# Unpublished AI draft. Human prose rewrite required before publication.
---

Connect your existing deepagents harness to Submilli for code execution.

## Use your coding agent

Open your existing deepagents project in your coding agent.
[Install the Submilli skill](/docs/install#the-skill) if needed, then restart
the coding agent to load it. Give it this prompt:

```text agent-prompt
Use the Submilli skill to connect this existing deepagents project
to Submilli over MCP.
Inspect the project. Preserve the existing agent, model, instructions,
tools, and configuration. Make only the changes needed to add Submilli.
Find any existing Submilli server, credentials, and registered Blueprint
in local configuration. If setup is missing, help me prepare it. Ask me
which operations the agent needs before defining Blueprint permissions.
Install the integration and configure the connection. Keep tokens out of
source code and responses, and help me configure missing credentials locally.
Verify the connection and tool discovery without a model request.
Tell me what changed and anything I still need to do.
```

After setup, [try it out](#try-it-out). To configure the connection yourself,
follow the manual steps below.

## Set up manually

Use `SubmilliMiddleware` to give your existing deepagents harness a tool for
running code under your Blueprint's permissions.
[Middleware](https://docs.langchain.com/oss/python/langchain/middleware/overview)
adds behavior around an agent's model and tool calls.

:::note[Before you start]
You'll need:

- An existing deepagents project with a configured model.
- A [running Submilli server](/docs/server/run-the-server) and a token it accepts.
- A [registered Blueprint](/docs/server/register-a-blueprint) that defines the
  permissions for the code your agent runs.
:::

### Set connection variables

Set these in the process that runs your agent, using the values prepared in
**Before you start**:

| Environment variable | Value |
| --- | --- |
| `SUBMILLI_SERVER` | Server address, such as `http://127.0.0.1:8128`, without a trailing slash |
| `SUBMILLI_SERVER_TOKEN` | Server token |
| `SUBMILLI_BLUEPRINT` | Registered Blueprint name |

### Install the integration

In your project's Python environment:

```sh
python -m pip install "langchain-submilli[deepagents]>=0.1"
```

### Add Submilli to your agent

Add `SubmilliMiddleware` to your existing `create_deep_agent` call. Keep your
model, prompt, tools, and other settings. If you already use middleware,
append `submilli` to that list.

```python title="Agent configuration (fragment)"
import os

from deepagents import create_deep_agent
from langchain_submilli import SubmilliMiddleware

submilli = SubmilliMiddleware(
    blueprint=os.environ["SUBMILLI_BLUEPRINT"],
    variables={},
)

agent = create_deep_agent(
    model=model,
    middleware=[submilli],
)
```

Here, `model` is your configured model. The middleware reads the server address
and token from the environment.

<details>
<summary>Does your Blueprint require variables or secrets?</summary>

Replace `variables={}` with your [Blueprint's variable values](/docs/reference/blueprint-file#variables).
For secrets supplied by your application (the `harness` source), pass their
names and values through the middleware's `secrets` argument.
See [Blueprint secrets](/docs/reference/blueprint-file#secrets).

</details>

### Verify the connection

In your application's async startup code, after creating `submilli`:

```python
await submilli.client.blueprints.prompt(submilli.blueprint)
print("Connected to Submilli.")
```

`Connected to Submilli.` confirms the server address, token, and Blueprint work,
without a model request.

Run your agent with `ainvoke` or `astream`. The middleware uses an async
client to communicate with the Submilli server, so synchronous `invoke` and
`stream` are not supported.

```python title="Agent call (fragment)"
result = await agent.ainvoke({"messages": messages})
```

Your agent now has the `submilli__typescript__execute` tool. Keep the client open
while the agent uses it, then close it during application shutdown:

```python
await submilli.client.aclose()
```

## Try it out

Test the agent in your application, using the connection you just configured.
Send the following message through your application's chat or test input. This
is a request to your running agent, not to the coding assistant that set it up.
It makes a model request using your application's existing model configuration.

> Use Submilli to run `export function main(): number { return 2 + 2; }` and report the result.

For the manual example, put this message in your existing `messages` list as
`{"role": "user", "content": "..."}`, replacing `...` with the message above.
Run `await agent.ainvoke({"messages": messages})` in your application's async
code, preserving any existing configuration and context arguments. Keep the
Submilli client open until the run finishes. Then print the
[returned messages](https://docs.langchain.com/oss/python/langchain/messages)
in the same async function:

```python
for message in result["messages"]:
    if message.type == "ai" and message.tool_calls:
        print("Tool calls:", message.tool_calls)
    elif message.type == "tool":
        print("Tool result:", message.tool_call_id, message.content)
print("Agent answer:", result["messages"][-1].content)
```

In the terminal running your application, find the call named
`submilli__typescript__execute` and the tool message with its matching
`tool_call_id`. The call's `args` should contain the program, and the tool
message's content should contain the returned `4`. If setup used MCP instead
of the native middleware, inspect the Submilli execution tool exposed by that
connection in the same returned messages.

A successful test shows the agent requesting Submilli's execution tool with
the program, the tool returning `4` without an error, and the agent reporting
that result. Match the tool call and result by their call ID. A chat answer of
`4` alone is insufficient: the model can calculate it without using Submilli.

This verifies the path from your agent through the Submilli server, code
execution, and the returned result. It does not test access to your business
systems or whether your Blueprint denies forbidden operations.

If there is no execution call, check that this run uses the agent configuration
with Submilli attached and that its tool settings allow the call. If the call
fails, read the tool error and [check the connection settings](#troubleshooting).
If the tool returns `4` but the agent gives no final answer, check your agent's
step or turn limit so it can continue after the tool result.

## Troubleshooting

- Connection refused: check `SUBMILLI_SERVER` and whether the server is running.
- `401`: use a token accepted by that server.
- Blueprint not found: check `SUBMILLI_BLUEPRINT` against the registered name.

See [server settings](/docs/reference/server-settings) for server configuration.
