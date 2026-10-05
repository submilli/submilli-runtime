---
title: "Connect OpenAI Agents"
description: "Run the research agent on the OpenAI Agents SDK: the connection scope is the session, then a real conversation."
slug: tutorials/connect-openai-agents
sidebar:
  order: 5
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "f2c742b9d2d1f8d881ebe0af39bb211917ebf07bbce1390f48ba181cdbe1ccaa"
  confirmedAt: "2026-10-05T13:01:53.009Z"
---

In this tutorial we will run the research agent on the OpenAI Agents SDK,
with its programs executed on the server as the signed-in user (`u_ada` in the examples). You need the server and the `research`
Blueprint from [Connect a harness](/docs/tutorials/connect-a-harness),
with `SUBMILLI_SERVER_TOKEN` still exported, Python 3.10 or later, and an
OpenAI key for the conversation. This SDK speaks to OpenAI's models,
whichever provider the Blueprint's model uses.

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


# The agent's brief, kept beside the blueprint.
INSTRUCTIONS = (pathlib.Path(__file__).parent.parent / "prompt.txt").read_text()


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
        # The SDK gives up on a tool call after 5 seconds by default; a
        # program that searches and reads pages takes longer.
        client_session_timeout_seconds=120,
    ) as submilli:
        agent = Agent(
            name="researcher",
            instructions=INSTRUCTIONS,
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
built and run inside it. Keep `client_session_timeout_seconds`. The SDK
gives up on a tool call after five seconds by default, and a program
that searches and reads pages takes longer. Without it, the run gets an
error while the server is still working. `model=None` leaves the choice
to the SDK's default. Pass a model name to choose one. `max_turns`
bounds the loop.

## One conversation

```sh
OPENAI_API_KEY=... python agent.py
```

This is one real run, with the SDK's default model, gpt-5.6-luna, made
after the other four tutorials' agents had answered the same question
for the same user. The model's programs are its own, and another run
writes different ones. Its first program listed the notebook, got
today's date, and searched:

```typescript
import jina from "@submilli/jina";
import * as fs from "submilli:fs";

function main(): string {
  const entries: string[] = [];
  for (const e of fs.list("/notes", false)) entries.push(e.path + " (" + e.kind + ")");
  const today = Temporal.Now.plainDateISO().toString();
  const r = jina.searchJson("latest stable Rust release release notes 2025", {site:"blog.rust-lang.org"});
  const out: string[] = ["today=" + today, "notes=" + entries.join(", ")];
  for (const x of r) out.push(x.title + " | " + x.url + " | " + x.description);
  return out.join("\n");
}
```

The answer began and ended:

```text
The latest stable Rust release is **1.99.0**, released **October 1, 2026**.
…
I saved and updated the research note at `/notes/rust-latest-release.md`.
```

It updated the note the other agents kept, a file on the server's
volume, there for the next conversation `u_ada` opens, on this harness
or any other.

You have the research agent running on the OpenAI Agents SDK, each
program it writes executed on the server as the signed-in user, and the
binding proved on the index before any model was involved. Project:
[`examples/harnesses/openai-agents/`](https://github.com/submilli/submilli-runtime/tree/main/examples/harnesses/openai-agents).
