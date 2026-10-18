---
title: "Connect OpenAI Agents SDK to Submilli"
description: "Add Submilli as an MCP server in an existing OpenAI Agents SDK harness."
slug: how-to/connect-openai-agents
sidebar:
  order: 5
authorship:
  label: ai-generated
  confirmed: true
  contentHash: "fc3b4bbccdabf6b7cc1b1a397b45b4c91be121ea75eaf3a7f96b6b9d1a01537c"
  confirmedAt: "2026-10-18T13:09:05.218103+00:00"
# Unpublished AI draft. Human prose rewrite required before publication.
---

Add Submilli as an MCP server in your OpenAI Agents SDK harness.
MCP (Model Context Protocol) connects the harness to external tools.
A harness is the software that runs your agent and handles its tool calls.

## Before you start

Have your harness project and these Submilli settings ready:

| Environment variable | Value |
| --- | --- |
| `SUBMILLI_SERVER` | Server address, such as `http://127.0.0.1:8128`, without a trailing slash |
| `SUBMILLI_SERVER_TOKEN` | A token accepted by that server |
| `SUBMILLI_BLUEPRINT` | The name of a registered Blueprint |

Set these variables in the process that runs your harness. A Blueprint sets
the permissions for code sent to Submilli. If needed,
[run the server](/docs/server/run-the-server) and
[register a Blueprint](/docs/server/register-a-blueprint) first.

## Install OpenAI Agents SDK

If it is not already installed in your project's Python environment:

```sh
python -m pip install "openai-agents>=0.3"
```

## Add the MCP server to your agent

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

## Verify the connection

Before `Runner.run`, inside the `async with` block, check that the server
offers its code execution tool:

```python
tools = await submilli.list_tools()
if not any(tool.name == "submilli__typescript__execute" for tool in tools):
    raise RuntimeError("Submilli's execute tool was not found")
print("Connected to Submilli.")
```

`Connected to Submilli.` confirms that the SDK can discover Submilli's
code execution tool. Listing tools does not call a model.

## Troubleshooting

- Connection refused: check `SUBMILLI_SERVER` and whether the server is running.
- `401`: use a token accepted by that server.
- Blueprint not found: check `SUBMILLI_BLUEPRINT` against the name registered
  on that server.

See [server settings](/docs/reference/server-settings) for server configuration.
