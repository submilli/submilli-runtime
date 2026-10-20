---
title: "Connect OpenAI Agents"
description: "Connect an OpenAI Agents SDK application to a registered Submilli Blueprint over MCP."
slug: tutorials/connect-openai-agents
docType: how-to
sidebar:
  order: 5
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "dc9ba8c7209c691492e3123afbbbcac1a0d8dac86eb90b18e540c944562ffa09"
  confirmedAt: "2026-10-05T13:01:53.009Z"
---

Connect an OpenAI Agents SDK application to Submilli over MCP, ask it to run
one program that computes the 40th Fibonacci number, and close the MCP session
when the run ends. You need Python 3.10 or
later, `submilli-server` 0.4.1 or later, an OpenAI API key, and a Blueprint
already registered on your server. For this example, use a Blueprint with no
required variables. The calculation does not need Package or tool permissions.

Start the [server](/docs/server/run-the-server) and [register a
Blueprint](/docs/server/register-a-blueprint) before running this example. Keep
`SUBMILLI_SERVER_TOKEN` exported in the shell where you run this example.
Set `SUBMILLI_BLUEPRINT` to the name of the Blueprint you registered:

```sh
export SUBMILLI_BLUEPRINT=your-registered-blueprint
```

If your Blueprint requires variables or harness secrets, send them in the
MCP connection headers described by the [MCP session
headers](/docs/application#three-decisions-the-model-has-no-part-in) reference.
The application supplies those values when the connection opens; the model
cannot set them.

## Install the SDK

Create a project and install the OpenAI Agents SDK:

```sh
mkdir openai-agents && cd openai-agents
python3 -m venv .venv
. .venv/bin/activate
pip install "openai-agents>=0.3"
```

## Write the agent

Save this as `agent.py`:

```python title="agent.py"
"""An OpenAI Agents SDK application backed by Submilli over MCP."""

import asyncio
import os

from agents import Agent, Runner
from agents.mcp import MCPServerStreamableHttp


SERVER = os.environ.get("SUBMILLI_SERVER", "http://127.0.0.1:8128")
SERVER_TOKEN = os.environ["SUBMILLI_SERVER_TOKEN"]
BLUEPRINT = os.environ["SUBMILLI_BLUEPRINT"]


async def main() -> None:
    async with MCPServerStreamableHttp(
        name="submilli",
        params={
            "url": f"{SERVER}/mcp/{BLUEPRINT}",
            "headers": {"Authorization": f"Bearer {SERVER_TOKEN}"},
        },
        client_session_timeout_seconds=120,
    ) as submilli:
        agent = Agent(
            name="fibonacci-agent",
            instructions="Use Submilli to write and run a program that computes the 40th Fibonacci number.",
            mcp_servers=[submilli],
        )
        result = await Runner.run(
            agent,
            "Use Submilli to compute the 40th Fibonacci number.",
            max_turns=20,
        )
        print(result.final_output)


if __name__ == "__main__":
    asyncio.run(main())
```

The MCP URL names the registered Blueprint. The `async with` block is the
session: it opens the connection before the agent runs and closes it after
the result is printed. Keep `client_session_timeout_seconds=120`; the SDK's
five-second default is too short for a program that needs several tool calls.
`max_turns` bounds the model's agent loop.

This example leaves `Agent.tools` empty and exposes Submilli through
`mcp_servers`. Keep application tools out of `Agent.tools` if their calls must
pass through Blueprint permissions: tools added there are called directly by
the agent and bypass Submilli. This MCP connection does not support application-tool callbacks.
To reuse existing tools from Submilli programs, use the
[LangChain middleware integration](/docs/tutorials/connect-deepagents) or
implement the [HTTP callback lifecycle](/docs/reference/http-api#application-tools).

## Try it out

Run the application with the OpenAI key:

```sh
OPENAI_API_KEY=... python agent.py
```

The agent must make a Submilli execute-tool call whose result contains
`102334155`. The final answer alone does not verify the connection; inspect
the Agents SDK trace and confirm both the Submilli execute call and its result.

```text
102334155
```

The model wrote the program, while the runtime compiled and ran it in the
registered Blueprint's session. Check the trace before treating the example
as connected.

You now have an OpenAI Agents SDK application connected to a registered
Blueprint over MCP, with the session closed by the connection context. For
the shared application-tool lifecycle and the LangChain-specific `ptc`
contract, read the [harness tools overview](/docs/tutorials/connect-a-harness#call-harness-tools-from-programs)
and the [`langchain-submilli` reference](https://github.com/submilli/submilli-python/tree/main/packages/langchain-submilli#application-tools).
