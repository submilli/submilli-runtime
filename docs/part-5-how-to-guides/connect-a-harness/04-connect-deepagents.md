---
docType: how-to
title: "Connect deepagents to Submilli"
description: "Create a deepagents agent that uses Submilli for permitted code execution."
showDescription: false
slug: how-to/connect-deepagents
sidebar:
  order: 4
authorship:
  label: ai-generated
  confirmed: true
  contentHash: "537420563e03cdffd6a5b212dd1a494cea7eafefeeee23de93aac78a6b120bcc"
  confirmedAt: "2026-10-18T13:09:05.218103+00:00"
# Unpublished AI draft. Human prose rewrite required before publication.
---

Create a fresh deepagents agent whose execution tools run through Submilli,
then try an allowed operation and a denied operation with the same Blueprint.

## Use your coding agent

Open a project directory in your coding agent.
[Install the Submilli skill](/docs/install#the-skill) if needed, then restart
the coding agent to load it. Give it this prompt:

```text agent-prompt
Use the Submilli skill to create a fresh deepagents demo agent connected
to Submilli through the native SubmilliMiddleware integration. Write dedicated
demo files without replacing
an existing agent or its configuration. Inspect the project and help me choose
and configure a model if needed. Keep credentials local and out of responses.
Download https://submilli.ai/docs/examples/permission-demo/blueprint.yaml
and https://submilli.ai/docs/examples/permission-demo/billing-source.zip
into a new demo directory. Prepare the billing Package with submilli build
publish-local and register the Blueprint with submilli server blueprint apply.
Find a local Submilli server and token, or help me prepare them. Publish the
Package under the same user and SUBMILLI_HOME as that server. Use the
permission-demo Blueprint. Give the new agent only Submilli execution tools.
Disable built-in execution tools, other tool providers, and delegation routes
that could act outside Submilli. Follow this guide's framework-specific setup.
Verify connection and the allowed cus_northwind and denied cus_initech
listCharges calls without a model request. Tell me how to start the agent
and where to inspect tool results, plus any remaining setup I must complete.
```

After setup, [try it out](#try-it-out). To configure the connection yourself,
follow the manual steps below.

<details>
<summary>Connecting an existing agent</summary>

Replace its execution tools with Submilli. Remove other tool providers,
file and shell tools, network tools, and delegation routes that can perform
operations outside Submilli. Blueprint permissions apply to code run by
Submilli; leaving another execution route available bypasses that boundary.

Apply the exclusions below in a dedicated process. Remove extra tools,
middleware that adds tools, custom backends, and subagents. The registered profile applies
to other agents using the same model in that process.

</details>

## Set up manually

Use `SubmilliMiddleware` to give a fresh deepagents agent a tool for
running code under your Blueprint's permissions.
[Middleware](https://docs.langchain.com/oss/python/langchain/middleware/overview)
adds behavior around an agent's model and tool calls.

:::note[Before you start]
You'll need:

- A deepagents project with a configured model.
- The [Submilli CLI](/docs/install) and a [local server](/docs/server/run-the-server)
  running under the same user and `SUBMILLI_HOME`, with a token it accepts.
:::

### Set connection variables

Set these environment variables in your shell and the process that runs your agent:

| Environment variable | Value |
| --- | --- |
| `SUBMILLI_SERVER` | Server address, such as `http://127.0.0.1:8128`, without a trailing slash |
| `SUBMILLI_SERVER_TOKEN` | Server token |
| `SUBMILLI_BLUEPRINT` | `permission-demo` |

### Prepare the permission demo

Download the [demo Blueprint](/docs/examples/permission-demo/blueprint.yaml)
and [billing Package source](/docs/examples/permission-demo/billing-source.zip).
The Package uses local sample charges and needs no billing credentials.
Save both downloads in a new `permission-demo` directory, then prepare and
register the demo with your running server:

```sh
cd permission-demo
unzip billing-source.zip
submilli build publish-local
SUBMILLI_SERVER_URL="$SUBMILLI_SERVER" submilli server blueprint apply blueprint.yaml
```

The Blueprint permits
listing charges for `cus_northwind` and denies that operation for other customers.

### Install the integration

In your project's Python environment:

```sh
python -m pip install "langchain-submilli[deepagents]>=0.1" "deepagents==0.7.28"
```

### Create the demo agent

Run this example in a dedicated demo process. Set `DEEPAGENTS_MODEL` to your
configured model identifier. The profile disables deepagents' built-in tools
and default subagent, so this agent exposes only Submilli tools.

```python title="Fresh demo agent"
import os

from deepagents import (
    GeneralPurposeSubagentProfile,
    HarnessProfile,
    create_deep_agent,
    register_harness_profile,
)
from langchain_submilli import SubmilliMiddleware

model_name = os.environ["DEEPAGENTS_MODEL"]
register_harness_profile(
    model_name,
    HarnessProfile(
        excluded_tools=frozenset({
            "ls", "read_file", "write_file", "edit_file", "delete",
            "glob", "grep", "execute", "task", "write_todos",
        }),
        general_purpose_subagent=GeneralPurposeSubagentProfile(enabled=False),
    ),
)
submilli = SubmilliMiddleware(
    blueprint=os.environ["SUBMILLI_BLUEPRINT"],
    variables={},
)
agent = create_deep_agent(
    model=model_name,
    tools=[],
    subagents=[],
    middleware=[submilli],
)
```

The middleware reads the server address and token from the environment.
The profile applies to other agents using the same model in this process.

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

1. Open your application's chat or test input with the fresh demo agent.
2. Ask it to perform the allowed operation:

   > Use the billing Package to list charges for customer `cus_northwind`.

   Check the Submilli tool result for `ch_a1` (4900) and `ch_a2` (1250).
3. Ask it to attempt the denied operation:

   > Use the billing Package to list charges for customer `cus_initech`.
   > Attempt the operation through Submilli and report the runtime result.
   > Keep the customer ID and Blueprint permissions unchanged.

   Check for a Submilli call returning `permission_denied` for
   `acme.com/charges.list`, with no charges.

An agent's answer or refusal alone does not demonstrate the permission check.
Look for both runtime results in tool activity. Expand the inspection steps
below if your app hides them.

<details>
<summary>If the agent declines to attempt the denied operation</summary>

Ask it to run this exact program through Submilli and report the tool result:

```typescript
import { listCharges, Charge } from "@acme/billing";

function main(): Charge[] {
  return listCharges("cus_initech");
}
```

If it still declines without making a tool call, the runtime denial has not
been tested. Use the direct demo check below to verify the Blueprint independently.

```sh
curl -fsS "$SUBMILLI_SERVER/v1/execute" \
  -H "Authorization: Bearer $SUBMILLI_SERVER_TOKEN" \
  -H "Content-Type: application/json" \
  --data '{"blueprint":"permission-demo","code":"import { listCharges, Charge } from \"@acme/billing\"; function main(): Charge[] { return listCharges(\"cus_initech\"); }"}'
```

Check for `error.kind` equal to `"permission_denied"`, Capability
`acme.com/charges.list`, and `result: null`. Older runtimes may return
`runtime_error` with a `PermissionDeniedError` message instead.

</details>

<details>
<summary>Inspect the Submilli call</summary>

To inspect a run in code, put the test message in your `messages` list as
`{"role": "user", "content": "..."}`, replacing `...` with the test message.
Run `await agent.ainvoke({"messages": messages})` in your application's async
code, using the demo configuration. Keep the
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
`tool_call_id`. The call's `args` should contain each program, and the tool
message's content should contain the allowed charges or the permission error. If setup used MCP instead
of the native middleware, inspect the Submilli execution tool exposed by that
connection in the same returned messages.

Match the tool call and result by their call ID and check for an error.

If there is no execution call, check that this run uses the agent configuration
with Submilli attached and that its tool settings allow the call. If the call
fails, read the tool error and [check the connection settings](#troubleshooting).
If the tool returns charges but the agent gives no final answer, check your agent's
step or turn limit so it can continue after the tool result.

</details>

## Troubleshooting

- Connection refused: check `SUBMILLI_SERVER` and whether the server is running.
- `401`: use a token accepted by that server.
- Blueprint not found: check `SUBMILLI_BLUEPRINT` against the registered name.

See [server settings](/docs/reference/server-settings) for server configuration.
