---
docType: how-to
title: "Connect OpenAI Agents SDK to Submilli"
description: "Create an OpenAI Agents SDK agent that uses Submilli for permitted code execution."
showDescription: false
slug: how-to/connect-openai-agents
sidebar:
  order: 5
authorship:
  label: ai-generated
  confirmed: true
  contentHash: "1a7ca5361290a01ca70baa6684aa5b8f638f906b808b099189a091b01e4edc8c"
  confirmedAt: "2026-10-18T13:09:05.218103+00:00"
# Unpublished AI draft. Human prose rewrite required before publication.
---

Create a fresh OpenAI Agents SDK agent that sends its execution requests to your
existing Submilli server and registered Blueprint, then run a small program and
see its result in your terminal.

## Use your coding agent

Open a project directory in your coding agent.
[Install the Submilli skill](/docs/install#the-skill) if needed, then restart
the coding agent to load it. Give it this prompt:

```text agent-prompt
Read https://submilli.ai/docs/how-to/connect-openai-agents/ as the source for this setup.

Use the Submilli skill and this guide to create a fresh OpenAI Agents SDK
agent in a new or existing project folder. Preserve my existing agent
configuration. Create the complete submilli-agent.py file shown in this guide,
including its command-line prompt and tool-result logging. Connect it to my
existing Submilli server URL, server token, and registered Blueprint. Configure
the Agent with tools=[], handoffs=[], and mcp_servers=[submilli], so Submilli
is its only execution route. Inspect my provider/model setup and ask only for
essential missing choices; do not guess values or reveal secrets. Install the
dependencies shown here and verify the connection with --check without making
a model request. Leave the file ready for the exact Try it out command. Tell me any remaining human
setup.
```

After setup, [try it out](#try-it-out). To set up the same file yourself,
follow the manual path.

<details>
<summary>Connecting an existing agent</summary>

1. Build the MCP connection with `MCPServerStreamableHttp`, using the server
   URL and Blueprint path `SUBMILLI_SERVER/mcp/SUBMILLI_BLUEPRINT`.
2. Use the bearer token in `SUBMILLI_SERVER_TOKEN`.
3. Pass the connection to the Agent as `mcp_servers=[submilli]`.
4. Set `tools=[]` and `handoffs=[]`.
5. Remove other providers from the Agent's `tools` and `handoffs` lists,
   including tools or agents assembled from local `tools/` or `handoffs/`
   configuration.

This keeps the Agent's callable execution surface on the configured Submilli
MCP server. The Blueprint still governs what the server can execute.

</details>

## Set up manually

### Prerequisites

Before this guide, use the [Submilli install guide](/docs/install), [server
guide](/docs/server/run-the-server), and [Blueprint registration
guide](/docs/server/register-a-blueprint) if you still need to prepare the
service. This page assumes you have a running server, a token it accepts, and a
Blueprint already registered on it.
You also need Python 3.11 or later and model access for OpenAI Agents SDK.

### Prepare the agent folder

Open the folder where you want to save the agent and create its virtual environment:

```sh
python3 -m venv .venv
. .venv/bin/activate
python -m pip install "openai-agents>=0.3"
```

Replace the example values, then run these commands in bash or zsh. The file
reads this terminal's environment and does not automatically load `.env` files.

```sh
export SUBMILLI_SERVER='http://127.0.0.1:8128'
export SUBMILLI_SERVER_TOKEN='your-server-token'
export SUBMILLI_BLUEPRINT='your-registered-blueprint'
export OPENAI_API_KEY='your-openai-api-key'
```

This example uses OpenAI's `gpt-5-mini`. To choose another model, edit `model`
in the file.

### Create the agent file

Create `submilli-agent.py` in the folder where you installed the dependencies,
and paste the code below.

```python title="submilli-agent.py"
import asyncio
import os
import sys


def required(name):
    value = os.environ.get(name)
    if not value:
        raise RuntimeError(f"Set {name} in this terminal before running this file.")
    return value

from agents import Agent, Runner
from agents.mcp import MCPServerStreamableHttp


async def main():
    required("OPENAI_API_KEY")
    server = required("SUBMILLI_SERVER").rstrip("/")
    blueprint = required("SUBMILLI_BLUEPRINT")
    token = required("SUBMILLI_SERVER_TOKEN")
    async with MCPServerStreamableHttp(
        name="submilli",
        params={
            "url": f"{server}/mcp/{blueprint}",
            "headers": {"Authorization": f"Bearer {token}"},
        },
        client_session_timeout_seconds=120,
    ) as submilli:
        tools = await submilli.list_tools()
        if not any(tool.name == "submilli__typescript__execute" for tool in tools):
            raise RuntimeError("Submilli's execute tool was not found")
        print("Connected to Submilli.")
        if sys.argv[1:] == ["--check"]:
            return
        agent = Agent(
            name="Submilli agent", model="gpt-5-mini", tools=[], handoffs=[],
            mcp_servers=[submilli],
            instructions="Run requested programs through Submilli and report tool results.",
        )
        prompt = " ".join(sys.argv[1:]) or 'Run this program through Submilli and report its result: export function main(): number { return 2 + 2; }'
        result = await Runner.run(agent, prompt, max_turns=10)
        for item in result.new_items:
            if item.type in ("tool_call_item", "tool_call_output_item"):
                print(item.type, item.raw_item)
        print("Agent answer:", result.final_output)


asyncio.run(main())
```

## Try it out

With the environment variables from setup and the virtual environment active,
run the saved file with its built-in 2 + 2 prompt:

```sh
python submilli-agent.py
```

Look for `Connected to Submilli.`, a Submilli execution tool call containing
`2 + 2`, and the returned value `4`. The tool call and returned value prove
that the program ran through the configured server and Blueprint.

## Troubleshooting

- Connection refused: check `SUBMILLI_SERVER` and whether the server is running.
- `401`: use a token accepted by that server.
- Blueprint not found: check `SUBMILLI_BLUEPRINT` against the registered name.
- Model authentication error: check the model provider’s credentials in this terminal.
- Tool result but no final answer: read the SDK error or turn-limit result printed by the file.

See [server settings](/docs/reference/server-settings) for server configuration.
