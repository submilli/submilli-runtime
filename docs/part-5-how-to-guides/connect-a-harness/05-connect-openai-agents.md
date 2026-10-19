---
docType: how-to
title: "Connect OpenAI Agents SDK to Submilli"
description: "Add Submilli as an MCP server in an existing OpenAI Agents SDK harness."
showDescription: false
slug: how-to/connect-openai-agents
sidebar:
  order: 5
authorship:
  label: ai-generated
  confirmed: true
  contentHash: "badd9bb87f17304cd16e06dd5f161ec533c9068f2b7603ee307d009f9756c5ed"
  confirmedAt: "2026-10-18T13:09:05.218103+00:00"
# Unpublished AI draft. Human prose rewrite required before publication.
---

Connect your existing OpenAI Agents SDK harness to Submilli for code execution.

## Use your coding agent

Open your existing OpenAI Agents SDK project in your coding agent.
[Install the Submilli skill](/docs/install#the-skill) if needed, then restart
the coding agent to load it. Give it this prompt:

```text agent-prompt
Use the Submilli skill to connect this existing OpenAI Agents SDK project
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

Use `MCPServerStreamableHttp` to connect your agent to the Submilli runtime over
[MCP](https://openai.github.io/openai-agents-python/mcp/), which lets an agent
discover and call tools on a server.

:::note[Before you start]
You’ll need:

- An existing OpenAI Agents SDK project with a configured model.
- A [running Submilli server](/docs/server/run-the-server) and a token it accepts.
- A [registered Blueprint](/docs/server/register-a-blueprint), which sets the
  permissions for code sent to Submilli.
:::

### Set connection variables

Use the server address, token, and Blueprint name prepared in **Before you start**.
Assign those values to these environment variables in the process that runs your agent:

| Environment variable | Value |
| --- | --- |
| `SUBMILLI_SERVER` | Server address, such as `http://127.0.0.1:8128`, without a trailing slash |
| `SUBMILLI_SERVER_TOKEN` | Server token |
| `SUBMILLI_BLUEPRINT` | Registered Blueprint name |


### Install OpenAI Agents SDK

If it is not already installed in your project's Python environment:

```sh
python -m pip install "openai-agents>=0.3"
```

### Add the MCP server to your agent

Create `MCPServerStreamableHttp` with your Submilli address and token.
Add it to your agent's `mcp_servers` list. The URL selects the Blueprint.

This fragment belongs in the async code that runs your existing `agent`.
Cloning it with the added server preserves its model, instructions, tools,
and other settings. Keep your existing `Runner.run` arguments.

```python title="Agent configuration and call (fragment)"
import os

from agents import Runner
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
    connected_agent = agent.clone(
        mcp_servers=[*agent.mcp_servers, submilli],
    )
    result = await Runner.run(connected_agent, messages)
```

Keep the `async with` block open until the agent finishes using the connection.
The timeout allows a Submilli tool call up to 120 seconds.

If your Blueprint requires variables, add a `submilli-variables` header.
See [MCP variable bindings](/docs/application#three-decisions-the-model-has-no-part-in).
If your Blueprint requires secrets from your application (the `harness`
source), add `submilli-secrets` containing a base64url-encoded JSON object
of their names and values. See
[Blueprint secrets](/docs/reference/blueprint-file#secrets).

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

1. Open your application's chat or test input with the connected agent.
2. Send this message:

   > Run this TypeScript program in Submilli and report its return value:

   ```typescript
   export function main(): number {
     return 2 + 2;
   }
   ```

3. Check for the answer `4` and a Submilli execution call returning `4` in
   your application's tool activity. If your app hides tool activity, use the
   inspection steps below.

The tool call confirms that your agent reached Submilli and ran the program.
The answer alone does not prove execution. Test your business permissions separately.

<details>
<summary>Inspect the Submilli call</summary>

To inspect a run in code, use the test message as the input to
`await Runner.run(connected_agent, ...)`, inside the existing `async with`
block. Preserve your other run arguments. Add this after the call to print
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
its `output` for the returned `4`. This prints the local run data; no tracing
service is needed.

Match the tool call and result by their call ID and check for an error.

If there is no execution call, check that this run uses the agent configuration
with Submilli attached and that its tool settings allow the call. If the call
fails, read the tool error and [check the connection settings](#troubleshooting).
If the tool returns `4` but the agent gives no final answer, check your agent's
step or turn limit so it can continue after the tool result.

</details>

## Troubleshooting

- Connection refused: check `SUBMILLI_SERVER` and whether the server is running.
- `401`: use a token accepted by that server.
- Blueprint not found: check `SUBMILLI_BLUEPRINT` against the name registered
  on that server.

See [server settings](/docs/reference/server-settings) for server configuration.
