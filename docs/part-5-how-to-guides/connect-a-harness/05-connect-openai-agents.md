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
  contentHash: "c40edebf6341ff48bcf42b502321d7fa93a607521af6070671bacc75814c2e0d"
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

Build the MCP connection with `MCPServerStreamableHttp`, using the server URL
and Blueprint path `SUBMILLI_SERVER/mcp/SUBMILLI_BLUEPRINT` and the bearer token
in `SUBMILLI_SERVER_TOKEN`. Pass that connection to the Agent as
`mcp_servers=[submilli]`, and set `tools=[]` and `handoffs=[]`. Remove other
providers from the Agent's `tools` and `handoffs` lists, including tools or
agents assembled from local `tools/` or `handoffs/` configuration. This keeps
the Agent's callable execution surface on the configured Submilli MCP server;
the Blueprint still governs what the server can execute.

</details>

## Set up manually

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

Set these environment variables in this terminal using your existing server,
token, Blueprint, and model configuration:

| Variable | Value |
| --- | --- |
| `SUBMILLI_SERVER` | The server URL where your Blueprint is registered |
| `SUBMILLI_SERVER_TOKEN` | A token accepted by that server |
| `SUBMILLI_BLUEPRINT` | The name of your registered Blueprint |
| `OPENAI_MODEL` | An OpenAI model ID available to your account; also export your existing `OPENAI_API_KEY` in this terminal |

The model credentials are separate from the Submilli server token. Reuse your
chosen provider’s setup. Do not put credentials in the file you save next.

### Save the runnable agent

Save this complete file as `submilli-agent.py` in the folder you opened.

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
    model = required("OPENAI_MODEL")
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
            name="Submilli agent", model=model, tools=[], handoffs=[],
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
