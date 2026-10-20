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
  contentHash: "6a4df5c8ffbe89da4b88f2cbe7654ad84447d9f53bd6db9685b8fc871c148d9b"
  confirmedAt: "2026-10-05T13:01:53.009Z"
---

## Use your coding agent

Choose an icon to open a prefilled prompt. Select your project, review, and send.

```text agent-prompt
Read https://submilli.ai/docs/tutorials/connect-openai-agents/ as the source for this setup.

Inspect my project and follow this guide to create agent.py.
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

Connect an OpenAI Agents SDK application to Submilli over MCP, ask it to run
a small arithmetic program, and close the MCP session
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

### Write the agent

Create `agent.py` in the project directory and paste the code below:

```python title="agent.py"
"""An OpenAI Agents SDK application backed by Submilli over MCP."""

import asyncio
import os
import sys

from agents import Agent, Runner
from agents.mcp import MCPServerStreamableHttp


args = sys.argv[1:]
checking = args == ["--check"]
prompt = " ".join(args)
if not checking and not prompt.strip():
    raise SystemExit("Usage: python agent.py '<message>' (or --check)")

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
        if checking:
            tools = await submilli.list_tools()
            if not any(tool.name == "submilli__typescript__execute" for tool in tools):
                raise RuntimeError("Submilli execute tool is unavailable")
            print("Connected to Submilli.")
            return
        agent = Agent(
            name="submilli-agent",
            # Change this model ID to use another OpenAI model.
            model="gpt-5-mini",
            instructions="Use Submilli to run the requested TypeScript code.",
            mcp_servers=[submilli],
        )
        result = await Runner.run(
            agent,
            prompt,
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

### Check the connection

With the setup environment active, run this without a model request:

```sh
python agent.py --check
```

Look for `Connected to Submilli.`. This checks the server and registered
Blueprint; the model runs only when you supply a message.

## Try it out

Run this from the project directory with the setup environment active:

```sh
python agent.py \
  'Run this program through Submilli and report its result:
export function main(): number { return 2 + 2; }'
```

The agent must make a Submilli execute-tool call whose result contains
`4`. The final answer alone does not verify the connection; inspect
the Agents SDK trace and confirm both the Submilli execute call and its result.

```text
4
```

The model passed the supplied program to Submilli, which compiled and ran it in the
registered Blueprint's session. Check the trace before treating the example
as connected.

You now have an OpenAI Agents SDK application connected to a registered
Blueprint over MCP, with the session closed by the connection context. For
the shared application-tool lifecycle and the LangChain-specific `ptc`
contract, read the [harness tools overview](/docs/tutorials/connect-a-harness#call-harness-tools-from-programs)
and the [`langchain-submilli` reference](https://github.com/submilli/submilli-python/tree/main/packages/langchain-submilli#application-tools).
