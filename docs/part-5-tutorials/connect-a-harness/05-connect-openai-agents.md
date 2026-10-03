---
title: "Connect OpenAI Agents"
description: "Run the research agent on the OpenAI Agents SDK: the connection scope is the session, then a real conversation."
slug: tutorials/connect-openai-agents
sidebar:
  order: 5
---

In this tutorial we will run the research agent on the OpenAI Agents SDK:
its programs executed on the server as the signed-in user, `u_ada` in the examples, then one real conversation. You need the server and the `research`
blueprint from [Connect a harness](/docs/tutorials/connect-a-harness),
with `SUBMILLI_SERVER_TOKEN` still exported, Python 3.10 or later, and an
OpenAI key for the conversation; this SDK speaks to OpenAI's models,
whichever provider the blueprint's own model uses.

## Start the project

In `harnesses`, make a directory for this harness, a virtual environment,
and install the dependencies. The full project is
[`examples/harnesses/openai-agents/`](https://github.com/submilli/submilli-runtime/tree/main/examples/harnesses/openai-agents).

```sh
mkdir openai-agents && cd openai-agents
python -m venv .venv && . .venv/bin/activate
pip install "openai-agents>=0.3"
```

## The agent

Save this as `agent.py`. It reads the brief from `../prompt.txt`:

```python title="agent.py"
"""An OpenAI Agents SDK agent that runs its programs on submilli-server, over MCP."""

import asyncio
import os
import pathlib

from agents import Agent, Runner
from agents.mcp import MCPServerStreamableHttp

SUBMILLI_SERVER = os.environ.get("SUBMILLI_SERVER", "http://127.0.0.1:8128")
# The API token this application was given for the server.
SUBMILLI_SERVER_TOKEN = os.environ["SUBMILLI_SERVER_TOKEN"]
BLUEPRINT = "research"


def instructions(user_id: str) -> str:
    # The agent's brief, kept beside the blueprint; `{userId}` names the user.
    brief = (pathlib.Path(__file__).parent.parent / "prompt.txt").read_text()
    return brief.replace("{userId}", user_id)


async def answer(question: str, user_id: str, model=None) -> str:
    # One connection per user: the binding is fixed when it opens.
    async with MCPServerStreamableHttp(
        name="submilli",
        params={
            "url": f"{SUBMILLI_SERVER}/mcp/{BLUEPRINT}",
            "headers": {
                "Authorization": f"Bearer {SUBMILLI_SERVER_TOKEN}",
                "submilli-variables": f"userId={user_id}",
            },
        },
    ) as submilli:
        agent = Agent(
            name="researcher",
            instructions=instructions(user_id),
            model=model,
            mcp_servers=[submilli],
        )
        result = await Runner.run(agent, question, max_turns=20)
        return result.final_output


if __name__ == "__main__":
    # In a real application the user comes from the signed-in session.
    print(asyncio.run(answer("What is new in the latest stable release of Rust? Save a note with your sources.", "u_ada")))
```

Notice that the `async with` block is the session. The SDK connects when
the block opens and ends the session when it closes, so the agent is
built and run inside it. `model=None` leaves the choice to the SDK's
default; pass a model name to choose one. `max_turns` bounds the loop.

## One conversation

```sh
OPENAI_API_KEY=... python agent.py
```

The model's programs are its own, and another run writes different ones,
so watch for the shape: the model asks what it can import before it
writes anything; a program that oversteps, such as listing the volume's
root, is refused, and the model moves on as the denial tells it to; the
programs are small; and the answer ends by naming the note it saved under
`/u_ada/notes`, which is a file on the server's volume. Ask again and the
model reads that note before it searches.

You have the research agent running on the OpenAI Agents SDK, every
program it writes executed on the server as the signed-in user, and the
binding proved on the index before any model was involved. Project:
[`examples/harnesses/openai-agents/`](https://github.com/submilli/submilli-runtime/tree/main/examples/harnesses/openai-agents).
