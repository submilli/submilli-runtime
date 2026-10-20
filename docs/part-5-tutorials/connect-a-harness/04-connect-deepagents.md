---
title: "Connect deepagents"
description: "Run the research agent on LangChain deepagents with langchain-submilli: the user comes from each run's context, a thread keeps one session, deepagents' own file tools are hidden so notes go through Submilli, then a real conversation."
slug: tutorials/connect-deepagents
sidebar:
  order: 4
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "67236fc2328edcfa4e96de3d0fb14074bd2bdf84e3b79a445e8084148b1d0bc1"
  confirmedAt: "2026-10-09T15:40:52.000Z"
---

In this tutorial we will run the research agent on LangChain deepagents,
with its programs executed on the server as the signed-in user (`u_ada`
in the examples). The agent connects through `langchain-submilli`, whose
`SubmilliMiddleware` gives it Submilli's tools. You need the server and
the `research` Blueprint from [Connect a
harness](/docs/tutorials/connect-a-harness), with
`SUBMILLI_SERVER_TOKEN` still exported, submilli-server 0.4.1 or later,
Python 3.11 or later, and a key from your model provider for the
conversation. The agent file names Claude. For Google or OpenAI, the
`model` argument takes `google_genai:gemini-3.8-flash` or
`openai:gpt-5.5` instead, with that provider's LangChain Package
installed and its key in the environment.

## Start the project

In `harnesses`, make a directory for this harness, a virtual environment,
and install the dependencies. The `deepagents` extra installs deepagents
with the middleware. The full project is
[`examples/harnesses/deepagents/`](https://github.com/submilli/submilli-runtime/tree/main/examples/harnesses/deepagents).

```sh
mkdir deepagents && cd deepagents
python3 -m venv .venv && . .venv/bin/activate
pip install "langchain-submilli[deepagents]" "langchain-anthropic>=1.0"
```

## The agent

Save this as `agent.py`. It reads the brief from `../prompt.txt`:

```python title="agent.py"
"""A LangChain deepagents agent that runs its programs on submilli-server, through langchain-submilli."""

import asyncio
import pathlib
import sys
from typing import TypedDict

from deepagents import create_deep_agent
from langchain_core.runnables import RunnableConfig
from langgraph.checkpoint.memory import InMemorySaver

from langchain_submilli import SubmilliMiddleware

# The agent's brief, kept beside the blueprint.
INSTRUCTIONS = (pathlib.Path(__file__).parent.parent / "prompt.txt").read_text()


class Context(TypedDict):
    """What each run brings: the user it acts for."""

    user_id: str


# Server and token come from SUBMILLI_SERVER and SUBMILLI_SERVER_TOKEN.
submilli = SubmilliMiddleware(
    blueprint="research",
    # deepagents' own file tools keep files in the conversation, not in the
    # user's notebook. Hide them, so that notes go through Submilli.
    hide_filesystem_tools=True,
)

# Built once and shared: the user comes with each run, in its context.
agent = create_deep_agent(
    model="anthropic:claude-sonnet-5",
    system_prompt=INSTRUCTIONS,
    middleware=[submilli],
    checkpointer=InMemorySaver(),
    context_schema=Context,
)


async def answer(question: str, user_id: str, thread_id: str) -> str:
    config: RunnableConfig = {"configurable": {"thread_id": thread_id}}
    result = await agent.ainvoke(
        {"messages": [{"role": "user", "content": question}]}, config, context=Context(user_id=user_id)
    )
    for message in result["messages"]:
        if message.type == "tool" and message.artifact:
            # Where each program's run is in the server's audit trail.
            print(f"[{message.name}] execution {message.artifact.get('execution_id')}", file=sys.stderr)
    return str(result["messages"][-1].text)


async def main() -> None:
    question = "What is new in the latest stable release of Rust? Save a note with your sources."
    config: RunnableConfig = {"configurable": {"thread_id": "example"}}
    try:
        # In a real application the user comes from the signed-in session.
        print(await answer(question, user_id="u_ada", thread_id="example"))
    finally:
        # The session belongs to the thread. This one is done with it.
        await submilli.aclose_thread(agent, config)


if __name__ == "__main__":
    asyncio.run(main())
```

Notice that the agent is built once and shared by all users. The
middleware reads the server's address from `SUBMILLI_SERVER`, or uses
`http://127.0.0.1:8128`, and the token from `SUBMILLI_SERVER_TOKEN`. The
user comes with each run, in its context, and the middleware binds
`user_id` to the Blueprint's `userId` variable. For other variables, or
for a Blueprint's `harness` secrets, `variables=` and `secrets=` take a
function from the context to a `dict[str, str]`. The model never sees
these values, and none of its tools can set them. The middleware gives
the model six tools, with the descriptions the server writes for the
Blueprint:

- `submilli__typescript__execute` runs a program.
- `submilli__typescript__last_run` returns the thread's last run again,
  with everything it logged.
- `submilli__typescript__packages__search` and
  `submilli__typescript__packages__docs` find the Packages the Blueprint
  allows and return their documentation.
- `submilli__typescript__builtins__list` and
  `submilli__typescript__builtins__docs` do the same for the built-ins.

None of them reads or writes files. Programs do that, through
`submilli:fs`.

Notice the checkpointer. The first time the model runs a program in a
thread, the middleware opens a session on the server, bound to the
Blueprint and to this run's variables, and keeps the session's id in
the agent's state. The checkpointer saves that state, so the thread's
next turn runs in the same session and finds the same files.
`InMemorySaver` keeps the state in this process. With a checkpointer
that stores it, such as LangGraph's Postgres one, the thread can also
resume in the same session from another process. A run on that thread
whose context names another user is refused with
`SubmilliSessionMismatchError` before the model is called.
`aclose_thread` ends the session when the conversation is over.
Otherwise the Blueprint's idle timeout ends it.

Notice `hide_filesystem_tools=True` too. deepagents gives its agent file
tools of its own, which keep files in the conversation's state. Those
files are not the notebook. They never reach Submilli and are gone when
the conversation ends. The flag removes `ls`, `read_file`, `write_file`,
`edit_file`, `delete`, `glob`, `grep` and `execute` from the agent. It
doesn't reach deepagents' built-in `general-purpose` subagent. To deny
the file tools there too, also pass
`permissions=[FilesystemPermission(operations=["read", "write"], paths=["/**"], mode="deny")]`
to `create_deep_agent`.

Each program goes to the server with an `Idempotency-Key` made from the
tool call's id. When a call is retried, as when a thread resumes from a
checkpoint taken mid-call, the server replays the recorded result and
the program doesn't run twice. Each tool message's `artifact` carries
the run's `execution_id`, its place in the server's [audit
trail](/docs/reference/audit-trail), and `agent.py` prints it.

## One conversation

```sh
ANTHROPIC_API_KEY=... python agent.py
```

This is one real run, with Claude Sonnet 5 as the model, for a `u_ada`
whose notebook held one file, `check.md`, from the test program in
Connect a harness. The model's
programs are its own, and another run writes different ones, so watch
for the shape.

The model's first program was written for Node.js, with an `async main`
and a dynamic `import`, and it didn't compile. The model read the docs
for `submilli:fs` and `@submilli/jina`, and after one more failed
attempt it listed the notebook, looked for a note on Rust, and asked for
today's date, in one program:

```typescript
import * as fs from "submilli:fs";

function main(): string {
  const entries: string[] = [];
  for (const e of fs.list("/notes", false)) {
    entries.push(`${e.kind}:${e.path}`);
  }
  let rustNote = "NO NOTE YET";
  if (fs.exists("/notes/rust.md")) {
    const t = fs.readText("/notes/rust.md");
    rustNote = t ?? "undefined (too large?)";
  }
  const today = Temporal.Now.plainDateISO().toString();
  return JSON.stringify({ entries, rustNote, today });
}
```

```json
{"entries":["file:/notes/check.md"],"rustNote":"NO NOTE YET","today":"2026-10-15"}
```

There was no note on Rust. In four more programs it searched, read the
release announcement, and wrote what it learned to `/notes/rust.md`.
Then it answered:

```text
## Latest stable Rust: 1.99.0 (released 2026‑10‑01)

As of today (2026‑10‑15), **Rust 1.99.0** is the current stable release (confirmed via blog.rust-lang.org's "latest" redirect and cross-checked against releases.rs, which lists Stable: 1.99.0, Beta: 1.100.0, Nightly: 1.101.0).
…
I saved this (with sources) to `/notes/rust.md` for future sessions.
```

Before the answer, `agent.py` printed the execution id of each program
on standard error:

```text
[submilli__typescript__execute] execution abae7f2c-26bc-4489-94be-5ae71f60bb7b
…
[submilli__typescript__execute] execution a1dd6af1-6122-463a-a03c-7c66c47e8428
```

The run took nine tool calls: seven programs, two of which failed to
compile, and two documentation lookups. Then `aclose_thread` ended the
session, and the note outlived it. `/notes` is the user's notebook, so
the next session for `u_ada`, from this harness or another, finds it.
Run the test program as `u_ada`:

```sh
submilli server run-code ../note.ts --blueprint research --var userId=u_ada
```

```text
notes before: rust.md, check.md
```

## Your tools in programs

Programs can also call your application's own LangChain tools, such as
one that queries your database. Pass them to the middleware as
`ptc=[...]`, for programmatic tool calling, and programs import them as
the `@tools` Package. `interrupt_on=` asks a person to approve those
calls through LangChain's human-in-the-loop flow. The Blueprint has to
grant the `tools.call` capability. The package's README shows how, in
[Application tools](https://github.com/submilli/submilli-python/tree/main/packages/langchain-submilli#application-tools).

## With your coding agent

With the [skill](/docs/install#the-skill) installed, your coding
assistant finds the mistakes this page warns about. This project was a
deepagents chat app that keeps each conversation's messages itself and
sends the whole history with each turn, so its agent had no
checkpointer:

```text
My deepagents chat app saves the pages it downloads under /work in one turn, and on the next turn of the same conversation its program says they aren't there. The notes in /notes are fine. Why?
```

The assistant reads `app.py` and the Blueprint and names the cause in a
few steps. The middleware keeps the session's id in the agent's state,
and without a checkpointer that state is dropped when `ainvoke` returns,
so each turn opens a new session. Everything outside `/notes` lasts
only as long as its session, so the new session has no pages. Its fix
is the one in `agent.py`, a checkpointer and a `thread_id` for each
conversation, with only the new message sent each turn. It adds that each turn had left a session open on the
server until its idle timeout.

You have the research agent running on deepagents, each program it
writes executed on the server as the user its run's context names, and
its notes kept in that user's notebook. For the middleware's other
options, read the [langchain-submilli
README](https://github.com/submilli/submilli-python/tree/main/packages/langchain-submilli#readme).
For the endpoints it calls, read the [HTTP API
reference](/docs/reference/http-api). Project:
[`examples/harnesses/deepagents/`](https://github.com/submilli/submilli-runtime/tree/main/examples/harnesses/deepagents).
