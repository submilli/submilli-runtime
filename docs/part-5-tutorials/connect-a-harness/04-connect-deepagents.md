---
title: "Connect deepagents"
description: "Run the research agent on LangChain deepagents: one session held open for the run, the framework's own file tools denied so notes go through Submilli, then a real conversation."
slug: tutorials/connect-deepagents
sidebar:
  order: 4
---

In this tutorial we will run the research agent on LangChain deepagents:
its programs executed on the server as the signed-in user, `u_ada` in the examples, then one real conversation. You need the server and the `research`
blueprint from [Connect a harness](/docs/tutorials/connect-a-harness),
with `SUBMILLI_SERVER_TOKEN` still exported, Python 3.11 or later, and a
key from your model provider for the conversation. The agent file
names Claude; for Google or OpenAI, the `model` argument takes
`google_genai:gemini-3.8-flash` or `openai:gpt-4o-mini` instead, with
that provider's LangChain package installed and its key in the
environment.

## Start the project

In `harnesses`, make a directory for this harness, a virtual environment,
and install the dependencies. The full project is
[`examples/harnesses/deepagents/`](https://github.com/submilli/submilli-runtime/tree/main/examples/harnesses/deepagents).

```sh
mkdir deepagents && cd deepagents
python -m venv .venv && . .venv/bin/activate
pip install "deepagents>=0.7" "langchain-anthropic>=1.0" "langchain-mcp-adapters>=0.3"
```

## The agent

Save this as `agent.py`. It reads the brief from `../prompt.txt`:

```python title="agent.py"
"""A LangChain deepagents agent that runs its programs on submilli-server, over MCP."""

import asyncio
import os
import pathlib

from deepagents import create_deep_agent
from deepagents.middleware.filesystem import FilesystemPermission
from langchain_mcp_adapters.sessions import create_session
from langchain_mcp_adapters.tools import load_mcp_tools

SUBMILLI_SERVER = os.environ.get("SUBMILLI_SERVER", "http://127.0.0.1:8128")
# The API token this application was given for the server.
SUBMILLI_SERVER_TOKEN = os.environ["SUBMILLI_SERVER_TOKEN"]
BLUEPRINT = "research"


def instructions(user_id: str) -> str:
    # The agent's brief, kept beside the blueprint; `{userId}` names the user.
    brief = (pathlib.Path(__file__).parent.parent / "prompt.txt").read_text()
    return brief.replace("{userId}", user_id)


async def answer(question: str, user_id: str, model="anthropic:claude-haiku-4-5") -> str:
    submilli = {
        "transport": "streamable_http",
        "url": f"{SUBMILLI_SERVER}/mcp/{BLUEPRINT}",
        "headers": {
            "Authorization": f"Bearer {SUBMILLI_SERVER_TOKEN}",
            "submilli-variables": f"userId={user_id}",
        },
    }

    # One session per user: the binding is fixed when the session opens.
    async with create_session(submilli) as session:
        await session.initialize()
        agent = create_deep_agent(
            model=model,
            tools=await load_mcp_tools(session),
            system_prompt=instructions(user_id),
            # deepagents has file tools of its own, which keep files in the
            # conversation. Deny them, so that notes go through Submilli.
            permissions=[FilesystemPermission(operations=["read", "write"], paths=["/**"], mode="deny")],
        )
        result = await agent.ainvoke({"messages": [{"role": "user", "content": question}]})
        return result["messages"][-1].text


if __name__ == "__main__":
    # In a real application the user comes from the signed-in session.
    print(asyncio.run(answer("What is new in the latest stable release of Rust? Save a note with your sources.", "u_ada")))
```

Notice that the agent is built and run inside the `create_session`
block. That block opens one connection, which the tools share for as
long as it runs. Tools loaded without a session, as
`MultiServerMCPClient.get_tools()` does, open a new connection for every
call, and each program would run in a session of its own and find none
of the state the last one left.

Notice the `permissions` line too. deepagents gives its agent a planning
tool and file tools of its own, which keep files in the conversation's
state. Those files are not the notebook: they never reach Submilli and
are gone when the conversation ends. Run without the line, a model asked
to save a note used those tools, and the note was never written. The
line denies them. An agent built with LangChain's `create_agent` or with
LangGraph takes the same tools.

## One conversation

```sh
ANTHROPIC_API_KEY=... python agent.py
```

In the book's run, with Claude Haiku 4.5, the model's programs were
its own, and another run writes different ones, so watch for the shape: the model asks what it can import before it
writes anything; a program that oversteps, such as listing the volume's
root, is refused, and the model moves on as the denial tells it to; the
programs are small; and the answer ends by naming the note it saved under
`/u_ada/notes`, which is a file on the server's volume. Ask again and the
model reads that note before it searches.

## With your coding agent

With the [skill](/docs/install#the-skill) installed, your coding
assistant finds the mistakes this page warns about. This project was a
deepagents agent that loads its tools with
`MultiServerMCPClient.get_tools()`, under a blueprint whose `vfs` is
`per_session`:

```text
My deepagents agent saves pages under /pages in one program, and the next program can't find them. Why?
```

The assistant reads `agent.py` and the blueprint and names the cause in a
few steps. Tools loaded without a session open a new MCP session for
every call. Under `per_session`, each program therefore gets a new,
empty filesystem. Its fix is the one above: one session, held open for
the whole run. It adds that files still won't survive between separate
runs of the script, which takes a named volume.

You have the research agent running on deepagents, every program it
writes executed on the server as the signed-in user, and the binding proved on
the index before any model was involved. Project:
[`examples/harnesses/deepagents/`](https://github.com/submilli/submilli-runtime/tree/main/examples/harnesses/deepagents).
