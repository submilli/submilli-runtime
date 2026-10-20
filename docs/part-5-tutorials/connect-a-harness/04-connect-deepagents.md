---
title: "Connect Deep Agents"
description: "Connect a LangChain Deep Agents application to a registered Submilli Blueprint with langchain-submilli."
slug: tutorials/connect-deepagents
docType: how-to
sidebar:
  order: 4
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "3dc9c0498837b0ff38aa44cea710435ae968524be3b57faf69988c3a5ed97144"
  confirmedAt: "2026-10-09T15:40:52.000Z"
---

## Use your coding agent

Choose an icon to open a prefilled prompt. Select your project, review, and send.

```text agent-prompt
Read https://submilli.ai/docs/tutorials/connect-deepagents/ as the source for this setup.

Inspect my project and follow this guide to create agent.py.
Reuse valid local Submilli connection settings and provider credentials. If
SUBMILLI_SERVER, SUBMILLI_SERVER_TOKEN, or SUBMILLI_BLUEPRINT is missing, ask
for that value or its local configuration source. Do not invent values,
expose secrets, create a Blueprint, or change the server. Ask for missing
provider credentials without printing them. Install only the
missing dependencies shown in this guide. Preserve its tool restrictions and
session cleanup. Prepare the file without running it or making a model request.
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

Connect a LangChain Deep Agents application to Submilli, run a small
arithmetic program, and close the server session when the
thread ends. You need Python 3.11 or later,
`submilli-server` 0.4.1 or later, a model-provider key, and a Blueprint
already registered on your server. For this example, use a Blueprint with no
required variables. The calculation does not need Package or tool permissions.

Start the [server](/docs/server/run-the-server), [create a Blueprint with no
grants](/docs/blueprints/start-a-blueprint#start-from-nothing-allowed), and
[register it](/docs/server/register-a-blueprint#register-it) before running this
example. Keep
`SUBMILLI_SERVER_TOKEN` exported in the shell where you run this example.
If your Blueprint requires variables or harness secrets, pass them with the
middleware's `variables=` and `secrets=` callables from your run context. The
values are bound when the session opens; the model cannot set them. The
[`langchain-submilli` middleware reference](https://github.com/submilli/submilli-python/tree/main/packages/langchain-submilli#middleware-reference)
describes the binding options.

### Install the integration

Create a project and install Deep Agents, the Submilli middleware, and the
OpenAI LangChain integration:

```sh
mkdir deepagents && cd deepagents
python3 -m venv .venv
. .venv/bin/activate
pip install "langchain-submilli[deepagents]" \
  "deepagents==0.7.28" langchain-openai
```

### Write the agent

Create `agent.py` in the project directory and paste the code below:

```python title="agent.py"
import asyncio
import os
import sys

from deepagents import (
    GeneralPurposeSubagentProfile,
    HarnessProfile,
    create_deep_agent,
    register_harness_profile,
)
from langchain_submilli import SubmilliMiddleware
from langgraph.checkpoint.memory import InMemorySaver
from submilli import AsyncSubmilli

prompt = " ".join(sys.argv[1:])
if not prompt.strip():
    raise SystemExit("Usage: python agent.py '<message>'")

model_name = "openai:gpt-5-mini"
register_harness_profile(
    model_name,
    HarnessProfile(
        excluded_tools=frozenset({
            "ls", "read_file", "write_file", "edit_file", "delete",
            "glob", "grep", "execute", "task", "write_todos",
        }),
        general_purpose_subagent=GeneralPurposeSubagentProfile(enabled=False),
    ),
)


async def main() -> None:
    async with AsyncSubmilli() as client:
        submilli = SubmilliMiddleware(
            blueprint=os.environ["SUBMILLI_BLUEPRINT"], client=client,
        )
        agent = create_deep_agent(
            model=model_name,
            tools=[],
            subagents=[],
            middleware=[submilli],
            checkpointer=InMemorySaver(),
        )
        config = {"configurable": {"thread_id": "submilli-example"}}
        try:
            result = await agent.ainvoke(
                {"messages": [{"role": "user", "content": prompt}]}, config,
            )
            for message in result["messages"]:
                if message.type == "tool" and message.name == "submilli__typescript__execute":
                    print(f"Submilli execute result: {message.content}", file=sys.stderr)
            print(result["messages"][-1].text)
        finally:
            await submilli.aclose_thread(agent, config)


if __name__ == "__main__":
    asyncio.run(main())
```

`AsyncSubmilli` reads the server address from `SUBMILLI_SERVER` or uses
`http://127.0.0.1:8128`, and reads the token from
`SUBMILLI_SERVER_TOKEN`. The checkpointer keeps the server session id in the
thread state. `aclose_thread` closes that session in the `finally` block. The async context
closes the HTTP client.

The profile removes Deep Agents' built-in tools, including file, shell, and
subagent delegation tools, and disables its general-purpose subagent.
`tools=[]` and `subagents=[]` add no direct tools or custom subagents. This
example exposes only Submilli's tools to the model. The profile is registered
for `model_name` in this process; use the same name in `create_deep_agent`.

## Try it out

Run this from the project directory with the setup environment active:

```sh
python agent.py \
  'Run this program through Submilli and report its result:
export function main(): number { return 2 + 2; }'
```

The agent should make a `submilli__typescript__execute` tool call and print
its tool result on standard error. The final answer should contain:

```text
4
```

The model passed the supplied program to Submilli, which compiled and ran it in the
registered Blueprint's session. Check the printed execute-tool result as well
as the final answer to confirm that the connection completed.

You now have a Deep Agents application whose program execution is governed by
an already registered Blueprint and whose thread closes its server session.
For the same connection through another harness, see the [OpenAI Agents
SDK](/docs/tutorials/connect-openai-agents) tutorial.

## Use application tools

<a id="your-tools-in-programs"></a>

To reuse application tools under Blueprint permissions, pass your LangChain
tools to `SubmilliMiddleware`'s `ptc` parameter. They become callable from
Submilli programs; the middleware does not expose them directly to the model.
Keep `create_deep_agent`'s `tools` empty: adding the same tools there creates a
direct path that bypasses Submilli. Use `interrupt_on` for callback
calls that need LangChain human-in-the-loop approval. The
[harness tools overview](/docs/tutorials/connect-a-harness#call-harness-tools-from-programs)
describes the shared lifecycle; the [`langchain-submilli` reference](https://github.com/submilli/submilli-python/tree/main/packages/langchain-submilli#application-tools)
covers the integration contract.
