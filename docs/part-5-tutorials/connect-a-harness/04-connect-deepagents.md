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
  contentHash: "a1c83543e8c29845b9689558c033a282216a64d5940842935147c9b985a1abee"
  confirmedAt: "2026-10-09T15:40:52.000Z"
---

Connect a LangChain Deep Agents application to Submilli, run one program that
computes the 40th Fibonacci number, and close the server session when the
thread ends. You need Python 3.11 or later,
`submilli-server` 0.4.1 or later, a model-provider key, and a Blueprint
already registered on your server. For this example, use a Blueprint with no
required variables. The calculation does not need Package or tool permissions.

Start the [server](/docs/server/run-the-server) and [register a
Blueprint](/docs/server/register-a-blueprint) before running this example. Keep
`SUBMILLI_SERVER_TOKEN` exported in the shell where you run this example.
Set `SUBMILLI_BLUEPRINT` to the name of the Blueprint you registered:

```sh
export SUBMILLI_BLUEPRINT=your-registered-blueprint
```

If your Blueprint requires variables or harness secrets, pass them with the
middleware's `variables=` and `secrets=` callables from your run context. The
values are bound when the session opens; the model cannot set them. The
[`langchain-submilli` middleware reference](https://github.com/submilli/submilli-python/tree/main/packages/langchain-submilli#configuration)
describes the binding options.

## Install the integration

Create a project and install Deep Agents, the Submilli middleware, and the
Anthropic LangChain integration:

```sh
mkdir deepagents && cd deepagents
python3 -m venv .venv
. .venv/bin/activate
pip install "langchain-submilli[deepagents]" \
  "deepagents==0.7.28" "langchain-anthropic>=1.0"
```

## Write the agent

Save this as `agent.py`:

```python title="agent.py"
"""A Deep Agents application backed by Submilli."""

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


model_name = "anthropic:claude-sonnet-5"
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

submilli = SubmilliMiddleware(
    blueprint=os.environ["SUBMILLI_BLUEPRINT"],
)

agent = create_deep_agent(
    model=model_name,
    tools=[],
    subagents=[],
    middleware=[submilli],
    checkpointer=InMemorySaver(),
)


async def main() -> None:
    config = {"configurable": {"thread_id": "fibonacci-example"}}
    try:
        result = await agent.ainvoke(
            {
                "messages": [
                    {
                        "role": "user",
                        "content": "Use Submilli to write and run a program that computes the 40th Fibonacci number.",
                    }
                ]
            },
            config,
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

`SubmilliMiddleware` reads the server address from `SUBMILLI_SERVER` or uses
`http://127.0.0.1:8128`, and reads the token from
`SUBMILLI_SERVER_TOKEN`. The checkpointer keeps the server session id in the
thread state. `aclose_thread` closes that session in the `finally` block.

The profile removes Deep Agents' built-in tools, including file, shell, and
subagent delegation tools, and disables its general-purpose subagent.
`tools=[]` and `subagents=[]` add no direct tools or custom subagents. This
example exposes only Submilli's tools to the model. The profile is registered
for `model_name` in this process; use the same name in `create_deep_agent`.

## Try it out

Run the application with your provider key:

```sh
ANTHROPIC_API_KEY=... python agent.py
```

The agent should make a `submilli__typescript__execute` tool call and print
its tool result on standard error. The final answer should contain:

```text
102334155
```

The model wrote the program, while the runtime compiled and ran it in the
registered Blueprint's session. Check the printed execute-tool result as well
as the final answer to confirm that the connection completed.

To reuse application tools under Blueprint permissions, pass your LangChain
tools to `SubmilliMiddleware`'s `ptc` parameter. They become callable from
Submilli programs; the middleware does not expose them directly to the model.
Keep `create_deep_agent`'s `tools` empty: adding the same tools there creates a
direct path that bypasses Submilli. Use `interrupt_on` for callback
calls that need LangChain human-in-the-loop approval. The
[harness tools overview](/docs/tutorials/connect-a-harness#call-harness-tools-from-programs)
describes the shared lifecycle; the [`langchain-submilli` reference](https://github.com/submilli/submilli-python/tree/main/packages/langchain-submilli#your-tools-in-programs)
covers the integration contract.

You now have a Deep Agents application whose program execution is governed by
an already registered Blueprint and whose thread closes its server session.
For the same connection through another harness, see the [OpenAI Agents
SDK](/docs/tutorials/connect-openai-agents) tutorial.
