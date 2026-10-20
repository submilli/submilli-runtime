---
docType: how-to
title: "Connect OpenAI Agents SDK to Submilli"
description: "Create a OpenAI Agents SDK agent that uses Submilli for permitted code execution."
showDescription: false
slug: how-to/connect-openai-agents
sidebar:
  order: 5
authorship:
  label: ai-generated
  confirmed: true
  contentHash: "80bee30a6b04c09c88815dcb4d909b1f8ff9a4a6d240488973a9c180870127d1"
  confirmedAt: "2026-10-18T13:09:05.218103+00:00"
# Unpublished AI draft. Human prose rewrite required before publication.
---

Create a fresh OpenAI Agents SDK agent whose execution tools run through Submilli,
then try an allowed operation and a denied operation with the same Blueprint.

## Use your coding agent

Open a project directory in your coding agent.
[Install the Submilli skill](/docs/install#the-skill) if needed, then restart
the coding agent to load it. Give it this prompt:

```text agent-prompt
Use the Submilli skill to create a fresh OpenAI Agents SDK demo agent connected
to Submilli through MCP. Write dedicated demo files without replacing
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

Replace `tools` and `handoffs` with empty lists and `mcp_servers` with
`[submilli]`. Remove hosted tools and agents exposed as tools too.

</details>

## Set up manually

Use `MCPServerStreamableHttp` to connect your agent to the Submilli runtime over
[MCP](https://openai.github.io/openai-agents-python/mcp/), which lets an agent
discover and call tools on a server.

:::note[Before you start]
You’ll need:

- A OpenAI Agents SDK project with a configured model.
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

### Install OpenAI Agents SDK

If it is not already installed in your project's Python environment:

```sh
python -m pip install "openai-agents>=0.3"
```

### Create the demo agent

Use a new agent with only the Submilli MCP server. Set `OPENAI_MODEL` to
your configured model name. This fragment belongs in your demo's async
function; use the message under **Try it out** as `messages`.

```python title="Agent configuration and call (fragment)"
import os

from agents import Agent, Runner
from agents.mcp import MCPServerStreamableHttp

async with MCPServerStreamableHttp(
    name="submilli",
    params={
        "url": f"{os.environ['SUBMILLI_SERVER']}/mcp/{os.environ['SUBMILLI_BLUEPRINT']}",
        "headers": {
            "Authorization": f"Bearer {os.environ['SUBMILLI_SERVER_TOKEN']}",
        },
    },
    client_session_timeout_seconds=120,
) as submilli:
    connected_agent = Agent(
        name="Submilli demo",
        model=os.environ["OPENAI_MODEL"],
        tools=[],
        handoffs=[],
        mcp_servers=[submilli],
    )
    result = await Runner.run(connected_agent, messages)
```

Keep the `async with` block open until the agent finishes using the connection.
The timeout allows a Submilli tool call up to 120 seconds.

<details>
<summary>Using a different Blueprint with variables or secrets</summary>

If your Blueprint requires variables, add a `submilli-variables` header.
See [MCP variable bindings](/docs/application#three-decisions-the-model-has-no-part-in).
If your Blueprint requires secrets from your application (the `harness`
source), add `submilli-secrets` containing a base64url-encoded JSON object
of their names and values. See
[Blueprint secrets](/docs/reference/blueprint-file#secrets).

</details>

### Verify the connection

Before `Runner.run`, inside the `async with` block, check that the server
offers `submilli__typescript__execute`:

```python
tools = await submilli.list_tools()
if not any(tool.name == "submilli__typescript__execute" for tool in tools):
    raise RuntimeError("Submilli's execute tool was not found")
print("Connected to Submilli.")
```

`Connected to Submilli.` confirms that the SDK can discover
`submilli__typescript__execute`. Listing tools does not call a model.

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

To inspect a run in code, use the test message as the input to
`await Runner.run(connected_agent, ...)`, inside the existing `async with`
block. Use the demo agent above. Add this after the call to print
its [run items](https://openai.github.io/openai-agents-python/results/)
in your application's terminal:

```python
for item in result.new_items:
    if item.type in ("tool_call_item", "tool_call_output_item"):
        print(item.type, item.raw_item)
print("Agent answer:", result.final_output)
```

Find a function call named `submilli__typescript__execute`. Its `arguments`
contain the program. Find the output item with the same `call_id` and inspect
its `output` for the allowed charges or the permission error. This prints the local run data; no tracing
service is needed.

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
- Blueprint not found: check `SUBMILLI_BLUEPRINT` against the name registered
  on that server.

See [server settings](/docs/reference/server-settings) for server configuration.
